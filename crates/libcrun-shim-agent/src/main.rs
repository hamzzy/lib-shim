use libcrun_shim_proto::*;
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

#[cfg(target_os = "linux")]
use libcrun_sys::safe as crun;

// Wrapper to make raw pointers Send + Sync
// This is safe because libcrun context is thread-safe for read operations
#[cfg(target_os = "linux")]
struct LibcrunContext(*mut libcrun_sys::libcrun_context_t);

#[cfg(target_os = "linux")]
unsafe impl Send for LibcrunContext {}
#[cfg(target_os = "linux")]
unsafe impl Sync for LibcrunContext {}

#[cfg(target_os = "linux")]
struct LibcrunContainer(*mut libcrun_sys::libcrun_container_t);

#[cfg(target_os = "linux")]
unsafe impl Send for LibcrunContainer {}
#[cfg(target_os = "linux")]
unsafe impl Sync for LibcrunContainer {}

/// Configuration for container health checks
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct HealthCheckConfig {
    #[serde(default)]
    command: Vec<String>,
    #[serde(default)]
    interval_secs: Option<u64>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    retries: Option<u32>,
    #[serde(default)]
    start_period_secs: Option<u64>,
}

/// Serializable container state for persistence
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedContainerState {
    id: String,
    rootfs: String,
    command: Vec<String>,
    env: Vec<String>,
    working_dir: String,
    status: String,
    pid: Option<u32>,
    created_at: u64,
    #[serde(default)]
    health_check: Option<HealthCheckConfig>,
    #[serde(default)]
    last_health_check: Option<u64>,
    #[serde(default)]
    health_status: String,
    #[serde(default)]
    consecutive_failures: u32,
}

// Container state in the agent

struct ContainerState {
    id: String,
    rootfs: String,
    command: Vec<String>,
    env: Vec<String>,
    working_dir: String,
    status: String,
    pid: Option<u32>,
    created_at: u64,
    health_check: Option<HealthCheckConfig>,
    last_health_check: Option<u64>,
    health_status: String,
    consecutive_failures: u32,
    #[cfg(target_os = "linux")]
    libcrun_container: Option<LibcrunContainer>,
}

impl ContainerState {
    fn to_persisted(&self) -> PersistedContainerState {
        PersistedContainerState {
            id: self.id.clone(),
            rootfs: self.rootfs.clone(),
            command: self.command.clone(),
            env: self.env.clone(),
            working_dir: self.working_dir.clone(),
            status: self.status.clone(),
            pid: self.pid,
            created_at: self.created_at,
            health_check: self.health_check.clone(),
            last_health_check: self.last_health_check,
            health_status: self.health_status.clone(),
            consecutive_failures: self.consecutive_failures,
        }
    }

    fn from_persisted(p: PersistedContainerState) -> Self {
        Self {
            id: p.id,
            rootfs: p.rootfs,
            command: p.command,
            env: p.env,
            working_dir: p.working_dir,
            status: p.status,
            pid: p.pid,
            created_at: p.created_at,
            health_check: p.health_check,
            last_health_check: p.last_health_check,
            health_status: if p.health_status.is_empty() {
                "unknown".to_string()
            } else {
                p.health_status
            },
            consecutive_failures: p.consecutive_failures,
            #[cfg(target_os = "linux")]
            libcrun_container: None,
        }
    }
}

/// Agent state directory for persistence
const STATE_DIR: &str = "/var/run/libcrun-shim";
const STATE_FILE: &str = "/var/run/libcrun-shim/state.json";

/// Get current Unix timestamp in seconds
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// Shared state for the agent
struct AgentState {
    containers: RwLock<HashMap<String, ContainerState>>,
    #[allow(dead_code)]
    state_dir: PathBuf,
    #[cfg(target_os = "linux")]
    libcrun_context: Option<LibcrunContext>,
    #[cfg(target_os = "linux")]
    libcrun_available: bool,
}

impl AgentState {
    fn new() -> Self {
        // Ensure state directory exists
        let state_dir = PathBuf::from(STATE_DIR);
        if let Err(e) = std::fs::create_dir_all(&state_dir) {
            log::warn!("Failed to create state directory: {}", e);
        }

        #[cfg(target_os = "linux")]
        {
            // Try to initialize libcrun context
            let (context, available) = match crun::context_new() {
                Ok(ctx) => {
                    log::info!("libcrun initialized successfully in agent - using real container operations");
                    (Some(LibcrunContext(ctx)), true)
                }
                Err(e) => {
                    log::warn!(
                        "libcrun not available in agent: {}, using fallback mode",
                        e.message
                    );
                    (None, false)
                }
            };

            let state = Self {
                containers: RwLock::new(HashMap::new()),
                state_dir,
                libcrun_context: context,
                libcrun_available: available,
            };

            // Recover any persisted state
            state.recover_state();
            state
        }

        #[cfg(not(target_os = "linux"))]
        {
            let state = Self {
                containers: RwLock::new(HashMap::new()),
                state_dir,
            };

            // Recover any persisted state
            state.recover_state();
            state
        }
    }

    /// Persist current container state to disk
    fn persist_state(&self) {
        let containers = self.containers.read().unwrap();
        let persisted: Vec<PersistedContainerState> =
            containers.values().map(|c| c.to_persisted()).collect();

        match serde_json::to_string_pretty(&persisted) {
            Ok(json) => {
                if let Err(e) = std::fs::write(STATE_FILE, json) {
                    log::error!("Failed to persist state: {}", e);
                }
            }
            Err(e) => {
                log::error!("Failed to serialize state: {}", e);
            }
        }
    }

    /// Recover state from disk and detect orphaned containers
    fn recover_state(&self) {
        let state_path = PathBuf::from(STATE_FILE);
        if !state_path.exists() {
            log::info!("No previous state found");
            return;
        }

        match std::fs::read_to_string(&state_path) {
            Ok(json) => {
                match serde_json::from_str::<Vec<PersistedContainerState>>(&json) {
                    Ok(persisted) => {
                        log::info!(
                            "Recovering {} containers from previous state",
                            persisted.len()
                        );
                        let mut containers = self.containers.write().unwrap();

                        for p in persisted {
                            // Check if the container process is still running
                            let is_running = if let Some(pid) = p.pid {
                                Self::is_process_running(pid)
                            } else {
                                false
                            };

                            if is_running {
                                log::info!(
                                    "Container {} (pid {}) still running, recovering",
                                    p.id,
                                    p.pid.unwrap_or(0)
                                );
                                let mut state = ContainerState::from_persisted(p);
                                state.status = "running".to_string();
                                containers.insert(state.id.clone(), state);
                            } else {
                                // Container process not running - mark as orphaned
                                log::warn!("Container {} was orphaned (pid {} not running), marking for cleanup", 
                                    p.id, p.pid.unwrap_or(0));
                                let mut state = ContainerState::from_persisted(p);
                                state.status = "orphaned".to_string();
                                state.pid = None;
                                containers.insert(state.id.clone(), state);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to parse state file: {}", e);
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to read state file: {}", e);
            }
        }
    }

    /// Check if a process is running
    fn is_process_running(pid: u32) -> bool {
        // Use kill(0) to check if process exists
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    /// Cleanup orphaned containers
    fn cleanup_orphans(&self) {
        let mut containers = self.containers.write().unwrap();
        let orphans: Vec<String> = containers
            .iter()
            .filter(|(_, c)| c.status == "orphaned")
            .map(|(id, _)| id.clone())
            .collect();

        for id in orphans {
            log::info!("Cleaning up orphaned container: {}", id);
            // Try to clean up any remaining resources
            if let Some(container) = containers.get(&id) {
                // Clean up container directory
                let container_dir = format!("{}/{}", STATE_DIR, container.id);
                let _ = std::fs::remove_dir_all(&container_dir);
            }
            containers.remove(&id);
        }
        drop(containers);
        self.persist_state();
    }

    /// Graceful shutdown - stop all containers
    fn graceful_shutdown(&self) {
        log::info!("Initiating graceful shutdown...");

        let container_ids: Vec<String> = {
            let containers = self.containers.read().unwrap();
            containers
                .iter()
                .filter(|(_, c)| c.status == "running" || c.status == "Running")
                .map(|(id, _)| id.clone())
                .collect()
        };

        for id in container_ids {
            log::info!("Stopping container {} during shutdown", id);
            if let Err(e) = self.stop_container(&id) {
                log::error!("Failed to stop container {}: {}", id, e);
            }
        }

        // Final state persist
        self.persist_state();
        log::info!("Graceful shutdown complete");
    }

    /// Run health checks for all containers that have them configured
    fn run_health_checks(&self) {
        let containers = self.containers.read().unwrap();

        for (id, container) in containers.iter() {
            if container.status != "Running" && container.status != "running" {
                continue;
            }

            // Check if container has health check configured
            if let Some(health_check) = &container.health_check {
                if health_check.command.is_empty() {
                    continue;
                }

                // Check if enough time has passed since last check
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let last_check = container.last_health_check.unwrap_or(0);
                let interval = health_check.interval_secs.unwrap_or(30);

                if now - last_check < interval {
                    continue;
                }

                // Run health check
                log::debug!("Running health check for container {}", id);
                let result = self.execute_health_check(id, &health_check.command);

                match result {
                    Ok(healthy) => {
                        if healthy {
                            log::debug!("Container {} health check passed", id);
                        } else {
                            log::warn!("Container {} health check failed", id);
                        }
                    }
                    Err(e) => {
                        log::warn!("Container {} health check error: {}", id, e);
                    }
                }
            }
        }
    }

    /// Execute a health check command for a container
    fn execute_health_check(
        &self,
        _container_id: &str,
        command: &[String],
    ) -> Result<bool, String> {
        if command.is_empty() {
            return Err("Empty health check command".to_string());
        }

        let output = std::process::Command::new(&command[0])
            .args(&command[1..])
            .output()
            .map_err(|e| format!("Failed to execute health check: {}", e))?;

        Ok(output.status.success())
    }

    /// Stop a container by ID
    fn stop_container(&self, id: &str) -> Result<(), String> {
        let mut containers = self.containers.write().unwrap();
        if let Some(container) = containers.get_mut(id) {
            if let Some(pid) = container.pid {
                // Send SIGTERM first
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGTERM);
                }

                // Wait briefly for graceful shutdown
                std::thread::sleep(std::time::Duration::from_secs(2));

                // Check if still running, send SIGKILL
                if Self::is_process_running(pid) {
                    log::warn!("Container {} did not stop gracefully, sending SIGKILL", id);
                    unsafe {
                        libc::kill(pid as libc::pid_t, libc::SIGKILL);
                    }
                }
            }
            container.status = "stopped".to_string();
            container.pid = None;
            Ok(())
        } else {
            Err(format!("Container {} not found", id))
        }
    }

    #[cfg(target_os = "linux")]
    fn build_oci_config_json(
        rootfs: &str,
        command: &[String],
        env: &[String],
        working_dir: &str,
        container_id: &str,
        stdio: &libcrun_shim_proto::StdioConfigProto,
        network: &libcrun_shim_proto::NetworkConfigProto,
        volumes: &[libcrun_shim_proto::VolumeMountProto],
        resources: &libcrun_shim_proto::ResourceLimitsProto,
    ) -> Result<String, String> {
        // Ensure PATH is in env if not provided
        let mut env_vec = env.to_vec();
        let has_path = env_vec.iter().any(|e| e.starts_with("PATH="));
        if !has_path {
            env_vec.push(
                "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string(),
            );
        }

        // Build mounts array with default mounts + user volumes
        let mut mounts = vec![
            serde_json::json!({
                "destination": "/proc",
                "type": "proc",
                "source": "proc"
            }),
            serde_json::json!({
                "destination": "/dev",
                "type": "tmpfs",
                "source": "tmpfs",
                "options": ["nosuid", "strictatime", "mode=755", "size=65536k"]
            }),
            serde_json::json!({
                "destination": "/dev/pts",
                "type": "devpts",
                "source": "devpts",
                "options": ["nosuid", "noexec", "newinstance", "ptmxmode=0666", "mode=0620"]
            }),
            serde_json::json!({
                "destination": "/dev/shm",
                "type": "tmpfs",
                "source": "shm",
                "options": ["nosuid", "noexec", "nodev", "mode=1777", "size=65536k"]
            }),
            serde_json::json!({
                "destination": "/sys",
                "type": "sysfs",
                "source": "sysfs",
                "options": ["nosuid", "noexec", "nodev", "ro"]
            }),
        ];

        // Add user-defined volume mounts
        for volume in volumes {
            let mut mount = serde_json::json!({
                "destination": volume.destination,
                "type": "bind",
                "source": volume.source,
            });

            if !volume.options.is_empty() {
                mount["options"] = serde_json::json!(volume.options);
            }

            mounts.push(mount);
        }

        // Build rlimits array with defaults + resource limits
        let mut rlimits = vec![serde_json::json!({
            "type": "RLIMIT_NOFILE",
            "hard": 1024,
            "soft": 1024
        })];

        // Add resource limits
        if let Some(memory) = resources.memory {
            if memory > 0 {
                rlimits.push(serde_json::json!({
                    "type": "RLIMIT_AS",
                    "hard": memory,
                    "soft": memory
                }));
            }
        }

        if let Some(pids) = resources.pids {
            if pids > 0 {
                rlimits.push(serde_json::json!({
                    "type": "RLIMIT_NPROC",
                    "hard": pids,
                    "soft": pids
                }));
            }
        }

        // Build resources object
        let mut resources_obj = serde_json::json!({
            "devices": [
                {
                    "allow": false,
                    "access": "rwm"
                }
            ]
        });

        // Add CPU and memory limits
        if resources.cpu.is_some() || resources.memory.is_some() {
            let mut cpu_obj = serde_json::json!({});
            if let Some(cpu) = resources.cpu {
                if cpu > 0.0 {
                    cpu_obj["shares"] = serde_json::json!((cpu * 1024.0) as u64);
                    cpu_obj["quota"] = serde_json::json!((cpu * 100000.0) as i64);
                    cpu_obj["period"] = serde_json::json!(100000);
                }
            }

            let mut memory_obj = serde_json::json!({});
            if let Some(memory) = resources.memory {
                if memory > 0 {
                    memory_obj["limit"] = serde_json::json!(memory);
                }
            }
            if let Some(memory_swap) = resources.memory_swap {
                if memory_swap > 0 {
                    memory_obj["swap"] = serde_json::json!(memory_swap);
                }
            }

            if !cpu_obj.as_object().unwrap().is_empty() {
                resources_obj["cpu"] = cpu_obj;
            }
            if !memory_obj.as_object().unwrap().is_empty() {
                resources_obj["memory"] = memory_obj;
            }
        }

        // Determine network namespace based on network mode
        let network_namespace = match network.mode.as_str() {
            "host" => None, // No network namespace for host mode
            "none" => Some(serde_json::json!({
                "type": "network"
            })),
            _ => Some(serde_json::json!({
                "type": "network"
            })),
        };

        let mut namespaces = vec![
            serde_json::json!({"type": "pid"}),
            serde_json::json!({"type": "ipc"}),
            serde_json::json!({"type": "uts"}),
            serde_json::json!({"type": "mount"}),
        ];

        if let Some(ns) = network_namespace {
            namespaces.push(ns);
        }

        let oci_config = serde_json::json!({
            "ociVersion": "1.0.0",
            "process": {
                "terminal": stdio.tty,
                "user": {
                    "uid": 0,
                    "gid": 0
                },
                "args": command,
                "env": env_vec,
                "cwd": working_dir,
                "capabilities": {
                    "bounding": [
                        "CAP_AUDIT_WRITE",
                        "CAP_KILL",
                        "CAP_NET_BIND_SERVICE"
                    ],
                    "effective": [
                        "CAP_AUDIT_WRITE",
                        "CAP_KILL",
                        "CAP_NET_BIND_SERVICE"
                    ],
                    "inheritable": [
                        "CAP_AUDIT_WRITE",
                        "CAP_KILL",
                        "CAP_NET_BIND_SERVICE"
                    ],
                    "permitted": [
                        "CAP_AUDIT_WRITE",
                        "CAP_KILL",
                        "CAP_NET_BIND_SERVICE"
                    ],
                    "ambient": [
                        "CAP_AUDIT_WRITE",
                        "CAP_KILL",
                        "CAP_NET_BIND_SERVICE"
                    ]
                },
                "rlimits": rlimits,
                "noNewPrivileges": true
            },
            "root": {
                "path": rootfs,
                "readonly": false
            },
            "hostname": container_id,
            "mounts": mounts,
            "linux": {
                "resources": resources_obj,
                "namespaces": namespaces,
                "maskedPaths": [
                    "/proc/kcore",
                    "/proc/latency",
                    "/proc/timer_list",
                    "/proc/timer_stats",
                    "/proc/sched_debug",
                    "/proc/scsi",
                    "/sys/firmware"
                ],
                "readonlyPaths": [
                    "/proc/asound",
                    "/proc/bus",
                    "/proc/fs",
                    "/proc/irq",
                    "/proc/sys",
                    "/proc/sysrq-trigger"
                ]
            }
        });

        serde_json::to_string_pretty(&oci_config).map_err(|e| e.to_string())
    }
}

impl Drop for AgentState {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        {
            // Clean up libcrun context
            if let Some(LibcrunContext(ctx)) = self.libcrun_context.take() {
                crun::context_free(ctx);
            }

            // Clean up containers
            let containers = self.containers.write().unwrap();
            for (_, state) in containers.iter() {
                #[cfg(target_os = "linux")]
                if let Some(LibcrunContainer(container)) = state.libcrun_container {
                    crun::container_free(container);
                }
            }
        }
    }
}

/// Agent configuration
struct AgentConfig {
    socket_path: String,
    vsock_port: u32,
    vsock_enabled: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            socket_path: "/tmp/libcrun-shim.sock".to_string(),
            vsock_port: 1234,
            vsock_enabled: false,
        }
    }
}

fn parse_args() -> AgentConfig {
    let args: Vec<String> = std::env::args().collect();
    let mut config = AgentConfig::default();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--version" | "-V" => {
                println!("libcrun-shim-agent {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!("libcrun-shim-agent - Container runtime agent");
                println!();
                println!("Usage: libcrun-shim-agent [OPTIONS]");
                println!();
                println!("Options:");
                println!("  --socket PATH     Unix socket path (default: /tmp/libcrun-shim.sock)");
                println!("  --vsock-port PORT Vsock port for VM communication");
                println!("  --version         Print version");
                println!("  --help            Print help");
                std::process::exit(0);
            }
            "--socket" => {
                i += 1;
                if i < args.len() {
                    config.socket_path = args[i].clone();
                }
            }
            "--vsock-port" => {
                i += 1;
                if i < args.len() {
                    config.vsock_port = args[i].parse().unwrap_or(1234);
                    config.vsock_enabled = true;
                }
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
            }
        }
        i += 1;
    }

    config
}

/// Global shutdown flag
static SHUTDOWN_FLAG: AtomicBool = AtomicBool::new(false);

fn main() {
    // Parse command line arguments
    let config = parse_args();

    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    log::info!("libcrun-shim-agent v{}", env!("CARGO_PKG_VERSION"));

    // Create shared state
    let state = Arc::new(AgentState::new());

    // Clean up any orphaned containers from previous runs
    state.cleanup_orphans();

    // Setup signal handlers
    let state_for_signals = Arc::clone(&state);
    let mut signals =
        Signals::new([SIGTERM, SIGINT, SIGHUP]).expect("Failed to register signal handlers");

    std::thread::spawn(move || {
        for sig in signals.forever() {
            match sig {
                SIGTERM => {
                    log::info!("Received SIGTERM, initiating graceful shutdown");
                    SHUTDOWN_FLAG.store(true, Ordering::SeqCst);
                    state_for_signals.graceful_shutdown();
                    std::process::exit(0);
                }
                SIGINT => {
                    log::info!("Received SIGINT, initiating graceful shutdown");
                    SHUTDOWN_FLAG.store(true, Ordering::SeqCst);
                    state_for_signals.graceful_shutdown();
                    std::process::exit(0);
                }
                SIGHUP => {
                    log::info!("Received SIGHUP, reloading configuration");
                    // Could reload config here if needed
                    state_for_signals.persist_state();
                }
                _ => {}
            }
        }
    });

    // If vsock is enabled, we'd listen on vsock here
    // For now, use Unix socket
    if config.vsock_enabled {
        log::info!(
            "Vsock mode enabled on port {} (not implemented, using Unix socket)",
            config.vsock_port
        );
    }

    // Remove old socket if it exists
    let _ = std::fs::remove_file(&config.socket_path);

    // Listen on a Unix socket for RPC requests
    let listener = UnixListener::bind(&config.socket_path).expect("Failed to bind to socket");

    // Set non-blocking so we can check shutdown flag
    listener
        .set_nonblocking(true)
        .expect("Failed to set non-blocking");

    log::info!("Agent listening on {}", config.socket_path);

    // Ensure socket is cleaned up on drop
    struct SocketGuard(String);
    impl Drop for SocketGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _guard = SocketGuard(config.socket_path.clone());

    // Persist state periodically
    let state_for_persist = Arc::clone(&state);
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(30));
        if SHUTDOWN_FLAG.load(Ordering::SeqCst) {
            break;
        }
        state_for_persist.persist_state();
    });

    // Container watchdog - monitors container health and detects orphans
    let state_for_watchdog = Arc::clone(&state);
    std::thread::spawn(move || {
        log::info!("Container watchdog started");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(10));
            if SHUTDOWN_FLAG.load(Ordering::SeqCst) {
                break;
            }

            // Check all running containers
            let mut containers = state_for_watchdog.containers.write().unwrap();
            let mut orphaned = Vec::new();

            for (id, container) in containers.iter() {
                if container.status == "Running" {
                    if let Some(pid) = container.pid {
                        // Check if process is still alive
                        let alive = unsafe { libc::kill(pid as libc::pid_t, 0) == 0 };
                        if !alive {
                            log::warn!(
                                "Container {} (PID {}) is no longer running - marking as orphaned",
                                id,
                                pid
                            );
                            orphaned.push(id.clone());
                        }
                    }
                }
            }

            // Mark orphaned containers
            for id in orphaned {
                if let Some(container) = containers.get_mut(&id) {
                    container.status = "orphaned".to_string();
                    container.pid = None;
                }
            }

            drop(containers);

            // Check health for containers with health checks
            state_for_watchdog.run_health_checks();
        }
        log::info!("Container watchdog stopped");
    });

    // Main accept loop with shutdown check
    loop {
        if SHUTDOWN_FLAG.load(Ordering::SeqCst) {
            log::info!("Shutdown flag set, exiting main loop");
            break;
        }

        match listener.accept() {
            Ok((stream, _)) => {
                let state_clone = Arc::clone(&state);
                std::thread::spawn(move || handle_client(stream, state_clone));
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No connection ready, sleep briefly and check shutdown flag
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                log::error!("Connection error: {}", e);
            }
        }
    }

    // Final cleanup
    state.graceful_shutdown();
}

fn handle_client(mut stream: UnixStream, state: Arc<AgentState>) {
    let mut buffer = vec![0u8; 4096];

    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break, // Connection closed
            Ok(n) => {
                let request = match deserialize_request(&buffer[..n]) {
                    Ok(req) => req,
                    Err(e) => {
                        log::warn!("Failed to parse request: {}", e);
                        let response = Response::Error(format!("Parse error: {}", e));
                        let _ = stream.write_all(&serialize_response(&response));
                        continue;
                    }
                };

                let response = handle_request(request, &state);
                if let Err(e) = stream.write_all(&serialize_response(&response)) {
                    log::error!("Write error: {}", e);
                    break;
                }
            }
            Err(e) => {
                log::error!("Read error: {}", e);
                break;
            }
        }
    }
}

fn handle_request(request: Request, state: &AgentState) -> Response {
    match request {
        Request::Create(req) => {
            // Validate request
            if req.id.is_empty() {
                return Response::Error("Container ID cannot be empty".to_string());
            }
            if req.command.is_empty() {
                return Response::Error("Command cannot be empty".to_string());
            }

            // Check if container already exists
            {
                let containers = state.containers.read().unwrap();
                if containers.contains_key(&req.id) {
                    return Response::Error(format!("Container '{}' already exists", req.id));
                }
            }

            log::info!("Creating container: id={}, rootfs={}", req.id, req.rootfs);

            // Try to use libcrun if available
            #[cfg(target_os = "linux")]
            let libcrun_container = if state.libcrun_available {
                // Build OCI config JSON
                let oci_json = match AgentState::build_oci_config_json(
                    &req.rootfs,
                    &req.command,
                    &req.env,
                    &req.working_dir,
                    &req.id,
                    &req.stdio,
                    &req.network,
                    &req.volumes,
                    &req.resources,
                ) {
                    Ok(json) => json,
                    Err(e) => {
                        return Response::Error(format!("Failed to build OCI config: {}", e));
                    }
                };

                // Load container from JSON config
                match crun::container_load_from_memory(&oci_json) {
                    Ok(container) => {
                        // Create the container using libcrun
                        if let Some(LibcrunContext(ctx)) = &state.libcrun_context {
                            match crun::container_create(*ctx, container, &req.id) {
                                Ok(_) => {
                                    log::info!(
                                        "Container '{}' created successfully via libcrun",
                                        req.id
                                    );
                                    Some(LibcrunContainer(container))
                                }
                                Err(e) => {
                                    crun::container_free(container);
                                    return Response::Error(format!(
                                        "libcrun failed to create container: {}",
                                        e.message
                                    ));
                                }
                            }
                        } else {
                            crun::container_free(container);
                            None
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "libcrun container load failed: {}, using fallback mode",
                            e.message
                        );
                        None
                    }
                }
            } else {
                None
            };

            #[cfg(not(target_os = "linux"))]
            let _libcrun_container: Option<*mut libcrun_sys::libcrun_container_t> = None;

            // Convert health check from proto if present
            let health_check = req.health_check.map(|hc| HealthCheckConfig {
                command: hc.command,
                interval_secs: if hc.interval_secs > 0 {
                    Some(hc.interval_secs)
                } else {
                    None
                },
                timeout_secs: if hc.timeout_secs > 0 {
                    Some(hc.timeout_secs)
                } else {
                    None
                },
                retries: if hc.retries > 0 {
                    Some(hc.retries)
                } else {
                    None
                },
                start_period_secs: if hc.start_period_secs > 0 {
                    Some(hc.start_period_secs)
                } else {
                    None
                },
            });

            let container_state = ContainerState {
                id: req.id.clone(),
                rootfs: req.rootfs,
                command: req.command,
                env: req.env,
                working_dir: req.working_dir,
                status: "Created".to_string(),
                pid: None,
                created_at: current_timestamp(),
                health_check,
                last_health_check: None,
                health_status: "unknown".to_string(),
                consecutive_failures: 0,
                #[cfg(target_os = "linux")]
                libcrun_container,
            };

            state
                .containers
                .write()
                .unwrap()
                .insert(req.id.clone(), container_state);
            state.persist_state();
            Response::Created(req.id)
        }
        Request::Start(id) => {
            let mut containers = state.containers.write().unwrap();
            let container = containers.get_mut(&id);

            match container {
                None => Response::Error(format!("Container '{}' not found", id)),
                Some(c) => {
                    if c.status == "Running" {
                        Response::Error(format!("Container '{}' is already running", id))
                    } else if c.status == "Stopped" {
                        Response::Error(format!(
                            "Container '{}' is stopped and cannot be restarted",
                            id
                        ))
                    } else {
                        // Try to start container via libcrun if available
                        #[cfg(target_os = "linux")]
                        if state.libcrun_available {
                            if let Some(LibcrunContainer(container)) = c.libcrun_container {
                                if let Some(LibcrunContext(ctx)) = &state.libcrun_context {
                                    match crun::container_start(*ctx, container, &id) {
                                        Ok(_) => {
                                            log::info!(
                                                "Container '{}' started successfully via libcrun",
                                                id
                                            );
                                            // Try to get actual PID from container state
                                            c.pid = crun::get_container_pid(&id).or_else(|| {
                                                // Fallback: try to get from container state API
                                                None
                                            });

                                            // If we still don't have a PID, use placeholder
                                            if c.pid.is_none() {
                                                log::warn!("Could not retrieve PID for container '{}' from libcrun state, using placeholder", id);
                                                c.pid = Some(std::process::id());
                                            // Placeholder
                                            } else {
                                                log::debug!("Container '{}' PID: {:?}", id, c.pid);
                                            }
                                        }
                                        Err(e) => {
                                            return Response::Error(format!(
                                                "libcrun failed to start container: {}",
                                                e.message
                                            ));
                                        }
                                    }
                                }
                            }
                        }

                        if c.status != "Running" {
                            log::info!("Starting container: {} (fallback mode)", id);
                            c.status = "Running".to_string();
                            c.pid = Some(std::process::id()); // Placeholder
                        }

                        drop(containers);
                        state.persist_state();
                        Response::Started
                    }
                }
            }
        }
        Request::Stop(id) => {
            let mut containers = state.containers.write().unwrap();
            let container = containers.get_mut(&id);

            match container {
                None => Response::Error(format!("Container '{}' not found", id)),
                Some(c) => {
                    if c.status != "Running" {
                        Response::Error(format!("Container '{}' is not running", id))
                    } else {
                        // Try to stop container via libcrun if available
                        #[cfg(target_os = "linux")]
                        if state.libcrun_available {
                            if let Some(LibcrunContainer(container)) = c.libcrun_container {
                                if let Some(LibcrunContext(ctx)) = &state.libcrun_context {
                                    // Use SIGTERM to stop gracefully
                                    match crun::container_kill(*ctx, container, &id, libc::SIGTERM)
                                    {
                                        Ok(_) => {
                                            log::info!("Container '{}' stopped successfully via libcrun (SIGTERM)", id);
                                        }
                                        Err(e) => {
                                            return Response::Error(format!(
                                                "libcrun failed to stop container: {}",
                                                e.message
                                            ));
                                        }
                                    }
                                    // Put container back
                                    c.libcrun_container = Some(LibcrunContainer(container));
                                }
                            }
                        }

                        log::info!("Stopping container: {}", id);
                        c.status = "Stopped".to_string();
                        c.pid = None;
                        drop(containers);
                        state.persist_state();
                        Response::Stopped
                    }
                }
            }
        }
        Request::Delete(id) => {
            let mut containers = state.containers.write().unwrap();
            let container = containers.get(&id);

            match container {
                None => Response::Error(format!("Container '{}' not found", id)),
                Some(c) => {
                    if c.status == "Running" {
                        Response::Error(format!(
                            "Cannot delete running container '{}'. Stop it first.",
                            id
                        ))
                    } else {
                        // Try to delete container via libcrun if available
                        #[cfg(target_os = "linux")]
                        if state.libcrun_available {
                            if let Some(LibcrunContainer(container)) = c.libcrun_container {
                                if let Some(LibcrunContext(ctx)) = &state.libcrun_context {
                                    match crun::container_delete(*ctx, container, &id) {
                                        Ok(_) => {
                                            log::info!(
                                                "Container '{}' deleted successfully via libcrun",
                                                id
                                            );
                                        }
                                        Err(e) => {
                                            // Still remove from our state even if libcrun delete fails
                                            log::warn!("libcrun delete failed for container '{}': {}. Removing from internal state anyway.", id, e.message);
                                        }
                                    }
                                    // Free the container pointer
                                    crun::container_free(container);
                                }
                            }
                        }

                        // Clean up any container-specific state files
                        let container_state_dir = format!("{}/{}", STATE_DIR, id);
                        let _ = std::fs::remove_dir_all(&container_state_dir);

                        log::info!("Deleting container: {}", id);
                        containers.remove(&id);
                        drop(containers);
                        state.persist_state();
                        Response::Deleted
                    }
                }
            }
        }
        Request::List => {
            let containers = state.containers.read().unwrap();
            let list: Vec<ContainerInfoProto> = containers
                .values()
                .map(|c| ContainerInfoProto {
                    id: c.id.clone(),
                    status: c.status.clone(),
                    pid: c.pid,
                })
                .collect();

            Response::List(list)
        }
        Request::Metrics(id) => {
            let containers = state.containers.read().unwrap();
            match containers.get(&id) {
                Some(container) => {
                    let metrics = collect_container_metrics(&id, container.pid);
                    Response::Metrics(metrics)
                }
                None => Response::Error(format!("Container not found: {}", id)),
            }
        }
        Request::AllMetrics => {
            let containers = state.containers.read().unwrap();
            let metrics: Vec<ContainerMetricsProto> = containers
                .iter()
                .map(|(id, c)| collect_container_metrics(id, c.pid))
                .collect();
            Response::AllMetrics(metrics)
        }
        Request::Logs(req) => {
            let containers = state.containers.read().unwrap();
            if !containers.contains_key(&req.id) {
                return Response::Error(format!("Container not found: {}", req.id));
            }

            // Read logs from container log directory
            let log_dir = format!("/var/log/containers/{}", req.id);
            let stdout = read_log_file(&format!("{}/stdout.log", log_dir), req.tail);
            let stderr = read_log_file(&format!("{}/stderr.log", log_dir), req.tail);

            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            Response::Logs(libcrun_shim_proto::LogsProto {
                id: req.id,
                stdout,
                stderr,
                timestamp,
            })
        }
        Request::Health(id) => {
            let containers = state.containers.read().unwrap();
            match containers.get(&id) {
                Some(container) => {
                    // Basic health check based on container state
                    let status = if container.status == "running" {
                        "healthy"
                    } else if container.status == "created" {
                        "starting"
                    } else {
                        "none"
                    };

                    Response::Health(libcrun_shim_proto::HealthStatusProto {
                        id: id.clone(),
                        status: status.to_string(),
                        failing_streak: 0,
                        last_output: String::new(),
                        last_check: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0),
                    })
                }
                None => Response::Error(format!("Container not found: {}", id)),
            }
        }
        Request::Exec(req) => {
            let containers = state.containers.read().unwrap();
            let container = match containers.get(&req.id) {
                Some(c) => c,
                None => return Response::Error(format!("Container not found: {}", req.id)),
            };

            if container.status != "running" {
                return Response::Error(format!("Container '{}' is not running", req.id));
            }

            // Execute command using nsenter
            #[cfg(target_os = "linux")]
            if let Some(pid) = container.pid {
                let mut cmd = std::process::Command::new("nsenter");
                cmd.args(&["-t", &pid.to_string(), "-m", "-u", "-i", "-n", "-p", "--"]);
                cmd.args(&req.command);

                match cmd.output() {
                    Ok(output) => {
                        return Response::Exec(libcrun_shim_proto::ExecResultProto {
                            exit_code: output.status.code().unwrap_or(-1),
                            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                        });
                    }
                    Err(e) => {
                        return Response::Error(format!("Failed to execute command: {}", e));
                    }
                }
            }

            Response::Error("Container PID not available".to_string())
        }
    }
}

fn read_log_file(path: &str, tail: u32) -> String {
    if let Ok(content) = std::fs::read_to_string(path) {
        if tail > 0 {
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(tail as usize);
            lines[start..].join("\n")
        } else {
            content
        }
    } else {
        String::new()
    }
}

/// Collect metrics for a container from cgroups
#[allow(unused_variables)]
fn collect_container_metrics(id: &str, pid: Option<u32>) -> ContainerMetricsProto {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    #[allow(unused_mut)]
    let mut metrics = ContainerMetricsProto {
        id: id.to_string(),
        timestamp,
        ..Default::default()
    };

    #[cfg(target_os = "linux")]
    {
        if let Some(pid) = pid {
            // Try cgroup v2 first, then v1
            if let Some(cgroup_path) = find_cgroup_path(pid) {
                metrics.cpu = read_cpu_metrics(&cgroup_path);
                metrics.memory = read_memory_metrics(&cgroup_path);
                metrics.blkio = read_blkio_metrics(&cgroup_path);
                metrics.pids = read_pids_metrics(&cgroup_path);
            }
            // Network metrics from /proc/net
            metrics.network = read_network_metrics(pid);
        }
    }

    metrics
}

#[cfg(target_os = "linux")]
fn find_cgroup_path(pid: u32) -> Option<String> {
    // Try to find cgroup path from /proc/[pid]/cgroup
    let cgroup_file = format!("/proc/{}/cgroup", pid);
    if let Ok(content) = std::fs::read_to_string(&cgroup_file) {
        // cgroup v2: single line "0::/path"
        // cgroup v1: multiple lines "hierarchy:controllers:path"
        for line in content.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 {
                let path = parts[2];
                // cgroup v2
                if parts[0] == "0" && parts[1].is_empty() {
                    let cgroup_base = "/sys/fs/cgroup";
                    return Some(format!("{}{}", cgroup_base, path));
                }
            }
        }
        // Fallback for cgroup v1 - use first memory controller path
        for line in content.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 && parts[1].contains("memory") {
                return Some(format!("/sys/fs/cgroup/memory{}", parts[2]));
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn read_cpu_metrics(cgroup_path: &str) -> CpuMetricsProto {
    let mut cpu = CpuMetricsProto::default();

    // cgroup v2: cpu.stat
    let cpu_stat = format!("{}/cpu.stat", cgroup_path);
    if let Ok(content) = std::fs::read_to_string(&cpu_stat) {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let value = parts[1].parse().unwrap_or(0);
                match parts[0] {
                    "usage_usec" => cpu.usage_total = value * 1000, // Convert to ns
                    "user_usec" => cpu.usage_user = value * 1000,
                    "system_usec" => cpu.usage_system = value * 1000,
                    "nr_throttled" => cpu.throttled_periods = value,
                    "throttled_usec" => cpu.throttled_time = value * 1000,
                    _ => {}
                }
            }
        }
    }

    // cgroup v1 fallback: cpuacct.usage
    let cpuacct_usage = format!("{}/cpuacct.usage", cgroup_path);
    if cpu.usage_total == 0 {
        if let Ok(content) = std::fs::read_to_string(&cpuacct_usage) {
            cpu.usage_total = content.trim().parse().unwrap_or(0);
        }
    }

    cpu
}

#[cfg(target_os = "linux")]
fn read_memory_metrics(cgroup_path: &str) -> MemoryMetricsProto {
    let mut mem = MemoryMetricsProto::default();

    // cgroup v2: memory.current, memory.max, memory.stat
    if let Ok(content) = std::fs::read_to_string(format!("{}/memory.current", cgroup_path)) {
        mem.usage = content.trim().parse().unwrap_or(0);
    }
    if let Ok(content) = std::fs::read_to_string(format!("{}/memory.max", cgroup_path)) {
        mem.limit = content.trim().parse().unwrap_or(u64::MAX);
    }
    if let Ok(content) = std::fs::read_to_string(format!("{}/memory.peak", cgroup_path)) {
        mem.max_usage = content.trim().parse().unwrap_or(0);
    }
    if let Ok(content) = std::fs::read_to_string(format!("{}/memory.swap.current", cgroup_path)) {
        mem.swap = content.trim().parse().unwrap_or(0);
    }
    if let Ok(content) = std::fs::read_to_string(format!("{}/memory.stat", cgroup_path)) {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                match parts[0] {
                    "file" | "cache" => mem.cache = parts[1].parse().unwrap_or(0),
                    "anon" => mem.rss = parts[1].parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
    }

    // cgroup v1 fallback
    if mem.usage == 0 {
        if let Ok(content) =
            std::fs::read_to_string(format!("{}/memory.usage_in_bytes", cgroup_path))
        {
            mem.usage = content.trim().parse().unwrap_or(0);
        }
        if let Ok(content) =
            std::fs::read_to_string(format!("{}/memory.limit_in_bytes", cgroup_path))
        {
            mem.limit = content.trim().parse().unwrap_or(u64::MAX);
        }
        if let Ok(content) =
            std::fs::read_to_string(format!("{}/memory.max_usage_in_bytes", cgroup_path))
        {
            mem.max_usage = content.trim().parse().unwrap_or(0);
        }
    }

    // Calculate percentage
    if mem.limit > 0 && mem.limit != u64::MAX {
        mem.usage_percent = (mem.usage as f64 / mem.limit as f64) * 100.0;
    }

    mem
}

#[cfg(target_os = "linux")]
fn read_blkio_metrics(cgroup_path: &str) -> BlkioMetricsProto {
    let mut blkio = BlkioMetricsProto::default();

    // cgroup v2: io.stat
    if let Ok(content) = std::fs::read_to_string(format!("{}/io.stat", cgroup_path)) {
        for line in content.lines() {
            // Format: "major:minor rbytes=X wbytes=Y rios=Z wios=W"
            for part in line.split_whitespace() {
                if let Some(value) = part.strip_prefix("rbytes=") {
                    blkio.read_bytes += value.parse::<u64>().unwrap_or(0);
                } else if let Some(value) = part.strip_prefix("wbytes=") {
                    blkio.write_bytes += value.parse::<u64>().unwrap_or(0);
                } else if let Some(value) = part.strip_prefix("rios=") {
                    blkio.read_ops += value.parse::<u64>().unwrap_or(0);
                } else if let Some(value) = part.strip_prefix("wios=") {
                    blkio.write_ops += value.parse::<u64>().unwrap_or(0);
                }
            }
        }
    }

    // cgroup v1 fallback
    if blkio.read_bytes == 0 {
        if let Ok(content) =
            std::fs::read_to_string(format!("{}/blkio.throttle.io_service_bytes", cgroup_path))
        {
            for line in content.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let value: u64 = parts[2].parse().unwrap_or(0);
                    match parts[1] {
                        "Read" => blkio.read_bytes += value,
                        "Write" => blkio.write_bytes += value,
                        _ => {}
                    }
                }
            }
        }
    }

    blkio
}

#[cfg(target_os = "linux")]
fn read_pids_metrics(cgroup_path: &str) -> PidsMetricsProto {
    let mut pids = PidsMetricsProto::default();

    // cgroup v2
    if let Ok(content) = std::fs::read_to_string(format!("{}/pids.current", cgroup_path)) {
        pids.current = content.trim().parse().unwrap_or(0);
    }
    if let Ok(content) = std::fs::read_to_string(format!("{}/pids.max", cgroup_path)) {
        pids.limit = content.trim().parse().unwrap_or(0);
    }

    pids
}

#[cfg(target_os = "linux")]
fn read_network_metrics(pid: u32) -> NetworkMetricsProto {
    let mut net = NetworkMetricsProto::default();

    // Read from /proc/[pid]/net/dev
    let net_dev = format!("/proc/{}/net/dev", pid);
    if let Ok(content) = std::fs::read_to_string(&net_dev) {
        for line in content.lines().skip(2) {
            // Skip header lines
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 10 {
                let iface = parts[0].trim_end_matches(':');
                // Skip loopback
                if iface == "lo" {
                    continue;
                }
                net.rx_bytes += parts[1].parse::<u64>().unwrap_or(0);
                net.rx_packets += parts[2].parse::<u64>().unwrap_or(0);
                net.rx_errors += parts[3].parse::<u64>().unwrap_or(0);
                net.rx_dropped += parts[4].parse::<u64>().unwrap_or(0);
                net.tx_bytes += parts[9].parse::<u64>().unwrap_or(0);
                net.tx_packets += parts[10].parse::<u64>().unwrap_or(0);
                net.tx_errors += parts[11].parse::<u64>().unwrap_or(0);
                net.tx_dropped += parts[12].parse::<u64>().unwrap_or(0);
            }
        }
    }

    net
}
