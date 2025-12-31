use crate::*;
use std::io::{Read, Write};

/// Vsock client for communicating with the Linux VM guest
pub struct VsockClient {
    port: u32,
    // In a real implementation, this would hold a vsock socket
    // For now, we'll use Unix socket as fallback
    use_unix_fallback: bool,
}

impl VsockClient {
    pub fn new(port: u32) -> Self {
        Self {
            port,
            use_unix_fallback: true, // Use Unix socket fallback for now
        }
    }
    
    pub fn connect(&self) -> Result<VsockStream> {
        if self.use_unix_fallback {
            // Fallback to Unix socket when vsock is not available
            // This allows development/testing without a VM
            use std::os::unix::net::UnixStream;
            let stream = UnixStream::connect("/tmp/libcrun-shim.sock")
                .map_err(|e| ShimError::Runtime(format!("Failed to connect via Unix socket: {}", e)))?;
            Ok(VsockStream::Unix(stream))
        } else {
            // In a real implementation, use vsock crate:
            // use vsock::VsockStream;
            // let stream = VsockStream::connect(self.port)
            //     .map_err(|e| ShimError::Runtime(format!("Failed to connect via vsock: {}", e)))?;
            // Ok(VsockStream::Vsock(stream))
            
            // For now, return error indicating vsock is not implemented
            Err(ShimError::Runtime(
                "Vsock not yet implemented. Use Unix socket fallback.".to_string()
            ))
        }
    }
}

/// Abstraction over vsock or Unix socket streams
pub enum VsockStream {
    #[cfg(target_os = "macos")]
    Unix(std::os::unix::net::UnixStream),
    // In real implementation:
    // Vsock(vsock::VsockStream),
}

impl Read for VsockStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            VsockStream::Unix(stream) => stream.read(buf),
            // VsockStream::Vsock(stream) => stream.read(buf),
        }
    }
}

impl Write for VsockStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            VsockStream::Unix(stream) => stream.write(buf),
            // VsockStream::Vsock(stream) => stream.write(buf),
        }
    }
    
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            VsockStream::Unix(stream) => stream.flush(),
            // VsockStream::Vsock(stream) => stream.flush(),
        }
    }
}

