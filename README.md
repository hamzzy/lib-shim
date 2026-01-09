# libcrun-shim(WIP)

A unified Rust library for running OCI containers on Linux and macOS with production-ready features.

## Features

- **Unified API** for Linux and macOS
- **Container lifecycle** management (create, start, stop, delete, list)
- **Health checks** with configurable retry logic
- **Metrics collection** (CPU, memory, network, block I/O, PIDs)
- **Container logs** retrieval and streaming
- **Image management** (pull, list, remove OCI images)
- **Error recovery** with automatic cleanup and orphan detection
- **Signal handling** for graceful shutdown
- **Container events** for real-time monitoring
- **macOS VM support** via Apple Virtualization Framework
- **Integration bridges** for containerd (Shim v2) and Kubernetes (CRI)

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
libcrun-shim = "0.1.0"

# Optional features
libcrun-shim = { version = "0.1.0", features = ["shim-v2", "cri"] }
```

## Usage

### Basic Example

```rust
use libcrun_shim::*;

#[tokio::main]
async fn main() -> Result<()> {
    let runtime = ContainerRuntime::new().await?;
    
    // Create container
    let config = ContainerConfig {
        id: "my-container".to_string(),
        rootfs: "/path/to/rootfs".into(),
        command: vec!["sh".to_string()],
        env: vec!["PATH=/usr/bin:/bin".to_string()],
        working_dir: "/".to_string(),
        ..Default::default()
    };
    
    let id = runtime.create(config).await?;
    runtime.start(&id).await?;
    
    // List containers
    let containers = runtime.list().await?;
    for container in &containers {
        println!("{}: {:?}", container.id, container.status);
    }
    
    // Cleanup
    runtime.stop(&id).await?;
    runtime.delete(&id).await?;
    
    Ok(())
}
```

### Health Checks

```rust
let config = ContainerConfig {
    id: "web-server".to_string(),
    rootfs: "/path/to/rootfs".into(),
    command: vec!["nginx".to_string()],
    health_check: Some(HealthCheck {
        command: vec!["curl".to_string(), "-f".to_string(), "http://localhost/health".to_string()],
        interval: 30,
        timeout: 10,
        retries: 3,
        start_period: 60,
    }),
    ..Default::default()
};

let id = runtime.create(config).await?;
runtime.start(&id).await?;

// Check health
let health = runtime.health(&id).await?;
println!("Health status: {:?}", health.status);
```

### Metrics

```rust
// Get metrics for a container
let metrics = runtime.metrics("my-container").await?;
println!("CPU: {} ns total", metrics.cpu.usage_total);
println!("Memory: {} / {} bytes", metrics.memory.usage, metrics.memory.limit);
println!("Network: RX {} TX {} bytes", metrics.network.rx_bytes, metrics.network.tx_bytes);
println!("Block I/O: Read {} Write {} bytes", metrics.blkio.read_bytes, metrics.blkio.write_bytes);
println!("PIDs: {}", metrics.pids.current);

// Get all container metrics
let all_metrics = runtime.all_metrics().await?;
for m in all_metrics {
    println!("{}: CPU {} ns, Memory {} bytes", m.container_id, m.cpu.usage_total, m.memory.usage);
}
```

### Container Logs

```rust
let logs = runtime.logs("my-container", LogOptions {
    follow: false,
    tail: Some(100),
    since: None,
    until: None,
}).await?;

println!("Stdout:\n{}", logs.stdout);
println!("Stderr:\n{}", logs.stderr);
```

### Container Events

```rust
let mut receiver = subscribe_events();

// Create and start container
let id = runtime.create(config).await?;
runtime.start(&id).await?;

// Listen for events
while let Some(event) = receiver.recv().await {
    match event.event_type {
        ContainerEventType::Start => println!("Container started"),
        ContainerEventType::Die => {
            println!("Container exited: {:?}", event.exit_code);
            break;
        }
        _ => {}
    }
}
```

### Image Management

```rust
let mut store = ImageStore::new(ImageStore::default_path())?;

// Pull image
let progress = |p: PullProgress| {
    if p.total_bytes > 0 {
        let percent = (p.downloaded_bytes as f64 / p.total_bytes as f64) * 100.0;
        println!("Progress: {:.1}%", percent);
    }
};
let image = store.pull("docker.io/library/alpine:latest", Some(Box::new(progress))).await?;

// List images
let images = store.list()?;
for img in images {
    println!("{} - {} bytes", img.reference.full_name(), img.size);
}

// Get rootfs
let rootfs = store.get_rootfs("alpine:latest")?;
```

### Error Recovery

```rust
// Cleanup stopped containers
let cleaned = runtime.cleanup_stopped().await?;

// List orphaned containers
let orphaned = runtime.list_orphaned().await?;

// Force delete
runtime.force_delete("stuck-container").await?;

// Graceful shutdown
runtime.shutdown().await?;
```

## CLI Tool

The `crun-shim` CLI provides a command-line interface:

```bash
# Container management
crun-shim create my-container --rootfs /path/to/rootfs --cmd sh
crun-shim start my-container
crun-shim stop my-container
crun-shim delete my-container
crun-shim list

# Monitoring
crun-shim stats my-container
crun-shim logs my-container
crun-shim health my-container
crun-shim events

# Image management
crun-shim pull alpine:latest
crun-shim images
crun-shim rmi alpine:latest

# Error recovery
crun-shim cleanup --orphaned --force
crun-shim recover
crun-shim shutdown
```

## Architecture

### Workspace Structure

```
libcrun-shim/
├── libcrun-shim/          # Main library (unified API)
├── libcrun-sys/           # FFI bindings to libcrun
├── libcrun-shim-proto/    # RPC protocol definitions
├── libcrun-shim-agent/    # VM guest agent (runs in Linux VM)
└── libcrun-shim-cli/      # Command-line tool
```

### Platform Implementations

**Linux:**
- Direct integration with `libcrun` via FFI (`libcrun-sys`)
- Uses `libcrun` for container operations when available
- Graceful fallback to stub implementation if `libcrun` not found
- In-memory state management with validation

**macOS:**
- VM-based execution using Apple Virtualization Framework
- Swift bridge (`VMBridge.swift`) for async VM operations
- Native vsock communication between host and guest
- RPC client communicates with `libcrun-shim-agent` running in VM
- Agent uses `libcrun` to manage containers inside the Linux VM

### Communication Flow (macOS)

```
┌─────────────────┐
│  Rust Runtime   │
│  (macOS Host)   │
└────────┬────────┘
         │ vsock
         ▼
┌─────────────────┐
│  Linux VM       │
│  ┌───────────┐  │
│  │  Agent    │  │
│  │  (RPC)    │  │
│  └─────┬─────┘  │
│        │        │
│        ▼        │
│  ┌───────────┐  │
│  │ libcrun   │  │
│  │ Container │  │
│  └───────────┘  │
└─────────────────┘
```

## Building

```bash
# Build library
cargo build

# Build with features
cargo build --features shim-v2,cri

# Build CLI
cargo build --package libcrun-shim-cli --release

# Run examples
cargo run --example basic_usage
cargo run --example health_metrics
cargo run --example production_setup
```

## Requirements

**Linux:**
- `libcrun` (optional, graceful fallback if not available)
- For real container operations, install `crun` or build from source

**macOS:**
- macOS 12.0+ (Virtualization Framework)
- Linux VM kernel and initramfs (see `vm-image/` directory)
- Cross-compiled `libcrun-shim-agent` for Linux

## Testing

```bash
# Unit tests
cargo test

# Integration tests
cargo test --test integration_tests

# Test on Linux (from macOS)
./scripts/test-linux.sh
```

## Features

- `image-pull` (default): OCI image pulling support
- `shim-v2`: Containerd Shim v2 bridge implementation
- `cri`: Kubernetes CRI bridge implementation

## Integration

### Containerd Shim v2

```rust
use libcrun_shim::*;

let shim = ShimV2::new(
    PathBuf::from("/run/containerd/shim.sock"),
    PathBuf::from("/var/lib/containerd/bundle"),
    "default".to_string(),
);
shim.serve().await?;
```

### Kubernetes CRI

```rust
use libcrun_shim::*;

let mut cri = CriServer::new(PathBuf::from("/run/cri.sock"));
cri.serve().await?;
```

## macOS VM Configuration

```rust
use libcrun_shim::*;

let config = RuntimeConfig {
    socket_path: PathBuf::from("/tmp/libcrun-shim.sock"),
    vsock_port: 1234,
    vm_asset_paths: vec![
        PathBuf::from("/usr/local/share/libcrun-shim/kernel"),
        PathBuf::from("/usr/local/share/libcrun-shim/initramfs.img"),
    ],
    vm_memory: 2 * 1024 * 1024 * 1024, // 2GB
    vm_cpus: 2,
    vm_disks: vec![VmDiskConfig {
        path: PathBuf::from("/tmp/vm-disk.img"),
        size: 10 * 1024 * 1024 * 1024,
        read_only: false,
        format: "raw".to_string(),
        create_if_missing: true,
    }],
    vm_network: VmNetworkConfig {
        mode: "nat".to_string(),
        port_forwards: vec![PortForward {
            host_port: 8080,
            guest_port: 80,
            protocol: "tcp".to_string(),
        }],
        bridge_interface: None,
    },
    virtio_fs_shares: vec![VirtioFsShare {
        tag: "host-share".to_string(),
        path: PathBuf::from("/tmp/host-share"),
        read_only: false,
    }],
    rosetta_config: RosettaConfig { enabled: true },
    ..Default::default()
};

let runtime = ContainerRuntime::new_with_config(config).await?;
```

## License

Apache-2.0
