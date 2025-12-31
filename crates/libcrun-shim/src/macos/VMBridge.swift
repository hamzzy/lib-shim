//
//  VMBridge.swift
//  Swift bridge for Virtualization Framework operations
//

import Foundation
import Virtualization

/// Bridge class for handling VM operations with proper async completion handlers
@available(macOS 12.0, *)
@objc public class VMBridge: NSObject {

    private var virtualMachine: VZVirtualMachine?
    private var completionHandler: ((Bool, String?) -> Void)?

    /// Create a Linux VM with the specified configuration
    @objc public func createVMWithKernelPath(_ kernelPath: String, initramfsPath: String, memoryBytes: UInt64, cpuCount: UInt32) -> Bool {
        do {
            // Create boot loader
            let bootLoader = VZLinuxBootLoader(kernelURL: URL(fileURLWithPath: kernelPath))
            bootLoader.initialRamdiskURL = URL(fileURLWithPath: initramfsPath)

            // Create vsock device
            let vsockDevice = VZVirtioSocketDeviceConfiguration()

            // Create VM configuration
            let config = VZVirtualMachineConfiguration()
            config.bootLoader = bootLoader
            config.socketDevices = [vsockDevice]

            // Set memory - ensure minimum 512MB
            let minMemory: UInt64 = 512 * 1024 * 1024
            let actualMemory = max(memoryBytes, minMemory)
            config.memorySize = actualMemory

            // Set CPU count - ensure minimum 1, maximum available cores
            let maxCpus = UInt32(ProcessInfo.processInfo.processorCount)
            let actualCpus = max(1, min(cpuCount, maxCpus))
            config.cpuCount = Int(actualCpus)

            print("VM configuration: memory=\(actualMemory / 1024 / 1024)MB, cpus=\(actualCpus)")

            // Validate configuration
            try config.validate()

            // Create VM instance
            virtualMachine = VZVirtualMachine(configuration: config)

            print("VM created successfully")
            return true

        } catch {
            print("Failed to create VM: \(error)")
            return false
        }
    }

    /// Start the VM with async completion handler
    @objc public func startVMWithCompletion(_ completion: @escaping (Bool, String?) -> Void) {
        guard let vm = virtualMachine else {
            completion(false, "VM not created")
            return
        }

        completionHandler = completion

        vm.start { result in
            switch result {
            case .success:
                print("VM started successfully")
                self.completionHandler?(true, nil)
            case .failure(let error):
                print("VM failed to start: \(error)")
                self.completionHandler?(false, error.localizedDescription)
            }
            self.completionHandler = nil
        }
    }

    /// Stop the VM with async completion handler
    @objc public func stopVMWithCompletion(_ completion: @escaping (Bool, String?) -> Void) {
        guard let vm = virtualMachine else {
            completion(false, "VM not created")
            return
        }

        completionHandler = completion

        // The stop() method uses a different signature - it passes an optional error
        vm.stop { error in
            if let error = error {
                print("VM failed to stop: \(error)")
                self.completionHandler?(false, error.localizedDescription)
            } else {
                print("VM stopped successfully")
                self.completionHandler?(true, nil)
            }
            self.completionHandler = nil
        }
    }

    /// Get VM state
    @objc public func getVMState() -> Int {
        guard let vm = virtualMachine else {
            return -1 // Invalid state
        }

        switch vm.state {
        case .stopped:
            return 1
        case .running:
            return 3
        case .paused:
            return 2
        case .error:
            return 4
        case .starting:
            return 0
        case .pausing:
            return 5
        case .resuming:
            return 6
        case .stopping:
            return 7
        case .saving:
            return 8
        case .restoring:
            return 9
        @unknown default:
            return -1
        }
    }

    /// Check if VM can start
    @objc public func canStartVM() -> Bool {
        guard let vm = virtualMachine else {
            return false
        }
        return vm.canStart
    }

    /// Check if VM can stop
    @objc public func canStopVM() -> Bool {
        guard let vm = virtualMachine else {
            return false
        }
        return vm.canStop
    }

    /// Get vsock device for communication
    @objc public func getVsockDevice() -> VZVirtioSocketDevice? {
        guard let vm = virtualMachine else {
            return nil
        }

        // Return the first vsock device
        return vm.socketDevices.first as? VZVirtioSocketDevice
    }

    /// Connect to a vsock port and return the file descriptor
    /// Returns -1 on error
    @objc public func connectToVsockPort(_ port: UInt32, completion: @escaping (Int32, String?) -> Void) {
        guard let vsockDevice = getVsockDevice() else {
            completion(-1, "No vsock device available")
            return
        }

        vsockDevice.connect(toPort: port) { result in
            switch result {
            case .success(let connection):
                // Get the file descriptor from the connection
                let fd = connection.fileDescriptor
                print("Vsock connection established, fd: \(fd)")
                completion(fd, nil)
            case .failure(let error):
                print("Vsock connection failed: \(error)")
                completion(-1, error.localizedDescription)
            }
        }
    }
}

// MARK: - C Interface Functions

/// Create a new VM bridge instance
@available(macOS 12.0, *)
@_cdecl("vm_bridge_create")
public func vm_bridge_create() -> UnsafeMutableRawPointer? {
    let bridge = VMBridge()
    return Unmanaged.passRetained(bridge).toOpaque()
}

/// Destroy VM bridge instance
@available(macOS 12.0, *)
@_cdecl("vm_bridge_destroy")
public func vm_bridge_destroy(_ handle: UnsafeMutableRawPointer?) {
    guard let handle = handle else { return }
    Unmanaged<VMBridge>.fromOpaque(handle).release()
}

/// Create VM with paths and resource configuration
@available(macOS 12.0, *)
@_cdecl("vm_bridge_create_vm")
public func vm_bridge_create_vm(_ handle: UnsafeMutableRawPointer?, _ kernelPath: UnsafePointer<CChar>, _ initramfsPath: UnsafePointer<CChar>, _ memoryBytes: UInt64, _ cpuCount: UInt32) -> Bool {
    guard let handle = handle else { return false }
    let bridge = Unmanaged<VMBridge>.fromOpaque(handle).takeUnretainedValue()

    let kernel = String(cString: kernelPath)
    let initramfs = String(cString: initramfsPath)

    return bridge.createVMWithKernelPath(kernel, initramfsPath: initramfs, memoryBytes: memoryBytes, cpuCount: cpuCount)
}

/// Start VM with completion callback
@available(macOS 12.0, *)
@_cdecl("vm_bridge_start_vm")
public func vm_bridge_start_vm(_ handle: UnsafeMutableRawPointer?, _ callback: @escaping @convention(c) (Bool, UnsafePointer<CChar>?) -> Void) {
    guard let handle = handle else {
        callback(false, "Invalid handle".cString(using: .utf8))
        return
    }

    let bridge = Unmanaged<VMBridge>.fromOpaque(handle).takeUnretainedValue()

    bridge.startVMWithCompletion { success, errorMessage in
        if success {
            callback(true, nil)
        } else {
            let cString = errorMessage?.cString(using: .utf8) ?? "Unknown error".cString(using: .utf8)
            callback(false, cString)
        }
    }
}

/// Stop VM with completion callback
@available(macOS 12.0, *)
@_cdecl("vm_bridge_stop_vm")
public func vm_bridge_stop_vm(_ handle: UnsafeMutableRawPointer?, _ callback: @escaping @convention(c) (Bool, UnsafePointer<CChar>?) -> Void) {
    guard let handle = handle else {
        callback(false, "Invalid handle".cString(using: .utf8))
        return
    }

    let bridge = Unmanaged<VMBridge>.fromOpaque(handle).takeUnretainedValue()

    bridge.stopVMWithCompletion { success, errorMessage in
        if success {
            callback(true, nil)
        } else {
            let cString = errorMessage?.cString(using: .utf8) ?? "Unknown error".cString(using: .utf8)
            callback(false, cString)
        }
    }
}

/// Get VM state
@available(macOS 12.0, *)
@_cdecl("vm_bridge_get_state")
public func vm_bridge_get_state(_ handle: UnsafeMutableRawPointer?) -> Int32 {
    guard let handle = handle else { return -1 }
    let bridge = Unmanaged<VMBridge>.fromOpaque(handle).takeUnretainedValue()
    return Int32(bridge.getVMState())
}

/// Check if VM can start
@available(macOS 12.0, *)
@_cdecl("vm_bridge_can_start")
public func vm_bridge_can_start(_ handle: UnsafeMutableRawPointer?) -> Bool {
    guard let handle = handle else { return false }
    let bridge = Unmanaged<VMBridge>.fromOpaque(handle).takeUnretainedValue()
    return bridge.canStartVM()
}

/// Check if VM can stop
@available(macOS 12.0, *)
@_cdecl("vm_bridge_can_stop")
public func vm_bridge_can_stop(_ handle: UnsafeMutableRawPointer?) -> Bool {
    guard let handle = handle else { return false }
    let bridge = Unmanaged<VMBridge>.fromOpaque(handle).takeUnretainedValue()
    return bridge.canStopVM()
}

/// Connect to vsock port
/// Callback receives file descriptor (or -1 on error) and optional error message
@available(macOS 12.0, *)
@_cdecl("vm_bridge_vsock_connect")
public func vm_bridge_vsock_connect(_ handle: UnsafeMutableRawPointer?, _ port: UInt32, _ callback: @escaping @convention(c) (Int32, UnsafePointer<CChar>?) -> Void) {
    guard let handle = handle else {
        callback(-1, "Invalid handle".cString(using: .utf8))
        return
    }

    let bridge = Unmanaged<VMBridge>.fromOpaque(handle).takeUnretainedValue()

    bridge.connectToVsockPort(port) { fd, errorMessage in
        if fd >= 0 {
            callback(fd, nil)
        } else {
            let cString = errorMessage?.cString(using: .utf8) ?? "Unknown error".cString(using: .utf8)
            callback(-1, cString)
        }
    }
}
