// Example: Basic usage of libcrun-shim
use libcrun_shim::*;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging (optional)
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    log::info!("Creating container runtime...");
    let runtime = ContainerRuntime::new().await?;

    // Create a container configuration
    let config = ContainerConfig {
        id: "example-container".to_string(),
        rootfs: PathBuf::from("/tmp/example-rootfs"),
        command: vec!["echo".to_string(), "Hello from container!".to_string()],
        env: vec!["PATH=/usr/bin:/bin".to_string()],
        working_dir: "/".to_string(),
        ..Default::default()
    };

    log::info!("Creating container: {}", config.id);
    let id = runtime.create(config).await?;
    log::info!("Container created: {}", id);

    // List containers
    log::info!("Listing containers:");
    let containers = runtime.list().await?;
    for container in &containers {
        log::info!("  - {}: {:?}", container.id, container.status);
    }

    // Start the container
    log::info!("Starting container: {}", id);
    runtime.start(&id).await?;
    log::info!("Container started");

    // List again to see running status
    let containers = runtime.list().await?;
    for container in &containers {
        log::info!(
            "  - {}: {:?} (PID: {:?})",
            container.id,
            container.status,
            container.pid
        );
    }

    // Stop the container
    log::info!("Stopping container: {}", id);
    runtime.stop(&id).await?;
    log::info!("Container stopped");

    // Delete the container
    log::info!("Deleting container: {}", id);
    runtime.delete(&id).await?;
    log::info!("Container deleted");

    // Final list (should be empty)
    let containers = runtime.list().await?;
    log::info!("Final container count: {}", containers.len());

    Ok(())
}
