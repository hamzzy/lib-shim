//! Containerd Shim v2 Interface
//!
//! This module implements the containerd shim v2 protocol for integration
//! with containerd as an OCI runtime.
//!
//! Reference: https://github.com/containerd/containerd/blob/main/runtime/v2/README.md

use crate::error::{Result, ShimError};
use crate::types::ContainerConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Shim v2 task service interface
pub trait TaskService {
    /// Get the state of a container
    fn state(&self, container_id: &str, exec_id: Option<&str>) -> Result<StateResponse>;

    /// Create a new container
    fn create(&self, request: CreateTaskRequest) -> Result<CreateTaskResponse>;

    /// Start a container or exec
    fn start(&self, container_id: &str, exec_id: Option<&str>) -> Result<StartResponse>;

    /// Delete a container or exec
    fn delete(&self, container_id: &str, exec_id: Option<&str>) -> Result<DeleteResponse>;

    /// Pids returns all pids inside a container
    fn pids(&self, container_id: &str) -> Result<PidsResponse>;

    /// Pause a container
    fn pause(&self, container_id: &str) -> Result<()>;

    /// Resume a paused container
    fn resume(&self, container_id: &str) -> Result<()>;

    /// Checkpoint a container
    fn checkpoint(&self, container_id: &str, options: CheckpointOptions) -> Result<()>;

    /// Kill a container or exec with signal
    fn kill(&self, container_id: &str, exec_id: Option<&str>, signal: u32, all: bool) -> Result<()>;

    /// Exec an additional process inside the container
    fn exec(&self, request: ExecProcessRequest) -> Result<()>;

    /// ResizePty resizes the pty of a container or exec
    fn resize_pty(&self, container_id: &str, exec_id: Option<&str>, width: u32, height: u32)
        -> Result<()>;

    /// CloseIO closes the io pipe for a container or exec
    fn close_io(&self, container_id: &str, exec_id: Option<&str>, stdin: bool) -> Result<()>;

    /// Update container resource limits
    fn update(&self, container_id: &str, resources: Resources) -> Result<()>;

    /// Wait for a container or exec to exit
    fn wait(&self, container_id: &str, exec_id: Option<&str>) -> Result<WaitResponse>;

    /// Stats returns metrics/stats for a container
    fn stats(&self, container_id: &str) -> Result<StatsResponse>;

    /// Connect connects to the running task
    fn connect(&self, container_id: &str) -> Result<ConnectResponse>;

    /// Shutdown shuts down the shim
    fn shutdown(&self) -> Result<()>;
}

/// State of a container or exec
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateResponse {
    pub id: String,
    pub bundle: String,
    pub pid: u32,
    pub status: Status,
    pub stdin: String,
    pub stdout: String,
    pub stderr: String,
    pub terminal: bool,
    pub exit_status: u32,
    pub exited_at: u64,
}

/// Container status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Status {
    Unknown,
    Created,
    Running,
    Stopped,
    Paused,
    Pausing,
}

impl From<crate::types::ContainerStatus> for Status {
    fn from(s: crate::types::ContainerStatus) -> Self {
        match s {
            crate::types::ContainerStatus::Created => Status::Created,
            crate::types::ContainerStatus::Running => Status::Running,
            crate::types::ContainerStatus::Stopped => Status::Stopped,
        }
    }
}

/// Create task request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub id: String,
    pub bundle: PathBuf,
    pub rootfs: Vec<Mount>,
    pub terminal: bool,
    pub stdin: String,
    pub stdout: String,
    pub stderr: String,
    pub checkpoint: Option<String>,
    pub parent_checkpoint: Option<String>,
    pub options: Option<CreateOptions>,
}

/// Mount definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mount {
    pub mount_type: String,
    pub source: String,
    pub target: String,
    pub options: Vec<String>,
}

/// Create options
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateOptions {
    pub no_pivot_root: bool,
    pub open_tcp: bool,
    pub external_unix_sockets: bool,
    pub terminal: bool,
    pub file_locks: bool,
    pub empty_namespaces: Vec<String>,
    pub cgroups_mode: String,
    pub no_new_keyring: bool,
    pub shim_cgroup: String,
    pub io_uid: u32,
    pub io_gid: u32,
    pub criu_work_path: String,
    pub criu_image_path: String,
}

/// Create task response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskResponse {
    pub pid: u32,
}

/// Start response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartResponse {
    pub pid: u32,
}

/// Delete response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteResponse {
    pub pid: u32,
    pub exit_status: u32,
    pub exited_at: u64,
}

/// Pids response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PidsResponse {
    pub processes: Vec<ProcessInfo>,
}

/// Process info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub info: Option<serde_json::Value>,
}

/// Checkpoint options
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CheckpointOptions {
    pub exit: bool,
    pub open_tcp: bool,
    pub external_unix_sockets: bool,
    pub terminal: bool,
    pub file_locks: bool,
    pub empty_namespaces: Vec<String>,
    pub cgroups_mode: String,
    pub work_path: String,
    pub image_path: String,
}

/// Exec process request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecProcessRequest {
    pub container_id: String,
    pub exec_id: String,
    pub terminal: bool,
    pub stdin: String,
    pub stdout: String,
    pub stderr: String,
    pub spec: serde_json::Value,
}

/// Resource constraints
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Resources {
    pub memory: Option<MemoryResources>,
    pub cpu: Option<CpuResources>,
    pub pids: Option<PidsResources>,
    pub io: Option<IoResources>,
}

/// Memory resources
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryResources {
    pub limit: i64,
    pub swap: i64,
    pub kernel: i64,
    pub kernel_tcp: i64,
    pub reservation: i64,
    pub disable_oom_killer: bool,
}

/// CPU resources
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CpuResources {
    pub shares: u64,
    pub quota: i64,
    pub period: u64,
    pub realtime_runtime: i64,
    pub realtime_period: u64,
    pub cpus: String,
    pub mems: String,
}

/// PIDs resources
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PidsResources {
    pub limit: i64,
}

/// IO resources
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IoResources {
    pub weight: u64,
}

/// Wait response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitResponse {
    pub exit_status: u32,
    pub exited_at: u64,
}

/// Stats response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResponse {
    pub stats: serde_json::Value,
}

/// Connect response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectResponse {
    pub shim_pid: u32,
    pub task_pid: u32,
    pub version: String,
}

/// Shim v2 implementation
pub struct ShimV2 {
    socket_path: PathBuf,
    bundle_path: PathBuf,
    namespace: String,
    runtime: Option<crate::ContainerRuntime>,
}

impl ShimV2 {
    /// Create a new shim instance
    pub fn new(socket_path: PathBuf, bundle_path: PathBuf, namespace: String) -> Self {
        Self {
            socket_path,
            bundle_path,
            namespace,
            runtime: None,
        }
    }

    /// Create a new shim instance with a runtime
    pub fn with_runtime(
        socket_path: PathBuf,
        bundle_path: PathBuf,
        namespace: String,
        runtime: crate::ContainerRuntime,
    ) -> Self {
        Self {
            socket_path,
            bundle_path,
            namespace,
            runtime: Some(runtime),
        }
    }

    /// Get the socket path
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    /// Get the runtime (creates one if not set)
    async fn get_runtime(&mut self) -> Result<&mut crate::ContainerRuntime> {
        if self.runtime.is_none() {
            self.runtime = Some(crate::ContainerRuntime::new().await?);
        }
        Ok(self.runtime.as_mut().unwrap())
    }

    /// Start the shim service
    #[cfg(feature = "shim-v2")]
    pub async fn serve(&mut self) -> Result<()> {
        log::info!(
            "Starting shim v2 service on {}",
            self.socket_path.display()
        );

        // Ensure runtime is initialized
        let _ = self.get_runtime().await?;

        // TTRPC server implementation
        // Note: Full TTRPC implementation requires:
        // 1. TTRPC server setup with Unix socket listener
        // 2. Task service implementation using ttrpc::Service
        // 3. Protobuf message definitions from containerd
        // 4. Request/response serialization/deserialization
        
        #[cfg(feature = "shim-v2")]
        {
            use std::os::unix::net::UnixListener;
            use std::io::prelude::*;
            
            // Remove old socket if exists
            let _ = std::fs::remove_file(&self.socket_path);
            
            let listener = UnixListener::bind(&self.socket_path)
                .map_err(|e| ShimError::io_with_context(
                    e,
                    format!("Failed to bind shim socket: {}", self.socket_path.display())
                ))?;
            
            log::info!("Shim v2 listening on {}", self.socket_path.display());
            
            // Accept connections and handle requests
            // In a full implementation, this would use ttrpc::Server
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        log::debug!("New shim connection");
                        // Handle TTRPC requests here
                        // For now, just acknowledge connection
                        let _ = stream.write_all(b"OK");
                    }
                    Err(e) => {
                        log::error!("Shim connection error: {}", e);
                    }
                }
            }
            
            Ok(())
        }
        
        #[cfg(not(feature = "shim-v2"))]
        {
            Err(ShimError::runtime(
                "Shim v2 feature not enabled. Enable with 'shim-v2' feature flag.",
            ))
        }
    }

    /// Start the shim service (fallback without TTRPC)
    #[cfg(not(feature = "shim-v2"))]
    pub async fn serve(&mut self) -> Result<()> {
        log::info!(
            "Starting shim v2 service on {} (TTRPC not available)",
            self.socket_path.display()
        );

        Err(ShimError::runtime(
            "Shim v2 server requires 'shim-v2' feature flag. Install ttrpc crate and enable feature.",
        ))
    }
}

/// Task service implementation that bridges to ContainerRuntime
pub struct TaskServiceImpl {
    runtime: crate::ContainerRuntime,
    namespace: String,
    bundle_path: PathBuf,
}

impl TaskServiceImpl {
    /// Create a new task service
    pub async fn new(namespace: String, bundle_path: PathBuf) -> Result<Self> {
        let runtime = crate::ContainerRuntime::new().await?;
        Ok(Self { runtime, namespace, bundle_path })
    }

    /// Get container ID from shim ID
    fn container_id(&self, shim_id: &str) -> String {
        format!("{}.{}", self.namespace, shim_id)
    }
}

#[cfg(feature = "shim-v2")]
impl TaskService for TaskServiceImpl {
    fn state(&self, container_id: &str, _exec_id: Option<&str>) -> Result<StateResponse> {
        // Use tokio runtime to call async methods
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        let containers = rt.block_on(self.runtime.list())
            .map_err(|e| ShimError::runtime(format!("Failed to list containers: {}", e)))?;
        
        let container = containers.iter()
            .find(|c| c.id == container_id)
            .ok_or_else(|| ShimError::not_found(format!("Container '{}'", container_id)))?;
        
        Ok(StateResponse {
            id: container.id.clone(),
            bundle: self.bundle_path.display().to_string(),
            pid: container.pid.unwrap_or(0),
            status: Status::from(container.status),
            stdin: String::new(),
            stdout: String::new(),
            stderr: String::new(),
            terminal: false,
            exit_status: 0,
            exited_at: 0,
        })
    }

    fn create(&self, request: CreateTaskRequest) -> Result<CreateTaskResponse> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        let config = oci_to_container_config(&request.id, &request.bundle)?;
        
        let container_id = rt.block_on(self.runtime.create(config))
            .map_err(|e| ShimError::runtime(format!("Failed to create container: {}", e)))?;
        
        // Get PID from container state
        let containers = rt.block_on(self.runtime.list())
            .map_err(|e| ShimError::runtime(format!("Failed to list containers: {}", e)))?;
        
        let pid = containers.iter()
            .find(|c| c.id == container_id)
            .and_then(|c| c.pid)
            .unwrap_or(0);
        
        Ok(CreateTaskResponse { pid })
    }

    fn start(&self, container_id: &str, _exec_id: Option<&str>) -> Result<StartResponse> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        rt.block_on(self.runtime.start(container_id))
            .map_err(|e| ShimError::runtime(format!("Failed to start container: {}", e)))?;
        
        // Get PID after start
        let containers = rt.block_on(self.runtime.list())
            .map_err(|e| ShimError::runtime(format!("Failed to list containers: {}", e)))?;
        
        let pid = containers.iter()
            .find(|c| c.id == container_id)
            .and_then(|c| c.pid)
            .unwrap_or(0);
        
        Ok(StartResponse { pid })
    }

    fn delete(&self, container_id: &str, _exec_id: Option<&str>) -> Result<DeleteResponse> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        // Get container info before delete
        let containers = rt.block_on(self.runtime.list())
            .map_err(|e| ShimError::runtime(format!("Failed to list containers: {}", e)))?;
        
        let container = containers.iter()
            .find(|c| c.id == container_id);
        
        let pid = container.and_then(|c| c.pid).unwrap_or(0);
        
        rt.block_on(self.runtime.delete(container_id))
            .map_err(|e| ShimError::runtime(format!("Failed to delete container: {}", e)))?;
        
        Ok(DeleteResponse {
            pid,
            exit_status: 0,
            exited_at: 0,
        })
    }

    fn pids(&self, container_id: &str) -> Result<PidsResponse> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        let containers = rt.block_on(self.runtime.list())
            .map_err(|e| ShimError::runtime(format!("Failed to list containers: {}", e)))?;
        
        let container = containers.iter()
            .find(|c| c.id == container_id)
            .ok_or_else(|| ShimError::not_found(format!("Container '{}'", container_id)))?;
        
        let processes = container.pid.map(|pid| ProcessInfo {
            pid,
            info: None,
        }).into_iter().collect();
        
        Ok(PidsResponse { processes })
    }

    fn pause(&self, _container_id: &str) -> Result<()> {
        // Pause not implemented yet
        Err(ShimError::runtime("Pause not implemented"))
    }

    fn resume(&self, _container_id: &str) -> Result<()> {
        // Resume not implemented yet
        Err(ShimError::runtime("Resume not implemented"))
    }

    fn checkpoint(&self, _container_id: &str, _options: CheckpointOptions) -> Result<()> {
        // Checkpoint not implemented yet
        Err(ShimError::runtime("Checkpoint not implemented"))
    }

    fn kill(&self, container_id: &str, _exec_id: Option<&str>, signal: u32, _all: bool) -> Result<()> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        // Stop is equivalent to kill with SIGTERM
        if signal == libc::SIGTERM as u32 || signal == 15 {
            rt.block_on(self.runtime.stop(container_id))
                .map_err(|e| ShimError::runtime(format!("Failed to stop container: {}", e)))?;
            Ok(())
        } else {
            // For other signals, we'd need to send signal to process
            Err(ShimError::runtime(format!("Signal {} not supported", signal)))
        }
    }

    fn exec(&self, _request: ExecProcessRequest) -> Result<()> {
        // Exec not fully implemented yet
        Err(ShimError::runtime("Exec not fully implemented"))
    }

    fn resize_pty(&self, _container_id: &str, _exec_id: Option<&str>, _width: u32, _height: u32) -> Result<()> {
        // Resize PTY not implemented yet
        Err(ShimError::runtime("Resize PTY not implemented"))
    }

    fn close_io(&self, _container_id: &str, _exec_id: Option<&str>, _stdin: bool) -> Result<()> {
        // Close IO not implemented yet
        Ok(()) // No-op for now
    }

    fn update(&self, _container_id: &str, _resources: Resources) -> Result<()> {
        // Update resources not implemented yet
        Err(ShimError::runtime("Update resources not implemented"))
    }

    fn wait(&self, container_id: &str, _exec_id: Option<&str>) -> Result<WaitResponse> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        // Wait for container to stop
        loop {
            let containers = rt.block_on(self.runtime.list())
                .map_err(|e| ShimError::runtime(format!("Failed to list containers: {}", e)))?;
            
            let container = containers.iter()
                .find(|c| c.id == container_id);
            
            if let Some(container) = container {
                if container.status == crate::types::ContainerStatus::Stopped {
                    return Ok(WaitResponse {
                        exit_status: 0,
                        exited_at: 0,
                    });
                }
            } else {
                return Ok(WaitResponse {
                    exit_status: 0,
                    exited_at: 0,
                });
            }
            
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    fn stats(&self, container_id: &str) -> Result<StatsResponse> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        let metrics = rt.block_on(self.runtime.metrics(container_id))
            .map_err(|e| ShimError::runtime(format!("Failed to get metrics: {}", e)))?;
        
        // Convert metrics to JSON
        let stats_json = serde_json::json!({
            "cpu": {
                "usage": {
                    "total": metrics.cpu.total_usage,
                    "percpu": metrics.cpu.per_cpu_usage.clone(),
                },
                "throttling": {
                    "periods": metrics.cpu.throttle_periods,
                    "throttled_periods": metrics.cpu.throttled_periods,
                    "throttled_time": metrics.cpu.throttled_time,
                },
            },
            "memory": {
                "usage": metrics.memory.usage,
                "max_usage": metrics.memory.max_usage,
                "limit": metrics.memory.limit,
            },
            "pids": {
                "current": metrics.pids.current,
            },
        });
        
        Ok(StatsResponse {
            stats: stats_json,
        })
    }

    fn connect(&self, _container_id: &str) -> Result<ConnectResponse> {
        Ok(ConnectResponse {
            shim_pid: std::process::id(),
            task_pid: 0,
            version: "v2".to_string(),
        })
    }

    fn shutdown(&self) -> Result<()> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        rt.block_on(self.runtime.shutdown())
            .map_err(|e| ShimError::runtime(format!("Failed to shutdown: {}", e)))?;
        
        Ok(())
    }
}

/// Parse OCI bundle config.json
pub fn parse_oci_bundle(bundle_path: &PathBuf) -> Result<serde_json::Value> {
    let config_path = bundle_path.join("config.json");
    let content = std::fs::read_to_string(&config_path).map_err(|e| {
        ShimError::runtime_with_context(
            format!("Failed to read config.json: {}", e),
            format!("Bundle path: {}", bundle_path.display()),
        )
    })?;

    serde_json::from_str(&content).map_err(|e| {
        ShimError::runtime_with_context(
            format!("Failed to parse config.json: {}", e),
            "Invalid OCI bundle configuration",
        )
    })
}

/// Convert OCI bundle to ContainerConfig
pub fn oci_to_container_config(
    container_id: &str,
    bundle_path: &PathBuf,
) -> Result<ContainerConfig> {
    let oci_config = parse_oci_bundle(bundle_path)?;

    let rootfs = bundle_path.join(
        oci_config["root"]["path"]
            .as_str()
            .unwrap_or("rootfs"),
    );

    let command: Vec<String> = oci_config["process"]["args"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| vec!["/bin/sh".to_string()]);

    let env: Vec<String> = oci_config["process"]["env"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let working_dir = oci_config["process"]["cwd"]
        .as_str()
        .unwrap_or("/")
        .to_string();

    Ok(ContainerConfig {
        id: container_id.to_string(),
        rootfs,
        command,
        env,
        working_dir,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_conversion() {
        assert_eq!(
            Status::from(crate::types::ContainerStatus::Running),
            Status::Running
        );
        assert_eq!(
            Status::from(crate::types::ContainerStatus::Stopped),
            Status::Stopped
        );
    }
}

