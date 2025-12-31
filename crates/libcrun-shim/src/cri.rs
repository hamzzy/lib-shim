//! Kubernetes Container Runtime Interface (CRI)
//!
//! This module implements the Kubernetes CRI for integration with kubelet.
//!
//! Reference: https://github.com/kubernetes/cri-api

use crate::error::{Result, ShimError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// CRI Runtime Service interface
pub trait RuntimeService {
    /// Version returns the runtime name, runtime version, and runtime API version.
    fn version(&self, version: &str) -> Result<VersionResponse>;

    /// RunPodSandbox creates and starts a pod-level sandbox.
    fn run_pod_sandbox(&self, config: PodSandboxConfig) -> Result<String>;

    /// StopPodSandbox stops any running processes that are part of the sandbox.
    fn stop_pod_sandbox(&self, pod_sandbox_id: &str) -> Result<()>;

    /// RemovePodSandbox removes the sandbox.
    fn remove_pod_sandbox(&self, pod_sandbox_id: &str) -> Result<()>;

    /// PodSandboxStatus returns the status of the PodSandbox.
    fn pod_sandbox_status(&self, pod_sandbox_id: &str, verbose: bool) -> Result<PodSandboxStatus>;

    /// ListPodSandbox returns a list of PodSandboxes.
    fn list_pod_sandbox(&self, filter: Option<PodSandboxFilter>) -> Result<Vec<PodSandbox>>;

    /// CreateContainer creates a new container in the given PodSandbox.
    fn create_container(
        &self,
        pod_sandbox_id: &str,
        config: ContainerConfig,
        sandbox_config: PodSandboxConfig,
    ) -> Result<String>;

    /// StartContainer starts the container.
    fn start_container(&self, container_id: &str) -> Result<()>;

    /// StopContainer stops a running container.
    fn stop_container(&self, container_id: &str, timeout: i64) -> Result<()>;

    /// RemoveContainer removes the container.
    fn remove_container(&self, container_id: &str) -> Result<()>;

    /// ListContainers lists all containers.
    fn list_containers(&self, filter: Option<ContainerFilter>) -> Result<Vec<Container>>;

    /// ContainerStatus returns the status of the container.
    fn container_status(&self, container_id: &str, verbose: bool) -> Result<ContainerStatusResponse>;

    /// UpdateContainerResources updates the resource constraints of the container.
    fn update_container_resources(
        &self,
        container_id: &str,
        resources: LinuxContainerResources,
    ) -> Result<()>;

    /// ReopenContainerLog reopens the container log file.
    fn reopen_container_log(&self, container_id: &str) -> Result<()>;

    /// ExecSync runs a command in a container synchronously.
    fn exec_sync(&self, container_id: &str, cmd: Vec<String>, timeout: i64) -> Result<ExecSyncResponse>;

    /// Exec prepares a streaming endpoint to execute a command in the container.
    fn exec(&self, request: ExecRequest) -> Result<ExecResponse>;

    /// Attach prepares a streaming endpoint to attach to a running container.
    fn attach(&self, request: AttachRequest) -> Result<AttachResponse>;

    /// PortForward prepares a streaming endpoint to forward ports from a PodSandbox.
    fn port_forward(&self, request: PortForwardRequest) -> Result<PortForwardResponse>;

    /// ContainerStats returns stats of the container.
    fn container_stats(&self, container_id: &str) -> Result<ContainerStats>;

    /// ListContainerStats returns stats of all running containers.
    fn list_container_stats(&self, filter: Option<ContainerStatsFilter>) -> Result<Vec<ContainerStats>>;

    /// UpdateRuntimeConfig updates the runtime configuration.
    fn update_runtime_config(&self, runtime_config: RuntimeConfig) -> Result<()>;

    /// Status returns the status of the runtime.
    fn status(&self, verbose: bool) -> Result<RuntimeStatus>;
}

/// CRI Image Service interface
pub trait ImageService {
    /// ListImages lists existing images.
    fn list_images(&self, filter: Option<ImageFilter>) -> Result<Vec<Image>>;

    /// ImageStatus returns the status of the image.
    fn image_status(&self, image: ImageSpec, verbose: bool) -> Result<ImageStatusResponse>;

    /// PullImage pulls an image with authentication config.
    fn pull_image(
        &self,
        image: ImageSpec,
        auth: Option<AuthConfig>,
        sandbox_config: Option<PodSandboxConfig>,
    ) -> Result<String>;

    /// RemoveImage removes the image.
    fn remove_image(&self, image: ImageSpec) -> Result<()>;

    /// ImageFsInfo returns information of the filesystem that is used to store images.
    fn image_fs_info(&self) -> Result<Vec<FilesystemUsage>>;
}

/// Version response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionResponse {
    pub version: String,
    pub runtime_name: String,
    pub runtime_version: String,
    pub runtime_api_version: String,
}

/// Pod sandbox config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodSandboxConfig {
    pub metadata: PodSandboxMetadata,
    pub hostname: String,
    pub log_directory: String,
    pub dns_config: Option<DNSConfig>,
    pub port_mappings: Vec<PortMapping>,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
    pub linux: Option<LinuxPodSandboxConfig>,
}

/// Pod sandbox metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodSandboxMetadata {
    pub name: String,
    pub uid: String,
    pub namespace: String,
    pub attempt: u32,
}

/// DNS config
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DNSConfig {
    pub servers: Vec<String>,
    pub searches: Vec<String>,
    pub options: Vec<String>,
}

/// Port mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    pub protocol: Protocol,
    pub container_port: i32,
    pub host_port: i32,
    pub host_ip: String,
}

/// Protocol
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Protocol {
    TCP,
    UDP,
    SCTP,
}

/// Linux pod sandbox config
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinuxPodSandboxConfig {
    pub cgroup_parent: String,
    pub security_context: Option<LinuxSandboxSecurityContext>,
    pub sysctls: HashMap<String, String>,
    pub overhead: Option<LinuxContainerResources>,
    pub resources: Option<LinuxContainerResources>,
}

/// Linux sandbox security context
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinuxSandboxSecurityContext {
    pub namespace_options: Option<NamespaceOption>,
    pub selinux_options: Option<SELinuxOption>,
    pub run_as_user: Option<Int64Value>,
    pub run_as_group: Option<Int64Value>,
    pub readonly_rootfs: bool,
    pub supplemental_groups: Vec<i64>,
    pub privileged: bool,
    pub seccomp: Option<SecurityProfile>,
    pub apparmor: Option<SecurityProfile>,
}

/// Namespace option
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NamespaceOption {
    pub network: NamespaceMode,
    pub pid: NamespaceMode,
    pub ipc: NamespaceMode,
    pub target_id: String,
    pub user_namespaces: Option<UserNamespace>,
}

/// Namespace mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum NamespaceMode {
    #[default]
    POD,
    CONTAINER,
    NODE,
    TARGET,
}

/// User namespace
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserNamespace {
    pub mode: UserNamespaceMode,
    pub uids: Vec<IDMapping>,
    pub gids: Vec<IDMapping>,
}

/// User namespace mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum UserNamespaceMode {
    #[default]
    POD,
    NODE,
}

/// ID mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IDMapping {
    pub host_id: u32,
    pub container_id: u32,
    pub length: u32,
}

/// SELinux option
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SELinuxOption {
    pub user: String,
    pub role: String,
    pub r#type: String,
    pub level: String,
}

/// Int64 value wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Int64Value {
    pub value: i64,
}

/// Security profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityProfile {
    pub profile_type: ProfileType,
    pub localhost_ref: String,
}

/// Profile type
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ProfileType {
    RuntimeDefault,
    Unconfined,
    Localhost,
}

/// Linux container resources
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinuxContainerResources {
    pub cpu_period: i64,
    pub cpu_quota: i64,
    pub cpu_shares: i64,
    pub memory_limit_in_bytes: i64,
    pub oom_score_adj: i64,
    pub cpuset_cpus: String,
    pub cpuset_mems: String,
    pub hugepage_limits: Vec<HugepageLimit>,
    pub unified: HashMap<String, String>,
    pub memory_swap_limit_in_bytes: i64,
}

/// Hugepage limit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HugepageLimit {
    pub page_size: String,
    pub limit: u64,
}

/// Pod sandbox status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodSandboxStatus {
    pub id: String,
    pub metadata: PodSandboxMetadata,
    pub state: PodSandboxState,
    pub created_at: i64,
    pub network: Option<PodSandboxNetworkStatus>,
    pub linux: Option<LinuxPodSandboxStatus>,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
    pub runtime_handler: String,
}

/// Pod sandbox state
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PodSandboxState {
    SANDBOX_READY,
    SANDBOX_NOTREADY,
}

/// Pod sandbox network status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodSandboxNetworkStatus {
    pub ip: String,
    pub additional_ips: Vec<PodIP>,
}

/// Pod IP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodIP {
    pub ip: String,
}

/// Linux pod sandbox status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinuxPodSandboxStatus {
    pub namespaces: Namespace,
}

/// Namespace
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Namespace {
    pub network: String,
    pub options: Option<NamespaceOption>,
}

/// Pod sandbox filter
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PodSandboxFilter {
    pub id: Option<String>,
    pub state: Option<PodSandboxStateValue>,
    pub label_selector: HashMap<String, String>,
}

/// Pod sandbox state value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodSandboxStateValue {
    pub state: PodSandboxState,
}

/// Pod sandbox
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodSandbox {
    pub id: String,
    pub metadata: PodSandboxMetadata,
    pub state: PodSandboxState,
    pub created_at: i64,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
    pub runtime_handler: String,
}

/// Container config (CRI)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub metadata: ContainerMetadata,
    pub image: ImageSpec,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub working_dir: String,
    pub envs: Vec<KeyValue>,
    pub mounts: Vec<Mount>,
    pub devices: Vec<Device>,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
    pub log_path: String,
    pub stdin: bool,
    pub stdin_once: bool,
    pub tty: bool,
    pub linux: Option<LinuxContainerConfig>,
}

/// Container metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerMetadata {
    pub name: String,
    pub attempt: u32,
}

/// Image spec
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSpec {
    pub image: String,
    pub annotations: HashMap<String, String>,
}

/// Key value pair
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

/// Mount
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mount {
    pub container_path: String,
    pub host_path: String,
    pub readonly: bool,
    pub selinux_relabel: bool,
    pub propagation: MountPropagation,
}

/// Mount propagation
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MountPropagation {
    PROPAGATION_PRIVATE,
    PROPAGATION_HOST_TO_CONTAINER,
    PROPAGATION_BIDIRECTIONAL,
}

/// Device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub container_path: String,
    pub host_path: String,
    pub permissions: String,
}

/// Linux container config
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinuxContainerConfig {
    pub resources: LinuxContainerResources,
    pub security_context: Option<LinuxContainerSecurityContext>,
}

/// Linux container security context
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinuxContainerSecurityContext {
    pub capabilities: Option<Capability>,
    pub privileged: bool,
    pub namespace_options: Option<NamespaceOption>,
    pub selinux_options: Option<SELinuxOption>,
    pub run_as_user: Option<Int64Value>,
    pub run_as_group: Option<Int64Value>,
    pub run_as_username: String,
    pub readonly_rootfs: bool,
    pub supplemental_groups: Vec<i64>,
    pub no_new_privs: bool,
    pub masked_paths: Vec<String>,
    pub readonly_paths: Vec<String>,
    pub seccomp: Option<SecurityProfile>,
    pub apparmor: Option<SecurityProfile>,
}

/// Capability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Capability {
    pub add_capabilities: Vec<String>,
    pub drop_capabilities: Vec<String>,
}

/// Container filter
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContainerFilter {
    pub id: Option<String>,
    pub state: Option<ContainerStateValue>,
    pub pod_sandbox_id: Option<String>,
    pub label_selector: HashMap<String, String>,
}

/// Container state value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStateValue {
    pub state: ContainerState,
}

/// Container state
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ContainerState {
    CONTAINER_CREATED,
    CONTAINER_RUNNING,
    CONTAINER_EXITED,
    CONTAINER_UNKNOWN,
}

/// Container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container {
    pub id: String,
    pub pod_sandbox_id: String,
    pub metadata: ContainerMetadata,
    pub image: ImageSpec,
    pub image_ref: String,
    pub state: ContainerState,
    pub created_at: i64,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
}

/// Container status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStatusResponse {
    pub status: ContainerStatusInfo,
    pub info: HashMap<String, String>,
}

/// Container status info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStatusInfo {
    pub id: String,
    pub metadata: ContainerMetadata,
    pub state: ContainerState,
    pub created_at: i64,
    pub started_at: i64,
    pub finished_at: i64,
    pub exit_code: i32,
    pub image: ImageSpec,
    pub image_ref: String,
    pub reason: String,
    pub message: String,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
    pub mounts: Vec<Mount>,
    pub log_path: String,
}

/// Exec sync response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecSyncResponse {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

/// Exec request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecRequest {
    pub container_id: String,
    pub cmd: Vec<String>,
    pub tty: bool,
    pub stdin: bool,
    pub stdout: bool,
    pub stderr: bool,
}

/// Exec response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResponse {
    pub url: String,
}

/// Attach request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachRequest {
    pub container_id: String,
    pub stdin: bool,
    pub tty: bool,
    pub stdout: bool,
    pub stderr: bool,
}

/// Attach response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachResponse {
    pub url: String,
}

/// Port forward request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortForwardRequest {
    pub pod_sandbox_id: String,
    pub port: Vec<i32>,
}

/// Port forward response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortForwardResponse {
    pub url: String,
}

/// Container stats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStats {
    pub attributes: ContainerAttributes,
    pub cpu: Option<CpuUsage>,
    pub memory: Option<MemoryUsage>,
    pub writable_layer: Option<FilesystemUsage>,
}

/// Container attributes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerAttributes {
    pub id: String,
    pub metadata: ContainerMetadata,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
}

/// CPU usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuUsage {
    pub timestamp: i64,
    pub usage_core_nano_seconds: Option<UInt64Value>,
    pub usage_nano_cores: Option<UInt64Value>,
}

/// UInt64 value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UInt64Value {
    pub value: u64,
}

/// Memory usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUsage {
    pub timestamp: i64,
    pub working_set_bytes: Option<UInt64Value>,
    pub available_bytes: Option<UInt64Value>,
    pub usage_bytes: Option<UInt64Value>,
    pub rss_bytes: Option<UInt64Value>,
    pub page_faults: Option<UInt64Value>,
    pub major_page_faults: Option<UInt64Value>,
}

/// Container stats filter
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContainerStatsFilter {
    pub id: Option<String>,
    pub pod_sandbox_id: Option<String>,
    pub label_selector: HashMap<String, String>,
}

/// Runtime config
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeConfig {
    pub network_config: Option<NetworkConfig>,
}

/// Network config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub pod_cidr: String,
}

/// Runtime status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub conditions: Vec<RuntimeCondition>,
}

/// Runtime condition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCondition {
    pub r#type: String,
    pub status: bool,
    pub reason: String,
    pub message: String,
}

/// Image filter
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageFilter {
    pub image: Option<ImageSpec>,
}

/// Image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Image {
    pub id: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub size: u64,
    pub uid: Option<Int64Value>,
    pub username: String,
    pub spec: Option<ImageSpec>,
}

/// Image status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageStatusResponse {
    pub image: Option<Image>,
    pub info: HashMap<String, String>,
}

/// Auth config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub username: String,
    pub password: String,
    pub auth: String,
    pub server_address: String,
    pub identity_token: String,
    pub registry_token: String,
}

/// Filesystem usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemUsage {
    pub timestamp: i64,
    pub fs_id: FilesystemIdentifier,
    pub used_bytes: Option<UInt64Value>,
    pub inodes_used: Option<UInt64Value>,
}

/// Filesystem identifier
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemIdentifier {
    pub mountpoint: String,
}

/// CRI server implementation
pub struct CriServer {
    socket_path: PathBuf,
    runtime: Option<crate::ContainerRuntime>,
    image_store: Option<crate::ImageStore>,
}

impl CriServer {
    /// Create a new CRI server
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            runtime: None,
            image_store: None,
        }
    }

    /// Create a new CRI server with runtime and image store
    pub fn with_services(
        socket_path: PathBuf,
        runtime: crate::ContainerRuntime,
        image_store: crate::ImageStore,
    ) -> Self {
        Self {
            socket_path,
            runtime: Some(runtime),
            image_store: Some(image_store),
        }
    }

    /// Get or create runtime
    async fn get_runtime(&mut self) -> Result<&mut crate::ContainerRuntime> {
        if self.runtime.is_none() {
            self.runtime = Some(crate::ContainerRuntime::new().await?);
        }
        Ok(self.runtime.as_mut().unwrap())
    }

    /// Get or create image store
    fn get_image_store(&mut self) -> Result<&mut crate::ImageStore> {
        if self.image_store.is_none() {
            self.image_store = Some(
                crate::ImageStore::new(crate::ImageStore::default_path())
                    .map_err(|e| ShimError::runtime(format!("Failed to create image store: {}", e)))?
            );
        }
        Ok(self.image_store.as_mut().unwrap())
    }

    /// Start the CRI server
    #[cfg(feature = "cri")]
    pub async fn serve(&mut self) -> Result<()> {
        log::info!("Starting CRI server on {}", self.socket_path.display());

        // Ensure services are initialized
        let _ = self.get_runtime().await?;
        let _ = self.get_image_store()?;

        // gRPC server implementation
        // Note: Full gRPC implementation requires:
        // 1. Tonic server setup with Unix socket listener
        // 2. RuntimeService and ImageService implementations
        // 3. CRI protobuf definitions from kubernetes/cri-api
        // 4. Request/response serialization/deserialization
        
        #[cfg(feature = "cri")]
        {
            use std::os::unix::net::UnixListener;
            use std::io::prelude::*;
            
            // Remove old socket if exists
            let _ = std::fs::remove_file(&self.socket_path);
            
            let listener = UnixListener::bind(&self.socket_path)
                .map_err(|e| ShimError::io_with_context(
                    e,
                    format!("Failed to bind CRI socket: {}", self.socket_path.display())
                ))?;
            
            log::info!("CRI server listening on {}", self.socket_path.display());
            
            // Accept connections and handle requests
            // In a full implementation, this would use tonic::Server
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        log::debug!("New CRI connection");
                        // Handle gRPC requests here
                        // For now, just acknowledge connection
                        let _ = stream.write_all(b"OK");
                    }
                    Err(e) => {
                        log::error!("CRI connection error: {}", e);
                    }
                }
            }
            
            Ok(())
        }
        
        #[cfg(not(feature = "cri"))]
        {
            Err(ShimError::runtime(
                "CRI feature not enabled. Enable with 'cri' feature flag.",
            ))
        }
    }

    /// Start the CRI server (fallback without gRPC)
    #[cfg(not(feature = "cri"))]
    pub async fn serve(&mut self) -> Result<()> {
        log::info!(
            "Starting CRI server on {} (gRPC not available)",
            self.socket_path.display()
        );

        Err(ShimError::runtime(
            "CRI server requires 'cri' feature flag. Install tonic/prost and enable feature.",
        ))
    }
}

/// CRI Runtime Service implementation that bridges to ContainerRuntime
pub struct RuntimeServiceImpl {
    runtime: crate::ContainerRuntime,
}

impl RuntimeServiceImpl {
    /// Create a new runtime service
    pub async fn new() -> Result<Self> {
        let runtime = crate::ContainerRuntime::new().await?;
        Ok(Self { runtime })
    }
}

#[cfg(feature = "cri")]
impl RuntimeService for RuntimeServiceImpl {
    fn version(&self, _version: &str) -> Result<VersionResponse> {
        Ok(VersionResponse {
            version: "0.1.0".to_string(),
            runtime_name: "libcrun-shim".to_string(),
            runtime_version: "0.1.0".to_string(),
            runtime_api_version: "v1alpha2".to_string(),
        })
    }

    fn run_pod_sandbox(&self, config: PodSandboxConfig) -> Result<String> {
        // Create a pod sandbox (essentially a container with special networking)
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        let container_config = crate::types::ContainerConfig {
            id: format!("pod-{}", config.metadata.uid),
            rootfs: PathBuf::from("/"), // Pod sandbox uses minimal rootfs
            command: vec!["pause".to_string()], // Pause container for pod
            env: vec![],
            working_dir: "/".to_string(),
            ..Default::default()
        };
        
        let id = rt.block_on(self.runtime.create(container_config))
            .map_err(|e| ShimError::runtime(format!("Failed to create pod sandbox: {}", e)))?;
        
        Ok(id)
    }

    fn stop_pod_sandbox(&self, pod_sandbox_id: &str) -> Result<()> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        rt.block_on(self.runtime.stop(pod_sandbox_id))
            .map_err(|e| ShimError::runtime(format!("Failed to stop pod sandbox: {}", e)))?;
        
        Ok(())
    }

    fn remove_pod_sandbox(&self, pod_sandbox_id: &str) -> Result<()> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        rt.block_on(self.runtime.delete(pod_sandbox_id))
            .map_err(|e| ShimError::runtime(format!("Failed to remove pod sandbox: {}", e)))?;
        
        Ok(())
    }

    fn pod_sandbox_status(&self, pod_sandbox_id: &str, _verbose: bool) -> Result<PodSandboxStatus> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        let containers = rt.block_on(self.runtime.list())
            .map_err(|e| ShimError::runtime(format!("Failed to list containers: {}", e)))?;
        
        let container = containers.iter()
            .find(|c| c.id == pod_sandbox_id)
            .ok_or_else(|| ShimError::not_found(format!("Pod sandbox '{}'", pod_sandbox_id)))?;
        
        let state = match container.status {
            crate::types::ContainerStatus::Running => PodSandboxState::SANDBOX_READY,
            _ => PodSandboxState::SANDBOX_NOTREADY,
        };
        
        Ok(PodSandboxStatus {
            id: container.id.clone(),
            metadata: PodSandboxMetadata {
                name: container.id.clone(),
                uid: container.id.clone(),
                namespace: "default".to_string(),
                attempt: 0,
            },
            state,
            created_at: 0,
            network: None,
            linux: None,
            labels: std::collections::HashMap::new(),
            annotations: std::collections::HashMap::new(),
            runtime_handler: "libcrun-shim".to_string(),
        })
    }

    fn list_pod_sandbox(&self, _filter: Option<PodSandboxFilter>) -> Result<Vec<PodSandbox>> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        let containers = rt.block_on(self.runtime.list())
            .map_err(|e| ShimError::runtime(format!("Failed to list containers: {}", e)))?;
        
        let sandboxes: Vec<PodSandbox> = containers.iter()
            .filter(|c| c.id.starts_with("pod-"))
            .map(|c| PodSandbox {
                id: c.id.clone(),
                metadata: PodSandboxMetadata {
                    name: c.id.clone(),
                    uid: c.id.clone(),
                    namespace: "default".to_string(),
                    attempt: 0,
                },
                state: match c.status {
                    crate::types::ContainerStatus::Running => PodSandboxState::SANDBOX_READY,
                    _ => PodSandboxState::SANDBOX_NOTREADY,
                },
                created_at: 0,
                labels: std::collections::HashMap::new(),
                annotations: std::collections::HashMap::new(),
                runtime_handler: "libcrun-shim".to_string(),
            })
            .collect();
        
        Ok(sandboxes)
    }

    fn create_container(
        &self,
        pod_sandbox_id: &str,
        config: ContainerConfig,
        _sandbox_config: PodSandboxConfig,
    ) -> Result<String> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        // Convert CRI ContainerConfig to our ContainerConfig
        let container_config = crate::types::ContainerConfig {
            id: format!("{}-{}", pod_sandbox_id, config.metadata.name),
            rootfs: PathBuf::from("/"), // Would come from image
            command: config.command.clone(),
            env: config.envs.iter().map(|kv| format!("{}={}", kv.key, kv.value)).collect(),
            working_dir: config.working_dir.clone(),
            ..Default::default()
        };
        
        let id = rt.block_on(self.runtime.create(container_config))
            .map_err(|e| ShimError::runtime(format!("Failed to create container: {}", e)))?;
        
        Ok(id)
    }

    fn start_container(&self, container_id: &str) -> Result<()> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        rt.block_on(self.runtime.start(container_id))
            .map_err(|e| ShimError::runtime(format!("Failed to start container: {}", e)))?;
        
        Ok(())
    }

    fn stop_container(&self, container_id: &str, _timeout: i64) -> Result<()> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        rt.block_on(self.runtime.stop(container_id))
            .map_err(|e| ShimError::runtime(format!("Failed to stop container: {}", e)))?;
        
        Ok(())
    }

    fn remove_container(&self, container_id: &str) -> Result<()> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        rt.block_on(self.runtime.delete(container_id))
            .map_err(|e| ShimError::runtime(format!("Failed to remove container: {}", e)))?;
        
        Ok(())
    }

    fn list_containers(&self, _filter: Option<ContainerFilter>) -> Result<Vec<Container>> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        let containers = rt.block_on(self.runtime.list())
            .map_err(|e| ShimError::runtime(format!("Failed to list containers: {}", e)))?;
        
        let cri_containers: Vec<Container> = containers.iter()
            .filter(|c| !c.id.starts_with("pod-"))
            .map(|c| Container {
                id: c.id.clone(),
                pod_sandbox_id: "unknown".to_string(), // Would track this
                metadata: ContainerMetadata {
                    name: c.id.clone(),
                    attempt: 0,
                },
                image: ImageSpec {
                    image: "unknown".to_string(),
                    annotations: std::collections::HashMap::new(),
                },
                image_ref: "unknown".to_string(),
                state: match c.status {
                    crate::types::ContainerStatus::Created => ContainerState::CONTAINER_CREATED,
                    crate::types::ContainerStatus::Running => ContainerState::CONTAINER_RUNNING,
                    crate::types::ContainerStatus::Stopped => ContainerState::CONTAINER_EXITED,
                },
                created_at: 0,
                labels: std::collections::HashMap::new(),
                annotations: std::collections::HashMap::new(),
            })
            .collect();
        
        Ok(cri_containers)
    }

    fn container_status(&self, container_id: &str, _verbose: bool) -> Result<ContainerStatusResponse> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        let containers = rt.block_on(self.runtime.list())
            .map_err(|e| ShimError::runtime(format!("Failed to list containers: {}", e)))?;
        
        let container = containers.iter()
            .find(|c| c.id == container_id)
            .ok_or_else(|| ShimError::not_found(format!("Container '{}'", container_id)))?;
        
        Ok(ContainerStatusResponse {
            status: ContainerStatusInfo {
                id: container.id.clone(),
                metadata: ContainerMetadata {
                    name: container.id.clone(),
                    attempt: 0,
                },
                state: match container.status {
                    crate::types::ContainerStatus::Created => ContainerState::CONTAINER_CREATED,
                    crate::types::ContainerStatus::Running => ContainerState::CONTAINER_RUNNING,
                    crate::types::ContainerStatus::Stopped => ContainerState::CONTAINER_EXITED,
                },
                created_at: 0,
                started_at: 0,
                finished_at: 0,
                exit_code: 0,
                image: ImageSpec {
                    image: "unknown".to_string(),
                    annotations: std::collections::HashMap::new(),
                },
                image_ref: "unknown".to_string(),
                reason: String::new(),
                message: String::new(),
                labels: std::collections::HashMap::new(),
                annotations: std::collections::HashMap::new(),
                mounts: vec![],
                log_path: String::new(),
            },
            info: std::collections::HashMap::new(),
        })
    }

    fn update_container_resources(
        &self,
        _container_id: &str,
        _resources: LinuxContainerResources,
    ) -> Result<()> {
        // Resource updates not fully implemented
        Err(ShimError::runtime("Update container resources not implemented"))
    }

    fn reopen_container_log(&self, _container_id: &str) -> Result<()> {
        // Log reopening not implemented
        Ok(()) // No-op
    }

    fn exec_sync(&self, container_id: &str, cmd: Vec<String>, _timeout: i64) -> Result<ExecSyncResponse> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        let (exit_code, stdout, stderr) = rt.block_on(self.runtime.exec(container_id, cmd))
            .map_err(|e| ShimError::runtime(format!("Failed to exec: {}", e)))?;
        
        Ok(ExecSyncResponse {
            stdout: stdout.into_bytes(),
            stderr: stderr.into_bytes(),
            exit_code,
        })
    }

    fn exec(&self, _request: ExecRequest) -> Result<ExecResponse> {
        // Streaming exec not fully implemented
        Err(ShimError::runtime("Streaming exec not implemented"))
    }

    fn attach(&self, _request: AttachRequest) -> Result<AttachResponse> {
        // Attach not fully implemented
        Err(ShimError::runtime("Attach not implemented"))
    }

    fn port_forward(&self, _request: PortForwardRequest) -> Result<PortForwardResponse> {
        // Port forward not fully implemented
        Err(ShimError::runtime("Port forward not implemented"))
    }

    fn container_stats(&self, container_id: &str) -> Result<ContainerStats> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        let metrics = rt.block_on(self.runtime.metrics(container_id))
            .map_err(|e| ShimError::runtime(format!("Failed to get metrics: {}", e)))?;
        
        Ok(ContainerStats {
            attributes: ContainerAttributes {
                id: container_id.to_string(),
                metadata: ContainerMetadata {
                    name: container_id.to_string(),
                    attempt: 0,
                },
                labels: std::collections::HashMap::new(),
                annotations: std::collections::HashMap::new(),
            },
            cpu: Some(CpuUsage {
                timestamp: 0,
                usage_core_nano_seconds: Some(UInt64Value { value: metrics.cpu.total_usage }),
                usage_nano_cores: Some(UInt64Value { value: 0 }),
            }),
            memory: Some(MemoryUsage {
                timestamp: 0,
                working_set_bytes: Some(UInt64Value { value: metrics.memory.usage }),
                available_bytes: None,
                usage_bytes: Some(UInt64Value { value: metrics.memory.usage }),
                rss_bytes: Some(UInt64Value { value: metrics.memory.usage }),
                page_faults: None,
                major_page_faults: None,
            }),
            writable_layer: None,
        })
    }

    fn list_container_stats(&self, _filter: Option<ContainerStatsFilter>) -> Result<Vec<ContainerStats>> {
        // List stats not fully implemented
        Err(ShimError::runtime("List container stats not implemented"))
    }

    fn update_runtime_config(&self, _runtime_config: RuntimeConfig) -> Result<()> {
        // Runtime config update not implemented
        Ok(()) // No-op
    }

    fn status(&self, _verbose: bool) -> Result<RuntimeStatus> {
        Ok(RuntimeStatus {
            conditions: vec![RuntimeCondition {
                r#type: "Ready".to_string(),
                status: true,
                reason: "Running".to_string(),
                message: "Runtime is ready".to_string(),
            }],
        })
    }
}

/// CRI Image Service implementation that bridges to ImageStore
pub struct ImageServiceImpl {
    image_store: crate::ImageStore,
}

impl ImageServiceImpl {
    /// Create a new image service
    pub fn new() -> Result<Self> {
        let image_store = crate::ImageStore::new(crate::ImageStore::default_path())
            .map_err(|e| ShimError::runtime(format!("Failed to create image store: {}", e)))?;
        Ok(Self { image_store })
    }
}

#[cfg(feature = "cri")]
impl ImageService for ImageServiceImpl {
    fn list_images(&self, _filter: Option<ImageFilter>) -> Result<Vec<Image>> {
        // List images from store
        let images = self.image_store.list()
            .map_err(|e| ShimError::runtime(format!("Failed to list images: {}", e)))?;
        
        let cri_images: Vec<Image> = images.iter()
            .map(|img| Image {
                id: img.id.clone(),
                repo_tags: vec![img.reference.full_name()],
                repo_digests: vec![],
                size: img.size,
                uid: None,
                username: String::new(),
                spec: Some(ImageSpec {
                    image: img.reference.full_name(),
                    annotations: std::collections::HashMap::new(),
                }),
            })
            .collect();
        
        Ok(cri_images)
    }

    fn image_status(&self, image: ImageSpec, _verbose: bool) -> Result<ImageStatusResponse> {
        // Get image status from store
        let images = self.image_store.list()
            .map_err(|e| ShimError::runtime(format!("Failed to list images: {}", e)))?;
        
        let img = images.iter()
            .find(|i| i.reference.full_name() == image.image);
        
        if let Some(img) = img {
            Ok(ImageStatusResponse {
                image: Some(Image {
                    id: img.id.clone(),
                    repo_tags: vec![img.reference.full_name()],
                    repo_digests: vec![],
                    size: img.size,
                    uid: None,
                    username: String::new(),
                    spec: Some(image),
                }),
                info: std::collections::HashMap::new(),
            })
        } else {
            Ok(ImageStatusResponse {
                image: None,
                info: std::collections::HashMap::new(),
            })
        }
    }

    fn pull_image(
        &self,
        image: ImageSpec,
        _auth: Option<AuthConfig>,
        _sandbox_config: Option<PodSandboxConfig>,
    ) -> Result<String> {
        // Pull image using store
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ShimError::runtime(format!("Failed to create runtime: {}", e)))?;
        
        let info = rt.block_on(self.image_store.pull(&image.image, None))
            .map_err(|e| ShimError::runtime(format!("Failed to pull image: {}", e)))?;
        
        Ok(info.id)
    }

    fn remove_image(&self, image: ImageSpec) -> Result<()> {
        // Remove image from store
        self.image_store.remove(&image.image)
            .map_err(|e| ShimError::runtime(format!("Failed to remove image: {}", e)))?;
        
        Ok(())
    }

    fn image_fs_info(&self) -> Result<Vec<FilesystemUsage>> {
        // Get filesystem info for images
        Ok(vec![FilesystemUsage {
            timestamp: 0,
            fs_id: FilesystemIdentifier {
                mountpoint: crate::ImageStore::default_path().display().to_string(),
            },
            used_bytes: None,
            inodes_used: None,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_response() {
        let version = VersionResponse {
            version: "0.1.0".to_string(),
            runtime_name: "libcrun-shim".to_string(),
            runtime_version: "0.1.0".to_string(),
            runtime_api_version: "v1".to_string(),
        };

        assert_eq!(version.runtime_name, "libcrun-shim");
    }
}

