use crate::types::RuntimeConfig;
use crate::*;
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "macos")]
use objc::runtime::Class;

// FFI declarations for Swift VM bridge
#[cfg(target_os = "macos")]
#[allow(dead_code)]
extern "C" {
    fn vm_bridge_create() -> *mut c_void;
    fn vm_bridge_destroy(handle: *mut c_void);
    fn vm_bridge_create_vm(
        handle: *mut c_void,
        kernel_path: *const c_char,
        initramfs_path: *const c_char,
        memory_bytes: u64,
        cpu_count: u32,
    ) -> bool;
    fn vm_bridge_create_vm_full(
        handle: *mut c_void,
        kernel_path: *const c_char,
        initramfs_path: *const c_char,
        memory_bytes: u64,
        cpu_count: u32,
        disk_paths: *const *const c_char,
        disk_sizes: *const u64,
        disk_read_only: *const bool,
        disk_count: u32,
        network_mode: *const c_char,
        bridge_interface: *const c_char,
    ) -> bool;
    fn vm_bridge_start_vm(handle: *mut c_void, callback: extern "C" fn(bool, *const c_char));
    fn vm_bridge_stop_vm(handle: *mut c_void, callback: extern "C" fn(bool, *const c_char));
    fn vm_bridge_get_state(handle: *mut c_void) -> i32;
    fn vm_bridge_can_start(handle: *mut c_void) -> bool;
    fn vm_bridge_can_stop(handle: *mut c_void) -> bool;
    fn vm_bridge_list_network_interfaces(callback: extern "C" fn(*const c_char));
}

// Global state for async completion - used by callbacks
#[cfg(target_os = "macos")]
static VM_START_SUCCESS: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static VM_START_COMPLETE: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
#[allow(dead_code)]
static VM_STOP_SUCCESS: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
#[allow(dead_code)]
static VM_STOP_COMPLETE: AtomicBool = AtomicBool::new(false);

// Callback functions for Swift bridge
#[cfg(target_os = "macos")]
extern "C" fn vm_start_callback(success: bool, error_msg: *const c_char) {
    if !success && !error_msg.is_null() {
        let error = unsafe { std::ffi::CStr::from_ptr(error_msg).to_string_lossy() };
        log::error!("VM start failed: {}", error);
    } else if success {
        log::info!("VM start completed successfully");
    }
    VM_START_SUCCESS.store(success, Ordering::SeqCst);
    VM_START_COMPLETE.store(true, Ordering::SeqCst);
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
extern "C" fn vm_stop_callback(success: bool, error_msg: *const c_char) {
    if !success && !error_msg.is_null() {
        let error = unsafe { std::ffi::CStr::from_ptr(error_msg).to_string_lossy() };
        log::error!("VM stop failed: {}", error);
    } else if success {
        log::info!("VM stop completed successfully");
    }
    VM_STOP_SUCCESS.store(success, Ordering::SeqCst);
    VM_STOP_COMPLETE.store(true, Ordering::SeqCst);
}

/// Virtual Machine wrapper using Apple's Virtualization Framework via Swift bridge
#[allow(dead_code)]
pub struct VirtualMachine {
    vm_id: String,
    kernel_path: Option<PathBuf>,
    initramfs_path: Option<PathBuf>,
    is_running: bool,
    vsock_port: u32,
    config: RuntimeConfig,
    #[cfg(target_os = "macos")]
    vm_bridge_handle: Option<*mut c_void>,
}

// VirtualMachine is not Send/Sync due to raw pointer, but we only use it on main thread
#[cfg(target_os = "macos")]
unsafe impl Send for VirtualMachine {}
#[cfg(target_os = "macos")]
unsafe impl Sync for VirtualMachine {}

impl VirtualMachine {
    /// Start a VM with default configuration
    #[allow(dead_code)]
    pub async fn start() -> Result<Self> {
        Self::start_with_config(RuntimeConfig::from_env()).await
    }

    /// Start a VM with custom configuration
    pub async fn start_with_config(config: RuntimeConfig) -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            // Check if Virtualization Framework is available
            if !Self::is_virtualization_available() {
                log::warn!("Virtualization Framework not available, using fallback mode");
                return Self::start_fallback(config);
            }

            // Find VM assets using configured search paths
            let search_paths = config.get_vm_asset_search_paths();
            let kernel_path = Self::find_vm_asset("kernel", &search_paths);
            let initramfs_path = Self::find_vm_asset("initramfs.cpio.gz", &search_paths);

            if kernel_path.is_none() || initramfs_path.is_none() {
                log::info!(
                    "VM assets not found in paths: {:?}, assuming external VM is running",
                    search_paths
                );
                return Self::start_fallback(config);
            }

            let kernel_path_val = kernel_path.as_ref().unwrap().clone();
            let initramfs_path_val = initramfs_path.as_ref().unwrap().clone();

            log::info!("Found VM kernel: {}", kernel_path_val.display());
            log::info!("Found VM initramfs: {}", initramfs_path_val.display());

            // Create Swift bridge
            let bridge_handle = unsafe { vm_bridge_create() };
            if bridge_handle.is_null() {
                log::warn!("Failed to create VM bridge, using fallback mode");
                return Self::start_fallback(config);
            }

            log::info!("Swift VM bridge created successfully");

            // Create VM configuration
            let kernel_cstr = match CString::new(kernel_path_val.to_string_lossy().as_ref()) {
                Ok(cstr) => cstr,
                Err(e) => {
                    log::warn!("Invalid kernel path: {}", e);
                    unsafe { vm_bridge_destroy(bridge_handle) };
                    return Self::start_fallback(config);
                }
            };

            let initramfs_cstr = match CString::new(initramfs_path_val.to_string_lossy().as_ref()) {
                Ok(cstr) => cstr,
                Err(e) => {
                    log::warn!("Invalid initramfs path: {}", e);
                    unsafe { vm_bridge_destroy(bridge_handle) };
                    return Self::start_fallback(config);
                }
            };

            log::info!(
                "Creating VM with memory={}MB, cpus={}, disks={}, network={}",
                config.vm_memory / 1024 / 1024,
                config.vm_cpus,
                config.vm_disks.len(),
                config.vm_network.mode
            );

            // Use full config if disks or custom network are configured
            let create_result = if !config.vm_disks.is_empty() || config.vm_network.mode != "nat" {
                // Prepare disk configurations
                let disk_paths_cstrings: Vec<CString> = config
                    .vm_disks
                    .iter()
                    .filter_map(|d| CString::new(d.path.to_string_lossy().as_ref()).ok())
                    .collect();
                let disk_paths_ptrs: Vec<*const c_char> =
                    disk_paths_cstrings.iter().map(|s| s.as_ptr()).collect();
                let disk_sizes: Vec<u64> = config.vm_disks.iter().map(|d| d.size).collect();
                let disk_read_only: Vec<bool> =
                    config.vm_disks.iter().map(|d| d.read_only).collect();

                // Network mode
                let network_mode_cstr =
                    CString::new(config.vm_network.mode.as_str()).unwrap_or_default();
                let bridge_interface_cstr = config
                    .vm_network
                    .bridge_interface
                    .as_ref()
                    .and_then(|s| CString::new(s.as_str()).ok());
                let bridge_ptr = bridge_interface_cstr
                    .as_ref()
                    .map(|s| s.as_ptr())
                    .unwrap_or(std::ptr::null());

                for disk in &config.vm_disks {
                    log::info!(
                        "  Disk: {} ({}MB, {})",
                        disk.path.display(),
                        disk.size / 1024 / 1024,
                        if disk.read_only { "ro" } else { "rw" }
                    );
                }

                unsafe {
                    vm_bridge_create_vm_full(
                        bridge_handle,
                        kernel_cstr.as_ptr(),
                        initramfs_cstr.as_ptr(),
                        config.vm_memory,
                        config.vm_cpus,
                        if disk_paths_ptrs.is_empty() {
                            std::ptr::null()
                        } else {
                            disk_paths_ptrs.as_ptr()
                        },
                        if disk_sizes.is_empty() {
                            std::ptr::null()
                        } else {
                            disk_sizes.as_ptr()
                        },
                        if disk_read_only.is_empty() {
                            std::ptr::null()
                        } else {
                            disk_read_only.as_ptr()
                        },
                        config.vm_disks.len() as u32,
                        network_mode_cstr.as_ptr(),
                        bridge_ptr,
                    )
                }
            } else {
                // Simple creation without disks/network
                unsafe {
                    vm_bridge_create_vm(
                        bridge_handle,
                        kernel_cstr.as_ptr(),
                        initramfs_cstr.as_ptr(),
                        config.vm_memory,
                        config.vm_cpus,
                    )
                }
            };

            if !create_result {
                log::warn!("Failed to create VM via Swift bridge, using fallback mode");
                unsafe { vm_bridge_destroy(bridge_handle) };
                return Self::start_fallback(config);
            }

            log::info!(
                "VM created successfully via Swift bridge (memory={}MB, cpus={}, disks={})",
                config.vm_memory / 1024 / 1024,
                config.vm_cpus,
                config.vm_disks.len()
            );

            // Reset completion flags
            VM_START_COMPLETE.store(false, Ordering::SeqCst);
            VM_START_SUCCESS.store(false, Ordering::SeqCst);

            // Start VM via Swift bridge
            unsafe {
                vm_bridge_start_vm(bridge_handle, vm_start_callback);
            }

            // Wait for completion with timeout from config
            let timeout_ms = config.connection_timeout * 1000;
            let start_time = std::time::Instant::now();

            while !VM_START_COMPLETE.load(Ordering::SeqCst) {
                if start_time.elapsed().as_millis() as u64 > timeout_ms {
                    log::warn!("VM start timed out after {}s", config.connection_timeout);
                    unsafe { vm_bridge_destroy(bridge_handle) };
                    return Self::start_fallback(config);
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }

            if VM_START_SUCCESS.load(Ordering::SeqCst) {
                log::info!("VM started successfully via Swift bridge");
                Ok(Self {
                    vm_id: "libcrun-shim-vm".to_string(),
                    kernel_path,
                    initramfs_path,
                    is_running: true,
                    vsock_port: config.vsock_port,
                    config,
                    vm_bridge_handle: Some(bridge_handle),
                })
            } else {
                log::warn!("VM start failed via Swift bridge, using fallback mode");
                unsafe { vm_bridge_destroy(bridge_handle) };
                Self::start_fallback(config)
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            Self::start_fallback(config)
        }
    }

    #[cfg(target_os = "macos")]
    fn is_virtualization_available() -> bool {
        let class = Class::get("VZVirtualMachineConfiguration");
        class.is_some()
    }

    fn start_fallback(config: RuntimeConfig) -> Result<Self> {
        log::info!("Using fallback mode - assuming external VM is running");
        log::info!("Fallback socket path: {}", config.socket_path.display());
        log::info!("Fallback vsock port: {}", config.vsock_port);
        Ok(Self {
            vm_id: "libcrun-shim-vm".to_string(),
            kernel_path: None,
            initramfs_path: None,
            is_running: true,
            vsock_port: config.vsock_port,
            config,
            #[cfg(target_os = "macos")]
            vm_bridge_handle: None,
        })
    }

    /// Get the runtime configuration
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Check if VM operations are available via Swift bridge
    #[cfg(target_os = "macos")]
    pub fn has_vm_control(&self) -> bool {
        self.vm_bridge_handle.is_some()
    }

    /// Get the VM bridge handle for vsock connections
    #[cfg(target_os = "macos")]
    pub fn get_bridge_handle(&self) -> Option<*mut c_void> {
        self.vm_bridge_handle
    }

    /// Get VM state (0=starting, 1=stopped, 2=paused, 3=running, 4=error)
    #[cfg(target_os = "macos")]
    #[allow(dead_code)]
    pub fn get_state(&self) -> i32 {
        if let Some(handle) = self.vm_bridge_handle {
            unsafe { vm_bridge_get_state(handle) }
        } else {
            -1
        }
    }

    fn find_vm_asset(name: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
        // First, check directly provided paths
        for base_path in search_paths {
            // Check the base path directly (if it's a file)
            if base_path.file_name().map(|n| n == name).unwrap_or(false) && base_path.exists() {
                return Some(base_path.clone());
            }

            // Check as a directory containing vm-assets
            let asset_path = base_path.join("vm-assets").join(name);
            if asset_path.exists() {
                log::debug!("Found VM asset at: {}", asset_path.display());
                return Some(asset_path);
            }

            // Check as a directory directly containing the asset
            let direct_path = base_path.join(name);
            if direct_path.exists() {
                log::debug!("Found VM asset at: {}", direct_path.display());
                return Some(direct_path);
            }
        }

        // Also check relative paths for development
        let dev_paths = [
            PathBuf::from("vm-assets").join(name),
            PathBuf::from("../vm-assets").join(name),
        ];

        for path in dev_paths {
            if path.exists() {
                log::debug!("Found VM asset at: {}", path.display());
                return Some(path);
            }
        }

        log::debug!("VM asset '{}' not found in any search path", name);
        None
    }

    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    #[allow(dead_code)]
    pub fn get_vsock_port(&self) -> u32 {
        self.vsock_port
    }

    #[allow(dead_code)]
    pub async fn stop(&mut self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            if let Some(bridge_handle) = self.vm_bridge_handle {
                // Reset completion flags
                VM_STOP_COMPLETE.store(false, Ordering::SeqCst);
                VM_STOP_SUCCESS.store(false, Ordering::SeqCst);

                // Stop VM via Swift bridge
                unsafe {
                    vm_bridge_stop_vm(bridge_handle, vm_stop_callback);
                }

                // Wait for completion with timeout
                let timeout_ms = 10000; // 10 seconds
                let start_time = std::time::Instant::now();

                while !VM_STOP_COMPLETE.load(Ordering::SeqCst) {
                    if start_time.elapsed().as_millis() > timeout_ms {
                        log::warn!("VM stop timed out, forcing cleanup");
                        break;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }

                if VM_STOP_SUCCESS.load(Ordering::SeqCst) {
                    log::info!("VM stopped successfully via Swift bridge");
                } else {
                    log::warn!("VM stop may not have completed cleanly");
                }

                // Destroy bridge
                unsafe {
                    vm_bridge_destroy(bridge_handle);
                }
                self.vm_bridge_handle = None;
            }
        }

        self.is_running = false;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn wait_until_ready(&self, timeout_secs: u64) -> Result<()> {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        while start.elapsed() < timeout {
            use super::rpc::RpcClient;
            if RpcClient::connect_vsock(self.get_vsock_port()).is_ok() {
                return Ok(());
            }

            std::thread::sleep(Duration::from_millis(100));
        }

        Err(ShimError::runtime_with_context(
            "VM did not become ready within the timeout period",
            format!(
                "Timeout: {} seconds. Check VM logs and ensure agent is running.",
                timeout_secs
            ),
        ))
    }
}

impl Drop for VirtualMachine {
    fn drop(&mut self) {
        if self.is_running {
            #[cfg(target_os = "macos")]
            {
                if let Some(bridge_handle) = self.vm_bridge_handle.take() {
                    log::info!("Cleaning up VM bridge on drop");
                    unsafe {
                        vm_bridge_destroy(bridge_handle);
                    }
                }
            }
            self.is_running = false;
        }
    }
}
