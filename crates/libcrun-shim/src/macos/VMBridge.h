//
//  VMBridge.h
//  C interface for VM operations
//

#ifndef VMBridge_h
#define VMBridge_h

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Opaque handle for VM bridge
typedef void* VMBridgeHandle;

// Completion callback type
typedef void (*VMCompletionCallback)(bool success, const char* error_message);

// Create/destroy VM bridge
VMBridgeHandle vm_bridge_create(void);
void vm_bridge_destroy(VMBridgeHandle handle);

// VM operations
bool vm_bridge_create_vm(VMBridgeHandle handle, const char* kernel_path, const char* initramfs_path, uint64_t memory_bytes, uint32_t cpu_count);

// Full VM creation with disks and network
bool vm_bridge_create_vm_full(
    VMBridgeHandle handle,
    const char* kernel_path,
    const char* initramfs_path,
    uint64_t memory_bytes,
    uint32_t cpu_count,
    const char** disk_paths,
    const uint64_t* disk_sizes,
    const bool* disk_read_only,
    uint32_t disk_count,
    const char* network_mode,
    const char* bridge_interface
);

void vm_bridge_start_vm(VMBridgeHandle handle, VMCompletionCallback callback);
void vm_bridge_stop_vm(VMBridgeHandle handle, VMCompletionCallback callback);

// Network interface listing
typedef void (*NetworkInterfaceCallback)(const char* interfaces);
void vm_bridge_list_network_interfaces(NetworkInterfaceCallback callback);

// VM state queries
int32_t vm_bridge_get_state(VMBridgeHandle handle);
bool vm_bridge_can_start(VMBridgeHandle handle);
bool vm_bridge_can_stop(VMBridgeHandle handle);

// Vsock connection
// Callback receives file descriptor (or -1 on error) and optional error message
typedef void (*VsockConnectionCallback)(int32_t fd, const char* error_message);
void vm_bridge_vsock_connect(VMBridgeHandle handle, uint32_t port, VsockConnectionCallback callback);

#ifdef __cplusplus
}
#endif

#endif /* VMBridge_h */
