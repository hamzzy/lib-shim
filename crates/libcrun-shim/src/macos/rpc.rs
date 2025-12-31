use super::vsock::{VsockClient, VsockStream};
use crate::types::RuntimeConfig;
use crate::*;
use libcrun_shim_proto::*;
use std::io::{Read, Write};

pub struct RpcClient {
    stream: VsockStream,
}

impl RpcClient {
    /// Connect with default configuration (from environment)
    pub fn connect() -> Result<Self> {
        Self::connect_with_config(&RuntimeConfig::from_env())
    }

    /// Connect with custom configuration
    pub fn connect_with_config(config: &RuntimeConfig) -> Result<Self> {
        let vsock_client = VsockClient::with_config(config);

        match vsock_client.connect() {
            Ok(stream) => {
                log::info!(
                    "RPC connection established (port: {}, socket: {})",
                    config.vsock_port,
                    config.socket_path.display()
                );
                Ok(Self { stream })
            }
            Err(e) => {
                log::error!("Failed to establish RPC connection: {}", e);
                Err(e)
            }
        }
    }

    /// Connect with a VM bridge handle for native vsock
    #[cfg(target_os = "macos")]
    pub fn connect_with_vm_bridge(
        config: &RuntimeConfig,
        vm_bridge_handle: *mut std::os::raw::c_void,
    ) -> Result<Self> {
        let vsock_client = VsockClient::with_vm_bridge(config, vm_bridge_handle);
        let stream = vsock_client.connect()?;
        log::info!(
            "RPC connection established via VM bridge (port: {})",
            config.vsock_port
        );
        Ok(Self { stream })
    }

    /// Connect via vsock with specified port (legacy method for compatibility)
    pub fn connect_vsock(port: u32) -> Result<Self> {
        let mut config = RuntimeConfig::from_env();
        config.vsock_port = port;
        Self::connect_with_config(&config)
    }

    /// Create an RPC client from an existing stream
    pub fn from_stream(stream: VsockStream) -> Self {
        Self { stream }
    }

    pub fn call(&mut self, request: Request) -> Result<Response> {
        let data = serialize_request(&request);
        self.stream.write_all(&data)?;
        self.stream.flush()?;

        let mut buffer = vec![0u8; 4096];
        let n = self.stream.read(&mut buffer)?;

        deserialize_response(&buffer[..n]).map_err(|e| ShimError::Serialization {
            message: e.to_string(),
            context: Some("Failed to deserialize RPC response".to_string()),
        })
    }
}
