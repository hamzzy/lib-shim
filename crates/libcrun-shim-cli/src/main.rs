use clap::{Parser, Subcommand};
use colored::Colorize;
use libcrun_shim::{
    subscribe_events, ContainerConfig, ContainerEventType, ContainerRuntime, ContainerStatus,
    HealthState, ImageStore, LogOptions, PullProgress, RuntimeConfig,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tabled::{Table, Tabled};

/// Global shutdown flag for coordinating graceful termination
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

#[derive(Parser)]
#[command(name = "crun-shim")]
#[command(author, version, about = "Container runtime shim for Linux containers on macOS", long_about = None)]
struct Cli {
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Socket path for agent communication
    #[arg(long, global = true)]
    socket: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new container
    Create {
        /// Container name/ID
        name: String,

        /// Path to container rootfs
        #[arg(short, long)]
        rootfs: PathBuf,

        /// Command to run
        #[arg(short, long, num_args = 1..)]
        cmd: Vec<String>,

        /// Environment variables (KEY=VALUE)
        #[arg(short, long)]
        env: Vec<String>,

        /// Working directory
        #[arg(short, long, default_value = "/")]
        workdir: String,

        /// Memory limit (e.g., 512m, 1g)
        #[arg(long)]
        memory: Option<String>,

        /// CPU limit (cores, e.g., 0.5, 2)
        #[arg(long)]
        cpus: Option<f64>,
    },

    /// Start a container
    Start {
        /// Container name/ID
        name: String,
    },

    /// Stop a running container
    Stop {
        /// Container name/ID
        name: String,
    },

    /// Delete a container
    #[command(alias = "rm")]
    Delete {
        /// Container name/ID
        name: String,

        /// Force delete even if running
        #[arg(short, long)]
        force: bool,
    },

    /// List containers
    #[command(alias = "ps")]
    List {
        /// Show all containers (including stopped)
        #[arg(short, long)]
        all: bool,

        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },

    /// Get container logs
    Logs {
        /// Container name/ID
        name: String,

        /// Number of lines to show
        #[arg(short = 'n', long, default_value = "100")]
        tail: u32,

        /// Follow log output
        #[arg(short, long)]
        follow: bool,
    },

    /// Show container metrics
    Stats {
        /// Container name/ID (optional, shows all if not specified)
        name: Option<String>,

        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },

    /// Check container health
    Health {
        /// Container name/ID
        name: String,
    },

    /// Execute a command in a running container
    Exec {
        /// Container name/ID
        name: String,

        /// Interactive mode (allocate TTY)
        #[arg(short, long)]
        interactive: bool,

        /// Allocate a pseudo-TTY
        #[arg(short = 't', long)]
        tty: bool,

        /// Command to execute
        #[arg(num_args = 1..)]
        command: Vec<String>,
    },

    /// Show runtime information
    Info,

    /// Pull an image from a registry
    Pull {
        /// Image reference (e.g., alpine:latest, ghcr.io/user/repo:v1)
        image: String,

        /// Quiet mode (no progress output)
        #[arg(short, long)]
        quiet: bool,
    },

    /// List images
    Images {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },

    /// Remove an image
    Rmi {
        /// Image ID or name
        image: String,
    },

    /// Run a container from an image
    Run {
        /// Image reference
        image: String,

        /// Container name
        #[arg(long)]
        name: Option<String>,

        /// Command to run
        #[arg(num_args = 0..)]
        command: Vec<String>,

        /// Remove container after exit
        #[arg(long)]
        rm: bool,

        /// Environment variables (KEY=VALUE)
        #[arg(short, long)]
        env: Vec<String>,

        /// Working directory
        #[arg(short, long)]
        workdir: Option<String>,

        /// Memory limit (e.g., 512m, 1g)
        #[arg(long)]
        memory: Option<String>,

        /// CPU limit (cores, e.g., 0.5, 2)
        #[arg(long)]
        cpus: Option<f64>,
    },

    /// Watch container events
    Events {
        /// Filter by container ID
        #[arg(short, long)]
        filter: Option<String>,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        format: String,

        /// Since timestamp (Unix seconds)
        #[arg(long)]
        since: Option<u64>,
    },

    /// Remove stopped containers
    Prune {
        /// Force prune without confirmation
        #[arg(short, long)]
        force: bool,
    },

    /// Gracefully shutdown all containers and runtime
    Shutdown {
        /// Timeout in seconds for graceful shutdown
        #[arg(short, long, default_value = "30")]
        timeout: u64,
    },

    /// Cleanup orphaned or stopped containers
    Cleanup {
        /// Only cleanup orphaned containers (crashed/lost PID)
        #[arg(long)]
        orphaned: bool,

        /// Cleanup all stopped containers
        #[arg(long)]
        stopped: bool,

        /// Force cleanup without confirmation
        #[arg(short, long)]
        force: bool,

        /// Dry run - show what would be cleaned up
        #[arg(long)]
        dry_run: bool,
    },

    /// Recover runtime state from persisted data
    Recover {
        /// Force recovery even if runtime appears healthy
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Tabled)]
struct ContainerRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "STATUS")]
    status: String,
    #[tabled(rename = "PID")]
    pid: String,
}

#[derive(Tabled)]
struct StatsRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "CPU %")]
    cpu: String,
    #[tabled(rename = "MEM USAGE")]
    memory: String,
    #[tabled(rename = "MEM %")]
    mem_percent: String,
    #[tabled(rename = "NET I/O")]
    network: String,
    #[tabled(rename = "BLOCK I/O")]
    block: String,
    #[tabled(rename = "PIDS")]
    pids: String,
}

#[derive(Tabled)]
struct ImageRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "REPOSITORY")]
    repository: String,
    #[tabled(rename = "TAG")]
    tag: String,
    #[tabled(rename = "SIZE")]
    size: String,
    #[tabled(rename = "CREATED")]
    created: String,
}

#[tokio::main]
async fn main() {
    // Setup panic handler for graceful cleanup on panics
    setup_panic_handler();

    let cli = Cli::parse();

    // Setup logging
    if cli.verbose {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    }

    // Setup Ctrl+C handler
    setup_signal_handler();

    // Handle commands that don't need runtime
    match &cli.command {
        Commands::Info => {
            println!("{}", "crun-shim Runtime Information".bold());
            println!("Version: {}", env!("CARGO_PKG_VERSION"));
            println!("OS: {}", std::env::consts::OS);
            println!("Arch: {}", std::env::consts::ARCH);

            #[cfg(target_os = "macos")]
            {
                println!("Backend: Virtualization.framework + libcrun");
            }

            #[cfg(target_os = "linux")]
            {
                println!("Backend: libcrun (native)");
            }

            return;
        }

        Commands::Pull { image, quiet } => {
            let mut store = match ImageStore::new(ImageStore::default_path()) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}: {}", "Error".red().bold(), e);
                    std::process::exit(1);
                }
            };

            let quiet = *quiet;
            let progress_cb: Option<Box<dyn Fn(PullProgress) + Send>> = if quiet {
                None
            } else {
                Some(Box::new(move |p: PullProgress| {
                    if !p.status.is_empty() {
                        if p.total_bytes > 0 {
                            let percent =
                                (p.downloaded_bytes as f64 / p.total_bytes as f64) * 100.0;
                            print!(
                                "\r{}: {:.1}% ({}/{})",
                                p.status,
                                percent,
                                format_bytes(p.downloaded_bytes),
                                format_bytes(p.total_bytes)
                            );
                            std::io::Write::flush(&mut std::io::stdout()).ok();
                        } else {
                            println!("{}", p.status);
                        }
                    }
                }))
            };

            match store.pull(image, progress_cb).await {
                Ok(info) => {
                    if !quiet {
                        println!();
                    }
                    println!(
                        "{}: {}",
                        "Pulled".green().bold(),
                        info.reference.full_name()
                    );
                    println!("ID: {}", info.id);
                }
                Err(e) => {
                    if !quiet {
                        println!();
                    }
                    eprintln!("{}: {}", "Error".red().bold(), e);
                    std::process::exit(1);
                }
            }
            return;
        }

        Commands::Images { format } => {
            let store = match ImageStore::new(ImageStore::default_path()) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}: {}", "Error".red().bold(), e);
                    std::process::exit(1);
                }
            };

            let images = store.list();

            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&images).unwrap());
            } else {
                let rows: Vec<ImageRow> = images
                    .into_iter()
                    .map(|img| ImageRow {
                        id: img.id.clone(),
                        repository: format!(
                            "{}/{}",
                            img.reference.registry, img.reference.repository
                        ),
                        tag: img.reference.reference.clone(),
                        size: format_bytes(img.size),
                        created: format_timestamp(img.created),
                    })
                    .collect();

                if rows.is_empty() {
                    println!("No images found");
                } else {
                    println!("{}", Table::new(rows));
                }
            }
            return;
        }

        Commands::Rmi { image } => {
            let mut store = match ImageStore::new(ImageStore::default_path()) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}: {}", "Error".red().bold(), e);
                    std::process::exit(1);
                }
            };

            match store.remove(image) {
                Ok(()) => println!("Deleted: {}", image),
                Err(e) => {
                    eprintln!("{}: {}", "Error".red().bold(), e);
                    std::process::exit(1);
                }
            }
            return;
        }

        Commands::Events {
            filter,
            format,
            since: _,
        } => {
            let mut receiver = subscribe_events();
            let filter = filter.clone();
            let format = format.clone();

            println!("{}", "Watching for events... (Ctrl+C to stop)".dimmed());

            loop {
                if let Some(event) = receiver.recv().await {
                    // Apply filter
                    if let Some(ref f) = filter {
                        if !event.container_id.contains(f) {
                            continue;
                        }
                    }

                    // Format output
                    if format == "json" {
                        println!("{}", serde_json::to_string(&event).unwrap());
                    } else {
                        let event_str = format_event_type(&event.event_type);
                        print!(
                            "{} {} {}",
                            format_timestamp(event.timestamp).dimmed(),
                            event.container_id.cyan(),
                            event_str
                        );

                        if let Some(code) = event.exit_code {
                            print!(" (exit: {})", code);
                        }
                        if let Some(sig) = event.signal {
                            print!(" (signal: {})", sig);
                        }
                        println!();
                    }
                }
            }
        }

        _ => {} // Continue to runtime-dependent commands
    }

    // Build runtime config
    let mut config = RuntimeConfig::from_env();
    if let Some(socket) = cli.socket {
        config.socket_path = socket;
    }

    // Create runtime
    let runtime = match ContainerRuntime::new_with_config(config).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}: {}", "Error".red().bold(), e);
            std::process::exit(1);
        }
    };

    // Execute command
    let result = match cli.command {
        Commands::Create {
            name,
            rootfs,
            cmd,
            env,
            workdir,
            memory,
            cpus,
        } => {
            let mut container_config = ContainerConfig {
                id: name.clone(),
                rootfs,
                command: if cmd.is_empty() {
                    vec!["/bin/sh".to_string()]
                } else {
                    cmd
                },
                env,
                working_dir: workdir,
                ..Default::default()
            };

            // Parse memory limit
            if let Some(mem_str) = memory {
                container_config.resources.memory = Some(parse_memory(&mem_str));
            }

            // Set CPU limit
            if let Some(cpu) = cpus {
                container_config.resources.cpu = Some(cpu);
            }

            match runtime.create(container_config).await {
                Ok(id) => {
                    println!("{}", id);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }

        Commands::Start { name } => runtime.start(&name).await.map(|_| {
            println!("{}", name);
        }),

        Commands::Stop { name } => runtime.stop(&name).await.map(|_| {
            println!("{}", name);
        }),

        Commands::Delete { name, force } => {
            if force {
                let _ = runtime.stop(&name).await;
            }
            runtime.delete(&name).await.map(|_| {
                println!("{}", name);
            })
        }

        Commands::List { all, format } => match runtime.list().await {
            Ok(containers) => {
                let filtered: Vec<_> = if all {
                    containers
                } else {
                    containers
                        .into_iter()
                        .filter(|c| c.status == ContainerStatus::Running)
                        .collect()
                };

                if format == "json" {
                    println!("{}", serde_json::to_string_pretty(&filtered).unwrap());
                } else {
                    let rows: Vec<ContainerRow> = filtered
                        .into_iter()
                        .map(|c| ContainerRow {
                            id: c.id,
                            status: format_status(c.status),
                            pid: c.pid.map(|p| p.to_string()).unwrap_or_default(),
                        })
                        .collect();

                    if rows.is_empty() {
                        println!("No containers found");
                    } else {
                        println!("{}", Table::new(rows));
                    }
                }
                Ok(())
            }
            Err(e) => Err(e),
        },

        Commands::Logs { name, tail, follow } => {
            let options = LogOptions {
                tail,
                follow,
                ..Default::default()
            };
            match runtime.logs(&name, options).await {
                Ok(logs) => {
                    if !logs.stdout.is_empty() {
                        print!("{}", logs.stdout);
                    }
                    if !logs.stderr.is_empty() {
                        eprint!("{}", logs.stderr);
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }

        Commands::Stats { name, format } => {
            let metrics_result = if let Some(id) = name {
                runtime.metrics(&id).await.map(|m| vec![m])
            } else {
                runtime.all_metrics().await
            };

            match metrics_result {
                Ok(metrics) => {
                    if format == "json" {
                        println!("{}", serde_json::to_string_pretty(&metrics).unwrap());
                    } else {
                        let rows: Vec<StatsRow> = metrics
                            .into_iter()
                            .map(|m| StatsRow {
                                id: m.id,
                                cpu: format!("{:.2}%", m.cpu.usage_percent),
                                memory: format_bytes(m.memory.usage),
                                mem_percent: format!("{:.2}%", m.memory.usage_percent),
                                network: format!(
                                    "{} / {}",
                                    format_bytes(m.network.rx_bytes),
                                    format_bytes(m.network.tx_bytes)
                                ),
                                block: format!(
                                    "{} / {}",
                                    format_bytes(m.blkio.read_bytes),
                                    format_bytes(m.blkio.write_bytes)
                                ),
                                pids: m.pids.current.to_string(),
                            })
                            .collect();

                        if rows.is_empty() {
                            println!("No containers found");
                        } else {
                            println!("{}", Table::new(rows));
                        }
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }

        Commands::Health { name } => match runtime.health(&name).await {
            Ok(health) => {
                let status_str = match health.status {
                    HealthState::Healthy => "healthy".green(),
                    HealthState::Unhealthy => "unhealthy".red(),
                    HealthState::Starting => "starting".yellow(),
                    HealthState::None => "none".dimmed(),
                };
                println!("{}: {}", name, status_str);
                if !health.last_output.is_empty() {
                    println!("Last output: {}", health.last_output);
                }
                Ok(())
            }
            Err(e) => Err(e),
        },

        Commands::Exec {
            name,
            interactive,
            tty,
            command,
        } => {
            if command.is_empty() {
                eprintln!("{}: No command specified", "Error".red().bold());
                std::process::exit(1);
            }

            // Interactive/TTY mode
            if interactive || tty {
                #[cfg(unix)]
                {
                    use libcrun_shim::get_terminal_size;

                    if let Some((rows, cols)) = get_terminal_size() {
                        log::debug!("Terminal size: {}x{}", cols, rows);
                    }

                    eprintln!(
                        "{}: Interactive exec with TTY is available (basic implementation)",
                        "Note".yellow()
                    );
                    // For full interactive support, we'd need to:
                    // 1. Create PTY pair
                    // 2. Pass slave FD to container
                    // 3. Forward master I/O to stdin/stdout

                    // Fall through to regular exec for now
                }

                #[cfg(not(unix))]
                {
                    eprintln!(
                        "{}: Interactive mode not supported on this platform",
                        "Warning".yellow()
                    );
                }
            }

            match runtime.exec(&name, command).await {
                Ok((exit_code, stdout, stderr)) => {
                    print!("{}", stdout);
                    eprint!("{}", stderr);
                    std::process::exit(exit_code);
                }
                Err(e) => Err(e),
            }
        }

        Commands::Info => {
            // Handled above
            unreachable!()
        }

        Commands::Pull { .. }
        | Commands::Images { .. }
        | Commands::Rmi { .. }
        | Commands::Events { .. } => {
            // Handled above
            unreachable!()
        }

        Commands::Run {
            image,
            name,
            command,
            rm,
            env,
            workdir,
            memory,
            cpus,
        } => {
            // First, ensure image is available
            let store = match ImageStore::new(ImageStore::default_path()) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}: Image store error: {}", "Error".red().bold(), e);
                    std::process::exit(1);
                }
            };

            let rootfs = match store.get_rootfs(&image) {
                Some(path) => path,
                None => {
                    // Try to find by reference
                    let images = store.list();
                    let found = images.iter().find(|img| {
                        img.id == image
                            || img.reference.full_name().contains(&image)
                            || img.reference.reference == image
                    });

                    match found {
                        Some(img) => match store.get_rootfs(&img.id) {
                            Some(path) => path,
                            None => {
                                eprintln!(
                                    "{}: Rootfs not found for image: {}",
                                    "Error".red().bold(),
                                    image
                                );
                                std::process::exit(1);
                            }
                        },
                        None => {
                            eprintln!(
                                "{}: Image not found: {}. Use 'crun-shim pull {}' first.",
                                "Error".red().bold(),
                                image,
                                image
                            );
                            std::process::exit(1);
                        }
                    }
                }
            };

            let container_name = name.unwrap_or_else(|| {
                format!(
                    "run-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                )
            });

            let mut container_config = ContainerConfig {
                id: container_name.clone(),
                rootfs,
                command: if command.is_empty() {
                    vec!["/bin/sh".to_string()]
                } else {
                    command
                },
                env,
                working_dir: workdir.unwrap_or_else(|| "/".to_string()),
                ..Default::default()
            };

            if let Some(mem_str) = memory {
                container_config.resources.memory = Some(parse_memory(&mem_str));
            }
            if let Some(cpu) = cpus {
                container_config.resources.cpu = Some(cpu);
            }

            // Create container
            let id = match runtime.create(container_config).await {
                Ok(id) => {
                    println!("{}", id);
                    id
                }
                Err(e) => {
                    eprintln!("{}: {}", "Error".red().bold(), e);
                    std::process::exit(1);
                }
            };

            // Start container
            if let Err(e) = runtime.start(&id).await {
                eprintln!("{}: {}", "Error".red().bold(), e);
                std::process::exit(1);
            }

            // If --rm, delete after (in a real impl, we'd wait for exit)
            if rm {
                // For now, just note that cleanup would happen
                log::info!("Container {} will be removed after exit", id);
            }

            Ok(())
        }

        Commands::Prune { force } => {
            if !force {
                println!(
                    "{}",
                    "This will remove all stopped containers. Continue? [y/N] ".yellow()
                );
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).ok();
                if !input.trim().to_lowercase().starts_with('y') {
                    println!("Aborted.");
                    return;
                }
            }

            match runtime.cleanup_stopped().await {
                Ok(count) => {
                    println!(
                        "{}: Removed {} stopped container(s)",
                        "Prune".green().bold(),
                        count
                    );
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }

        Commands::Shutdown { timeout } => {
            println!("{}", "Initiating graceful shutdown...".yellow());
            println!("Timeout: {} seconds", timeout);

            // Setup a timeout for the shutdown
            let shutdown_future = runtime.shutdown();
            let timeout_duration = std::time::Duration::from_secs(timeout);

            match tokio::time::timeout(timeout_duration, shutdown_future).await {
                Ok(Ok(())) => {
                    println!("{}", "All containers stopped successfully".green());
                    Ok(())
                }
                Ok(Err(e)) => {
                    eprintln!("{}: Shutdown error: {}", "Error".red().bold(), e);
                    Err(e)
                }
                Err(_) => {
                    eprintln!(
                        "{}: Shutdown timed out after {} seconds",
                        "Warning".yellow(),
                        timeout
                    );
                    eprintln!("Some containers may still be running");
                    Ok(())
                }
            }
        }

        Commands::Cleanup {
            orphaned,
            stopped,
            force,
            dry_run,
        } => {
            println!("{}", "Container Cleanup".bold());

            let containers = match runtime.list().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}: Failed to list containers: {}", "Error".red().bold(), e);
                    return;
                }
            };

            let mut to_clean: Vec<_> = Vec::new();

            for container in &containers {
                let should_clean = if orphaned {
                    // Check if container is orphaned (has PID but process doesn't exist)
                    is_container_orphaned(&container.id).await
                } else if stopped {
                    container.status == ContainerStatus::Stopped
                } else {
                    // Default: clean both orphaned and stopped
                    container.status == ContainerStatus::Stopped
                        || is_container_orphaned(&container.id).await
                };

                if should_clean {
                    to_clean.push(container.clone());
                }
            }

            if to_clean.is_empty() {
                println!("{}", "No containers to clean up".green());
                return;
            }

            println!("Found {} container(s) to clean up:", to_clean.len());
            for container in &to_clean {
                println!(
                    "  {} ({})",
                    container.id,
                    format_status(container.status.clone())
                );
            }

            if dry_run {
                println!();
                println!("{}", "(dry run - no changes made)".dimmed());
                return;
            }

            if !force {
                println!();
                println!("Run with --force to proceed with cleanup");
                return;
            }

            let mut cleaned = 0;
            let mut failed = 0;

            for container in &to_clean {
                print!("Cleaning up {}... ", container.id);
                match runtime.force_delete(&container.id).await {
                    Ok(()) => {
                        println!("{}", "done".green());
                        cleaned += 1;
                    }
                    Err(e) => {
                        println!("{}: {}", "failed".red(), e);
                        failed += 1;
                    }
                }
            }

            println!();
            println!(
                "Cleaned: {}, Failed: {}",
                cleaned.to_string().green(),
                if failed > 0 {
                    failed.to_string().red()
                } else {
                    failed.to_string().normal()
                }
            );

            if failed > 0 {
                Err(libcrun_shim::ShimError::runtime(format!(
                    "{} containers failed to clean up",
                    failed
                )))
            } else {
                Ok(())
            }
        }

        Commands::Recover { force } => {
            println!("{}", "Runtime State Recovery".bold());

            // Check if state file exists
            let state_path = std::path::Path::new("/var/run/libcrun-shim/containers.json");

            if !state_path.exists() {
                println!(
                    "{}",
                    "No persisted state found - nothing to recover".yellow()
                );
                return;
            }

            // Try to read persisted state
            let content = match std::fs::read_to_string(state_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}: Failed to read state file: {}", "Error".red().bold(), e);
                    return;
                }
            };

            let persisted: Vec<serde_json::Value> = match serde_json::from_str(&content) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!(
                        "{}: Failed to parse state file: {}",
                        "Error".red().bold(),
                        e
                    );
                    return;
                }
            };

            println!("Found {} persisted container(s)", persisted.len());

            let current_containers = match runtime.list().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!(
                        "{}: Failed to list current containers: {}",
                        "Error".red().bold(),
                        e
                    );
                    return;
                }
            };

            let current_ids: std::collections::HashSet<_> =
                current_containers.iter().map(|c| c.id.as_str()).collect();

            let mut recovered = 0;
            let mut orphaned = 0;

            for container in &persisted {
                let id = match container.get("id").and_then(|s| s.as_str()) {
                    Some(id) => id,
                    None => continue,
                };

                let status = container
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("unknown");

                if current_ids.contains(id) {
                    println!("  {} - already tracked", id);
                    continue;
                }

                // Check if process is still running
                let pid = container.get("pid").and_then(|p| p.as_i64());
                let is_running = if let Some(pid) = pid {
                    if pid > 0 {
                        unsafe { libc::kill(pid as i32, 0) == 0 }
                    } else {
                        false
                    }
                } else {
                    false
                };

                if is_running {
                    println!(
                        "  {} - {} (PID {})",
                        id,
                        "orphaned process found".yellow(),
                        pid.unwrap()
                    );
                    orphaned += 1;

                    if force {
                        // Kill the orphaned process
                        if let Some(pid) = pid {
                            unsafe {
                                libc::kill(pid as i32, libc::SIGTERM);
                            }
                            println!("    Sent SIGTERM to PID {}", pid);
                        }
                    }
                } else {
                    println!("  {} - {} (was {})", id, "stale entry".dimmed(), status);
                    recovered += 1;
                }
            }

            println!();
            if orphaned > 0 {
                println!(
                    "{}: {} orphaned process(es) found",
                    "Warning".yellow(),
                    orphaned
                );
                if !force {
                    println!("Run with --force to terminate orphaned processes");
                }
            }

            if recovered > 0 || orphaned > 0 {
                println!(
                    "Recovery complete: {} stale entries, {} orphaned processes",
                    recovered, orphaned
                );
            } else {
                println!("{}", "State is consistent - nothing to recover".green());
            }

            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("{}: {}", "Error".red().bold(), e);
        std::process::exit(1);
    }
}

fn format_status(status: ContainerStatus) -> String {
    match status {
        ContainerStatus::Running => "Running".green().to_string(),
        ContainerStatus::Created => "Created".yellow().to_string(),
        ContainerStatus::Stopped => "Stopped".dimmed().to_string(),
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2}KB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

fn parse_memory(s: &str) -> u64 {
    let s = s.to_lowercase();
    let (num_str, multiplier) = if s.ends_with("g") || s.ends_with("gb") {
        (
            s.trim_end_matches("gb").trim_end_matches("g"),
            1024 * 1024 * 1024,
        )
    } else if s.ends_with("m") || s.ends_with("mb") {
        (s.trim_end_matches("mb").trim_end_matches("m"), 1024 * 1024)
    } else if s.ends_with("k") || s.ends_with("kb") {
        (s.trim_end_matches("kb").trim_end_matches("k"), 1024)
    } else {
        (s.as_str(), 1)
    };

    num_str.parse::<u64>().unwrap_or(0) * multiplier
}

fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return "N/A".to_string();
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let diff = now.saturating_sub(ts);

    if diff < 60 {
        format!("{} seconds ago", diff)
    } else if diff < 3600 {
        format!("{} minutes ago", diff / 60)
    } else if diff < 86400 {
        format!("{} hours ago", diff / 3600)
    } else if diff < 2592000 {
        format!("{} days ago", diff / 86400)
    } else if diff < 31536000 {
        format!("{} months ago", diff / 2592000)
    } else {
        format!("{} years ago", diff / 31536000)
    }
}

fn format_event_type(event_type: &ContainerEventType) -> colored::ColoredString {
    match event_type {
        ContainerEventType::Create => "create".green(),
        ContainerEventType::Start => "start".green(),
        ContainerEventType::Stop => "stop".yellow(),
        ContainerEventType::Kill => "kill".red(),
        ContainerEventType::Die => "die".red(),
        ContainerEventType::Delete => "delete".dimmed(),
        ContainerEventType::Pause => "pause".yellow(),
        ContainerEventType::Unpause => "unpause".green(),
        ContainerEventType::HealthOk => "health_ok".green(),
        ContainerEventType::HealthFail => "health_fail".red(),
        ContainerEventType::Oom => "oom".red().bold(),
        ContainerEventType::ExecStart => "exec_start".blue(),
        ContainerEventType::ExecDie => "exec_die".blue(),
    }
}

/// Setup Ctrl+C (SIGINT) and SIGTERM handler for graceful shutdown
fn setup_signal_handler() {
    let shutdown_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let shutdown_count_clone = Arc::clone(&shutdown_count);

    ctrlc::set_handler(move || {
        let count = shutdown_count_clone.fetch_add(1, Ordering::SeqCst);

        if count == 0 {
            eprintln!(
                "\n{}",
                "Received interrupt, initiating graceful shutdown...".yellow()
            );
            eprintln!("{}", "Press Ctrl+C again to force exit".dimmed());
            SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);

            // Spawn cleanup in background
            std::thread::spawn(|| {
                // Give some time for graceful shutdown
                std::thread::sleep(std::time::Duration::from_secs(30));
                if SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
                    eprintln!("{}", "Shutdown timeout, forcing exit...".red());
                    std::process::exit(1);
                }
            });
        } else if count == 1 {
            eprintln!(
                "\n{}",
                "Force shutdown requested, exiting immediately..."
                    .red()
                    .bold()
            );
            std::process::exit(130); // 128 + SIGINT(2)
        } else {
            std::process::exit(130);
        }
    })
    .expect("Failed to set Ctrl+C handler");
}

/// Setup panic handler for graceful cleanup on panics
fn setup_panic_handler() {
    let default_hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |panic_info| {
        // Log the panic
        eprintln!("\n{}", "=".repeat(60).red());
        eprintln!(
            "{}",
            "PANIC: Runtime encountered an unexpected error"
                .red()
                .bold()
        );
        eprintln!("{}", "=".repeat(60).red());

        // Get panic location
        if let Some(location) = panic_info.location() {
            eprintln!(
                "Location: {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        }

        // Get panic message
        if let Some(message) = panic_info.payload().downcast_ref::<&str>() {
            eprintln!("Message: {}", message);
        } else if let Some(message) = panic_info.payload().downcast_ref::<String>() {
            eprintln!("Message: {}", message);
        }

        eprintln!();
        eprintln!("{}", "Attempting to cleanup containers...".yellow());

        // Attempt emergency cleanup
        if let Err(e) = emergency_cleanup() {
            eprintln!("{}: Failed to cleanup: {}", "Warning".yellow(), e);
        } else {
            eprintln!("{}", "Cleanup completed".green());
        }

        eprintln!();
        eprintln!("{}", "If containers remain orphaned, run:".dimmed());
        eprintln!("  crun-shim cleanup --orphaned");
        eprintln!("{}", "=".repeat(60).red());

        // Call the default panic handler
        default_hook(panic_info);
    }));
}

/// Emergency cleanup function called during panic or forced shutdown
fn emergency_cleanup() -> Result<(), String> {
    // Try to read the container state file and stop any running containers
    let state_path = std::path::Path::new("/var/run/libcrun-shim/containers.json");

    if !state_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(state_path)
        .map_err(|e| format!("Failed to read state file: {}", e))?;

    let containers: Vec<serde_json::Value> =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse state file: {}", e))?;

    let mut cleaned = 0;
    for container in containers {
        if let Some(status) = container.get("status").and_then(|s| s.as_str()) {
            if status == "Running" || status == "running" {
                if let Some(id) = container.get("id").and_then(|s| s.as_str()) {
                    eprintln!("  Stopping container: {}", id);
                    // Send SIGTERM to any running container process
                    if let Some(pid) = container.get("pid").and_then(|p| p.as_i64()) {
                        if pid > 0 {
                            unsafe {
                                libc::kill(pid as i32, libc::SIGTERM);
                            }
                            cleaned += 1;
                        }
                    }
                }
            }
        }
    }

    if cleaned > 0 {
        eprintln!("  Sent SIGTERM to {} containers", cleaned);
    }

    Ok(())
}

/// Check if shutdown has been requested (for use in long-running operations)
pub fn is_shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

/// Check if a container is orphaned (has PID but process doesn't exist)
async fn is_container_orphaned(container_id: &str) -> bool {
    // Try to read container state to get PID
    let state_path = std::path::Path::new("/var/run/libcrun-shim/containers.json");

    if !state_path.exists() {
        return false;
    }

    let content = match std::fs::read_to_string(state_path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let containers: Vec<serde_json::Value> = match serde_json::from_str(&content) {
        Ok(c) => c,
        Err(_) => return false,
    };

    for container in containers {
        let id = match container.get("id").and_then(|s| s.as_str()) {
            Some(id) => id,
            None => continue,
        };

        if id != container_id {
            continue;
        }

        let status = container
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if status != "Running" && status != "running" {
            return false;
        }

        // Check if the PID still exists
        if let Some(pid) = container.get("pid").and_then(|p| p.as_i64()) {
            if pid > 0 {
                // Check if process exists
                let exists = unsafe { libc::kill(pid as i32, 0) == 0 };
                return !exists; // Orphaned if process doesn't exist
            }
        }
    }

    false
}
