use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub id: String,
    pub rootfs: PathBuf,
    pub command: Vec<String>,
    pub env: Vec<String>,
    pub working_dir: String,
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

