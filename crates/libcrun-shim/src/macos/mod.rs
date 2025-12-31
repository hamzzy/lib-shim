mod vm;
pub mod rpc;
mod vsock;

use crate::*;
use libcrun_shim_proto::*;

pub struct MacOsRuntime {
    #[allow(dead_code)]
    vm: vm::VirtualMachine,
    #[allow(dead_code)]
    rpc: rpc::RpcClient,
}

impl MacOsRuntime {
    pub async fn new() -> Result<Self> {
        let vm = vm::VirtualMachine::start()?;
        // Wait a bit for VM to boot
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        let rpc = rpc::RpcClient::connect()?;
        
        Ok(Self { vm, rpc })
    }
}

impl RuntimeImpl for MacOsRuntime {
    async fn create(&self, config: ContainerConfig) -> Result<String> {
        let req = Request::Create(CreateRequest {
            id: config.id.clone(),
            rootfs: config.rootfs.display().to_string(),
            command: config.command,
            env: config.env,
            working_dir: config.working_dir,
        });
        
        let mut rpc = rpc::RpcClient::connect()?;
        match rpc.call(req)? {
            Response::Created(id) => Ok(id),
            Response::Error(e) => Err(ShimError::Runtime(e)),
            _ => Err(ShimError::Runtime("Unexpected response".to_string())),
        }
    }
    
    async fn start(&self, id: &str) -> Result<()> {
        let mut rpc = rpc::RpcClient::connect()?;
        match rpc.call(Request::Start(id.to_string()))? {
            Response::Started => Ok(()),
            Response::Error(e) => Err(ShimError::Runtime(e)),
            _ => Err(ShimError::Runtime("Unexpected response".to_string())),
        }
    }
    
    async fn stop(&self, id: &str) -> Result<()> {
        let mut rpc = rpc::RpcClient::connect()?;
        match rpc.call(Request::Stop(id.to_string()))? {
            Response::Stopped => Ok(()),
            Response::Error(e) => Err(ShimError::Runtime(e)),
            _ => Err(ShimError::Runtime("Unexpected response".to_string())),
        }
    }
    
    async fn delete(&self, id: &str) -> Result<()> {
        let mut rpc = rpc::RpcClient::connect()?;
        match rpc.call(Request::Delete(id.to_string()))? {
            Response::Deleted => Ok(()),
            Response::Error(e) => Err(ShimError::Runtime(e)),
            _ => Err(ShimError::Runtime("Unexpected response".to_string())),
        }
    }
    
    async fn list(&self) -> Result<Vec<ContainerInfo>> {
        let mut rpc = rpc::RpcClient::connect()?;
        match rpc.call(Request::List)? {
            Response::List(list) => {
                Ok(list.into_iter().map(|info| ContainerInfo {
                    id: info.id,
                    status: match info.status.as_str() {
                        "Created" => ContainerStatus::Created,
                        "Running" => ContainerStatus::Running,
                        _ => ContainerStatus::Stopped,
                    },
                    pid: info.pid,
                }).collect())
            }
            Response::Error(e) => Err(ShimError::Runtime(e)),
            _ => Err(ShimError::Runtime("Unexpected response".to_string())),
        }
    }
}

