use crate::types::RuntimeConfig;
use crate::*;
use std::io::{Read, Write};
use std::os::raw::{c_char, c_int, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

// FFI declarations for Swift VM bridge vsock functions
#[cfg(target_os = "macos")]
extern "C" {
    fn vm_bridge_vsock_connect(
        handle: *mut c_void,
        port: u32,
        callback: extern "C" fn(i32, *const c_char),
    );
}

// Global state for vsock connection completion
#[cfg(target_os = "macos")]
static VSOCK_CONNECT_FD: AtomicI32 = AtomicI32::new(-1);
#[cfg(target_os = "macos")]
static VSOCK_CONNECT_COMPLETE: AtomicBool = AtomicBool::new(false);

// Callback for vsock connection
#[cfg(target_os = "macos")]
extern "C" fn vsock_connect_callback(fd: i32, error_msg: *const c_char) {
    if fd < 0 && !error_msg.is_null() {
        let error = unsafe { std::ffi::CStr::from_ptr(error_msg).to_string_lossy() };
        log::error!("Vsock connection failed: {}", error);
    } else if fd >= 0 {
        log::info!("Vsock connection established, fd: {}", fd);
    }
    VSOCK_CONNECT_FD.store(fd, Ordering::SeqCst);
    VSOCK_CONNECT_COMPLETE.store(true, Ordering::SeqCst);
}

/// Vsock client for communicating with the Linux VM guest
pub struct VsockClient {
    port: u32,
    socket_path: PathBuf,
    use_unix_fallback: bool,
    connection_timeout: u64,
    #[cfg(target_os = "macos")]
    vm_bridge_handle: Option<*mut c_void>,
}

impl VsockClient {
    /// Create a vsock client with default configuration
    #[allow(dead_code)]
    pub fn new(port: u32) -> Self {
        let config = RuntimeConfig::default();
        Self {
            port,
            socket_path: config.socket_path,
            use_unix_fallback: true,
            connection_timeout: config.connection_timeout,
            #[cfg(target_os = "macos")]
            vm_bridge_handle: None,
        }
    }

    /// Create a vsock client with custom configuration
    pub fn with_config(config: &RuntimeConfig) -> Self {
        Self {
            port: config.vsock_port,
            socket_path: config.socket_path.clone(),
            use_unix_fallback: true,
            connection_timeout: config.connection_timeout,
            #[cfg(target_os = "macos")]
            vm_bridge_handle: None,
        }
    }

    /// Create a vsock client with access to the VM bridge handle
    #[cfg(target_os = "macos")]
    pub fn with_vm_bridge(config: &RuntimeConfig, vm_bridge_handle: *mut c_void) -> Self {
        Self {
            port: config.vsock_port,
            socket_path: config.socket_path.clone(),
            use_unix_fallback: false,
            connection_timeout: config.connection_timeout,
            vm_bridge_handle: Some(vm_bridge_handle),
        }
    }

    pub fn connect(&self) -> Result<VsockStream> {
        if self.use_unix_fallback {
            return self.connect_unix_socket();
        }

        #[cfg(target_os = "macos")]
        {
            if let Some(handle) = self.vm_bridge_handle {
                log::debug!("Attempting native vsock connection to port {}", self.port);

                // #region host log
                let _ = std::fs::OpenOptions::new().create(true).append(true).open("/Users/user/libcrun-shim/.cursor/debug.log").and_then(|mut f| {
                    use std::io::Write;
                    writeln!(f, r#"{{"id":"log_{}","timestamp":{},"location":"host:vsock:connect:entry","message":"Starting vsock connection","data":{{"port":{},"hypothesisId":"B,C"}},"sessionId":"debug-session","runId":"run1"}}"#, 
                        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(), self.port)
                });
                // #endregion

                // Reset connection state
                VSOCK_CONNECT_COMPLETE.store(false, Ordering::SeqCst);
                VSOCK_CONNECT_FD.store(-1, Ordering::SeqCst);

                // Initiate connection via Swift bridge
                unsafe {
                    vm_bridge_vsock_connect(handle, self.port, vsock_connect_callback);
                }

                // Wait for completion with timeout
                let timeout_ms = self.connection_timeout * 1000;
                let start_time = std::time::Instant::now();

                while !VSOCK_CONNECT_COMPLETE.load(Ordering::SeqCst) {
                    if start_time.elapsed().as_millis() as u64 > timeout_ms {
                        log::warn!(
                            "Vsock connection timed out after {}s, falling back to Unix socket",
                            self.connection_timeout
                        );
                        return self.connect_unix_socket();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }

                let fd = VSOCK_CONNECT_FD.load(Ordering::SeqCst);
                if fd >= 0 {
                    log::info!("Native vsock connection established, fd: {}", fd);
                    // #region host log
                    let _ = std::fs::OpenOptions::new().create(true).append(true).open("/Users/user/libcrun-shim/.cursor/debug.log").and_then(|mut f| {
                        use std::io::Write;
                        writeln!(f, r#"{{"id":"log_{}","timestamp":{},"location":"host:vsock:connect:success","message":"Vsock connected","data":{{"fd":{},"port":{},"hypothesisId":"B,C"}},"sessionId":"debug-session","runId":"run1"}}"#, 
                            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(), fd, self.port)
                    });
                    // #endregion
                    return Ok(VsockStream::VsockFd(VsockStreamFd::new(fd)));
                } else {
                    log::warn!("Vsock connection failed, falling back to Unix socket");
                    // #region host log
                    let _ = std::fs::OpenOptions::new().create(true).append(true).open("/Users/user/libcrun-shim/.cursor/debug.log").and_then(|mut f| {
                        use std::io::Write;
                        writeln!(f, r#"{{"id":"log_{}","timestamp":{},"location":"host:vsock:connect:failed","message":"Vsock connection failed","data":{{"fd":{},"port":{},"hypothesisId":"B,C,D"}},"sessionId":"debug-session","runId":"run1"}}"#, 
                            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(), fd, self.port)
                    });
                    // #endregion
                    return self.connect_unix_socket();
                }
            } else {
                log::debug!("No VM bridge handle, using Unix socket fallback");
                return self.connect_unix_socket();
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            self.connect_unix_socket()
        }
    }

    fn connect_unix_socket(&self) -> Result<VsockStream> {
        use std::os::unix::net::UnixStream;
        log::debug!(
            "Connecting to Unix socket at: {}",
            self.socket_path.display()
        );
        let stream = UnixStream::connect(&self.socket_path).map_err(|e| {
            ShimError::runtime_with_context(
                format!("Failed to connect via Unix socket: {}", e),
                format!(
                    "Ensure agent is running and socket is available at: {}",
                    self.socket_path.display()
                ),
            )
        })?;
        log::info!(
            "Unix socket connection established at: {}",
            self.socket_path.display()
        );
        Ok(VsockStream::Unix(stream))
    }
}

/// Abstraction over vsock or Unix socket streams
pub enum VsockStream {
    Unix(std::os::unix::net::UnixStream),
    #[cfg(target_os = "macos")]
    VsockFd(VsockStreamFd),
}

/// Vsock stream using file descriptor from Swift bridge
#[cfg(target_os = "macos")]
pub struct VsockStreamFd {
    fd: c_int,
}

#[cfg(target_os = "macos")]
impl VsockStreamFd {
    pub fn new(fd: c_int) -> Self {
        Self { fd }
    }
}

#[cfg(target_os = "macos")]
impl Drop for VsockStreamFd {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe {
                libc::close(self.fd);
            }
        }
    }
}

impl Read for VsockStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            VsockStream::Unix(stream) => stream.read(buf),
            #[cfg(target_os = "macos")]
            VsockStream::VsockFd(stream) => {
                if stream.fd < 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotConnected,
                        "Vsock file descriptor is invalid",
                    ));
                }

                let result =
                    unsafe { libc::read(stream.fd, buf.as_mut_ptr() as *mut c_void, buf.len()) };

                if result < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(result as usize)
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
            VsockStream::VsockFd(stream) => {
                if stream.fd < 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotConnected,
                        "Vsock file descriptor is invalid",
                    ));
                }

                let result =
                    unsafe { libc::write(stream.fd, buf.as_ptr() as *const c_void, buf.len()) };

                if result < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(result as usize)
                }
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            VsockStream::Unix(stream) => stream.flush(),
            #[cfg(target_os = "macos")]
            VsockStream::VsockFd(stream) => {
                if stream.fd >= 0 {
                    unsafe {
                        libc::fsync(stream.fd);
                    }
                }
                Ok(())
            }
        }
    }
}
