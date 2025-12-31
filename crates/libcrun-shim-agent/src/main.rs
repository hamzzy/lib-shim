use libcrun_shim_proto::*;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
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

// Container state in the agent
struct ContainerState {
    id: String,
    rootfs: String,
    command: Vec<String>,
    env: Vec<String>,
    working_dir: String,
    status: String,
    pid: Option<u32>,
    #[cfg(target_os = "linux")]
    libcrun_container: Option<LibcrunContainer>,
}

// Shared state for the agent
struct AgentState {
    containers: RwLock<HashMap<String, ContainerState>>,
    #[cfg(target_os = "linux")]
    libcrun_context: Option<LibcrunContext>,
    #[cfg(target_os = "linux")]
    libcrun_available: bool,
}

impl AgentState {
    fn new() -> Self {
        #[cfg(target_os = "linux")]
        {
            // Try to initialize libcrun context
            let (context, available) = match crun::context_new() {
                Ok(ctx) => {
                    log::info!("libcrun initialized successfully in agent - using real container operations");
                    (Some(LibcrunContext(ctx)), true)
                }
                Err(e) => {
                    log::warn!("libcrun not available in agent: {}, using fallback mode", e.message);
                    (None, false)
                }
            };
            
            Self {
                containers: RwLock::new(HashMap::new()),
                libcrun_context: context,
                libcrun_available: available,
            }
        }
        
        #[cfg(not(target_os = "linux"))]
        {
            Self {
                containers: RwLock::new(HashMap::new()),
            }
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
            env_vec.push("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string());
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
        let mut rlimits = vec![
            serde_json::json!({
                "type": "RLIMIT_NOFILE",
                "hard": 1024,
                "soft": 1024
            }),
        ];
        
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
        
        serde_json::to_string_pretty(&oci_config)
            .map_err(|e| e.to_string())
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

fn main() {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();
    
    // Remove old socket if it exists
    let _ = std::fs::remove_file("/tmp/libcrun-shim.sock");
    
    // Listen on a Unix socket for RPC requests
    let listener = UnixListener::bind("/tmp/libcrun-shim.sock")
        .expect("Failed to bind to socket");
    
    log::info!("Agent listening on /tmp/libcrun-shim.sock");
    
    // Create shared state
    let state = Arc::new(AgentState::new());
    
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state_clone = Arc::clone(&state);
                std::thread::spawn(move || handle_client(stream, state_clone));
            }
            Err(e) => {
                log::error!("Connection error: {}", e);
            }
        }
    }
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
                                    log::info!("Container '{}' created successfully via libcrun", req.id);
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
                        log::warn!("libcrun container load failed: {}, using fallback mode", e.message);
                        None
                    }
                }
            } else {
                None
            };
            
            #[cfg(not(target_os = "linux"))]
            let libcrun_container: Option<*mut libcrun_sys::libcrun_container_t> = None;
            
            let container_state = ContainerState {
                id: req.id.clone(),
                rootfs: req.rootfs,
                command: req.command,
                env: req.env,
                working_dir: req.working_dir,
                status: "Created".to_string(),
                pid: None,
                #[cfg(target_os = "linux")]
                libcrun_container,
            };
            
            state.containers.write().unwrap().insert(req.id.clone(), container_state);
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
                        Response::Error(format!("Container '{}' is stopped and cannot be restarted", id))
                    } else {
                        // Try to start container via libcrun if available
                        #[cfg(target_os = "linux")]
                        if state.libcrun_available {
                            if let Some(LibcrunContainer(container)) = c.libcrun_container {
                                if let Some(LibcrunContext(ctx)) = &state.libcrun_context {
                                    match crun::container_start(*ctx, container, &id) {
                                        Ok(_) => {
                                            log::info!("Container '{}' started successfully via libcrun", id);
                                            // Try to get actual PID from container state
                                            c.pid = crun::get_container_pid(&id)
                                                .or_else(|| {
                                                    // Fallback: try to get from container state API
                                                    None
                                                });
                                            
                                            // If we still don't have a PID, use placeholder
                                            if c.pid.is_none() {
                                                log::warn!("Could not retrieve PID for container '{}' from libcrun state, using placeholder", id);
                                                c.pid = Some(std::process::id()); // Placeholder
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
                                    match crun::container_kill(*ctx, container, &id, libc::SIGTERM) {
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
                        Response::Error(format!("Cannot delete running container '{}'. Stop it first.", id))
                    } else {
                        // Try to delete container via libcrun if available
                        #[cfg(target_os = "linux")]
                        if state.libcrun_available {
                            if let Some(LibcrunContainer(container)) = c.libcrun_container {
                                if let Some(LibcrunContext(ctx)) = &state.libcrun_context {
                                    match crun::container_delete(*ctx, container, &id) {
                                        Ok(_) => {
                                            log::info!("Container '{}' deleted successfully via libcrun", id);
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
                        
                        log::info!("Deleting container: {}", id);
                        containers.remove(&id);
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
    }
}

/// Collect metrics for a container from cgroups
fn collect_container_metrics(id: &str, pid: Option<u32>) -> ContainerMetricsProto {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

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
        if let Ok(content) = std::fs::read_to_string(format!("{}/memory.usage_in_bytes", cgroup_path)) {
            mem.usage = content.trim().parse().unwrap_or(0);
        }
        if let Ok(content) = std::fs::read_to_string(format!("{}/memory.limit_in_bytes", cgroup_path)) {
            mem.limit = content.trim().parse().unwrap_or(u64::MAX);
        }
        if let Ok(content) = std::fs::read_to_string(format!("{}/memory.max_usage_in_bytes", cgroup_path)) {
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
        if let Ok(content) = std::fs::read_to_string(format!("{}/blkio.throttle.io_service_bytes", cgroup_path)) {
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

