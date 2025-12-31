pub mod rpc;
mod vm;
mod vsock;

use crate::types::RuntimeConfig;
use crate::*;
use libcrun_shim_proto::*;

pub struct MacOsRuntime {
    #[allow(dead_code)]
    vm: vm::VirtualMachine,
    #[allow(dead_code)]
    rpc: rpc::RpcClient,
    config: RuntimeConfig,
}

impl MacOsRuntime {
    /// Create a new runtime with default configuration (from environment)
    pub async fn new() -> Result<Self> {
        Self::new_with_config(RuntimeConfig::from_env()).await
    }

    /// Create a new runtime with custom configuration
    pub async fn new_with_config(config: RuntimeConfig) -> Result<Self> {
        log::info!("Starting MacOsRuntime with configuration:");
        log::info!("  Socket path: {}", config.socket_path.display());
        log::info!("  Vsock port: {}", config.vsock_port);
        log::info!("  Connection timeout: {}s", config.connection_timeout);
        if !config.vm_asset_paths.is_empty() {
            log::info!("  Custom VM asset paths: {:?}", config.vm_asset_paths);
        }

        let vm = vm::VirtualMachine::start_with_config(config.clone()).await?;

        #[cfg(target_os = "macos")]
        {
            if vm.has_vm_control() {
                log::info!("VM started via Swift bridge - waiting for guest to boot...");
                // Kernel boot + initramfs + agent startup typically takes 15-20s
                tokio::time::sleep(tokio::time::Duration::from_secs(20)).await;
                log::info!("Boot wait complete, attempting to connect to agent");
            } else {
                log::info!("Using fallback mode - assuming external VM is running");
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }

        // Connect to agent with retry logic
        #[cfg(target_os = "macos")]
        let rpc = {
            let max_retries = 5;
            let retry_delay = tokio::time::Duration::from_secs(3);
            let mut connected_client: Option<rpc::RpcClient> = None;
            let mut last_error = None;

            for attempt in 1..=max_retries {
                log::info!("Connection attempt {}/{}", attempt, max_retries);

                // Try vsock first if bridge is available
                if let Some(handle) = vm.get_bridge_handle() {
                    log::debug!("Attempting vsock connection via Swift bridge");
                    match rpc::RpcClient::connect_with_vm_bridge(vm.config(), handle) {
                        Ok(client) => {
                            log::info!("Connected to VM agent via native vsock");
                            connected_client = Some(client);
                            break;
                        }
                        Err(e) => {
                            log::debug!("Vsock connection failed: {}", e);
                        }
                    }
                }

                // Try Unix socket as fallback
                match rpc::RpcClient::connect_with_config(vm.config()) {
                    Ok(client) => {
                        log::info!("Connected to VM agent via Unix socket");
                        connected_client = Some(client);
                        break;
                    }
                    Err(e) => {
                        log::debug!("Unix socket connection failed: {}", e);
                        last_error = Some(e);
                    }
                }

                if attempt < max_retries {
                    log::info!("Retrying in {}s...", retry_delay.as_secs());
                    tokio::time::sleep(retry_delay).await;
                }
            }

            match connected_client {
                Some(client) => client,
                None => {
                    return Err(last_error.unwrap_or_else(|| {
                        ShimError::runtime("Failed to connect to agent after all retries")
                    }));
                }
            }
        };

        #[cfg(not(target_os = "macos"))]
        let rpc = rpc::RpcClient::connect_with_config(&config)?;

        log::info!("Connected to VM agent via RPC");

        Ok(Self { vm, rpc, config })
    }

    /// Get the runtime configuration
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }
}

impl RuntimeImpl for MacOsRuntime {
    async fn create(&self, container_config: ContainerConfig) -> Result<String> {
        use libcrun_shim_proto::*;
        let req = Request::Create(CreateRequest {
            id: container_config.id.clone(),
            rootfs: container_config.rootfs.display().to_string(),
            command: container_config.command,
            env: container_config.env,
            working_dir: container_config.working_dir,
            stdio: StdioConfigProto {
                tty: container_config.stdio.tty,
                open_stdin: container_config.stdio.open_stdin,
                stdin_path: container_config
                    .stdio
                    .stdin_path
                    .as_ref()
                    .map(|p| p.display().to_string()),
                stdout_path: container_config
                    .stdio
                    .stdout_path
                    .as_ref()
                    .map(|p| p.display().to_string()),
                stderr_path: container_config
                    .stdio
                    .stderr_path
                    .as_ref()
                    .map(|p| p.display().to_string()),
            },
            network: NetworkConfigProto {
                mode: container_config.network.mode,
                port_mappings: container_config
                    .network
                    .port_mappings
                    .into_iter()
                    .map(|pm| PortMappingProto {
                        host_port: pm.host_port,
                        container_port: pm.container_port,
                        protocol: pm.protocol,
                        host_ip: pm.host_ip,
                    })
                    .collect(),
                interfaces: container_config
                    .network
                    .interfaces
                    .into_iter()
                    .map(|ni| NetworkInterfaceProto {
                        name: ni.name,
                        interface_type: ni.interface_type,
                        config: ni.config,
                    })
                    .collect(),
            },
            volumes: container_config
                .volumes
                .into_iter()
                .map(|vm| VolumeMountProto {
                    source: vm.source.display().to_string(),
                    destination: vm.destination.display().to_string(),
                    options: vm.options,
                })
                .collect(),
            resources: ResourceLimitsProto {
                cpu: container_config.resources.cpu,
                memory: container_config.resources.memory,
                memory_swap: container_config.resources.memory_swap,
                pids: container_config.resources.pids,
                blkio_weight: container_config.resources.blkio_weight,
            },
            health_check: container_config.health_check.map(|hc| HealthCheckProto {
                command: hc.command,
                interval_secs: hc.interval,
                timeout_secs: hc.timeout,
                retries: hc.retries,
                start_period_secs: hc.start_period,
            }),
        });

        let mut rpc = rpc::RpcClient::connect_with_config(&self.config)?;
        match rpc.call(req)? {
            Response::Created(id) => Ok(id),
            Response::Error(e) => Err(ShimError::runtime_with_context(
                e,
                "RPC create request failed",
            )),
            _ => Err(ShimError::runtime(
                "Unexpected response type from RPC create request",
            )),
        }
    }

    async fn start(&self, id: &str) -> Result<()> {
        let mut rpc = rpc::RpcClient::connect_with_config(&self.config)?;
        match rpc.call(Request::Start(id.to_string()))? {
            Response::Started => Ok(()),
            Response::Error(e) => Err(ShimError::runtime_with_context(
                e,
                format!("RPC start request failed for container: {}", id),
            )),
            _ => Err(ShimError::runtime(
                "Unexpected response type from RPC start request",
            )),
        }
    }

    async fn stop(&self, id: &str) -> Result<()> {
        let mut rpc = rpc::RpcClient::connect_with_config(&self.config)?;
        match rpc.call(Request::Stop(id.to_string()))? {
            Response::Stopped => Ok(()),
            Response::Error(e) => Err(ShimError::runtime_with_context(
                e,
                format!("RPC stop request failed for container: {}", id),
            )),
            _ => Err(ShimError::runtime(
                "Unexpected response type from RPC stop request",
            )),
        }
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let mut rpc = rpc::RpcClient::connect_with_config(&self.config)?;
        match rpc.call(Request::Delete(id.to_string()))? {
            Response::Deleted => Ok(()),
            Response::Error(e) => Err(ShimError::runtime_with_context(
                e,
                format!("RPC delete request failed for container: {}", id),
            )),
            _ => Err(ShimError::runtime(
                "Unexpected response type from RPC delete request",
            )),
        }
    }

    async fn list(&self) -> Result<Vec<ContainerInfo>> {
        let mut rpc = rpc::RpcClient::connect_with_config(&self.config)?;
        match rpc.call(Request::List)? {
            Response::List(list) => Ok(list
                .into_iter()
                .map(|info| ContainerInfo {
                    id: info.id,
                    status: match info.status.as_str() {
                        "Created" => ContainerStatus::Created,
                        "Running" => ContainerStatus::Running,
                        _ => ContainerStatus::Stopped,
                    },
                    pid: info.pid,
                })
                .collect()),
            Response::Error(e) => Err(ShimError::runtime_with_context(
                e,
                "RPC list request failed",
            )),
            _ => Err(ShimError::runtime(
                "Unexpected response type from RPC list request",
            )),
        }
    }

    async fn metrics(&self, id: &str) -> Result<ContainerMetrics> {
        let mut rpc = rpc::RpcClient::connect_with_config(&self.config)?;
        match rpc.call(Request::Metrics(id.to_string()))? {
            Response::Metrics(m) => Ok(proto_to_metrics(m)),
            Response::Error(e) => Err(ShimError::runtime_with_context(
                e,
                format!("RPC metrics request failed for container: {}", id),
            )),
            _ => Err(ShimError::runtime(
                "Unexpected response type from RPC metrics request",
            )),
        }
    }

    async fn all_metrics(&self) -> Result<Vec<ContainerMetrics>> {
        let mut rpc = rpc::RpcClient::connect_with_config(&self.config)?;
        match rpc.call(Request::AllMetrics)? {
            Response::AllMetrics(list) => Ok(list.into_iter().map(proto_to_metrics).collect()),
            Response::Error(e) => Err(ShimError::runtime_with_context(
                e,
                "RPC all_metrics request failed",
            )),
            _ => Err(ShimError::runtime(
                "Unexpected response type from RPC all_metrics request",
            )),
        }
    }

    async fn logs(&self, id: &str, options: LogOptions) -> Result<ContainerLogs> {
        let mut rpc = rpc::RpcClient::connect_with_config(&self.config)?;
        let req = Request::Logs(libcrun_shim_proto::LogsRequest {
            id: id.to_string(),
            tail: options.tail,
            since: options.since,
            timestamps: options.timestamps,
        });
        match rpc.call(req)? {
            Response::Logs(l) => Ok(ContainerLogs {
                id: l.id,
                stdout: l.stdout,
                stderr: l.stderr,
                timestamp: l.timestamp,
            }),
            Response::Error(e) => Err(ShimError::runtime_with_context(
                e,
                format!("RPC logs request failed for container: {}", id),
            )),
            _ => Err(ShimError::runtime(
                "Unexpected response type from RPC logs request",
            )),
        }
    }

    async fn health(&self, id: &str) -> Result<HealthStatus> {
        let mut rpc = rpc::RpcClient::connect_with_config(&self.config)?;
        match rpc.call(Request::Health(id.to_string()))? {
            Response::Health(h) => Ok(HealthStatus {
                id: h.id,
                status: match h.status.as_str() {
                    "healthy" => HealthState::Healthy,
                    "unhealthy" => HealthState::Unhealthy,
                    "starting" => HealthState::Starting,
                    _ => HealthState::None,
                },
                failing_streak: h.failing_streak,
                last_output: h.last_output,
                last_check: h.last_check,
            }),
            Response::Error(e) => Err(ShimError::runtime_with_context(
                e,
                format!("RPC health request failed for container: {}", id),
            )),
            _ => Err(ShimError::runtime(
                "Unexpected response type from RPC health request",
            )),
        }
    }

    async fn exec(&self, id: &str, command: Vec<String>) -> Result<(i32, String, String)> {
        let mut rpc = rpc::RpcClient::connect_with_config(&self.config)?;
        let req = Request::Exec(libcrun_shim_proto::ExecRequest {
            id: id.to_string(),
            command,
            env: vec![],
            working_dir: None,
        });
        match rpc.call(req)? {
            Response::Exec(e) => Ok((e.exit_code, e.stdout, e.stderr)),
            Response::Error(e) => Err(ShimError::runtime_with_context(
                e,
                format!("RPC exec request failed for container: {}", id),
            )),
            _ => Err(ShimError::runtime(
                "Unexpected response type from RPC exec request",
            )),
        }
    }
}

/// Convert proto metrics to local types
fn proto_to_metrics(m: libcrun_shim_proto::ContainerMetricsProto) -> ContainerMetrics {
    ContainerMetrics {
        id: m.id,
        timestamp: m.timestamp,
        cpu: CpuMetrics {
            usage_total: m.cpu.usage_total,
            usage_user: m.cpu.usage_user,
            usage_system: m.cpu.usage_system,
            per_cpu: m.cpu.per_cpu,
            throttled_periods: m.cpu.throttled_periods,
            throttled_time: m.cpu.throttled_time,
            usage_percent: m.cpu.usage_percent,
        },
        memory: MemoryMetrics {
            usage: m.memory.usage,
            max_usage: m.memory.max_usage,
            limit: m.memory.limit,
            cache: m.memory.cache,
            rss: m.memory.rss,
            swap: m.memory.swap,
            usage_percent: m.memory.usage_percent,
        },
        blkio: BlkioMetrics {
            read_bytes: m.blkio.read_bytes,
            write_bytes: m.blkio.write_bytes,
            read_ops: m.blkio.read_ops,
            write_ops: m.blkio.write_ops,
        },
        network: NetworkMetrics {
            rx_bytes: m.network.rx_bytes,
            tx_bytes: m.network.tx_bytes,
            rx_packets: m.network.rx_packets,
            tx_packets: m.network.tx_packets,
            rx_errors: m.network.rx_errors,
            tx_errors: m.network.tx_errors,
            rx_dropped: m.network.rx_dropped,
            tx_dropped: m.network.tx_dropped,
        },
        pids: PidsMetrics {
            current: m.pids.current,
            limit: m.pids.limit,
        },
    }
}
