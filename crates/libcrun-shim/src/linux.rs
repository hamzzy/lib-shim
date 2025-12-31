use crate::*;
use std::collections::HashMap;
use std::sync::RwLock;

pub struct LinuxRuntime {
    containers: RwLock<HashMap<String, ContainerInfo>>,
}

impl LinuxRuntime {
    pub fn new() -> Result<Self> {
        // In a real implementation, initialize libcrun via FFI
        // For MVP, use a simple in-memory store
        Ok(Self {
            containers: RwLock::new(HashMap::new()),
        })
    }
}

impl RuntimeImpl for LinuxRuntime {
    async fn create(&self, config: ContainerConfig) -> Result<String> {
        // TODO: Call libcrun_container_create via FFI
        // For now, just store the config
        let info = ContainerInfo {
            id: config.id.clone(),
            status: ContainerStatus::Created,
            pid: None,
        };
        
        self.containers.write().unwrap().insert(config.id.clone(), info);
        Ok(config.id)
    }
    
    async fn start(&self, id: &str) -> Result<()> {
        // TODO: Call libcrun_container_start via FFI
        let mut containers = self.containers.write().unwrap();
        let container = containers.get_mut(id)
            .ok_or_else(|| ShimError::NotFound(id.to_string()))?;
        
        container.status = ContainerStatus::Running;
        container.pid = Some(std::process::id());
        Ok(())
    }
    
    async fn stop(&self, id: &str) -> Result<()> {
        // TODO: Call libcrun_container_kill via FFI
        let mut containers = self.containers.write().unwrap();
        let container = containers.get_mut(id)
            .ok_or_else(|| ShimError::NotFound(id.to_string()))?;
        
        container.status = ContainerStatus::Stopped;
        container.pid = None;
        Ok(())
    }
    
    async fn delete(&self, id: &str) -> Result<()> {
        // TODO: Call libcrun_container_delete via FFI
        self.containers.write().unwrap().remove(id)
            .ok_or_else(|| ShimError::NotFound(id.to_string()))?;
        Ok(())
    }
    
    async fn list(&self) -> Result<Vec<ContainerInfo>> {
        Ok(self.containers.read().unwrap().values().cloned().collect())
    }
}

