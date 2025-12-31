// Example: Basic usage of libcrun-shim
use libcrun_shim::*;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Creating container runtime...");
    let runtime = ContainerRuntime::new().await?;
    
    // Create a container configuration
    let config = ContainerConfig {
        id: "example-container".to_string(),
        rootfs: PathBuf::from("/tmp/example-rootfs"),
        command: vec!["echo".to_string(), "Hello from container!".to_string()],
        env: vec!["PATH=/usr/bin:/bin".to_string()],
        working_dir: "/".to_string(),
    };
    
    println!("Creating container: {}", config.id);
    let id = runtime.create(config).await?;
    println!("Container created: {}", id);
    
    // List containers
    println!("\nListing containers:");
    let containers = runtime.list().await?;
    for container in &containers {
        println!("  - {}: {:?}", container.id, container.status);
    }
    
    // Start the container
    println!("\nStarting container: {}", id);
    runtime.start(&id).await?;
    println!("Container started");
    
    // List again to see running status
    let containers = runtime.list().await?;
    for container in &containers {
        println!("  - {}: {:?} (PID: {:?})", container.id, container.status, container.pid);
    }
    
    // Stop the container
    println!("\nStopping container: {}", id);
    runtime.stop(&id).await?;
    println!("Container stopped");
    
    // Delete the container
    println!("\nDeleting container: {}", id);
    runtime.delete(&id).await?;
    println!("Container deleted");
    
    // Final list (should be empty)
    let containers = runtime.list().await?;
    println!("\nFinal container count: {}", containers.len());
    
    Ok(())
}

