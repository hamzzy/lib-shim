use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Runtime configuration for the container shim
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Unix socket path for agent communication
    #[serde(default = "default_socket_path")]
    pub socket_path: PathBuf,

    /// Vsock port for VM communication
    #[serde(default = "default_vsock_port")]
    pub vsock_port: u32,

    /// Additional paths to search for VM assets (kernel, initramfs)
    #[serde(default)]
    pub vm_asset_paths: Vec<PathBuf>,

    /// VM memory size in bytes (default: 2GB)
    #[serde(default = "default_vm_memory")]
    pub vm_memory: u64,

    /// Number of VM CPU cores (default: 4)
    #[serde(default = "default_vm_cpus")]
    pub vm_cpus: u32,

    /// Connection timeout in seconds
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout: u64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            vsock_port: default_vsock_port(),
            vm_asset_paths: vec![],
            vm_memory: default_vm_memory(),
            vm_cpus: default_vm_cpus(),
            connection_timeout: default_connection_timeout(),
        }
    }
}

fn default_socket_path() -> PathBuf {
    PathBuf::from("/tmp/libcrun-shim.sock")
}

fn default_vsock_port() -> u32 {
    1234
}

fn default_vm_memory() -> u64 {
    2 * 1024 * 1024 * 1024 // 2GB
}

fn default_vm_cpus() -> u32 {
    4
}

fn default_connection_timeout() -> u64 {
    30
}

impl RuntimeConfig {
    /// Create a new RuntimeConfig builder
    pub fn builder() -> RuntimeConfigBuilder {
        RuntimeConfigBuilder::default()
    }

    /// Load configuration from environment variables
    /// 
    /// Supported variables:
    /// - `LIBCRUN_SOCKET_PATH`: Unix socket path
    /// - `LIBCRUN_VSOCK_PORT`: Vsock port number
    /// - `LIBCRUN_VM_ASSET_PATHS`: Colon-separated list of paths
    /// - `LIBCRUN_VM_MEMORY`: VM memory in bytes
    /// - `LIBCRUN_VM_CPUS`: Number of VM CPUs
    /// - `LIBCRUN_CONNECTION_TIMEOUT`: Connection timeout in seconds
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(path) = std::env::var("LIBCRUN_SOCKET_PATH") {
            config.socket_path = PathBuf::from(path);
        }

        if let Ok(port) = std::env::var("LIBCRUN_VSOCK_PORT") {
            if let Ok(p) = port.parse() {
                config.vsock_port = p;
            }
        }

        if let Ok(paths) = std::env::var("LIBCRUN_VM_ASSET_PATHS") {
            config.vm_asset_paths = paths
                .split(':')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect();
        }

        if let Ok(memory) = std::env::var("LIBCRUN_VM_MEMORY") {
            if let Ok(m) = memory.parse() {
                config.vm_memory = m;
            }
        }

        if let Ok(cpus) = std::env::var("LIBCRUN_VM_CPUS") {
            if let Ok(c) = cpus.parse() {
                config.vm_cpus = c;
            }
        }

        if let Ok(timeout) = std::env::var("LIBCRUN_CONNECTION_TIMEOUT") {
            if let Ok(t) = timeout.parse() {
                config.connection_timeout = t;
            }
        }

        config
    }

    /// Get all VM asset search paths (including defaults)
    pub fn get_vm_asset_search_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.vm_asset_paths.clone();
        
        // Add default search paths
        let default_paths = [
            PathBuf::from("/usr/share/libcrun-shim"),
            PathBuf::from("/usr/local/share/libcrun-shim"),
            PathBuf::from("/opt/libcrun-shim"),
            dirs::data_local_dir()
                .map(|p| p.join("libcrun-shim"))
                .unwrap_or_else(|| PathBuf::from("~/.local/share/libcrun-shim")),
            dirs::home_dir()
                .map(|p| p.join(".libcrun-shim"))
                .unwrap_or_else(|| PathBuf::from("~/.libcrun-shim")),
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        ];

        for path in default_paths {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }

        paths
    }
}

/// Builder for RuntimeConfig
#[derive(Debug, Clone, Default)]
pub struct RuntimeConfigBuilder {
    socket_path: Option<PathBuf>,
    vsock_port: Option<u32>,
    vm_asset_paths: Vec<PathBuf>,
    vm_memory: Option<u64>,
    vm_cpus: Option<u32>,
    connection_timeout: Option<u64>,
}

impl RuntimeConfigBuilder {
    pub fn socket_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.socket_path = Some(path.into());
        self
    }

    pub fn vsock_port(mut self, port: u32) -> Self {
        self.vsock_port = Some(port);
        self
    }

    pub fn add_vm_asset_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.vm_asset_paths.push(path.into());
        self
    }

    pub fn vm_memory(mut self, bytes: u64) -> Self {
        self.vm_memory = Some(bytes);
        self
    }

    pub fn vm_cpus(mut self, cpus: u32) -> Self {
        self.vm_cpus = Some(cpus);
        self
    }

    pub fn connection_timeout(mut self, seconds: u64) -> Self {
        self.connection_timeout = Some(seconds);
        self
    }

    pub fn build(self) -> RuntimeConfig {
        RuntimeConfig {
            socket_path: self.socket_path.unwrap_or_else(default_socket_path),
            vsock_port: self.vsock_port.unwrap_or_else(default_vsock_port),
            vm_asset_paths: self.vm_asset_paths,
            vm_memory: self.vm_memory.unwrap_or_else(default_vm_memory),
            vm_cpus: self.vm_cpus.unwrap_or_else(default_vm_cpus),
            connection_timeout: self.connection_timeout.unwrap_or_else(default_connection_timeout),
        }
    }
}

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

