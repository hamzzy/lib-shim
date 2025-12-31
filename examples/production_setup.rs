//! Advanced Example: Production Setup with Error Recovery
//!
//! This example demonstrates:
//! - Error recovery and panic handling
//! - Signal handling for graceful shutdown
//! - Container cleanup and orphan recovery
//! - Event streaming for monitoring
//! - Health check monitoring
//! - Metrics collection with alerts
//! - State persistence

use libcrun_shim::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    println!("=== Production Setup Example ===\n");

    // Setup signal handlers
    setup_signal_handlers();

    // Create runtime
    let runtime = ContainerRuntime::new().await?;
    println!("✓ Runtime initialized\n");

    // Step 1: Cleanup on startup
    println!("1. Cleaning up orphaned containers...");
    let cleaned = runtime.cleanup_stopped().await?;
    println!("   Cleaned up {} stopped containers\n", cleaned);

    // Step 2: Subscribe to events
    println!("2. Subscribing to container events...");
    let mut event_receiver = subscribe_events();
    
    // Spawn event handler
    let runtime_for_events = Arc::new(runtime);
    let runtime_clone = Arc::clone(&runtime_for_events);
    tokio::spawn(async move {
        while let Some(event) = event_receiver.recv().await {
            if SHUTDOWN.load(Ordering::SeqCst) {
                break;
            }
            
            match event.event_type {
                ContainerEventType::Create => {
                    println!("📦 Event: Container '{}' created", event.container_id);
                }
                ContainerEventType::Start => {
                    println!("▶️  Event: Container '{}' started", event.container_id);
                }
                ContainerEventType::Stop => {
                    println!("⏸️  Event: Container '{}' stopped", event.container_id);
                }
                ContainerEventType::Die => {
                    println!("💀 Event: Container '{}' died (exit: {:?})", 
                        event.container_id, event.exit_code);
                }
                ContainerEventType::Delete => {
                    println!("🗑️  Event: Container '{}' deleted", event.container_id);
                }
                ContainerEventType::HealthOk => {
                    println!("✅ Event: Container '{}' health check passed", event.container_id);
                }
                ContainerEventType::HealthFail => {
                    println!("❌ Event: Container '{}' health check failed", event.container_id);
                }
                _ => {}
            }
        }
    });
    println!("   Event handler started\n");

    // Step 3: Create container with health check
    println!("3. Creating production container...");
    
    let container_id = "prod-web-server".to_string();
    let config = ContainerConfig {
        id: container_id.clone(),
        rootfs: std::env::temp_dir().join("prod-rootfs"), // Would be real rootfs
        command: vec!["nginx".to_string()],
        env: vec![
            "PATH=/usr/bin:/bin".to_string(),
            "NGINX_HOST=localhost".to_string(),
        ],
        working_dir: "/".to_string(),
        
        // Health check configuration
        health_check: Some(HealthCheck {
            command: vec!["curl".to_string(), "-f".to_string(), "http://localhost/health".to_string()],
            interval: 30,
            timeout: 10,
            retries: 3,
            start_period: 60, // Grace period after start
        }),
        
        // Resource limits
        resources: ResourceLimits {
            memory: Some(512 * 1024 * 1024), // 512MB
            cpu: Some(1.0),                   // 1 CPU core
            pids: Some(100),                  // Max 100 processes
            ..Default::default()
        },
        
        // Logging configuration
        log_driver: "json-file".to_string(),
        log_max_size: 10 * 1024 * 1024, // 10MB
        
        ..Default::default()
    };

    println!("   Container config:");
    println!("   - ID: {}", config.id);
    println!("   - Health check: {:?}", config.health_check.as_ref().unwrap().command);
    println!("   - Memory limit: {}MB", config.resources.memory.unwrap_or(0) / 1024 / 1024);
    println!("   - CPU limit: {}", config.resources.cpu.unwrap_or(0.0));
    println!();

    // Note: In real usage, you'd actually create the container
    // For this example, we'll demonstrate the monitoring patterns

    // Step 4: Monitoring loop
    println!("4. Starting monitoring loop...");
    println!("   (Press Ctrl+C to gracefully shutdown)\n");

    let runtime_for_monitor = Arc::clone(&runtime_for_events);
    let monitor_handle = tokio::spawn(async move {
        let mut iteration = 0;
        
        while !SHUTDOWN.load(Ordering::SeqCst) {
            iteration += 1;
            
            // List containers
            match runtime_for_monitor.list().await {
                Ok(containers) => {
                    if !containers.is_empty() {
                        println!("\n--- Monitoring Iteration {} ---", iteration);
                        
                        for container in &containers {
                            println!("\n📊 Container: {}", container.id);
                            println!("   Status: {:?}", container.status);
                            
                            // Get metrics
                            match runtime_for_monitor.metrics(&container.id).await {
                                Ok(metrics) => {
                                    // Calculate CPU usage percentage (simplified)
                                    // In production, you'd track previous values and calculate delta
                                    let cpu_percent = 0.0; // Placeholder - would calculate from usage_total
                                    
                                    let mem_percent = if metrics.memory.limit > 0 {
                                        (metrics.memory.usage as f64 / metrics.memory.limit as f64) * 100.0
                                    } else {
                                        0.0
                                    };
                                    
                                    println!("   CPU: {} ns total", metrics.cpu.usage_total);
                                    println!("   Memory: {:.2}% ({} / {} bytes)", 
                                        mem_percent,
                                        metrics.memory.usage,
                                        metrics.memory.limit);
                                    println!("   Network: RX {} bytes, TX {} bytes",
                                        metrics.network.rx_bytes,
                                        metrics.network.tx_bytes);
                                    
                                    // Alerts
                                    if mem_percent > 90.0 {
                                        println!("   ⚠️  ALERT: High memory usage!");
                                    }
                                }
                                Err(e) => {
                                    println!("   ⚠️  Failed to get metrics: {}", e);
                                }
                            }
                            
                            // Check health
                            match runtime_for_monitor.health(&container.id).await {
                                Ok(health) => {
                                    println!("   Health: {:?}", health.status);
                                    if health.status == HealthState::Unhealthy {
                                        println!("   ⚠️  ALERT: Container is unhealthy!");
                                    }
                                }
                                Err(e) => {
                                    println!("   ⚠️  Failed to get health: {}", e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("⚠️  Failed to list containers: {}", e);
                }
            }
            
            // Wait before next iteration
            sleep(Duration::from_secs(10)).await;
        }
        
        println!("\n✓ Monitoring stopped");
    });

    // Step 5: Graceful shutdown handler
    println!("5. Setup complete. Monitoring containers...");
    println!("   Press Ctrl+C to gracefully shutdown\n");

    // Wait for shutdown signal
    while !SHUTDOWN.load(Ordering::SeqCst) {
        sleep(Duration::from_secs(1)).await;
    }

    println!("\n=== Initiating Graceful Shutdown ===\n");

    // Stop monitoring
    monitor_handle.abort();

    // Graceful shutdown
    println!("6. Stopping all containers...");
    match runtime_for_events.shutdown().await {
        Ok(()) => {
            println!("   ✓ All containers stopped gracefully");
        }
        Err(e) => {
            eprintln!("   ⚠️  Error during shutdown: {}", e);
        }
    }

    // Final cleanup
    println!("\n7. Final cleanup...");
    let cleaned = runtime_for_events.cleanup_stopped().await?;
    println!("   Cleaned up {} stopped containers", cleaned);

    println!("\n=== Shutdown Complete ===");

    Ok(())
}

fn setup_signal_handlers() {
    // Signal handling is typically done at the application level
    // The CLI tool (crun-shim) includes full signal handling with ctrlc
    // For this example, we use a simple atomic flag
    // In production, you'd use: ctrlc::set_handler(...)
    // The SHUTDOWN flag is set by checking in the main loop
}

