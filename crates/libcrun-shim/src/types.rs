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

    /// Virtual disks to attach to the VM
    #[serde(default)]
    pub vm_disks: Vec<VmDiskConfig>,

    /// VM network configuration
    #[serde(default)]
    pub vm_network: VmNetworkConfig,
}

/// Virtual disk configuration for VM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmDiskConfig {
    /// Path to the disk image file
    pub path: PathBuf,
    /// Disk size in bytes (used when creating new disk)
    pub size: u64,
    /// Whether the disk is read-only
    #[serde(default)]
    pub read_only: bool,
    /// Disk format: "raw" or "qcow2" (default: raw)
    #[serde(default = "default_disk_format")]
    pub format: String,
    /// Whether to create the disk if it doesn't exist
    #[serde(default = "default_true")]
    pub create_if_missing: bool,
}

fn default_disk_format() -> String {
    "raw".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for VmDiskConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            size: 10 * 1024 * 1024 * 1024, // 10GB default
            read_only: false,
            format: default_disk_format(),
            create_if_missing: true,
        }
    }
}

/// VM network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmNetworkConfig {
    /// Network mode: "nat", "bridged", "none" (default: nat)
    #[serde(default = "default_vm_network_mode")]
    pub mode: String,
    /// Port forwarding rules (host_port -> guest_port)
    #[serde(default)]
    pub port_forwards: Vec<PortForward>,
    /// Bridge interface name (for bridged mode)
    pub bridge_interface: Option<String>,
}

fn default_vm_network_mode() -> String {
    "nat".to_string()
}

impl Default for VmNetworkConfig {
    fn default() -> Self {
        Self {
            mode: default_vm_network_mode(),
            port_forwards: vec![],
            bridge_interface: None,
        }
    }
}

/// Port forwarding rule for VM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortForward {
    /// Host port to listen on
    pub host_port: u16,
    /// Guest port to forward to
    pub guest_port: u16,
    /// Protocol: "tcp" or "udp" (default: tcp)
    #[serde(default = "default_protocol")]
    pub protocol: String,
    /// Host IP to bind to (default: 127.0.0.1)
    #[serde(default = "default_host_ip")]
    pub host_ip: String,
}

fn default_protocol() -> String {
    "tcp".to_string()
}

fn default_host_ip() -> String {
    "127.0.0.1".to_string()
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
            vm_disks: vec![],
            vm_network: VmNetworkConfig::default(),
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
    vm_disks: Vec<VmDiskConfig>,
    vm_network: Option<VmNetworkConfig>,
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

    /// Add a virtual disk to the VM
    pub fn add_vm_disk(mut self, disk: VmDiskConfig) -> Self {
        self.vm_disks.push(disk);
        self
    }

    /// Add a virtual disk by path and size
    pub fn add_disk(mut self, path: impl Into<PathBuf>, size_bytes: u64) -> Self {
        self.vm_disks.push(VmDiskConfig {
            path: path.into(),
            size: size_bytes,
            ..Default::default()
        });
        self
    }

    /// Set VM network configuration
    pub fn vm_network(mut self, network: VmNetworkConfig) -> Self {
        self.vm_network = Some(network);
        self
    }

    /// Add a port forward rule
    pub fn add_port_forward(mut self, host_port: u16, guest_port: u16) -> Self {
        let network = self.vm_network.get_or_insert_with(VmNetworkConfig::default);
        network.port_forwards.push(PortForward {
            host_port,
            guest_port,
            protocol: default_protocol(),
            host_ip: default_host_ip(),
        });
        self
    }

    /// Set VM network mode
    pub fn network_mode(mut self, mode: impl Into<String>) -> Self {
        let network = self.vm_network.get_or_insert_with(VmNetworkConfig::default);
        network.mode = mode.into();
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
            vm_disks: self.vm_disks,
            vm_network: self.vm_network.unwrap_or_default(),
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

/// Container resource metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContainerMetrics {
    /// Container ID
    pub id: String,
    /// Timestamp when metrics were collected (Unix epoch seconds)
    pub timestamp: u64,
    /// CPU metrics
    pub cpu: CpuMetrics,
    /// Memory metrics
    pub memory: MemoryMetrics,
    /// Block I/O metrics
    pub blkio: BlkioMetrics,
    /// Network metrics
    pub network: NetworkMetrics,
    /// PIDs metrics
    pub pids: PidsMetrics,
}

/// CPU usage metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CpuMetrics {
    /// Total CPU time consumed (nanoseconds)
    pub usage_total: u64,
    /// CPU time consumed in user mode (nanoseconds)
    pub usage_user: u64,
    /// CPU time consumed in kernel mode (nanoseconds)
    pub usage_system: u64,
    /// Per-CPU usage (nanoseconds per CPU)
    pub per_cpu: Vec<u64>,
    /// Number of periods with throttling active
    pub throttled_periods: u64,
    /// Total time throttled (nanoseconds)
    pub throttled_time: u64,
    /// CPU usage percentage (0.0 - 100.0 * num_cpus)
    pub usage_percent: f64,
}

/// Memory usage metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryMetrics {
    /// Current memory usage (bytes)
    pub usage: u64,
    /// Maximum memory usage recorded (bytes)
    pub max_usage: u64,
    /// Memory limit (bytes)
    pub limit: u64,
    /// Cache memory (bytes)
    pub cache: u64,
    /// RSS - Resident Set Size (bytes)
    pub rss: u64,
    /// Swap usage (bytes)
    pub swap: u64,
    /// Memory usage percentage (0.0 - 100.0)
    pub usage_percent: f64,
}

/// Block I/O metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlkioMetrics {
    /// Bytes read from block devices
    pub read_bytes: u64,
    /// Bytes written to block devices
    pub write_bytes: u64,
    /// Number of read operations
    pub read_ops: u64,
    /// Number of write operations
    pub write_ops: u64,
}

/// Network I/O metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkMetrics {
    /// Bytes received
    pub rx_bytes: u64,
    /// Bytes transmitted
    pub tx_bytes: u64,
    /// Packets received
    pub rx_packets: u64,
    /// Packets transmitted
    pub tx_packets: u64,
    /// Receive errors
    pub rx_errors: u64,
    /// Transmit errors
    pub tx_errors: u64,
    /// Receive drops
    pub rx_dropped: u64,
    /// Transmit drops
    pub tx_dropped: u64,
}

/// PIDs metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PidsMetrics {
    /// Current number of processes/threads
    pub current: u64,
    /// Maximum allowed (0 = unlimited)
    pub limit: u64,
}

/// VM-level metrics (macOS)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VmMetrics {
    /// VM state (running, stopped, etc.)
    pub state: String,
    /// VM uptime in seconds
    pub uptime_secs: u64,
    /// VM memory usage (bytes)
    pub memory_usage: u64,
    /// VM memory allocated (bytes)
    pub memory_allocated: u64,
    /// VM CPU usage percentage
    pub cpu_percent: f64,
    /// Number of containers running in VM
    pub container_count: u32,
}

