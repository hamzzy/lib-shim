use crate::*;
use libcrun_shim_proto::*;
use std::io::{Read, Write};
use super::vsock::{VsockClient, VsockStream};

pub struct RpcClient {
    stream: VsockStream,
}

impl RpcClient {
    pub fn connect() -> Result<Self> {
        // Try vsock first (for VM communication)
        // Fallback to Unix socket for development/testing
        let vsock_client = VsockClient::new(1234); // Default vsock port
        
        match vsock_client.connect() {
            Ok(stream) => Ok(Self { stream }),
            Err(_) => {
                // Fallback to Unix socket
                use std::os::unix::net::UnixStream;
                let unix_stream = UnixStream::connect("/tmp/libcrun-shim.sock")
                    .map_err(|e| ShimError::runtime_with_context(
                        format!("Failed to connect to Unix socket: {}", e),
                        "Make sure the agent is running and listening on /tmp/libcrun-shim.sock"
                    ))?;
                Ok(Self {
                    stream: VsockStream::Unix(unix_stream),
                })
            }
        }
    }
    
    pub fn connect_vsock(port: u32) -> Result<Self> {
        let vsock_client = VsockClient::new(port);
        let stream = vsock_client.connect()?;
        Ok(Self { stream })
    }
    
    /// Create an RPC client from an existing stream
    pub fn from_stream(stream: VsockStream) -> Result<Self> {
        Ok(Self { stream })
    }
    
    pub fn call(&mut self, request: Request) -> Result<Response> {
        let data = serialize_request(&request);
        self.stream.write_all(&data)?;
        self.stream.flush()?;
        
        let mut buffer = vec![0u8; 4096];
        let n = self.stream.read(&mut buffer)?;
        
        deserialize_response(&buffer[..n])
            .map_err(|e| ShimError::Serialization {
                message: e.to_string(),
                context: Some("Failed to deserialize RPC response".to_string()),
            })
    }
}

