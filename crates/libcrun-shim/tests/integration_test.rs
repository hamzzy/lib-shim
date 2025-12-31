#[cfg(target_os = "linux")]
use libcrun_shim::{ContainerConfig, ContainerRuntime, ContainerStatus};
#[cfg(target_os = "macos")]
use libcrun_shim_proto::{
    CreateRequest, NetworkConfigProto, Request, ResourceLimitsProto, Response, StdioConfigProto,
};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command};
use std::thread;
use std::time::Duration;

// Helper to start the agent in the background
fn start_agent() -> Option<Child> {
    // Try to find the agent binary
    // In a real scenario, this would be built and available
    // For now, we'll skip if not available
    let agent_path = std::env::var("CARGO_BIN_EXE_libcrun-shim-agent")
        .ok()
        .or_else(|| {
            // Try to find it in target directory
            let target_dir =
                std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
            let exe = format!("{}/debug/libcrun-shim-agent", target_dir);
            if std::path::Path::new(&exe).exists() {
                Some(exe)
            } else {
                None
            }
        });

    agent_path.and_then(|path| Command::new(path).spawn().ok())
}

// Helper to wait for agent to be ready
fn wait_for_agent(socket_path: &str, max_wait: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < max_wait {
        if UnixStream::connect(socket_path).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

#[tokio::test]
#[cfg(target_os = "macos")]
async fn test_macos_rpc_communication() {
    // Start the agent
    let mut agent = match start_agent() {
        Some(a) => a,
        None => {
            eprintln!("Skipping test: agent binary not found");
            return;
        }
    };

    // Wait for agent to be ready
    if !wait_for_agent("/tmp/libcrun-shim.sock", Duration::from_secs(5)) {
        let _ = agent.kill();
        panic!("Agent did not become ready in time");
    }

    // Test RPC communication
    use libcrun_shim::macos::rpc::RpcClient;
    let mut client = RpcClient::connect().unwrap();

    // Test Create
    let create_req = Request::Create(CreateRequest {
        id: "test-rpc".to_string(),
        rootfs: "/tmp/rootfs".to_string(),
        command: vec!["sh".to_string()],
        env: vec![],
        working_dir: "/".to_string(),
        stdio: StdioConfigProto::default(),
        network: NetworkConfigProto::default(),
        volumes: vec![],
        resources: ResourceLimitsProto::default(),
        health_check: None,
    });

    match client.call(create_req).unwrap() {
        Response::Created(id) => assert_eq!(id, "test-rpc"),
        other => panic!("Unexpected response: {:?}", other),
    }

    // Test List
    match client.call(Request::List).unwrap() {
        Response::List(containers) => {
            assert_eq!(containers.len(), 1);
            assert_eq!(containers[0].id, "test-rpc");
        }
        other => panic!("Unexpected response: {:?}", other),
    }

    // Cleanup
    let _ = agent.kill();
}

#[tokio::test]
#[cfg(target_os = "linux")]
async fn test_linux_runtime_integration() {
    // Create a temporary directory for rootfs
    let temp_dir = std::env::temp_dir().join(format!("test-rootfs-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let runtime = ContainerRuntime::new().await.unwrap();

    let config = ContainerConfig {
        id: "integration-test".to_string(),
        rootfs: temp_dir.clone(),
        command: vec!["echo".to_string(), "hello".to_string()],
        env: vec!["PATH=/usr/bin:/bin".to_string()],
        working_dir: "/".to_string(),
        stdio: Default::default(),
        network: Default::default(),
        volumes: vec![],
        resources: Default::default(),
        health_check: None,
        log_driver: "json-file".to_string(),
        log_max_size: 10 * 1024 * 1024,
    };

    // Create container
    let id = runtime.create(config).await.unwrap();
    assert_eq!(id, "integration-test");

    // Verify it's in Created state
    let containers = runtime.list().await.unwrap();
    assert_eq!(containers.len(), 1);
    assert_eq!(containers[0].status, ContainerStatus::Created);

    // Start container
    runtime.start("integration-test").await.unwrap();

    // Verify it's running
    let containers = runtime.list().await.unwrap();
    assert_eq!(containers[0].status, ContainerStatus::Running);

    // Stop container
    runtime.stop("integration-test").await.unwrap();

    // Delete container
    runtime.delete("integration-test").await.unwrap();

    // Verify it's gone
    let containers = runtime.list().await.unwrap();
    assert_eq!(containers.len(), 0);

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_dir);
}
