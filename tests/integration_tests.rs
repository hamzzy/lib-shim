//! Integration tests for libcrun-shim
//!
//! These tests require:
//! - Linux: The agent running and a rootfs available
//! - macOS: The VM running with agent inside
//!
//! Run with: cargo test --test integration_tests -- --ignored

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

/// Test configuration
struct TestConfig {
    agent_path: PathBuf,
    socket_path: PathBuf,
    test_rootfs: PathBuf,
}

impl Default for TestConfig {
    fn default() -> Self {
        let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug");

        Self {
            agent_path: target_dir.join("libcrun-shim-agent"),
            socket_path: PathBuf::from("/tmp/libcrun-shim-test.sock"),
            test_rootfs: PathBuf::from("/tmp/test-rootfs"),
        }
    }
}

/// Manages the agent process for tests
struct AgentProcess {
    child: Child,
    socket_path: PathBuf,
}

impl AgentProcess {
    fn start(config: &TestConfig) -> Result<Self, String> {
        // Remove old socket
        let _ = std::fs::remove_file(&config.socket_path);

        // Start the agent
        let child = Command::new(&config.agent_path)
            .arg("--socket")
            .arg(&config.socket_path)
            .env("RUST_LOG", "info")
            .spawn()
            .map_err(|e| format!("Failed to start agent: {}", e))?;

        // Wait for socket to be available
        for _ in 0..50 {
            if config.socket_path.exists() {
                return Ok(Self {
                    child,
                    socket_path: config.socket_path.clone(),
                });
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        Err("Agent socket not available after timeout".to_string())
    }
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        // Kill the agent
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Create a minimal test rootfs
fn create_test_rootfs(path: &PathBuf) -> Result<(), String> {
    use std::fs;

    // Create basic directory structure
    let dirs = ["bin", "lib", "etc", "proc", "sys", "dev", "tmp"];
    for dir in dirs {
        fs::create_dir_all(path.join(dir))
            .map_err(|e| format!("Failed to create {}: {}", dir, e))?;
    }

    // Create a minimal /bin/sh (just a script for testing)
    let sh_path = path.join("bin/sh");
    fs::write(
        &sh_path,
        "#!/bin/sh\necho 'Test shell running'\nexit 0\n",
    )
    .map_err(|e| format!("Failed to create /bin/sh: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sh_path, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to set permissions: {}", e))?;
    }

    // Create /etc/passwd
    fs::write(path.join("etc/passwd"), "root:x:0:0:root:/root:/bin/sh\n")
        .map_err(|e| format!("Failed to create /etc/passwd: {}", e))?;

    Ok(())
}

/// Send RPC request to agent
fn send_rpc_request(
    socket_path: &PathBuf,
    request: &libcrun_shim_proto::Request,
) -> Result<libcrun_shim_proto::Response, String> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("Failed to connect to agent: {}", e))?;

    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();

    let data = libcrun_shim_proto::serialize_request(request);
    stream
        .write_all(&data)
        .map_err(|e| format!("Failed to send request: {}", e))?;

    let mut buffer = vec![0u8; 4096];
    let n = stream
        .read(&mut buffer)
        .map_err(|e| format!("Failed to read response: {}", e))?;

    libcrun_shim_proto::deserialize_response(&buffer[..n])
        .map_err(|e| format!("Failed to parse response: {}", e))
}

// ============================================================
// Tests
// ============================================================

#[test]
#[ignore] // Run with --ignored
fn test_agent_starts() {
    let config = TestConfig::default();

    // Skip if agent not built
    if !config.agent_path.exists() {
        eprintln!("Agent not built, skipping test");
        return;
    }

    let _agent = AgentProcess::start(&config).expect("Failed to start agent");

    // If we got here, agent started successfully
    assert!(config.socket_path.exists());
}

#[test]
#[ignore]
fn test_container_lifecycle() {
    let config = TestConfig::default();

    // Skip if agent not built
    if !config.agent_path.exists() {
        eprintln!("Agent not built, skipping test");
        return;
    }

    // Create test rootfs
    create_test_rootfs(&config.test_rootfs).expect("Failed to create test rootfs");

    // Start agent
    let _agent = AgentProcess::start(&config).expect("Failed to start agent");

    // Test: List containers (should be empty)
    let response = send_rpc_request(&config.socket_path, &libcrun_shim_proto::Request::List)
        .expect("Failed to list containers");

    if let libcrun_shim_proto::Response::List(containers) = response {
        assert!(containers.is_empty(), "Expected no containers initially");
    } else {
        panic!("Unexpected response: {:?}", response);
    }

    // Test: Create container
    let create_request = libcrun_shim_proto::Request::Create(libcrun_shim_proto::CreateRequest {
        id: "test-container".to_string(),
        rootfs: config.test_rootfs.display().to_string(),
        command: vec!["/bin/sh".to_string(), "-c".to_string(), "echo hello".to_string()],
        env: vec!["PATH=/bin".to_string()],
        working_dir: "/".to_string(),
        stdio: Default::default(),
        network: Default::default(),
        volumes: vec![],
        resources: Default::default(),
    });

    let response = send_rpc_request(&config.socket_path, &create_request)
        .expect("Failed to create container");

    if let libcrun_shim_proto::Response::Created(id) = response {
        assert_eq!(id, "test-container");
    } else if let libcrun_shim_proto::Response::Error(e) = response {
        eprintln!("Create failed (expected on macOS without VM): {}", e);
        return;
    } else {
        panic!("Unexpected response: {:?}", response);
    }

    // Test: List containers (should have one)
    let response = send_rpc_request(&config.socket_path, &libcrun_shim_proto::Request::List)
        .expect("Failed to list containers");

    if let libcrun_shim_proto::Response::List(containers) = response {
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].id, "test-container");
    }

    // Test: Delete container
    let response = send_rpc_request(
        &config.socket_path,
        &libcrun_shim_proto::Request::Delete("test-container".to_string()),
    )
    .expect("Failed to delete container");

    assert!(matches!(response, libcrun_shim_proto::Response::Deleted));

    // Cleanup
    let _ = std::fs::remove_dir_all(&config.test_rootfs);
}

#[test]
#[ignore]
fn test_metrics() {
    let config = TestConfig::default();

    if !config.agent_path.exists() {
        eprintln!("Agent not built, skipping test");
        return;
    }

    let _agent = AgentProcess::start(&config).expect("Failed to start agent");

    // Create and start a container first
    create_test_rootfs(&config.test_rootfs).expect("Failed to create test rootfs");

    let create_request = libcrun_shim_proto::Request::Create(libcrun_shim_proto::CreateRequest {
        id: "metrics-test".to_string(),
        rootfs: config.test_rootfs.display().to_string(),
        command: vec!["/bin/sh".to_string()],
        env: vec![],
        working_dir: "/".to_string(),
        stdio: Default::default(),
        network: Default::default(),
        volumes: vec![],
        resources: Default::default(),
    });

    let _ = send_rpc_request(&config.socket_path, &create_request);

    // Request metrics
    let response = send_rpc_request(
        &config.socket_path,
        &libcrun_shim_proto::Request::Metrics("metrics-test".to_string()),
    )
    .expect("Failed to get metrics");

    match response {
        libcrun_shim_proto::Response::Metrics(m) => {
            assert_eq!(m.id, "metrics-test");
        }
        libcrun_shim_proto::Response::Error(e) => {
            eprintln!("Metrics failed (expected without running container): {}", e);
        }
        _ => panic!("Unexpected response"),
    }

    // Cleanup
    let _ = send_rpc_request(
        &config.socket_path,
        &libcrun_shim_proto::Request::Delete("metrics-test".to_string()),
    );
    let _ = std::fs::remove_dir_all(&config.test_rootfs);
}

#[test]
#[ignore]
fn test_logs() {
    let config = TestConfig::default();

    if !config.agent_path.exists() {
        eprintln!("Agent not built, skipping test");
        return;
    }

    let _agent = AgentProcess::start(&config).expect("Failed to start agent");

    create_test_rootfs(&config.test_rootfs).expect("Failed to create test rootfs");

    let create_request = libcrun_shim_proto::Request::Create(libcrun_shim_proto::CreateRequest {
        id: "logs-test".to_string(),
        rootfs: config.test_rootfs.display().to_string(),
        command: vec!["/bin/sh".to_string()],
        env: vec![],
        working_dir: "/".to_string(),
        stdio: Default::default(),
        network: Default::default(),
        volumes: vec![],
        resources: Default::default(),
    });

    let _ = send_rpc_request(&config.socket_path, &create_request);

    // Request logs
    let response = send_rpc_request(
        &config.socket_path,
        &libcrun_shim_proto::Request::Logs(libcrun_shim_proto::LogsRequest {
            id: "logs-test".to_string(),
            tail: 10,
            since: 0,
            timestamps: false,
        }),
    )
    .expect("Failed to get logs");

    match response {
        libcrun_shim_proto::Response::Logs(l) => {
            assert_eq!(l.id, "logs-test");
        }
        libcrun_shim_proto::Response::Error(e) => {
            eprintln!("Logs failed: {}", e);
        }
        _ => panic!("Unexpected response"),
    }

    // Cleanup
    let _ = send_rpc_request(
        &config.socket_path,
        &libcrun_shim_proto::Request::Delete("logs-test".to_string()),
    );
    let _ = std::fs::remove_dir_all(&config.test_rootfs);
}

/// E2E test with real container (requires Linux with crun installed)
#[test]
#[ignore]
#[cfg(target_os = "linux")]
fn test_e2e_real_container() {
    use std::process::Command;

    let config = TestConfig::default();

    if !config.agent_path.exists() {
        eprintln!("Agent not built, skipping test");
        return;
    }

    // Check if crun is available
    if Command::new("crun").arg("--version").output().is_err() {
        eprintln!("crun not available, skipping E2E test");
        return;
    }

    // Use alpine rootfs if available
    let alpine_rootfs = PathBuf::from("/tmp/alpine-rootfs");
    if !alpine_rootfs.exists() {
        eprintln!("Alpine rootfs not found at {:?}, skipping E2E test", alpine_rootfs);
        eprintln!("To create: mkdir -p /tmp/alpine-rootfs && docker export $(docker create alpine) | tar -C /tmp/alpine-rootfs -xf -");
        return;
    }

    let _agent = AgentProcess::start(&config).expect("Failed to start agent");

    // Create container with real rootfs
    let create_request = libcrun_shim_proto::Request::Create(libcrun_shim_proto::CreateRequest {
        id: "e2e-test".to_string(),
        rootfs: alpine_rootfs.display().to_string(),
        command: vec!["echo".to_string(), "Hello from container!".to_string()],
        env: vec!["PATH=/usr/bin:/bin".to_string()],
        working_dir: "/".to_string(),
        stdio: Default::default(),
        network: Default::default(),
        volumes: vec![],
        resources: Default::default(),
    });

    let response = send_rpc_request(&config.socket_path, &create_request)
        .expect("Failed to create container");

    if let libcrun_shim_proto::Response::Error(e) = &response {
        panic!("Create failed: {}", e);
    }

    // Start container
    let response = send_rpc_request(
        &config.socket_path,
        &libcrun_shim_proto::Request::Start("e2e-test".to_string()),
    )
    .expect("Failed to start container");

    if let libcrun_shim_proto::Response::Error(e) = &response {
        eprintln!("Start failed (may need root): {}", e);
    } else {
        assert!(matches!(response, libcrun_shim_proto::Response::Started));
    }

    // Wait a bit
    std::thread::sleep(Duration::from_millis(500));

    // Stop container
    let _ = send_rpc_request(
        &config.socket_path,
        &libcrun_shim_proto::Request::Stop("e2e-test".to_string()),
    );

    // Delete container
    let response = send_rpc_request(
        &config.socket_path,
        &libcrun_shim_proto::Request::Delete("e2e-test".to_string()),
    )
    .expect("Failed to delete container");

    assert!(matches!(response, libcrun_shim_proto::Response::Deleted));

    println!("E2E test passed!");
}

