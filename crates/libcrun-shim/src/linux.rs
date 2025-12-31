use crate::*;
use std::collections::HashMap;
use std::sync::RwLock;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use libcrun_sys::safe as crun;

// Internal container state that includes the config
struct ContainerState {
    config: ContainerConfig,
    info: ContainerInfo,
    #[cfg(target_os = "linux")]
    libcrun_container: Option<*mut libcrun_sys::libcrun_container_t>,
}

pub struct LinuxRuntime {
    containers: RwLock<HashMap<String, ContainerState>>,
    #[cfg(target_os = "linux")]
    libcrun_context: Option<*mut libcrun_sys::libcrun_context_t>,
    #[cfg(target_os = "linux")]
    libcrun_available: bool,
}

impl Drop for LinuxRuntime {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        {
            // Clean up libcrun context
            if let Some(ctx) = self.libcrun_context.take() {
                crun::context_free(ctx);
            }
            
            // Clean up any remaining containers
            let containers = self.containers.write().unwrap();
            for (_, state) in containers.iter() {
                #[cfg(target_os = "linux")]
                if let Some(container) = state.libcrun_container {
                    crun::container_free(container);
                }
            }
        }
    }
}

impl LinuxRuntime {
    pub fn new() -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            // Try to initialize libcrun context
            let (context, available) = match crun::context_new() {
                Ok(ctx) => (Some(ctx), true),
                Err(_) => {
                    // libcrun not available or failed to initialize
                    // Will use in-memory fallback
                    (None, false)
                }
            };
            
            if available {
                log::info!("libcrun initialized successfully - using real container operations");
            } else {
                log::warn!("libcrun not available, using in-memory container management (fallback mode)");
            }
            
            Ok(Self {
                containers: RwLock::new(HashMap::new()),
                libcrun_context: context,
                libcrun_available: available,
            })
        }
        
        #[cfg(not(target_os = "linux"))]
        {
            Ok(Self {
                containers: RwLock::new(HashMap::new()),
            })
        }
    }
    
    #[cfg(target_os = "linux")]
    fn build_oci_config_json(config: &ContainerConfig) -> Result<String> {
        // Build a complete OCI config JSON from our ContainerConfig
        // Following OCI Runtime Specification v1.0.0
        
        // Ensure PATH is in env if not provided
        let mut env = config.env.clone();
        let has_path = env.iter().any(|e| e.starts_with("PATH="));
        if !has_path {
            env.push("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_string());
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
                "destination": "/dev/mqueue",
                "type": "mqueue",
                "source": "mqueue",
                "options": ["nosuid", "noexec", "nodev"]
            }),
            serde_json::json!({
                "destination": "/sys",
                "type": "sysfs",
                "source": "sysfs",
                "options": ["nosuid", "noexec", "nodev", "ro"]
            }),
            serde_json::json!({
                "destination": "/sys/fs/cgroup",
                "type": "cgroup",
                "source": "cgroup",
                "options": ["nosuid", "noexec", "nodev", "relatime", "ro"]
            }),
        ];
        
        // Add user-defined volume mounts
        for volume in &config.volumes {
            let mut mount = serde_json::json!({
                "destination": volume.destination.display().to_string(),
                "type": "bind",
                "source": volume.source.display().to_string(),
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
        if let Some(memory) = config.resources.memory {
            if memory > 0 {
                rlimits.push(serde_json::json!({
                    "type": "RLIMIT_AS",
                    "hard": memory,
                    "soft": memory
                }));
            }
        }
        
        if let Some(pids) = config.resources.pids {
            if pids > 0 {
                rlimits.push(serde_json::json!({
                    "type": "RLIMIT_NPROC",
                    "hard": pids,
                    "soft": pids
                }));
            }
        }
        
        // Build resources object
        let mut resources = serde_json::json!({
            "devices": [
                {
                    "allow": false,
                    "access": "rwm"
                }
            ]
        });
        
        // Add CPU and memory limits
        if config.resources.cpu.is_some() || config.resources.memory.is_some() {
            let mut cpu_obj = serde_json::json!({});
            if let Some(cpu) = config.resources.cpu {
                if cpu > 0.0 {
                    cpu_obj["shares"] = serde_json::json!((cpu * 1024.0) as u64);
                    cpu_obj["quota"] = serde_json::json!((cpu * 100000.0) as i64);
                    cpu_obj["period"] = serde_json::json!(100000);
                }
            }
            
            let mut memory_obj = serde_json::json!({});
            if let Some(memory) = config.resources.memory {
                if memory > 0 {
                    memory_obj["limit"] = serde_json::json!(memory);
                }
            }
            if let Some(memory_swap) = config.resources.memory_swap {
                if memory_swap > 0 {
                    memory_obj["swap"] = serde_json::json!(memory_swap);
                }
            }
            
            if !cpu_obj.as_object().unwrap().is_empty() {
                resources["cpu"] = cpu_obj;
            }
            if !memory_obj.as_object().unwrap().is_empty() {
                resources["memory"] = memory_obj;
            }
        }
        
        // Determine network namespace based on network mode
        let network_namespace = match config.network.mode.as_str() {
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
                "terminal": config.stdio.tty,
                "user": {
                    "uid": 0,
                    "gid": 0
                },
                "args": config.command,
                "env": env,
                "cwd": config.working_dir,
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
                "path": config.rootfs.display().to_string(),
                "readonly": false
            },
            "hostname": config.id.clone(),
            "mounts": mounts,
            "linux": {
                "resources": resources,
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
            .map_err(|e| ShimError::Serialization(e.to_string()))
    }
    
    fn validate_config(config: &ContainerConfig) -> Result<()> {
        if config.id.is_empty() {
            return Err(ShimError::validation(
                "id",
                "Container ID cannot be empty"
            ));
        }
        
        if config.command.is_empty() {
            return Err(ShimError::validation(
                "command",
                "Command cannot be empty - container must have at least one command to execute"
            ));
        }
        
        if !config.rootfs.exists() {
            return Err(ShimError::runtime_with_context(
                format!("Rootfs path does not exist: {}", config.rootfs.display()),
                format!("Container ID: {}", config.id)
            ));
        }
        
        if !config.rootfs.is_dir() {
            return Err(ShimError::runtime_with_context(
                format!("Rootfs path is not a directory: {}", config.rootfs.display()),
                format!("Container ID: {}", config.id)
            ));
        }
        
        Ok(())
    }
}

impl RuntimeImpl for LinuxRuntime {
    async fn create(&self, config: ContainerConfig) -> Result<String> {
        // Validate the configuration
        Self::validate_config(&config)?;
        
        // Check if container already exists
        {
            let containers = self.containers.read().unwrap();
            if containers.contains_key(&config.id) {
                return Err(ShimError::runtime_with_context(
                    format!("Container '{}' already exists", config.id),
                    "Use a different container ID or delete the existing container first"
                ));
            }
        }
        
        log::debug!("Creating container: id={}, rootfs={}", 
            config.id, 
            config.rootfs.display()
        );
        
        // Try to use libcrun if available
        #[cfg(target_os = "linux")]
        let libcrun_container = if self.libcrun_available {
            // Build OCI config JSON
            let oci_json = match Self::build_oci_config_json(&config) {
                Ok(json) => {
                    log::debug!("Generated OCI config for container '{}'", config.id);
                    json
                }
                Err(e) => {
                    return Err(e);
                }
            };
            
            // Load container from JSON config
            match crun::container_load_from_memory(&oci_json) {
                Ok(container) => {
                    // Create the container using libcrun
                    if let Some(ctx) = self.libcrun_context {
                        match crun::container_create(ctx, container, &config.id) {
                            Ok(_) => {
                                log::info!("Container '{}' created successfully via libcrun", config.id);
                                Some(container)
                            }
                                Err(e) => {
                                    crun::container_free(container);
                                    return Err(ShimError::runtime_with_context(
                                        format!("libcrun failed to create container: {}", e.message),
                                        format!("Container ID: {}, Rootfs: {}", config.id, config.rootfs.display())
                                    ));
                                }
                        }
                    } else {
                        crun::container_free(container);
                        None
                    }
                }
                Err(e) => {
                    // Fall back to in-memory if libcrun fails
                    log::warn!("libcrun container load failed: {}, using fallback mode", e.message);
                    None
                }
            }
        } else {
            None
        };
        
        #[cfg(not(target_os = "linux"))]
        let libcrun_container = None;
        
        // Store the container state
        let container_id = config.id.clone();
        let info = ContainerInfo {
            id: container_id.clone(),
            status: ContainerStatus::Created,
            pid: None,
        };
        
        let state = ContainerState {
            config,
            info,
            #[cfg(target_os = "linux")]
            libcrun_container,
        };
        
        self.containers.write().unwrap().insert(container_id.clone(), state);
        Ok(container_id)
    }
    
    async fn start(&self, id: &str) -> Result<()> {
        log::debug!("Starting container: {}", id);
        
        let mut containers = self.containers.write().unwrap();
        let state = containers.get_mut(id)
            .ok_or_else(|| ShimError::not_found(format!("Container '{}'", id)))?;
        
        // Check if container is in a valid state to start
        match state.info.status {
            ContainerStatus::Running => {
                return Err(ShimError::runtime_with_context(
                    format!("Container '{}' is already running", id),
                    "Stop the container first if you want to restart it"
                ));
            }
            ContainerStatus::Stopped => {
                return Err(ShimError::runtime_with_context(
                    format!("Container '{}' is stopped and cannot be restarted", id),
                    "Delete the container and create a new one to restart"
                ));
            }
            ContainerStatus::Created => {
                // Valid state to start
                log::debug!("Container '{}' is in Created state, proceeding with start", id);
            }
        }
        
        // Try to start container via libcrun if available
        #[cfg(target_os = "linux")]
        if self.libcrun_available {
            if let Some(container) = state.libcrun_container {
                if let Some(ctx) = self.libcrun_context {
                                    match crun::container_start(ctx, container, id) {
                        Ok(_) => {
                            log::info!("Container '{}' started successfully via libcrun", id);
                            // Try to get actual PID from container state
                            state.info.pid = crun::get_container_pid(id)
                                .or_else(|| {
                                    // Fallback: try to get from container state API
                                    // Note: This would require parsing state, for now use filesystem method
                                    None
                                });
                            
                            // If we still don't have a PID, use placeholder
                            if state.info.pid.is_none() {
                                log::warn!("Could not retrieve PID for container '{}' from libcrun state, using placeholder", id);
                                state.info.pid = Some(std::process::id()); // Placeholder
                            } else {
                                log::debug!("Container '{}' PID: {:?}", id, state.info.pid);
                            }
                        }
                        Err(e) => {
                            return Err(ShimError::runtime_with_context(
                                format!("libcrun failed to start container: {}", e.message),
                                format!("Container ID: {}", id)
                            ));
                        }
                    }
                }
            }
        }
        
        state.info.status = ContainerStatus::Running;
        // If not using libcrun, use placeholder PID
        #[cfg(target_os = "linux")]
        if !self.libcrun_available || state.libcrun_container.is_none() {
            state.info.pid = Some(std::process::id()); // Placeholder
        }
        
        #[cfg(not(target_os = "linux"))]
        {
            state.info.pid = Some(std::process::id()); // Placeholder
        }
        
        Ok(())
    }
    
    async fn stop(&self, id: &str) -> Result<()> {
        log::debug!("Stopping container: {}", id);
        
        let mut containers = self.containers.write().unwrap();
        let state = containers.get_mut(id)
            .ok_or_else(|| ShimError::not_found(format!("Container '{}'", id)))?;
        
        // Check if container is running
        if state.info.status != ContainerStatus::Running {
            return Err(ShimError::runtime_with_context(
                format!("Container '{}' is not running", id),
                format!("Current status: {:?}. Only running containers can be stopped.", state.info.status)
            ));
        }
        
        // Try to stop container via libcrun if available
        #[cfg(target_os = "linux")]
        if self.libcrun_available {
            if let Some(container) = state.libcrun_container {
                if let Some(ctx) = self.libcrun_context {
                    // Use SIGTERM to stop gracefully
                                    match crun::container_kill(ctx, container, id, libc::SIGTERM) {
                        Ok(_) => {
                            log::info!("Container '{}' stopped successfully via libcrun (SIGTERM)", id);
                        }
                        Err(e) => {
                            return Err(ShimError::runtime_with_context(
                                format!("libcrun failed to stop container: {}", e.message),
                                format!("Container ID: {}, Signal: SIGTERM", id)
                            ));
                        }
                    }
                }
            }
        }
        
        state.info.status = ContainerStatus::Stopped;
        state.info.pid = None;
        
        Ok(())
    }
    
    async fn delete(&self, id: &str) -> Result<()> {
        log::debug!("Deleting container: {}", id);
        
        let mut containers = self.containers.write().unwrap();
        let state = containers.get(id)
            .ok_or_else(|| ShimError::not_found(format!("Container '{}'", id)))?;
        
        // Check if container is stopped
        if state.info.status == ContainerStatus::Running {
            return Err(ShimError::runtime_with_context(
                format!("Cannot delete running container '{}'", id),
                "Stop the container first using stop() before deleting it"
            ));
        }
        
        // Try to delete container via libcrun if available
        #[cfg(target_os = "linux")]
        if self.libcrun_available {
            if let Some(container) = state.libcrun_container {
                if let Some(ctx) = self.libcrun_context {
                                    match crun::container_delete(ctx, container, id) {
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
        
        containers.remove(id);
        Ok(())
    }
    
    async fn list(&self) -> Result<Vec<ContainerInfo>> {
        let containers = self.containers.read().unwrap();
        Ok(containers.values().map(|state| state.info.clone()).collect())
    }
}

