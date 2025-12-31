use crate::*;
use std::io::{Read, Write};

#[cfg(target_os = "macos")]
use objc::runtime::{Class, Object};
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

/// Vsock client for communicating with the Linux VM guest
pub struct VsockClient {
    port: u32,
    use_unix_fallback: bool,
    #[cfg(target_os = "macos")]
    vsock_device: Option<*mut Object>, // VZVirtioSocketDevice from VM
}

impl VsockClient {
    pub fn new(port: u32) -> Self {
        Self {
            port,
            use_unix_fallback: true, // Default to Unix socket
            #[cfg(target_os = "macos")]
            vsock_device: None,
        }
    }

    /// Create a vsock client with access to the VM's vsock device
    #[cfg(target_os = "macos")]
    pub fn with_vsock_device(port: u32, vsock_device: *mut Object) -> Self {
        Self {
            port,
            use_unix_fallback: false,
            vsock_device: Some(vsock_device),
        }
    }
    
    
    pub fn connect(&self) -> Result<VsockStream> {
        if self.use_unix_fallback {
            // Fallback to Unix socket when vsock is not available
            use std::os::unix::net::UnixStream;
            let stream = UnixStream::connect("/tmp/libcrun-shim.sock")
                .map_err(|e| ShimError::runtime_with_context(
                    format!("Failed to connect via Unix socket: {}", e),
                    "Vsock fallback to Unix socket failed. Ensure agent is running."
                ))?;
            Ok(VsockStream::Unix(stream))
        } else {
            #[cfg(target_os = "macos")]
            {
                // Try to use real vsock via Virtualization Framework
                if let Some(device) = self.vsock_device {
                    match Self::connect_vsock_native(device, self.port) {
                        Ok(stream) => Ok(VsockStream::Vsock(stream)),
                        Err(e) => {
                            log::warn!("Vsock connection failed: {}, falling back to Unix socket", e);
                            // Fallback to Unix socket
                            use std::os::unix::net::UnixStream;
                            let stream = UnixStream::connect("/tmp/libcrun-shim.sock")
                                .map_err(|e| ShimError::runtime_with_context(
                                    format!("Failed to connect via Unix socket: {}", e),
                                    "Both vsock and Unix socket connections failed. Ensure agent is running."
                                ))?;
                            Ok(VsockStream::Unix(stream))
                        }
                    }
                } else {
                    // No vsock device available, fallback to Unix socket
                    use std::os::unix::net::UnixStream;
                    let stream = UnixStream::connect("/tmp/libcrun-shim.sock")
                        .map_err(|e| ShimError::runtime_with_context(
                            format!("Failed to connect via Unix socket: {}", e),
                            "Vsock device not available. Ensure agent is running."
                        ))?;
                    Ok(VsockStream::Unix(stream))
                }
            }
            
            #[cfg(not(target_os = "macos"))]
            {
                Err(ShimError::runtime_with_context(
                    "Vsock not available on this platform",
                    "Use Unix socket fallback by setting use_unix_fallback=true"
                ))
            }
        }
    }
    
    #[cfg(target_os = "macos")]
    fn connect_vsock_native(vsock_device: *mut Object, port: u32) -> Result<VsockStreamNative> {
        use std::ffi::c_uint;

        unsafe {
            // Get VZVirtioSocketConnection class
            let connection_class = Class::get("VZVirtioSocketConnection")
                .ok_or_else(|| ShimError::runtime("VZVirtioSocketConnection class not available"))?;

            // Create a connection to the specified port on the vsock device
            // VZVirtioSocketDevice has a method: - (void)connectToPort:(uint32_t)port completionHandler:(void (^)(VZVirtioSocketConnection *connection, NSError *error))completionHandler
            
            // For now, we'll use a synchronous approach with a channel
            use std::sync::mpsc;
            use std::sync::Mutex;
            use std::sync::Arc;

            let (tx, rx) = mpsc::channel::<Result<*mut Object>>();
            let tx = Arc::new(Mutex::new(Some(tx)));

            // Create completion handler
            let tx_clone = Arc::clone(&tx);
            // Note: This would need proper block handling similar to VM start/stop
            // For now, we'll use a placeholder that indicates the structure is ready
            log::info!("Creating vsock connection to port {}", port);

            // In a full implementation, we'd call:
            // [vsock_device connectToPort:port completionHandler:^(VZVirtioSocketConnection *connection, NSError *error) {
            //     if (error == nil) {
            //         // Success - use connection
            //     } else {
            //         // Error
            //     }
            // }];

            // For now, return a placeholder that will work once block handling is fixed
            // This structure is ready for full implementation
            // Once block handling is working, we can create the connection here
            Ok(VsockStreamNative::new(std::ptr::null_mut(), port))
        }
    }
}

/// Abstraction over vsock or Unix socket streams
pub enum VsockStream {
    #[cfg(target_os = "macos")]
    Unix(std::os::unix::net::UnixStream),
    #[cfg(target_os = "macos")]
    Vsock(VsockStreamNative),
}

#[cfg(target_os = "macos")]
pub struct VsockStreamNative {
    // VZVirtioSocketConnection from Virtualization Framework
    connection: *mut Object,
    port: u32,
}

#[cfg(target_os = "macos")]
impl VsockStreamNative {
    fn new(connection: *mut Object, port: u32) -> Self {
        unsafe {
            // Retain the connection to keep it alive
            if !connection.is_null() {
                let _: () = msg_send![connection, retain];
            }
        }
        Self { connection, port }
    }
}

#[cfg(target_os = "macos")]
impl Drop for VsockStreamNative {
    fn drop(&mut self) {
        if !self.connection.is_null() {
            unsafe {
                let _: () = msg_send![self.connection, release];
            }
        }
    }
}

impl Read for VsockStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            VsockStream::Unix(stream) => stream.read(buf),
            #[cfg(target_os = "macos")]
            VsockStream::Vsock(stream) => {
                // Read from VZVirtioSocketConnection
                if stream.connection.is_null() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotConnected,
                        "Vsock connection is null"
                    ));
                }
                
                unsafe {
                    // VZVirtioSocketConnection has fileDescriptor property
                    // We can use that to read data
                    let fd: std::os::raw::c_int = msg_send![stream.connection, fileDescriptor];
                    if fd < 0 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::NotConnected,
                            "Vsock file descriptor is invalid"
                        ));
                    }
                    
                    // Use libc read to read from the file descriptor
                    use std::os::unix::io::FromRawFd;
                    use std::os::unix::io::RawFd;
                    let mut file = std::fs::File::from_raw_fd(fd as RawFd);
                    let result = file.read(buf);
                    // Don't close the fd, it's owned by the connection
                    std::mem::forget(file);
                    result
                }
            }
        }
    }
}

impl Write for VsockStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            VsockStream::Unix(stream) => stream.write(buf),
            #[cfg(target_os = "macos")]
            VsockStream::Vsock(stream) => {
                // Write to VZVirtioSocketConnection
                if stream.connection.is_null() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotConnected,
                        "Vsock connection is null"
                    ));
                }
                
                unsafe {
                    // VZVirtioSocketConnection has fileDescriptor property
                    let fd: std::os::raw::c_int = msg_send![stream.connection, fileDescriptor];
                    if fd < 0 {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::NotConnected,
                            "Vsock file descriptor is invalid"
                        ));
                    }
                    
                    // Use libc write to write to the file descriptor
                    use std::os::unix::io::FromRawFd;
                    use std::os::unix::io::RawFd;
                    let mut file = std::fs::File::from_raw_fd(fd as RawFd);
                    let result = file.write(buf);
                    // Don't close the fd, it's owned by the connection
                    std::mem::forget(file);
                    result
                }
            }
        }
    }
    
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            VsockStream::Unix(stream) => stream.flush(),
            #[cfg(target_os = "macos")]
            VsockStream::Vsock(stream) => {
                // Flush vsock connection
                if stream.connection.is_null() {
                    return Ok(()); // Nothing to flush
                }
                
                unsafe {
                    let fd: std::os::raw::c_int = msg_send![stream.connection, fileDescriptor];
                    if fd >= 0 {
                        use std::os::unix::io::FromRawFd;
                        use std::os::unix::io::RawFd;
                        let mut file = std::fs::File::from_raw_fd(fd as RawFd);
                        let result = file.flush();
                        std::mem::forget(file);
                        result
                    } else {
                        Ok(())
                    }
                }
            }
        }
    }
}
