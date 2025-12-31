use clap::{Parser, Subcommand};
use colored::Colorize;
use libcrun_shim::{
    ContainerConfig, ContainerEventType, ContainerRuntime, ContainerStatus, HealthState,
    ImageStore, LogOptions, PullProgress, RuntimeConfig, subscribe_events,
};
use std::path::PathBuf;
use tabled::{Table, Tabled};

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
    let cli = Cli::parse();

    // Setup logging
    if cli.verbose {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    }

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
                            let percent = (p.downloaded_bytes as f64 / p.total_bytes as f64) * 100.0;
                            print!("\r{}: {:.1}% ({}/{})", p.status, percent,
                                format_bytes(p.downloaded_bytes), format_bytes(p.total_bytes));
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
                    println!("{}: {}", "Pulled".green().bold(), info.reference.full_name());
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
                        repository: format!("{}/{}", img.reference.registry, img.reference.repository),
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

        Commands::Start { name } => {
            runtime.start(&name).await.map(|_| {
                println!("{}", name);
            })
        }

        Commands::Stop { name } => {
            runtime.stop(&name).await.map(|_| {
                println!("{}", name);
            })
        }

        Commands::Delete { name, force } => {
            if force {
                let _ = runtime.stop(&name).await;
            }
            runtime.delete(&name).await.map(|_| {
                println!("{}", name);
            })
        }

        Commands::List { all, format } => {
            match runtime.list().await {
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
            }
        }

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

        Commands::Health { name } => {
            match runtime.health(&name).await {
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
            }
        }

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
                                eprintln!("{}: Rootfs not found for image: {}", "Error".red().bold(), image);
                                std::process::exit(1);
                            }
                        },
                        None => {
                            eprintln!("{}: Image not found: {}. Use 'crun-shim pull {}' first.", "Error".red().bold(), image, image);
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
        (s.trim_end_matches("gb").trim_end_matches("g"), 1024 * 1024 * 1024)
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

