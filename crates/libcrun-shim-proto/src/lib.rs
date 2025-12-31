use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Create(CreateRequest),
    Start(String),
    Stop(String),
    Delete(String),
    List,
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
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfoProto {
    pub id: String,
    pub status: String,
    pub pid: Option<u32>,
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

