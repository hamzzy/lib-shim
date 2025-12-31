use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Create(CreateRequest),
    Start(String),
    Stop(String),
    Delete(String),
    List,
    /// Get metrics for a specific container
    Metrics(String),
    /// Get metrics for all containers
    AllMetrics,
    /// Get logs for a container
    Logs(LogsRequest),
    /// Get health status for a container
    Health(String),
    /// Execute a command in a container
    Exec(ExecRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsRequest {
    pub id: String,
    pub tail: u32,
    pub since: u64,
    pub timestamps: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecRequest {
    pub id: String,
    pub command: Vec<String>,
    pub env: Vec<String>,
    pub working_dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateRequest {
    pub id: String,
    pub rootfs: String,
    pub command: Vec<String>,
    pub env: Vec<String>,
    pub working_dir: String,
    
    // Advanced features
    pub stdio: StdioConfigProto,
    pub network: NetworkConfigProto,
    pub volumes: Vec<VolumeMountProto>,
    pub resources: ResourceLimitsProto,
    
    // Health check configuration
    #[serde(default)]
    pub health_check: Option<HealthCheckProto>,
}

/// Health check configuration for proto
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HealthCheckProto {
    pub command: Vec<String>,
    #[serde(default)]
    pub interval_secs: u64,
    #[serde(default)]
    pub timeout_secs: u64,
    #[serde(default)]
    pub retries: u32,
    #[serde(default)]
    pub start_period_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StdioConfigProto {
    pub tty: bool,
    pub open_stdin: bool,
    pub stdin_path: Option<String>,
    pub stdout_path: Option<String>,
    pub stderr_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkConfigProto {
    pub mode: String,
    pub port_mappings: Vec<PortMappingProto>,
    pub interfaces: Vec<NetworkInterfaceProto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMappingProto {
    pub host_port: u16,
    pub container_port: u16,
    pub protocol: String,
    pub host_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterfaceProto {
    pub name: String,
    pub interface_type: String,
    pub config: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMountProto {
    pub source: String,
    pub destination: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceLimitsProto {
    pub cpu: Option<f64>,
    pub memory: Option<u64>,
    pub memory_swap: Option<u64>,
    pub pids: Option<i64>,
    pub blkio_weight: Option<u16>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Created(String),
    Started,
    Stopped,
    Deleted,
    List(Vec<ContainerInfoProto>),
    /// Metrics for a single container
    Metrics(ContainerMetricsProto),
    /// Metrics for all containers
    AllMetrics(Vec<ContainerMetricsProto>),
    /// Container logs
    Logs(LogsProto),
    /// Health status
    Health(HealthStatusProto),
    /// Exec result
    Exec(ExecResultProto),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogsProto {
    pub id: String,
    pub stdout: String,
    pub stderr: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatusProto {
    pub id: String,
    pub status: String,  // "none", "starting", "healthy", "unhealthy"
    pub failing_streak: u32,
    pub last_output: String,
    pub last_check: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResultProto {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfoProto {
    pub id: String,
    pub status: String,
    pub pid: Option<u32>,
}

/// Container metrics for RPC
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContainerMetricsProto {
    pub id: String,
    pub timestamp: u64,
    pub cpu: CpuMetricsProto,
    pub memory: MemoryMetricsProto,
    pub blkio: BlkioMetricsProto,
    pub network: NetworkMetricsProto,
    pub pids: PidsMetricsProto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CpuMetricsProto {
    pub usage_total: u64,
    pub usage_user: u64,
    pub usage_system: u64,
    pub per_cpu: Vec<u64>,
    pub throttled_periods: u64,
    pub throttled_time: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryMetricsProto {
    pub usage: u64,
    pub max_usage: u64,
    pub limit: u64,
    pub cache: u64,
    pub rss: u64,
    pub swap: u64,
    pub usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlkioMetricsProto {
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_ops: u64,
    pub write_ops: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkMetricsProto {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PidsMetricsProto {
    pub current: u64,
    pub limit: u64,
}

pub fn serialize_request(req: &Request) -> Vec<u8> {
    bincode::serialize(req).unwrap()
}

pub fn deserialize_request(data: &[u8]) -> Result<Request, Box<dyn std::error::Error>> {
    Ok(bincode::deserialize(data)?)
}

pub fn serialize_response(resp: &Response) -> Vec<u8> {
    bincode::serialize(resp).unwrap()
}

pub fn deserialize_response(data: &[u8]) -> Result<Response, Box<dyn std::error::Error>> {
    Ok(bincode::deserialize(data)?)
}

