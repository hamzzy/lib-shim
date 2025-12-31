use crate::*;
use libcrun_shim_proto::*;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

pub struct RpcClient {
    stream: UnixStream,
}

impl RpcClient {
    pub fn connect() -> Result<Self> {
        // In real implementation, connect via vsock to VM
        // For MVP, connect to Unix socket (requires VM to expose it)
        let stream = UnixStream::connect("/tmp/libcrun-shim.sock")
            .map_err(|e| ShimError::Runtime(format!("Failed to connect: {}", e)))?;
        
        Ok(Self { stream })
    }
    
    pub fn call(&mut self, request: Request) -> Result<Response> {
        let data = serialize_request(&request);
        self.stream.write_all(&data)?;
        
        let mut buffer = vec![0u8; 4096];
        let n = self.stream.read(&mut buffer)?;
        
        deserialize_response(&buffer[..n])
            .map_err(|e| ShimError::Serialization(e.to_string()))
    }
}

