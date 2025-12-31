# libcrun-shim

A unified Rust library for running OCI containers on Linux and macOS.

## Features

- Single API for both Linux and macOS
- No external dependencies
- Async/await support
- Basic container lifecycle management

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

- `libcrun-shim`: Main library providing the unified API
- `libcrun-shim-proto`: RPC protocol for macOS VM communication
- `libcrun-shim-agent`: VM guest agent that runs inside the Linux VM
- `libcrun-sys`: FFI bindings to libcrun (placeholder for future implementation)

## Building

```bash
cargo build
```

## Running Tests

```bash
cargo test
```

## License

Apache-2.0

