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
    /// Create a new runtime with default configuration (from environment)
    pub async fn new() -> Result<Self> {
        Self::new_with_config(RuntimeConfig::from_env()).await
    }

    /// Create a new runtime with custom configuration
    pub async fn new_with_config(config: RuntimeConfig) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            let _ = config; // Linux doesn't use config yet
            return Ok(Self {
                inner: linux::LinuxRuntime::new()?,
            });
        }

        #[cfg(target_os = "macos")]
        return Ok(Self {
            inner: macos::MacOsRuntime::new_with_config(config).await?,
        });
    }

    /// Get the runtime configuration (macOS only)
    #[cfg(target_os = "macos")]
    pub fn config(&self) -> &RuntimeConfig {
        self.inner.config()
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

    /// Get metrics for a specific container
    pub async fn metrics(&self, id: &str) -> Result<ContainerMetrics> {
        self.inner.metrics(id).await
    }

    /// Get metrics for all containers
    pub async fn all_metrics(&self) -> Result<Vec<ContainerMetrics>> {
        self.inner.all_metrics().await
    }
}

#[cfg(target_os = "linux")]
trait RuntimeImpl {
    async fn create(&self, config: ContainerConfig) -> Result<String>;
    async fn start(&self, id: &str) -> Result<()>;
    async fn stop(&self, id: &str) -> Result<()>;
    async fn delete(&self, id: &str) -> Result<()>;
    async fn list(&self) -> Result<Vec<ContainerInfo>>;
    async fn metrics(&self, id: &str) -> Result<ContainerMetrics>;
    async fn all_metrics(&self) -> Result<Vec<ContainerMetrics>>;
}

#[cfg(target_os = "macos")]
trait RuntimeImpl {
    async fn create(&self, config: ContainerConfig) -> Result<String>;
    async fn start(&self, id: &str) -> Result<()>;
    async fn stop(&self, id: &str) -> Result<()>;
    async fn delete(&self, id: &str) -> Result<()>;
    async fn list(&self) -> Result<Vec<ContainerInfo>>;
    async fn metrics(&self, id: &str) -> Result<ContainerMetrics>;
    async fn all_metrics(&self) -> Result<Vec<ContainerMetrics>>;
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    #[cfg(target_os = "linux")]
    async fn test_create_and_list() {
        let runtime = ContainerRuntime::new().await.unwrap();
        
        // Create a temporary rootfs directory for testing
        let temp_rootfs = std::env::temp_dir().join(format!("test-rootfs-{}", std::process::id()));
        std::fs::create_dir_all(&temp_rootfs).unwrap();
        
        let config = ContainerConfig {
            id: "test-container".to_string(),
            rootfs: temp_rootfs.clone(),
            command: vec!["sh".to_string()],
            env: vec!["PATH=/usr/bin".to_string()],
            working_dir: "/".to_string(),
        };
        
        let id = runtime.create(config).await.unwrap();
        assert_eq!(id, "test-container");
        
        let containers = runtime.list().await.unwrap();
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].id, "test-container");
        
        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_rootfs);
    }

    #[tokio::test]
    #[cfg(target_os = "linux")]
    async fn test_container_lifecycle() {
        let runtime = ContainerRuntime::new().await.unwrap();
        
        // Create a temporary rootfs directory for testing
        let temp_rootfs = std::env::temp_dir().join(format!("test-lifecycle-{}", std::process::id()));
        std::fs::create_dir_all(&temp_rootfs).unwrap();
        
        let config = ContainerConfig {
            id: "test".to_string(),
            rootfs: temp_rootfs.clone(),
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
        
        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_rootfs);
    }
}

