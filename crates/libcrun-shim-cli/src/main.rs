use clap::{Parser, Subcommand};
use colored::Colorize;
use libcrun_shim::{
    ContainerConfig, ContainerRuntime, ContainerStatus, HealthState, LogOptions, RuntimeConfig,
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

        /// Command to execute
        #[arg(num_args = 1..)]
        command: Vec<String>,
    },

    /// Show runtime information
    Info,
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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Setup logging
    if cli.verbose {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    }

    // Handle info command separately (doesn't need runtime)
    if let Commands::Info = cli.command {
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

        Commands::Exec { name, command } => {
            if command.is_empty() {
                eprintln!("{}: No command specified", "Error".red().bold());
                std::process::exit(1);
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

