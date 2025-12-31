//! Medium Difficulty Example: Health Checks and Metrics
//!
//! This example demonstrates:
//! - Configuring health checks for containers
//! - Collecting and displaying container metrics
//! - Monitoring container health status
//! - Using metrics for resource monitoring

use libcrun_shim::*;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    println!("=== Health Checks and Metrics Example ===\n");

    // Create runtime
    let runtime = ContainerRuntime::new().await?;
    println!("✓ Runtime initialized\n");

    // Example 1: Create container with health check
    println!("1. Creating container with health check...");

    // For this example, we'll use a simple health check command
    // In production, you'd use something like: curl -f http://localhost/health
    let config = ContainerConfig {
        id: "web-server".to_string(),
        rootfs: std::env::temp_dir().join("example-rootfs"), // Placeholder
        command: vec!["sh".to_string(), "-c".to_string(), "sleep 3600".to_string()],
        env: vec!["PATH=/usr/bin:/bin".to_string()],
        working_dir: "/".to_string(),

        // Configure health check
        health_check: Some(HealthCheck {
            command: vec![
                "sh".to_string(),
                "-c".to_string(),
                "test -f /tmp/healthy".to_string(),
            ],
            interval: 10,     // Check every 10 seconds
            timeout: 5,       // 5 second timeout
            retries: 3,       // Mark unhealthy after 3 failures
            start_period: 30, // Ignore failures for first 30 seconds
        }),

        // Set resource limits
        resources: ResourceLimits {
            memory: Some(512 * 1024 * 1024), // 512MB
            cpu: Some(0.5),                  // 0.5 CPU cores
            ..Default::default()
        },

        ..Default::default()
    };

    // Note: In a real scenario, you'd have a proper rootfs
    // For this example, we'll just demonstrate the API
    println!("   Health check configured:");
    println!(
        "   - Command: {:?}",
        config.health_check.as_ref().unwrap().command
    );
    println!(
        "   - Interval: {}s",
        config.health_check.as_ref().unwrap().interval
    );
    println!(
        "   - Timeout: {}s",
        config.health_check.as_ref().unwrap().timeout
    );
    println!(
        "   - Retries: {}",
        config.health_check.as_ref().unwrap().retries
    );
    println!(
        "   - Start period: {}s",
        config.health_check.as_ref().unwrap().start_period
    );
    println!();

    // Example 2: Collect metrics (if container exists)
    println!("2. Metrics Collection Example:");
    println!("   (This would work with a real running container)\n");

    // Demonstrate metrics structure
    println!("   Available metrics:");
    println!("   - CPU: usage, throttling, per-CPU stats");
    println!("   - Memory: usage, limit, max usage, swap");
    println!("   - Network: RX/TX bytes, packets, errors");
    println!("   - Block I/O: read/write bytes, operations");
    println!("   - PIDs: current process count");
    println!();

    // Example 3: Monitor container health
    println!("3. Health Status Monitoring:");

    // Check health status
    println!("   To check container health:");
    println!("   ```rust");
    println!("   let health = runtime.health(\"web-server\").await?;");
    println!("   println!(\"Status: {{:?}}\", health.state);");
    println!("   ```");
    println!();

    // Example 4: Collect metrics for all containers
    println!("4. Collecting Metrics for All Containers:");
    println!("   ```rust");
    println!("   let all_metrics = runtime.all_metrics().await?;");
    println!("   for metrics in all_metrics {{");
    println!("       println!(\"Container: {{}}\", metrics.container_id);");
    println!("       println!(\"  CPU: {{:.2}}%\", metrics.cpu.usage_percent());");
    println!("       println!(\"  Memory: {{}} / {{}} bytes\",");
    println!("           metrics.memory.usage,");
    println!("           metrics.memory.limit);");
    println!("   }}");
    println!("   ```");
    println!();

    // Example 5: Real metrics collection (if containers exist)
    println!("5. Real Metrics Example:");

    let containers = runtime.list().await?;
    if !containers.is_empty() {
        println!(
            "   Found {} container(s), collecting metrics...\n",
            containers.len()
        );

        for container in &containers {
            match runtime.metrics(&container.id).await {
                Ok(metrics) => {
                    println!("   Container: {}", container.id);
                    println!("   - CPU Usage: {} ns total", metrics.cpu.usage_total);
                    println!(
                        "   - Memory: {} / {} bytes ({:.2}%)",
                        metrics.memory.usage,
                        metrics.memory.limit,
                        (metrics.memory.usage as f64 / metrics.memory.limit as f64) * 100.0
                    );
                    println!("   - Network RX: {} bytes", metrics.network.rx_bytes);
                    println!("   - Network TX: {} bytes", metrics.network.tx_bytes);
                    println!("   - Block I/O Read: {} bytes", metrics.blkio.read_bytes);
                    println!("   - Block I/O Write: {} bytes", metrics.blkio.write_bytes);
                    println!("   - PIDs: {}", metrics.pids.current);
                    println!();
                }
                Err(e) => {
                    println!("   Failed to get metrics for {}: {}", container.id, e);
                }
            }
        }
    } else {
        println!("   No containers running. Create a container first to see metrics.");
    }

    // Example 6: Continuous monitoring
    println!("6. Continuous Monitoring Pattern:");
    println!("   ```rust");
    println!("   loop {{");
    println!("       let metrics = runtime.metrics(\"my-container\").await?;");
    println!("       println!(\"CPU: {{}} ns, Memory: {{}}MB\",");
    println!("           metrics.cpu.usage_total,");
    println!("           metrics.memory.usage / 1024 / 1024);");
    println!("       ");
    println!("       // Alert if memory > 90%");
    println!("       let mem_percent = (metrics.memory.usage as f64 / metrics.memory.limit as f64) * 100.0;");
    println!("       if mem_percent > 90.0 {{");
    println!("           eprintln!(\"⚠️  High memory usage!\");");
    println!("       }}");
    println!("       ");
    println!("       sleep(Duration::from_secs(5)).await;");
    println!("   }}");
    println!("   ```");
    println!();

    // Example 7: Health check integration
    println!("7. Health Check Integration:");
    println!("   The container watchdog automatically:");
    println!("   - Runs health checks at configured intervals");
    println!("   - Tracks consecutive failures");
    println!("   - Updates container health status");
    println!("   - Marks containers as unhealthy after retry limit");
    println!();
    println!("   Check health status:");
    println!("   ```rust");
    println!("   let health = runtime.health(\"web-server\").await?;");
    println!("   match health.status {{");
    println!("       HealthState::Healthy => println!(\"Container is healthy\"),");
    println!("       HealthState::Unhealthy => println!(\"Container is unhealthy\"),");
    println!("       HealthState::Starting => println!(\"Container is starting\"),");
    println!("       HealthState::Unknown => println!(\"Health status unknown\"),");
    println!("   }}");
    println!("   ```");
    println!();

    println!("=== Example Complete ===");
    println!("\nTo use this with real containers:");
    println!("1. Create a proper rootfs or use an image");
    println!("2. Configure appropriate health check commands");
    println!("3. Start the container and monitor metrics");

    Ok(())
}
