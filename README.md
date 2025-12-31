# libcrun-shim

A unified Rust library for running OCI containers on Linux and macOS.

## Features

- Single unified API for both Linux and macOS
- Async/await support
- Complete container lifecycle management (create, start, stop, delete, list)
- Thread-safe state management
- Comprehensive error handling and validation
- macOS Virtualization Framework integration (ready for VM management)
- Vsock communication support (with Unix socket fallback)
- RPC protocol for host-guest communication
- Integration tests and examples

## Usage

```rust
use libcrun_shim::*;

#[tokio::main]
async fn main() {
    let runtime = ContainerRuntime::new().await.unwrap();
    
    let config = ContainerConfig {
        id: "my-container".to_string(),
        rootfs: "/path/to/rootfs".into(),
        command: vec!["sh".to_string()],
        env: vec![],
        working_dir: "/".to_string(),
    };
    
    runtime.create(config).await.unwrap();
    runtime.start("my-container").await.unwrap();
}
```

## Platform Support

- **Linux**: Direct libcrun integration
- **macOS**: Transparent VM-based execution

## Architecture

The library is organized as a Rust workspace with multiple crates:

- `libcrun-shim`: Main library providing the unified API with platform-specific implementations
- `libcrun-shim-proto`: RPC protocol for macOS VM communication (bincode-based)
- `libcrun-shim-agent`: VM guest agent that runs inside the Linux VM (Unix socket server)
- `libcrun-sys`: FFI bindings to libcrun (with bindgen, uses stubs when libcrun not available)

### Platform Implementations

**Linux**: Direct integration with libcrun via FFI bindings. Uses in-memory state management with validation and proper state transitions.

**macOS**: VM-based execution using:
- Virtualization Framework integration (structure ready for VM creation)
- Vsock communication with Unix socket fallback
- RPC client for communicating with the Linux VM guest agent

## Building

```bash
cargo build
```

**Note:** For real container operations on Linux, you need to install libcrun. See [INSTALL.md](INSTALL.md) for installation instructions.

## Running Tests

```bash
# Unit tests
cargo test

# Integration tests (requires agent binary)
cargo test --test integration_test

# Run example
cargo run --example basic_usage
```

## Implementation Status

✅ **Completed:**
- Core types and error handling
- Linux runtime with state management and validation
- macOS runtime with RPC client
- VM guest agent with thread-safe state
- RPC protocol (bincode-based)
- Integration tests
- Example programs
- Vsock communication structure (with Unix fallback)
- macOS Virtualization Framework structure

🚧 **In Progress / Future:**
- Actual libcrun FFI integration (structure ready, needs libcrun installed)
- Full macOS Virtualization Framework VM creation (structure ready)
- Real vsock implementation (currently uses Unix socket fallback)
- Container stdio handling
- File mounting support

## License

Apache-2.0

