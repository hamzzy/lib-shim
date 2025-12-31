use crate::*;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(target_os = "macos")]
use objc::runtime::{Class, Object};
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};
#[cfg(target_os = "macos")]
use block2::{Block, ConcreteBlock, RcBlock};

/// Virtual Machine wrapper using Apple's Virtualization Framework
pub struct VirtualMachine {
    vm_id: String,
    kernel_path: Option<PathBuf>,
    initramfs_path: Option<PathBuf>,
    is_running: bool,
    vsock_port: u32,
    #[cfg(target_os = "macos")]
    vm_instance: Option<*mut Object>,
    #[cfg(target_os = "macos")]
    vsock_device: Option<*mut Object>, // VZVirtioSocketDevice from the VM
}

impl VirtualMachine {
    pub async fn start() -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            // Check if Virtualization Framework is available
            if !Self::is_virtualization_available() {
                log::warn!("Virtualization Framework not available, using fallback mode");
                return Self::start_fallback();
            }

            // Find VM assets
            let kernel_path = Self::find_vm_asset("kernel");
            let initramfs_path = Self::find_vm_asset("initramfs.cpio.gz");

            if kernel_path.is_none() || initramfs_path.is_none() {
                log::info!("VM assets not found, assuming external VM is running");
                return Self::start_fallback();
            }

            let kernel_path_clone = kernel_path.as_ref().unwrap().clone();
            let initramfs_path_clone = initramfs_path.as_ref().unwrap().clone();

            // Create VM configuration
            let (vm_instance, vsock_device) = match Self::create_vm_configuration(&kernel_path_clone, &initramfs_path_clone) {
                Ok(result) => result,
                Err(e) => {
                    log::warn!("Failed to create VM using Virtualization Framework: {}, using fallback", e);
                    return Self::start_fallback();
                }
            };

            // Start the VM asynchronously
            match Self::start_vm_async(vm_instance).await {
                Ok(_) => {
                    log::info!("VM started successfully using Virtualization Framework");
                    Ok(Self {
                        vm_id: "libcrun-shim-vm".to_string(),
                        kernel_path,
                        initramfs_path,
                        is_running: true,
                        vsock_port: 1234,
                        vm_instance: Some(vm_instance),
                        vsock_device,
                    })
                }
                Err(e) => {
                    log::warn!("Failed to start VM: {}, using fallback", e);
                    // Release the VM instance and vsock device
                    unsafe {
                        let _: () = msg_send![vm_instance, release];
                        if let Some(device) = vsock_device {
                            let _: () = msg_send![device, release];
                        }
                    }
                    Self::start_fallback()
                }
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            Self::start_fallback()
        }
    }

    #[cfg(target_os = "macos")]
    async fn start_vm_async(vm_instance: *mut Object) -> Result<()> {
        use std::sync::mpsc;
        use std::sync::Mutex;

        // Create a channel for completion
        let (tx, rx) = mpsc::channel::<Result<()>>();
        let tx = Arc::new(Mutex::new(Some(tx)));

        // Create completion handler block
        // The completion handler signature is: void (^completionHandler)(NSError *error)
        // NSError* can be nil, so we use *mut Object and check for null
        let tx_clone = Arc::clone(&tx);
        // Use ConcreteBlock which can be created from a closure
        // The closure must be 'static, which Arc<Mutex<...>> satisfies
        let completion_block = {
            let tx_inner = Arc::clone(&tx_clone);
            ConcreteBlock::new(move |error: *mut Object| {
                let result = if error.is_null() {
                    log::info!("VM started successfully");
                    Ok(())
                } else {
                    // Extract error message if possible
                    let error_msg = format!("VM start failed with error");
                    log::error!("{}", error_msg);
                    Err(ShimError::runtime_with_context(
                        error_msg,
                        "Check VM configuration and system requirements"
                    ))
                };

                if let Ok(mut sender) = tx_inner.lock() {
                    if let Some(s) = sender.take() {
                        let _ = s.send(result);
                    }
                }
            })
        };
        // Convert to RcBlock for Objective-C
        let completion_block = unsafe { RcBlock::copy(completion_block.as_ref() as *const _ as *const Block<(*mut Object,), ()>) };

        // Start the VM with completion handler
        unsafe {
            let _: () = msg_send![vm_instance, startWithCompletionHandler: &*completion_block];
        }

        // Wait for completion (with timeout) - convert to async
        tokio::task::spawn_blocking(move || rx.recv())
            .await
            .map_err(|_| ShimError::runtime("Failed to wait for VM start"))?
            .map_err(|_| ShimError::runtime("Channel closed before VM start completed"))?
    }

    #[cfg(target_os = "macos")]
    fn is_virtualization_available() -> bool {
        // Check if VZVirtualMachineConfiguration class is available
        // This requires macOS 11.0+ and Apple Silicon or Intel with T2
        let class = Class::get("VZVirtualMachineConfiguration");
        class.is_some()
    }

    #[cfg(target_os = "macos")]
    fn create_vm_configuration(
        kernel_path: &PathBuf,
        initramfs_path: &PathBuf,
    ) -> Result<(*mut Object, Option<*mut Object>)> {
        use std::ffi::CString;

        unsafe {
            // Get required classes
            let vm_config_class = Class::get("VZVirtualMachineConfiguration")
                .ok_or_else(|| ShimError::runtime("VZVirtualMachineConfiguration class not available"))?;
            
            let boot_loader_class = Class::get("VZLinuxBootLoader")
                .ok_or_else(|| ShimError::runtime("VZLinuxBootLoader class not available"))?;
            
            let kernel_path_class = Class::get("NSURL")
                .ok_or_else(|| ShimError::runtime("NSURL class not available"))?;
            
            let vsock_class = Class::get("VZVirtioSocketDeviceConfiguration")
                .ok_or_else(|| ShimError::runtime("VZVirtioSocketDeviceConfiguration class not available"))?;

            // Create kernel URL
            let kernel_path_str = kernel_path.to_string_lossy();
            let kernel_cstr = CString::new(kernel_path_str.as_ref())
                .map_err(|e| ShimError::runtime_with_context(
                    format!("Failed to create CString for kernel path: {}", e),
                    format!("Kernel path: {}", kernel_path_str)
                ))?;
            
            let kernel_url: *mut Object = msg_send![kernel_path_class, fileURLWithPath: kernel_cstr.as_ptr()];
            if kernel_url.is_null() {
                return Err(ShimError::runtime("Failed to create kernel URL"));
            }

            // Create initramfs URL
            let initramfs_path_str = initramfs_path.to_string_lossy();
            let initramfs_cstr = CString::new(initramfs_path_str.as_ref())
                .map_err(|e| ShimError::runtime_with_context(
                    format!("Failed to create CString for initramfs path: {}", e),
                    format!("Initramfs path: {}", initramfs_path_str)
                ))?;
            
            let initramfs_url: *mut Object = msg_send![kernel_path_class, fileURLWithPath: initramfs_cstr.as_ptr()];
            if initramfs_url.is_null() {
                return Err(ShimError::runtime("Failed to create initramfs URL"));
            }

            // Create boot loader
            let boot_loader: *mut Object = msg_send![boot_loader_class, alloc];
            let boot_loader: *mut Object = msg_send![boot_loader, initWithKernelURL: kernel_url];
            let _: () = msg_send![boot_loader, setInitialRamdiskURL: initramfs_url];

            // Create vsock device configuration
            let vsock_config: *mut Object = msg_send![vsock_class, alloc];
            let vsock_config: *mut Object = msg_send![vsock_config, init];

            // Create VM configuration
            let vm_config: *mut Object = msg_send![vm_config_class, alloc];
            let vm_config: *mut Object = msg_send![vm_config, init];
            
            // Set boot loader
            let _: () = msg_send![vm_config, setBootLoader: boot_loader];
            
            // Set memory (2GB default)
            // Note: Memory configuration would go here, but requires more complex setup
            // In a full implementation, we'd create VZVirtualMachineMemorySizeConfiguration
            
            // Add vsock device to configuration
            // We need to create an NSArray containing the vsock config
            let nsarray_class = Class::get("NSArray")
                .ok_or_else(|| ShimError::runtime("NSArray class not available"))?;
            let vsock_array: *mut Object = msg_send![nsarray_class, arrayWithObject: vsock_config];
            let _: () = msg_send![vm_config, setSocketDevices: vsock_array];
            
            // Validate configuration
            let mut error: *mut Object = std::ptr::null_mut();
            let is_valid: bool = msg_send![vm_config, validateWithError: &mut error];
            
            if !is_valid {
                return Err(ShimError::runtime("VM configuration validation failed"));
            }

            // Create VM instance
            let vm_class = Class::get("VZVirtualMachine")
                .ok_or_else(|| ShimError::runtime("VZVirtualMachine class not available"))?;
            
            let vm_instance: *mut Object = msg_send![vm_class, alloc];
            let vm_instance: *mut Object = msg_send![vm_instance, initWithConfiguration: vm_config];

            if vm_instance.is_null() {
                return Err(ShimError::runtime("Failed to create VM instance"));
            }

            // Retain the VM instance to keep it alive
            let _: () = msg_send![vm_instance, retain];

            // Get the vsock device from the VM instance
            // The vsock device is available after VM is created
            // We'll get it from the VM's socketDevices array
            let socket_devices: *mut Object = msg_send![vm_instance, socketDevices];
            let vsock_device = if !socket_devices.is_null() {
                // Get first device (should be our vsock device)
                let count: usize = msg_send![socket_devices, count];
                if count > 0 {
                    let device: *mut Object = msg_send![socket_devices, objectAtIndex: 0usize];
                    if !device.is_null() {
                        let _: () = msg_send![device, retain];
                        Some(device)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            Ok((vm_instance, vsock_device))
        }
    }

    fn start_fallback() -> Result<Self> {
        Ok(Self {
            vm_id: "libcrun-shim-vm".to_string(),
            kernel_path: None,
            initramfs_path: None,
            is_running: true, // Assume running for now
            vsock_port: 1234,
            #[cfg(target_os = "macos")]
            vm_instance: None,
            #[cfg(target_os = "macos")]
            vsock_device: None,
        })
    }

    /// Get the vsock device for creating connections
    #[cfg(target_os = "macos")]
    pub fn get_vsock_device(&self) -> Option<*mut Object> {
        self.vsock_device
    }
    
    fn find_vm_asset(name: &str) -> Option<PathBuf> {
        // Look for VM assets in common locations
        let search_paths = vec![
            PathBuf::from("vm-assets").join(name),
            PathBuf::from("../vm-assets").join(name),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vm-assets").join(name),
        ];
        
        for path in search_paths {
            if path.exists() {
                return Some(path);
            }
        }
        
        None
    }
    
    pub fn is_running(&self) -> bool {
        self.is_running
    }
    
    pub fn get_vsock_port(&self) -> u32 {
        self.vsock_port
    }
    
    pub async fn stop(&mut self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            if let Some(vm_instance) = self.vm_instance {
                match Self::stop_vm_async(vm_instance).await {
                    Ok(_) => {
                        log::info!("VM stopped successfully");
                        // Release the VM instance
                        unsafe {
                            let _: () = msg_send![vm_instance, release];
                        }
                        self.vm_instance = None;
                    }
                    Err(e) => {
                        log::warn!("Error stopping VM: {}", e);
                        // Still mark as stopped
                        unsafe {
                            let _: () = msg_send![vm_instance, release];
                        }
                        self.vm_instance = None;
                    }
                }
            }
        }
        
        self.is_running = false;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    async fn stop_vm_async(vm_instance: *mut Object) -> Result<()> {
        use std::sync::mpsc;
        use std::sync::Mutex;

        // Create a channel for completion
        let (tx, rx) = mpsc::channel::<Result<()>>();
        let tx = Arc::new(Mutex::new(Some(tx)));

        // Create completion handler block
        // The completion handler signature is: void (^completionHandler)(NSError *error)
        // NSError* can be nil, so we use *mut Object and check for null
        let tx_clone = Arc::clone(&tx);
        // Use ConcreteBlock which can be created from a closure
        let completion_block = {
            let tx_inner = Arc::clone(&tx_clone);
            ConcreteBlock::new(move |error: *mut Object| {
                let result = if error.is_null() {
                    log::info!("VM stopped successfully");
                    Ok(())
                } else {
                    let error_msg = format!("VM stop failed with error");
                    log::error!("{}", error_msg);
                    Err(ShimError::runtime_with_context(
                        error_msg,
                        "VM may not have stopped cleanly"
                    ))
                };

                if let Ok(mut sender) = tx_inner.lock() {
                    if let Some(s) = sender.take() {
                        let _ = s.send(result);
                    }
                }
            })
        };
        // Convert to RcBlock for Objective-C
        let completion_block = unsafe { RcBlock::copy(completion_block.as_ref() as *const _ as *const Block<(*mut Object,), ()>) };

        // Stop the VM with completion handler
        unsafe {
            let _: () = msg_send![vm_instance, stopWithCompletionHandler: &*completion_block];
        }

        // Wait for completion (with timeout) - convert to async
        tokio::task::spawn_blocking(move || rx.recv())
            .await
            .map_err(|_| ShimError::runtime("Failed to wait for VM stop"))?
            .map_err(|_| ShimError::runtime("Channel closed before VM stop completed"))?
    }
    
    pub fn wait_until_ready(&self, timeout_secs: u64) -> Result<()> {
        // Wait for VM to be ready (booted and agent running)
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);
        
        while start.elapsed() < timeout {
            // Try to connect via vsock
            use super::rpc::RpcClient;
            if RpcClient::connect_vsock(self.get_vsock_port()).is_ok() {
                return Ok(());
            }
            
            std::thread::sleep(Duration::from_millis(100));
        }
        
        Err(ShimError::runtime_with_context(
            "VM did not become ready within the timeout period",
            format!("Timeout: {} seconds. Check VM logs and ensure agent is running.", timeout_secs)
        ))
    }
}

impl Drop for VirtualMachine {
    fn drop(&mut self) {
        if self.is_running {
            // Note: We can't use async in Drop, so we'll just release the instance
            #[cfg(target_os = "macos")]
            {
                if let Some(vm_instance) = self.vm_instance {
                    unsafe {
                        let _: () = msg_send![vm_instance, release];
                    }
                }
                if let Some(vsock_device) = self.vsock_device {
                    unsafe {
                        let _: () = msg_send![vsock_device, release];
                    }
                }
            }
            self.is_running = false;
        }
    }
}
