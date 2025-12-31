# libcrun-shim

A unified Rust library for running OCI containers on Linux and macOS.

## Features

- **Single unified API** for both Linux and macOS
- **Async/await support** for all operations
- **Complete container lifecycle** management (create, start, stop, delete, list)
- **Real libcrun integration** when available (with graceful fallback)
- **Thread-safe state management** with proper resource cleanup
- **Enhanced error messages** with context and actionable suggestions
- **Structured logging** using the `log` crate
- **OCI-compliant** container configuration generation
- **PID retrieval** from container state
- **Docker-based testing** for macOS developers
- **CI/CD ready** with GitHub Actions

## Usage

```rust
use libcrun_shim::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging (optional)
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();
    
    let runtime = ContainerRuntime::new().await?;
    
    let config = ContainerConfig {
        id: "my-container".to_string(),
        rootfs: "/path/to/rootfs".into(),
        command: vec!["sh".to_string()],
        env: vec!["PATH=/usr/bin:/bin".to_string()],
        working_dir: "/".to_string(),
    };
    
    // Create container
    let id = runtime.create(config).await?;
    log::info!("Container created: {}", id);
    
    // Start container
    runtime.start(&id).await?;
    log::info!("Container started");
    
    // List containers
    let containers = runtime.list().await?;
    for container in &containers {
        log::info!("Container: {} - Status: {:?} - PID: {:?}", 
            container.id, container.status, container.pid);
    }
    
    // Stop and delete
    runtime.stop(&id).await?;
    runtime.delete(&id).await?;
    
    Ok(())
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

### On macOS

```bash
# Unit tests (macOS-specific)
cargo test

# Test Linux functionality using Docker
./scripts/test-linux.sh
```

### On Linux

```bash
# Unit tests
cargo test

# Integration tests (requires agent binary)
cargo test --test integration_test

# Run example
cargo run --example basic_usage
```

**Note:** For testing Linux functionality on macOS, see [TESTING.md](TESTING.md) for detailed instructions.

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

✅ **Completed:**
- Real libcrun FFI integration in Linux runtime and agent
- PID retrieval from libcrun container state files
- Complete OCI config generation with namespaces, mounts, and security
- Enhanced error messages with context information
- Structured logging throughout the codebase
- Docker-based testing setup for macOS users
- GitHub Actions CI/CD for automated testing
- Thread-safe libcrun pointer wrappers
- Comprehensive validation and error handling

🚧 **Future Enhancements:**
- Full macOS Virtualization Framework VM creation (structure ready)
- Real vsock implementation (currently uses Unix socket fallback)
- Container stdio handling
- Advanced file mounting support
- Container networking configuration

## License

Apache-2.0

