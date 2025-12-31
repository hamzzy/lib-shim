//
//  VMBridge.swift
//  Swift bridge for Virtualization Framework operations
//

import Foundation
import Virtualization

/// Disk configuration for VM
@available(macOS 12.0, *)
public struct VMDiskConfig {
    var path: String
    var sizeBytes: UInt64
    var readOnly: Bool
    var createIfMissing: Bool
}

/// Network configuration for VM
@available(macOS 12.0, *)
public struct VMNetworkConfig {
    var mode: String  // "nat", "bridged", "none"
    var bridgeInterface: String?
}

/// Bridge class for handling VM operations with proper async completion handlers
@available(macOS 12.0, *)
@objc public class VMBridge: NSObject {

    private var virtualMachine: VZVirtualMachine?
    private var completionHandler: ((Bool, String?) -> Void)?
    private var diskAttachments: [VZDiskImageStorageDeviceAttachment] = []

    /// Create a Linux VM with the specified configuration (legacy method)
    @objc public func createVMWithKernelPath(_ kernelPath: String, initramfsPath: String, memoryBytes: UInt64, cpuCount: UInt32) -> Bool {
        return createVMWithFullConfig(
            kernelPath: kernelPath,
            initramfsPath: initramfsPath,
            memoryBytes: memoryBytes,
            cpuCount: cpuCount,
            disks: [],
            networkMode: "nat",
            bridgeInterface: nil
        )
    }

    /// Create a Linux VM with full configuration including disks and network
    public func createVMWithFullConfig(
        kernelPath: String,
        initramfsPath: String,
        memoryBytes: UInt64,
        cpuCount: UInt32,
        disks: [VMDiskConfig],
        networkMode: String,
        bridgeInterface: String?
    ) -> Bool {
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

            // Configure storage devices
            var storageDevices: [VZStorageDeviceConfiguration] = []
            for diskConfig in disks {
                if let diskDevice = createDiskDevice(config: diskConfig) {
                    storageDevices.append(diskDevice)
                    print("Added disk: \(diskConfig.path)")
                }
            }
            config.storageDevices = storageDevices

            // Configure network
            if let networkDevice = createNetworkDevice(mode: networkMode, bridgeInterface: bridgeInterface) {
                config.networkDevices = [networkDevice]
                print("Network configured: mode=\(networkMode)")
            }

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

    /// Create a disk device from configuration
    private func createDiskDevice(config: VMDiskConfig) -> VZVirtioBlockDeviceConfiguration? {
        let diskURL = URL(fileURLWithPath: config.path)
        let fileManager = FileManager.default

        // Create disk if missing and requested
        if config.createIfMissing && !fileManager.fileExists(atPath: config.path) {
            do {
                try createRawDiskImage(at: diskURL, sizeBytes: config.sizeBytes)
                print("Created disk image: \(config.path) (\(config.sizeBytes / 1024 / 1024)MB)")
            } catch {
                print("Failed to create disk image: \(error)")
                return nil
            }
        }

        // Attach disk
        do {
            let attachment = try VZDiskImageStorageDeviceAttachment(
                url: diskURL,
                readOnly: config.readOnly
            )
            diskAttachments.append(attachment)
            return VZVirtioBlockDeviceConfiguration(attachment: attachment)
        } catch {
            print("Failed to attach disk \(config.path): \(error)")
            return nil
        }
    }

    /// Create a raw disk image file
    private func createRawDiskImage(at url: URL, sizeBytes: UInt64) throws {
        let fileManager = FileManager.default

        // Create parent directory if needed
        let parentDir = url.deletingLastPathComponent()
        if !fileManager.fileExists(atPath: parentDir.path) {
            try fileManager.createDirectory(at: parentDir, withIntermediateDirectories: true)
        }

        // Create sparse file
        fileManager.createFile(atPath: url.path, contents: nil, attributes: nil)
        let handle = try FileHandle(forWritingTo: url)
        try handle.truncate(atOffset: sizeBytes)
        try handle.close()
    }

    /// Create a network device from configuration
    private func createNetworkDevice(mode: String, bridgeInterface: String?) -> VZNetworkDeviceConfiguration? {
        let networkDevice = VZVirtioNetworkDeviceConfiguration()

        switch mode.lowercased() {
        case "nat":
            // NAT networking using macOS's built-in NAT
            networkDevice.attachment = VZNATNetworkDeviceAttachment()
            return networkDevice

        case "bridged":
            // Bridged networking
            if let interfaceName = bridgeInterface {
                // Find the bridge interface
                let interfaces = VZBridgedNetworkInterface.networkInterfaces
                if let bridgeIface = interfaces.first(where: { $0.identifier == interfaceName }) {
                    networkDevice.attachment = VZBridgedNetworkDeviceAttachment(interface: bridgeIface)
                    return networkDevice
                } else {
                    print("Bridge interface '\(interfaceName)' not found. Available: \(interfaces.map { $0.identifier })")
                    // Fall back to first available
                    if let firstIface = interfaces.first {
                        print("Using first available interface: \(firstIface.identifier)")
                        networkDevice.attachment = VZBridgedNetworkDeviceAttachment(interface: firstIface)
                        return networkDevice
                    }
                }
            } else {
                // Use first available interface
                if let firstIface = VZBridgedNetworkInterface.networkInterfaces.first {
                    networkDevice.attachment = VZBridgedNetworkDeviceAttachment(interface: firstIface)
                    return networkDevice
                }
            }
            print("No bridge interface available, falling back to NAT")
            networkDevice.attachment = VZNATNetworkDeviceAttachment()
            return networkDevice

        case "none":
            return nil

        default:
            print("Unknown network mode '\(mode)', using NAT")
            networkDevice.attachment = VZNATNetworkDeviceAttachment()
            return networkDevice
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

/// Create VM with paths and resource configuration (legacy)
@available(macOS 12.0, *)
@_cdecl("vm_bridge_create_vm")
public func vm_bridge_create_vm(_ handle: UnsafeMutableRawPointer?, _ kernelPath: UnsafePointer<CChar>, _ initramfsPath: UnsafePointer<CChar>, _ memoryBytes: UInt64, _ cpuCount: UInt32) -> Bool {
    guard let handle = handle else { return false }
    let bridge = Unmanaged<VMBridge>.fromOpaque(handle).takeUnretainedValue()

    let kernel = String(cString: kernelPath)
    let initramfs = String(cString: initramfsPath)

    return bridge.createVMWithKernelPath(kernel, initramfsPath: initramfs, memoryBytes: memoryBytes, cpuCount: cpuCount)
}

/// Create VM with full configuration including disks and network
/// diskPaths, diskSizes, diskReadOnly are parallel arrays
/// networkMode: "nat", "bridged", "none"
@available(macOS 12.0, *)
@_cdecl("vm_bridge_create_vm_full")
public func vm_bridge_create_vm_full(
    _ handle: UnsafeMutableRawPointer?,
    _ kernelPath: UnsafePointer<CChar>,
    _ initramfsPath: UnsafePointer<CChar>,
    _ memoryBytes: UInt64,
    _ cpuCount: UInt32,
    _ diskPaths: UnsafePointer<UnsafePointer<CChar>?>?,
    _ diskSizes: UnsafePointer<UInt64>?,
    _ diskReadOnly: UnsafePointer<Bool>?,
    _ diskCount: UInt32,
    _ networkMode: UnsafePointer<CChar>,
    _ bridgeInterface: UnsafePointer<CChar>?
) -> Bool {
    guard let handle = handle else { return false }
    let bridge = Unmanaged<VMBridge>.fromOpaque(handle).takeUnretainedValue()

    let kernel = String(cString: kernelPath)
    let initramfs = String(cString: initramfsPath)
    let netMode = String(cString: networkMode)
    let bridgeIface = bridgeInterface.map { String(cString: $0) }

    // Parse disk configurations
    var disks: [VMDiskConfig] = []
    if diskCount > 0, let paths = diskPaths, let sizes = diskSizes, let readOnlys = diskReadOnly {
        for i in 0..<Int(diskCount) {
            if let pathPtr = paths[i] {
                let path = String(cString: pathPtr)
                let size = sizes[i]
                let ro = readOnlys[i]
                disks.append(VMDiskConfig(
                    path: path,
                    sizeBytes: size,
                    readOnly: ro,
                    createIfMissing: true
                ))
            }
        }
    }

    return bridge.createVMWithFullConfig(
        kernelPath: kernel,
        initramfsPath: initramfs,
        memoryBytes: memoryBytes,
        cpuCount: cpuCount,
        disks: disks,
        networkMode: netMode,
        bridgeInterface: bridgeIface
    )
}

/// Get list of available network interfaces for bridged mode
@available(macOS 12.0, *)
@_cdecl("vm_bridge_list_network_interfaces")
public func vm_bridge_list_network_interfaces(_ callback: @escaping @convention(c) (UnsafePointer<CChar>?) -> Void) {
    let interfaces = VZBridgedNetworkInterface.networkInterfaces
    let names = interfaces.map { $0.identifier }.joined(separator: ",")
    callback(names.cString(using: .utf8))
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
