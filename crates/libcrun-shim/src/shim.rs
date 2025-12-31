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
}

impl ShimV2 {
    /// Create a new shim instance
    pub fn new(socket_path: PathBuf, bundle_path: PathBuf, namespace: String) -> Self {
        Self {
            socket_path,
            bundle_path,
            namespace,
        }
    }

    /// Get the socket path
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    /// Start the shim service
    pub async fn serve(&self) -> Result<()> {
        log::info!(
            "Starting shim v2 service on {}",
            self.socket_path.display()
        );

        // Create Unix socket and listen for TTRPC requests
        // Full implementation would use ttrpc crate

        Err(ShimError::runtime(
            "Shim v2 server not fully implemented - use containerd's runc shim for now",
        ))
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

