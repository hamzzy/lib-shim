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
                println!("libcrun initialized successfully");
            } else {
                println!("libcrun not available, using in-memory container management");
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
    fn build_oci_config_json(config: &ContainerConfig) -> Result<String, serde_json::Error> {
        // Build a minimal OCI config JSON from our ContainerConfig
        // This is a simplified version - a real implementation would be more complete
        let oci_config = serde_json::json!({
            "ociVersion": "1.0.0",
            "process": {
                "terminal": false,
                "user": {
                    "uid": 0,
                    "gid": 0
                },
                "args": config.command,
                "env": config.env,
                "cwd": config.working_dir
            },
            "root": {
                "path": config.rootfs.display().to_string(),
                "readonly": false
            }
        });
        
        serde_json::to_string(&oci_config)
    }
    
    fn validate_config(config: &ContainerConfig) -> Result<()> {
        if config.id.is_empty() {
            return Err(ShimError::Runtime("Container ID cannot be empty".to_string()));
        }
        
        if config.command.is_empty() {
            return Err(ShimError::Runtime("Command cannot be empty".to_string()));
        }
        
        if !config.rootfs.exists() {
            return Err(ShimError::Runtime(format!(
                "Rootfs path does not exist: {}",
                config.rootfs.display()
            )));
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
                return Err(ShimError::Runtime(format!(
                    "Container '{}' already exists",
                    config.id
                )));
            }
        }
        
        // Try to use libcrun if available
        #[cfg(target_os = "linux")]
        let libcrun_container = if self.libcrun_available {
            // Build OCI config JSON
            let oci_json = match Self::build_oci_config_json(&config) {
                Ok(json) => json,
                Err(e) => {
                    return Err(ShimError::Runtime(format!(
                        "Failed to build OCI config: {}",
                        e
                    )));
                }
            };
            
            // Load container from JSON config
            match crun::container_load_from_memory(&oci_json) {
                Ok(container) => {
                    // Create the container using libcrun
                    if let Some(ctx) = self.libcrun_context {
                        match crun::container_create(ctx, container, &config.id) {
                            Ok(_) => {
                                println!("Container '{}' created via libcrun", config.id);
                                Some(container)
                            }
                            Err(e) => {
                                crun::container_free(container);
                                return Err(ShimError::Runtime(format!(
                                    "libcrun failed to create container: {}",
                                    e.message
                                )));
                            }
                        }
                    } else {
                        crun::container_free(container);
                        None
                    }
                }
                Err(e) => {
                    // Fall back to in-memory if libcrun fails
                    println!("libcrun container load failed: {}, using fallback", e.message);
                    None
                }
            }
        } else {
            None
        };
        
        #[cfg(not(target_os = "linux"))]
        let libcrun_container = None;
        
        // Store the container state
        let info = ContainerInfo {
            id: config.id.clone(),
            status: ContainerStatus::Created,
            pid: None,
        };
        
        let state = ContainerState {
            config,
            info,
            #[cfg(target_os = "linux")]
            libcrun_container,
        };
        
        self.containers.write().unwrap().insert(state.info.id.clone(), state);
        Ok(info.id)
    }
    
    async fn start(&self, id: &str) -> Result<()> {
        let mut containers = self.containers.write().unwrap();
        let state = containers.get_mut(id)
            .ok_or_else(|| ShimError::NotFound(id.to_string()))?;
        
        // Check if container is in a valid state to start
        match state.info.status {
            ContainerStatus::Running => {
                return Err(ShimError::Runtime(format!(
                    "Container '{}' is already running",
                    id
                )));
            }
            ContainerStatus::Stopped => {
                return Err(ShimError::Runtime(format!(
                    "Container '{}' is stopped and cannot be restarted (delete and recreate)",
                    id
                )));
            }
            ContainerStatus::Created => {
                // Valid state to start
            }
        }
        
        // Try to start container via libcrun if available
        #[cfg(target_os = "linux")]
        if self.libcrun_available {
            if let Some(container) = state.libcrun_container {
                if let Some(ctx) = self.libcrun_context {
                    match crun::container_start(ctx, container, id) {
                        Ok(_) => {
                            println!("Container '{}' started via libcrun", id);
                            // TODO: Get actual PID from container state
                            state.info.pid = Some(std::process::id()); // Placeholder
                        }
                        Err(e) => {
                            return Err(ShimError::Runtime(format!(
                                "libcrun failed to start container: {}",
                                e.message
                            )));
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
        let mut containers = self.containers.write().unwrap();
        let state = containers.get_mut(id)
            .ok_or_else(|| ShimError::NotFound(id.to_string()))?;
        
        // Check if container is running
        if state.info.status != ContainerStatus::Running {
            return Err(ShimError::Runtime(format!(
                "Container '{}' is not running (status: {:?})",
                id, state.info.status
            )));
        }
        
        // Try to stop container via libcrun if available
        #[cfg(target_os = "linux")]
        if self.libcrun_available {
            if let Some(container) = state.libcrun_container {
                if let Some(ctx) = self.libcrun_context {
                    // Use SIGTERM to stop gracefully
                    match crun::container_kill(ctx, container, id, libc::SIGTERM) {
                        Ok(_) => {
                            println!("Container '{}' stopped via libcrun", id);
                        }
                        Err(e) => {
                            return Err(ShimError::Runtime(format!(
                                "libcrun failed to stop container: {}",
                                e.message
                            )));
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
        let mut containers = self.containers.write().unwrap();
        let state = containers.get(id)
            .ok_or_else(|| ShimError::NotFound(id.to_string()))?;
        
        // Check if container is stopped
        if state.info.status == ContainerStatus::Running {
            return Err(ShimError::Runtime(format!(
                "Cannot delete running container '{}'. Stop it first.",
                id
            )));
        }
        
        // Try to delete container via libcrun if available
        #[cfg(target_os = "linux")]
        if self.libcrun_available {
            if let Some(container) = state.libcrun_container {
                if let Some(ctx) = self.libcrun_context {
                    match crun::container_delete(ctx, container, id) {
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
        
        containers.remove(id);
        Ok(())
    }
    
    async fn list(&self) -> Result<Vec<ContainerInfo>> {
        let containers = self.containers.read().unwrap();
        Ok(containers.values().map(|state| state.info.clone()).collect())
    }
}

