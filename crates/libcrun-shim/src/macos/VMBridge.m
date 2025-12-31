//
//  VMBridge.m
//  Objective-C wrapper for Swift VM bridge
//

#import "VMBridge.h"
#import <Foundation/Foundation.h>

// Forward declare Swift class
@class VMBridge;

// Global reference to VM bridge instance
static VMBridge* _vmBridgeInstance = nil;

// Completion callback storage
typedef void (*VMCompletionCallback)(bool success, const char* error_message);
static VMCompletionCallback _currentCallback = NULL;

// Callback wrapper function
static void completionCallback(bool success, NSString* errorMessage) {
    if (_currentCallback) {
        const char* error_cstr = errorMessage ? [errorMessage UTF8String] : NULL;
        _currentCallback(success, error_cstr);
        _currentCallback = NULL;
    }
}

VMBridgeHandle vm_bridge_create(void) {
    if (!_vmBridgeInstance) {
        _vmBridgeInstance = [[VMBridge alloc] init];
    }
    return (__bridge VMBridgeHandle)_vmBridgeInstance;
}

void vm_bridge_destroy(VMBridgeHandle handle) {
    if (_vmBridgeInstance) {
        _vmBridgeInstance = nil;
    }
}

bool vm_bridge_create_vm(VMBridgeHandle handle, const char* kernel_path, const char* initramfs_path) {
    VMBridge* bridge = (__bridge VMBridge*)handle;
    NSString* kernel = [NSString stringWithUTF8String:kernel_path];
    NSString* initramfs = [NSString stringWithUTF8String:initramfs_path];

    return [bridge createVMWithKernelPath:kernel initramfsPath:initramfs];
}

void vm_bridge_start_vm(VMBridgeHandle handle, VMCompletionCallback callback) {
    VMBridge* bridge = (__bridge VMBridge*)handle;
    _currentCallback = callback;

    [bridge startVMWithCompletion:completionCallback];
}

void vm_bridge_stop_vm(VMBridgeHandle handle, VMCompletionCallback callback) {
    VMBridge* bridge = (__bridge VMBridge*)handle;
    _currentCallback = callback;

    [bridge stopVMWithCompletion:completionCallback];
}

int32_t vm_bridge_get_state(VMBridgeHandle handle) {
    VMBridge* bridge = (__bridge VMBridge*)handle;
    return [bridge getVMState];
}

bool vm_bridge_can_start(VMBridgeHandle handle) {
    VMBridge* bridge = (__bridge VMBridge*)handle;
    return [bridge canStartVM];
}

bool vm_bridge_can_stop(VMBridgeHandle handle) {
    VMBridge* bridge = (__bridge VMBridge*)handle;
    return [bridge canStopVM];
}
