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
                    println!("libcrun initialized in agent");
                    (Some(LibcrunContext(ctx)), true)
                }
                Err(e) => {
                    println!("libcrun not available in agent: {}, using fallback", e.message);
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
    ) -> Result<String, String> {
        // Ensure PATH is in env if not provided
        let mut env_vec = env.to_vec();
        let has_path = env_vec.iter().any(|e| e.starts_with("PATH="));
        if !has_path {
            env_vec.push("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string());
        }
        
        let oci_config = serde_json::json!({
            "ociVersion": "1.0.0",
            "process": {
                "terminal": false,
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
                "rlimits": [
                    {
                        "type": "RLIMIT_NOFILE",
                        "hard": 1024,
                        "soft": 1024
                    }
                ],
                "noNewPrivileges": true
            },
            "root": {
                "path": rootfs,
                "readonly": false
            },
            "hostname": container_id,
            "mounts": [
                {
                    "destination": "/proc",
                    "type": "proc",
                    "source": "proc"
                },
                {
                    "destination": "/dev",
                    "type": "tmpfs",
                    "source": "tmpfs",
                    "options": ["nosuid", "strictatime", "mode=755", "size=65536k"]
                },
                {
                    "destination": "/dev/pts",
                    "type": "devpts",
                    "source": "devpts",
                    "options": ["nosuid", "noexec", "newinstance", "ptmxmode=0666", "mode=0620"]
                },
                {
                    "destination": "/dev/shm",
                    "type": "tmpfs",
                    "source": "shm",
                    "options": ["nosuid", "noexec", "nodev", "mode=1777", "size=65536k"]
                },
                {
                    "destination": "/sys",
                    "type": "sysfs",
                    "source": "sysfs",
                    "options": ["nosuid", "noexec", "nodev", "ro"]
                }
            ],
            "linux": {
                "resources": {
                    "devices": [
                        {
                            "allow": false,
                            "access": "rwm"
                        }
                    ]
                },
                "namespaces": [
                    {
                        "type": "pid"
                    },
                    {
                        "type": "network"
                    },
                    {
                        "type": "ipc"
                    },
                    {
                        "type": "uts"
                    },
                    {
                        "type": "mount"
                    }
                ],
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
    // Remove old socket if it exists
    let _ = std::fs::remove_file("/tmp/libcrun-shim.sock");
    
    // Listen on a Unix socket for RPC requests
    let listener = UnixListener::bind("/tmp/libcrun-shim.sock")
        .expect("Failed to bind to socket");
    
    println!("Agent listening on /tmp/libcrun-shim.sock");
    
    // Create shared state
    let state = Arc::new(AgentState::new());
    
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state_clone = Arc::clone(&state);
                std::thread::spawn(move || handle_client(stream, state_clone));
            }
            Err(e) => {
                eprintln!("Connection error: {}", e);
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
                        let response = Response::Error(format!("Parse error: {}", e));
                        let _ = stream.write_all(&serialize_response(&response));
                        continue;
                    }
                };
                
                let response = handle_request(request, &state);
                if let Err(e) = stream.write_all(&serialize_response(&response)) {
                    eprintln!("Write error: {}", e);
                    break;
                }
            }
            Err(e) => {
                eprintln!("Read error: {}", e);
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
            
            println!("Creating container: {}", req.id);
            
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
                                    println!("Container '{}' created via libcrun", req.id);
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
                        println!("libcrun container load failed: {}, using fallback", e.message);
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
                                            println!("Container '{}' started via libcrun", id);
                                            // Try to get actual PID from container state
                                            c.pid = crun::get_container_pid(&id)
                                                .or_else(|| {
                                                    // Fallback: try to get from container state API
                                                    None
                                                });
                                            
                                            // If we still don't have a PID, use placeholder
                                            if c.pid.is_none() {
                                                println!("Warning: Could not get PID for container '{}', using placeholder", id);
                                                c.pid = Some(std::process::id()); // Placeholder
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
                            println!("Starting container: {} (fallback mode)", id);
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
                                            println!("Container '{}' stopped via libcrun", id);
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
                        
                        println!("Stopping container: {}", id);
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
                                            println!("Container '{}' deleted via libcrun", id);
                                        }
                                        Err(e) => {
                                            // Still remove from our state even if libcrun delete fails
                                            println!("Warning: libcrun delete failed: {}", e.message);
                                        }
                                    }
                                    // Free the container pointer
                                    crun::container_free(container);
                                }
                            }
                        }
                        
                        println!("Deleting container: {}", id);
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
    }
}

