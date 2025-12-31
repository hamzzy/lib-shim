mod types;
mod error;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

pub use types::*;
pub use error::*;

pub struct ContainerRuntime {
    #[cfg(target_os = "linux")]
    inner: linux::LinuxRuntime,
    
    #[cfg(target_os = "macos")]
    inner: macos::MacOsRuntime,
}

impl ContainerRuntime {
    pub async fn new() -> Result<Self> {
        #[cfg(target_os = "linux")]
        return Ok(Self {
            inner: linux::LinuxRuntime::new()?,
        });
        
        #[cfg(target_os = "macos")]
        return Ok(Self {
            inner: macos::MacOsRuntime::new().await?,
        });
    }
    
    pub async fn create(&self, config: ContainerConfig) -> Result<String> {
        self.inner.create(config).await
    }
    
    pub async fn start(&self, id: &str) -> Result<()> {
        self.inner.start(id).await
    }
    
    pub async fn stop(&self, id: &str) -> Result<()> {
        self.inner.stop(id).await
    }
    
    pub async fn delete(&self, id: &str) -> Result<()> {
        self.inner.delete(id).await
    }
    
    pub async fn list(&self) -> Result<Vec<ContainerInfo>> {
        self.inner.list().await
    }
}

#[cfg(target_os = "linux")]
trait RuntimeImpl {
    async fn create(&self, config: ContainerConfig) -> Result<String>;
    async fn start(&self, id: &str) -> Result<()>;
    async fn stop(&self, id: &str) -> Result<()>;
    async fn delete(&self, id: &str) -> Result<()>;
    async fn list(&self) -> Result<Vec<ContainerInfo>>;
}

#[cfg(target_os = "macos")]
trait RuntimeImpl {
    async fn create(&self, config: ContainerConfig) -> Result<String>;
    async fn start(&self, id: &str) -> Result<()>;
    async fn stop(&self, id: &str) -> Result<()>;
    async fn delete(&self, id: &str) -> Result<()>;
    async fn list(&self) -> Result<Vec<ContainerInfo>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(target_os = "linux")]
    async fn test_create_and_list() {
        let runtime = ContainerRuntime::new().await.unwrap();
        
        let config = ContainerConfig {
            id: "test-container".to_string(),
            rootfs: "/tmp/rootfs".into(),
            command: vec!["sh".to_string()],
            env: vec!["PATH=/usr/bin".to_string()],
            working_dir: "/".to_string(),
        };
        
        let id = runtime.create(config).await.unwrap();
        assert_eq!(id, "test-container");
        
        let containers = runtime.list().await.unwrap();
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].id, "test-container");
    }

    #[tokio::test]
    #[cfg(target_os = "linux")]
    async fn test_container_lifecycle() {
        let runtime = ContainerRuntime::new().await.unwrap();
        
        let config = ContainerConfig {
            id: "test".to_string(),
            rootfs: "/tmp/rootfs".into(),
            command: vec!["sleep".to_string(), "10".to_string()],
            env: vec![],
            working_dir: "/".to_string(),
        };
        
        // Create
        runtime.create(config).await.unwrap();
        
        // Start
        runtime.start("test").await.unwrap();
        
        // List
        let containers = runtime.list().await.unwrap();
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].status, ContainerStatus::Running);
        
        // Stop
        runtime.stop("test").await.unwrap();
        
        // Delete
        runtime.delete("test").await.unwrap();
        
        // List should be empty
        let containers = runtime.list().await.unwrap();
        assert_eq!(containers.len(), 0);
    }
}

