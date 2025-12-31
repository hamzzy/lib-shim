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
        let vm = vm::VirtualMachine::start().await?;
        // Wait a bit for VM to boot
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        
        // Try to use vsock if available, otherwise fallback to Unix socket
        #[cfg(target_os = "macos")]
        let rpc = if let Some(vsock_device) = vm.get_vsock_device() {
            // Create vsock client with the device
            use vsock::VsockClient;
            let vsock_client = VsockClient::with_vsock_device(vm.get_vsock_port(), vsock_device);
            match vsock_client.connect() {
                Ok(stream) => rpc::RpcClient::from_stream(stream)?,
                Err(e) => {
                    log::warn!("Vsock connection failed: {}, falling back to Unix socket", e);
                    rpc::RpcClient::connect()?
                }
            }
        } else {
            rpc::RpcClient::connect()?
        };
        
        #[cfg(not(target_os = "macos"))]
        let rpc = rpc::RpcClient::connect()?;
        
        Ok(Self { vm, rpc })
    }
}

impl RuntimeImpl for MacOsRuntime {
    async fn create(&self, config: ContainerConfig) -> Result<String> {
        use libcrun_shim_proto::*;
        let req = Request::Create(CreateRequest {
            id: config.id.clone(),
            rootfs: config.rootfs.display().to_string(),
            command: config.command,
            env: config.env,
            working_dir: config.working_dir,
            stdio: StdioConfigProto {
                tty: config.stdio.tty,
                open_stdin: config.stdio.open_stdin,
                stdin_path: config.stdio.stdin_path.as_ref().map(|p| p.display().to_string()),
                stdout_path: config.stdio.stdout_path.as_ref().map(|p| p.display().to_string()),
                stderr_path: config.stdio.stderr_path.as_ref().map(|p| p.display().to_string()),
            },
            network: NetworkConfigProto {
                mode: config.network.mode,
                port_mappings: config.network.port_mappings.into_iter().map(|pm| PortMappingProto {
                    host_port: pm.host_port,
                    container_port: pm.container_port,
                    protocol: pm.protocol,
                    host_ip: pm.host_ip,
                }).collect(),
                interfaces: config.network.interfaces.into_iter().map(|ni| NetworkInterfaceProto {
                    name: ni.name,
                    interface_type: ni.interface_type,
                    config: ni.config,
                }).collect(),
            },
            volumes: config.volumes.into_iter().map(|vm| VolumeMountProto {
                source: vm.source.display().to_string(),
                destination: vm.destination.display().to_string(),
                options: vm.options,
            }).collect(),
            resources: ResourceLimitsProto {
                cpu: config.resources.cpu,
                memory: config.resources.memory,
                memory_swap: config.resources.memory_swap,
                pids: config.resources.pids,
                blkio_weight: config.resources.blkio_weight,
            },
        });
        
        let mut rpc = rpc::RpcClient::connect()?;
        match rpc.call(req)? {
            Response::Created(id) => Ok(id),
            Response::Error(e) => Err(ShimError::runtime_with_context(e, "RPC create request failed")),
            _ => Err(ShimError::runtime("Unexpected response type from RPC create request")),
        }
    }
    
    async fn start(&self, id: &str) -> Result<()> {
        let mut rpc = rpc::RpcClient::connect()?;
        match rpc.call(Request::Start(id.to_string()))? {
            Response::Started => Ok(()),
            Response::Error(e) => Err(ShimError::runtime_with_context(e, format!("RPC start request failed for container: {}", id))),
            _ => Err(ShimError::runtime("Unexpected response type from RPC start request")),
        }
    }
    
    async fn stop(&self, id: &str) -> Result<()> {
        let mut rpc = rpc::RpcClient::connect()?;
        match rpc.call(Request::Stop(id.to_string()))? {
            Response::Stopped => Ok(()),
            Response::Error(e) => Err(ShimError::runtime_with_context(e, format!("RPC stop request failed for container: {}", id))),
            _ => Err(ShimError::runtime("Unexpected response type from RPC stop request")),
        }
    }
    
    async fn delete(&self, id: &str) -> Result<()> {
        let mut rpc = rpc::RpcClient::connect()?;
        match rpc.call(Request::Delete(id.to_string()))? {
            Response::Deleted => Ok(()),
            Response::Error(e) => Err(ShimError::runtime_with_context(e, format!("RPC delete request failed for container: {}", id))),
            _ => Err(ShimError::runtime("Unexpected response type from RPC delete request")),
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
            Response::Error(e) => Err(ShimError::runtime_with_context(e, "RPC list request failed")),
            _ => Err(ShimError::runtime("Unexpected response type from RPC list request")),
        }
    }
}

