# libcrun-shim

A unified Rust library for running OCI containers on Linux and macOS with production-ready features including error recovery, health checks, metrics, and integration with containerd and Kubernetes.

## Features

### Core Features
- **Single unified API** for both Linux and macOS
- **Async/await support** for all operations
- **Complete container lifecycle** management (create, start, stop, delete, list)
- **Real libcrun integration** when available (with graceful fallback)
- **Thread-safe state management** with proper resource cleanup
- **Enhanced error messages** with context and actionable suggestions
- **Structured logging** using the `log` crate
- **OCI-compliant** container configuration generation

### Production Features
- ✅ **Error Recovery**: Panic handlers, container watchdog, orphan detection
- ✅ **Signal Handling**: Graceful shutdown with SIGTERM/SIGINT support
- ✅ **Container Cleanup**: Automatic cleanup, orphan recovery, state persistence
- ✅ **Health Checks**: Configurable health check commands with retry logic
- ✅ **Metrics Collection**: CPU, memory, network, block I/O, and PIDs metrics
- ✅ **Container Logs**: Log retrieval and streaming
- ✅ **Image Management**: OCI image pull, list, and removal
- ✅ **TTY/Interactive**: Full PTY support for interactive exec
- ✅ **Container Events**: Event streaming for container state changes
- ✅ **VirtioFS Sharing**: Host-guest file sharing for macOS VMs
- ✅ **Rosetta Support**: x86_64 container support on ARM Macs
- ✅ **Containerd Shim v2**: Bridge implementation for containerd integration
- ✅ **CRI Support**: Kubernetes Container Runtime Interface bridge

### Platform Support
- **Linux**: Direct libcrun integration with full container support
- **macOS**: Transparent VM-based execution using Apple Virtualization Framework

## Quick Start

### Basic Usage

```rust
use libcrun_shim::*;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    let runtime = ContainerRuntime::new().await?;
    
    let config = ContainerConfig {
        id: "my-container".to_string(),
        rootfs: "/path/to/rootfs".into(),
        command: vec!["sh".to_string()],
        env: vec!["PATH=/usr/bin:/bin".to_string()],
        working_dir: "/".to_string(),
        ..Default::default()
    };
    
    // Create and start container
    let id = runtime.create(config).await?;
    runtime.start(&id).await?;
    
    // List containers
    let containers = runtime.list().await?;
    for container in &containers {
        println!("Container: {} - Status: {:?}", container.id, container.status);
    }
    
    // Stop and cleanup
    runtime.stop(&id).await?;
    runtime.delete(&id).await?;
    
    Ok(())
}
```

## Examples

### Easy: Basic Container Lifecycle

See [`examples/basic_usage.rs`](examples/basic_usage.rs) for a simple container create/start/stop example.

### Medium: Health Checks and Metrics

See [`examples/health_metrics.rs`](examples/health_metrics.rs) for health check configuration and metrics collection.

### Advanced: Full Production Setup

See [`examples/production_setup.rs`](examples/production_setup.rs) for error recovery, signal handling, and event streaming.

## CLI Tool

The `crun-shim` CLI provides a complete command-line interface for container management:

### Container Management

```bash
# Create a container
crun-shim create my-container --rootfs /path/to/rootfs --cmd sh

# Start a container
crun-shim start my-container

# Stop a container
crun-shim stop my-container

# Delete a container
crun-shim delete my-container

# List all containers
crun-shim list

# View container stats
crun-shim stats my-container

# View container logs
crun-shim logs my-container

# Execute command in container
crun-shim exec my-container -- sh -c "echo hello"
```

### Error Recovery and Cleanup

```bash
# Cleanup orphaned containers
crun-shim cleanup --orphaned --force

# Cleanup stopped containers
crun-shim cleanup --stopped --force

# Dry run to see what would be cleaned
crun-shim cleanup --dry-run

# Recover runtime state
crun-shim recover

# Recover and terminate orphaned processes
crun-shim recover --force

# Graceful shutdown of all containers
crun-shim shutdown --timeout 30
```

### Image Management

```bash
# Pull an image
crun-shim pull docker.io/library/alpine:latest

# List images
crun-shim images

# Remove an image
crun-shim rmi alpine:latest
```

### Health Checks

```bash
# Create container with health check
crun-shim create web-server \
  --rootfs /path/to/rootfs \
  --cmd nginx \
  --health-cmd "curl -f http://localhost/health" \
  --health-interval 30 \
  --health-timeout 10 \
  --health-retries 3

# Check container health
crun-shim health web-server
```

### Metrics and Monitoring

```bash
# Get container metrics
crun-shim stats my-container

# Get all container metrics
crun-shim stats --all

# Stream container events
crun-shim events
```

## Health Check Configuration

Health checks allow you to monitor container health and automatically restart unhealthy containers.

### Programmatic Configuration

```rust
use libcrun_shim::*;

let config = ContainerConfig {
    id: "web-server".to_string(),
    rootfs: "/path/to/rootfs".into(),
    command: vec!["nginx".to_string()],
    health_check: Some(HealthCheck {
        command: vec!["curl".to_string(), "-f".to_string(), "http://localhost/health".to_string()],
        interval: 30,        // Check every 30 seconds
        timeout: 10,          // 10 second timeout per check
        retries: 3,           // Mark unhealthy after 3 failures
        start_period: 60,     // Ignore failures for first 60 seconds
    }),
    ..Default::default()
};
```

### Health Check Behavior

- **Interval**: Time between health checks (default: 30 seconds)
- **Timeout**: Maximum time for a health check to complete (default: 30 seconds)
- **Retries**: Number of consecutive failures before marking unhealthy (default: 3)
- **Start Period**: Grace period after container start where failures are ignored (default: 0)

The container watchdog automatically runs health checks and updates container health status.

## Metrics Collection

Collect detailed metrics about container resource usage.

### Getting Metrics

```rust
use libcrun_shim::*;

#[tokio::main]
async fn main() -> Result<()> {
    let runtime = ContainerRuntime::new().await?;
    
    // Get metrics for a specific container
    let metrics = runtime.metrics("my-container").await?;
    
    println!("CPU Usage: {}%", metrics.cpu.usage_percent());
    println!("Memory Usage: {} / {} bytes", 
        metrics.memory.usage, 
        metrics.memory.limit);
    println!("Network RX: {} bytes", metrics.network.rx_bytes);
    println!("Network TX: {} bytes", metrics.network.tx_bytes);
    println!("Block I/O Read: {} bytes", metrics.blkio.read_bytes);
    println!("Block I/O Write: {} bytes", metrics.blkio.write_bytes);
    println!("PIDs: {}", metrics.pids.current);
    
    // Get metrics for all containers
    let all_metrics = runtime.all_metrics().await?;
    for metrics in all_metrics {
        println!("Container {}: CPU {}%, Memory {} bytes", 
            metrics.container_id,
            metrics.cpu.usage_percent(),
            metrics.memory.usage);
    }
    
    Ok(())
}
```

### Available Metrics

- **CPU**: Total usage, per-CPU usage, throttling statistics
- **Memory**: Usage, limit, max usage, swap
- **Network**: RX/TX bytes, packets, errors
- **Block I/O**: Read/write bytes, operations
- **PIDs**: Current process count

## Container Logs

Retrieve and stream container logs.

```rust
use libcrun_shim::*;

#[tokio::main]
async fn main() -> Result<()> {
    let runtime = ContainerRuntime::new().await?;
    
    // Get container logs
    let logs = runtime.logs("my-container", LogOptions {
        follow: false,
        tail: Some(100),
        since: None,
        until: None,
    }).await?;
    
    println!("Stdout:\n{}", logs.stdout);
    println!("Stderr:\n{}", logs.stderr);
    
    Ok(())
}
```

## Container Events

Subscribe to container events for real-time monitoring.

```rust
use libcrun_shim::*;

#[tokio::main]
async fn main() -> Result<()> {
    let runtime = ContainerRuntime::new().await?;
    
    // Subscribe to events
    let mut receiver = subscribe_events();
    
    // Create and start a container
    let config = ContainerConfig {
        id: "event-test".to_string(),
        rootfs: "/path/to/rootfs".into(),
        command: vec!["sh".to_string()],
        ..Default::default()
    };
    
    let id = runtime.create(config).await?;
    runtime.start(&id).await?;
    
    // Listen for events
    while let Ok(event) = receiver.recv().await {
        println!("Event: {:?} - Container: {}", event.event_type, event.container_id);
        
        match event.event_type {
            ContainerEventType::Start => println!("Container started!"),
            ContainerEventType::Die => {
                println!("Container exited with code: {:?}", event.exit_code);
                break;
            }
            _ => {}
        }
    }
    
    Ok(())
}
```

## Error Recovery and Cleanup

The runtime includes comprehensive error recovery mechanisms.

### Automatic Cleanup

```rust
use libcrun_shim::*;

#[tokio::main]
async fn main() -> Result<()> {
    let runtime = ContainerRuntime::new().await?;
    
    // Cleanup all stopped containers
    let cleaned = runtime.cleanup_stopped().await?;
    println!("Cleaned up {} stopped containers", cleaned);
    
    // Force delete a container (even if running)
    runtime.force_delete("stuck-container").await?;
    
    // List orphaned containers
    let orphaned = runtime.list_orphaned().await?;
    for container in orphaned {
        println!("Orphaned: {}", container.id);
    }
    
    Ok(())
}
```

### Signal Handling

The CLI automatically handles SIGTERM/SIGINT for graceful shutdown:

```bash
# Graceful shutdown (stops all containers)
crun-shim shutdown

# Force shutdown after timeout
crun-shim shutdown --timeout 10
```

## Image Management

Pull, list, and manage OCI images.

```rust
use libcrun_shim::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Create image store
    let mut store = ImageStore::new(ImageStore::default_path())?;
    
    // Pull an image with progress callback
    let progress_cb = |progress: PullProgress| {
        if progress.total_bytes > 0 {
            let percent = (progress.downloaded_bytes as f64 / progress.total_bytes as f64) * 100.0;
            println!("Progress: {:.1}%", percent);
        } else {
            println!("Status: {}", progress.status);
        }
    };
    
    let image_info = store.pull("docker.io/library/alpine:latest", Some(Box::new(progress_cb))).await?;
    println!("Pulled image: {}", image_info.id);
    
    // List images
    let images = store.list()?;
    for image in images {
        println!("Image: {} - Size: {} bytes", image.reference.full_name(), image.size);
    }
    
    // Get rootfs path for an image
    let rootfs = store.get_rootfs("alpine:latest")?;
    println!("Rootfs: {}", rootfs.display());
    
    // Remove image
    store.remove("alpine:latest")?;
    
    Ok(())
}
```

## macOS VM Configuration

On macOS, containers run inside a Linux VM. Configure the VM with `RuntimeConfig`:

```rust
use libcrun_shim::*;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let config = RuntimeConfig {
        socket_path: PathBuf::from("/tmp/libcrun-shim.sock"),
        vsock_port: 1234,
        vm_asset_paths: vec![
            PathBuf::from("/usr/local/share/libcrun-shim/kernel"),
            PathBuf::from("/usr/local/share/libcrun-shim/initramfs.img"),
        ],
        vm_memory: 2 * 1024 * 1024 * 1024, // 2GB
        vm_cpus: 2,
        connection_timeout: 30,
        
        // Virtual disks
        vm_disks: vec![VmDiskConfig {
            path: PathBuf::from("/tmp/vm-disk.img"),
            size: 10 * 1024 * 1024 * 1024, // 10GB
            read_only: false,
            format: "raw".to_string(),
            create_if_missing: true,
        }],
        
        // Network configuration
        vm_network: VmNetworkConfig {
            mode: "nat".to_string(),
            port_forwards: vec![PortForward {
                host_port: 8080,
                guest_port: 80,
                protocol: "tcp".to_string(),
            }],
            bridge_interface: None,
        },
        
        // VirtioFS shares
        virtio_fs_shares: vec![VirtioFsShare {
            tag: "host-share".to_string(),
            path: PathBuf::from("/tmp/host-share"),
            read_only: false,
        }],
        
        // Rosetta support (ARM Macs)
        rosetta_config: RosettaConfig {
            enabled: true,
        },
    };
    
    let runtime = ContainerRuntime::new_with_config(config).await?;
    
    // Use runtime as normal
    // ...
    
    Ok(())
}
```

## Integration Features

### Containerd Shim v2

Enable containerd integration with the `shim-v2` feature:

```bash
cargo build --features shim-v2
```

```rust
use libcrun_shim::*;

let shim = ShimV2::new(
    PathBuf::from("/run/containerd/shim.sock"),
    PathBuf::from("/var/lib/containerd/bundle"),
    "default".to_string(),
);

// Start shim server
shim.serve().await?;
```

### Kubernetes CRI

Enable Kubernetes integration with the `cri` feature:

```bash
cargo build --features cri
```

```rust
use libcrun_shim::*;

let mut cri_server = CriServer::new(PathBuf::from("/run/cri.sock"));

// Start CRI server
cri_server.serve().await?;
```

## Architecture

The library is organized as a Rust workspace with multiple crates:

- `libcrun-shim`: Main library providing the unified API with platform-specific implementations
- `libcrun-shim-proto`: RPC protocol for macOS VM communication (bincode-based)
- `libcrun-shim-agent`: VM guest agent that runs inside the Linux VM (Unix socket server)
- `libcrun-shim-cli`: Command-line interface tool
- `libcrun-sys`: FFI bindings to libcrun (with bindgen, uses stubs when libcrun not available)

### Platform Implementations

**Linux**: Direct integration with libcrun via FFI bindings. Uses in-memory state management with validation and proper state transitions.

**macOS**: VM-based execution using:
- Apple Virtualization Framework for VM creation and management
- Native vsock communication for host-guest communication
- RPC client for communicating with the Linux VM guest agent
- Swift bridge for proper async Virtualization Framework operations

## Building

```bash
# Build everything
cargo build

# Build with specific features
cargo build --features shim-v2,cri

# Build release
cargo build --release

# Build CLI tool
cargo build --package libcrun-shim-cli --release
```

**Note:** For real container operations on Linux, you need to install libcrun. The CI workflow builds crun from source automatically.

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
cargo test --test integration_tests

# Run examples
cargo run --example basic_usage
cargo run --example health_metrics
cargo run --example production_setup
```

## Implementation Status

✅ **All Core Features Complete:**
- Container lifecycle management
- Error recovery and cleanup
- Signal handling
- Health checks
- Metrics collection
- Container logs
- Image management
- TTY/Interactive support
- Container events
- macOS VM integration
- VirtioFS sharing
- Rosetta support
- Containerd Shim v2 bridge
- Kubernetes CRI bridge

## License

Apache-2.0
