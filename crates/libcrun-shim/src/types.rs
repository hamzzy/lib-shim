use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub id: String,
    pub rootfs: PathBuf,
    pub command: Vec<String>,
    pub env: Vec<String>,
    pub working_dir: String,
    
    // Advanced features
    /// Container stdio configuration
    #[serde(default)]
    pub stdio: StdioConfig,
    
    /// Network configuration
    #[serde(default)]
    pub network: NetworkConfig,
    
    /// Volume mounts
    #[serde(default)]
    pub volumes: Vec<VolumeMount>,
    
    /// Resource limits
    #[serde(default)]
    pub resources: ResourceLimits,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            rootfs: PathBuf::new(),
            command: vec![],
            env: vec![],
            working_dir: "/".to_string(),
            stdio: StdioConfig::default(),
            network: NetworkConfig::default(),
            volumes: vec![],
            resources: ResourceLimits::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StdioConfig {
    /// Whether to allocate a pseudo-TTY
    pub tty: bool,
    /// Whether to keep stdin open
    pub open_stdin: bool,
    /// Stdin file path (if any)
    pub stdin_path: Option<PathBuf>,
    /// Stdout file path (if any)
    pub stdout_path: Option<PathBuf>,
    /// Stderr file path (if any)
    pub stderr_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Network mode: "none", "bridge", "host", or "container:<id>"
    #[serde(default = "default_network_mode")]
    pub mode: String,
    /// Port mappings (host_port -> container_port)
    #[serde(default)]
    pub port_mappings: Vec<PortMapping>,
    /// Additional network interfaces
    #[serde(default)]
    pub interfaces: Vec<NetworkInterface>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            mode: default_network_mode(),
            port_mappings: vec![],
            interfaces: vec![],
        }
    }
}

fn default_network_mode() -> String {
    "bridge".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    /// Host port (0 for random)
    pub host_port: u16,
    /// Container port
    pub container_port: u16,
    /// Protocol: "tcp" or "udp"
    pub protocol: String,
    /// Host IP to bind to (None for all interfaces)
    pub host_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    /// Interface name
    pub name: String,
    /// Interface type: "bridge", "macvlan", etc.
    pub interface_type: String,
    /// Additional configuration
    pub config: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    /// Source path on host
    pub source: PathBuf,
    /// Destination path in container
    pub destination: PathBuf,
    /// Mount options (e.g., "ro", "rw", "bind")
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceLimits {
    /// CPU limit (in cores, 0 = unlimited)
    pub cpu: Option<f64>,
    /// Memory limit (in bytes, 0 = unlimited)
    pub memory: Option<u64>,
    /// Memory swap limit (in bytes, 0 = unlimited)
    pub memory_swap: Option<u64>,
    /// PIDs limit (0 = unlimited)
    pub pids: Option<i64>,
    /// Block IO weight (10-1000)
    pub blkio_weight: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub id: String,
    pub status: ContainerStatus,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ContainerStatus {
    Created,
    Running,
    Stopped,
}

