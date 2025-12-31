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

