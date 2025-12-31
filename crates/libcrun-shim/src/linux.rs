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
}

pub struct LinuxRuntime {
    containers: RwLock<HashMap<String, ContainerState>>,
}

impl LinuxRuntime {
    pub fn new() -> Result<Self> {
        // In a real implementation, initialize libcrun runtime via FFI
        // For now, use a simple in-memory store
        Ok(Self {
            containers: RwLock::new(HashMap::new()),
        })
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
        
        // TODO: Call libcrun_container_create via FFI
        // For now, just store the config
        // In a real implementation:
        //   let container = crun::container_create(...)?;
        //   crun::container_create(&container, &config.id)?;
        
        let info = ContainerInfo {
            id: config.id.clone(),
            status: ContainerStatus::Created,
            pid: None,
        };
        
        let state = ContainerState {
            config,
            info,
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
        
        // TODO: Call libcrun_container_start via FFI
        // In a real implementation:
        //   let container = get_container(id)?;
        //   crun::container_start(&container, id)?;
        
        state.info.status = ContainerStatus::Running;
        // In a real implementation, get the actual PID from libcrun
        state.info.pid = Some(std::process::id()); // Placeholder
        
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
        
        // TODO: Call libcrun_container_kill via FFI
        // In a real implementation:
        //   let container = get_container(id)?;
        //   crun::container_kill(&container, id, libc::SIGTERM)?;
        
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
        
        // TODO: Call libcrun_container_delete via FFI
        // In a real implementation:
        //   let container = get_container(id)?;
        //   crun::container_delete(&container, id)?;
        
        containers.remove(id);
        Ok(())
    }
    
    async fn list(&self) -> Result<Vec<ContainerInfo>> {
        let containers = self.containers.read().unwrap();
        Ok(containers.values().map(|state| state.info.clone()).collect())
    }
}

