//! Serverless Platform Example
//!
//! This example demonstrates building a serverless/FaaS platform using libcrun-shim:
//! - Function registration and management
//! - Container pool with warm starts (cold start optimization)
//! - HTTP API for function invocation
//! - Automatic scaling and cleanup
//! - Request routing and load balancing
//! - Metrics and observability
//!
//! Run with: cargo run --example serverless_platform
//!
//! For stub mode (no real containers, works everywhere):
//!   STUB_MODE=1 cargo run --example serverless_platform
//!
//! Test with:
//!   curl -X POST http://localhost:3000/functions -H "Content-Type: application/json" \
//!     -d '{"name": "hello", "runtime": "shell", "handler": "echo", "code": "echo Hello $INPUT"}'
//!
//!   curl -X POST http://localhost:3000/invoke/hello -H "Content-Type: application/json" \
//!     -d '{"input": "World"}'
//!
//! Note: On macOS, this requires either:
//!   1. The libcrun-shim-agent running in a Linux VM, OR
//!   2. Running with STUB_MODE=1 for testing/development

use libcrun_shim::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::time::timeout;

/// Check if running in stub mode (for testing without real containers)
/// On macOS, stub mode is enabled by default since containers require a Linux VM
fn is_stub_mode() -> bool {
    // Check explicit env var first
    if let Ok(v) = std::env::var("STUB_MODE") {
        return v == "1" || v.to_lowercase() == "true";
    }
    
    // On macOS, default to stub mode (containers require Linux VM)
    #[cfg(target_os = "macos")]
    {
        // Allow override with REAL_MODE=1 if user has VM running
        if std::env::var("REAL_MODE").map(|v| v == "1").unwrap_or(false) {
            return false;
        }
        return true;
    }
    
    #[cfg(not(target_os = "macos"))]
    false
}

// ============================================================================
// Data Structures
// ============================================================================

/// Function definition
#[derive(Debug, Clone)]
struct Function {
    name: String,
    runtime: FunctionRuntime,
    handler: String,
    code: String,
    timeout_ms: u64,
    memory_mb: u64,
    env: HashMap<String, String>,
}

/// Supported runtimes
#[derive(Debug, Clone)]
enum FunctionRuntime {
    Shell,      // Simple shell script
    Python,     // Python 3
    Node,       // Node.js
    Custom(String), // Custom command
}

impl FunctionRuntime {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "shell" | "sh" | "bash" => FunctionRuntime::Shell,
            "python" | "python3" | "py" => FunctionRuntime::Python,
            "node" | "nodejs" | "js" => FunctionRuntime::Node,
            other => FunctionRuntime::Custom(other.to_string()),
        }
    }

    fn command(&self, handler: &str, code: &str) -> Vec<String> {
        match self {
            FunctionRuntime::Shell => {
                vec!["sh".to_string(), "-c".to_string(), code.to_string()]
            }
            FunctionRuntime::Python => {
                vec![
                    "python3".to_string(),
                    "-c".to_string(),
                    format!(
                        r#"
import json, os, sys
input_data = os.environ.get('INPUT', '{{}}')
{}
"#,
                        code
                    ),
                ]
            }
            FunctionRuntime::Node => {
                vec![
                    "node".to_string(),
                    "-e".to_string(),
                    format!(
                        r#"
const input = JSON.parse(process.env.INPUT || '{{}}');
{}
"#,
                        code
                    ),
                ]
            }
            FunctionRuntime::Custom(cmd) => {
                vec![cmd.clone(), handler.to_string()]
            }
        }
    }
}

/// Warm container in the pool
#[allow(dead_code)]
struct WarmContainer {
    container_id: String,
    function_name: String,
    created_at: Instant,
    last_used: Instant,
    invocations: u64,
}

/// Invocation result
#[derive(Debug)]
struct InvocationResult {
    request_id: String,
    success: bool,
    output: String,
    error: Option<String>,
    duration_ms: u64,
    cold_start: bool,
}

/// Platform metrics
struct PlatformMetrics {
    total_invocations: AtomicU64,
    successful_invocations: AtomicU64,
    failed_invocations: AtomicU64,
    cold_starts: AtomicU64,
    warm_starts: AtomicU64,
    total_duration_ms: AtomicU64,
}

impl Default for PlatformMetrics {
    fn default() -> Self {
        Self {
            total_invocations: AtomicU64::new(0),
            successful_invocations: AtomicU64::new(0),
            failed_invocations: AtomicU64::new(0),
            cold_starts: AtomicU64::new(0),
            warm_starts: AtomicU64::new(0),
            total_duration_ms: AtomicU64::new(0),
        }
    }
}

// ============================================================================
// Serverless Platform
// ============================================================================

struct ServerlessPlatform {
    runtime: Option<ContainerRuntime>,
    functions: RwLock<HashMap<String, Function>>,
    container_pool: RwLock<HashMap<String, Vec<WarmContainer>>>,
    metrics: PlatformMetrics,
    config: PlatformConfig,
    stub_mode: bool,
}

#[allow(dead_code)]
struct PlatformConfig {
    max_containers_per_function: usize,
    container_idle_timeout: Duration,
    default_timeout_ms: u64,
    default_memory_mb: u64,
    pool_cleanup_interval: Duration,
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            max_containers_per_function: 5,
            container_idle_timeout: Duration::from_secs(300), // 5 minutes
            default_timeout_ms: 30000, // 30 seconds
            default_memory_mb: 128,
            pool_cleanup_interval: Duration::from_secs(60),
        }
    }
}

impl ServerlessPlatform {
    async fn new() -> Result<Self> {
        let stub_mode = is_stub_mode();
        
        let runtime = if stub_mode {
            #[cfg(target_os = "macos")]
            println!("🍎 Running on macOS in STUB MODE (simulated containers)");
            #[cfg(not(target_os = "macos"))]
            println!("📦 Running in STUB MODE (simulated containers)");
            println!("   To use real containers: REAL_MODE=1 (requires Linux VM on macOS)\n");
            None
        } else {
            match ContainerRuntime::new().await {
                Ok(rt) => {
                    println!("🐧 Container runtime initialized successfully\n");
                    Some(rt)
                },
                Err(e) => {
                    println!("⚠️  Container runtime unavailable: {}", e);
                    println!("   Using STUB MODE instead\n");
                    None
                }
            }
        };

        let stub_mode = runtime.is_none();
        
        Ok(Self {
            runtime,
            functions: RwLock::new(HashMap::new()),
            container_pool: RwLock::new(HashMap::new()),
            metrics: PlatformMetrics::default(),
            config: PlatformConfig::default(),
            stub_mode,
        })
    }

    /// Register a new function
    async fn register_function(&self, func: Function) -> Result<()> {
        println!("📝 Registering function: {}", func.name);
        let mut functions = self.functions.write().await;
        functions.insert(func.name.clone(), func);
        Ok(())
    }

    /// List all registered functions
    async fn list_functions(&self) -> Vec<String> {
        let functions = self.functions.read().await;
        functions.keys().cloned().collect()
    }

    /// Delete a function and its containers
    async fn delete_function(&self, name: &str) -> Result<()> {
        println!("🗑️  Deleting function: {}", name);

        // Remove from registry
        {
            let mut functions = self.functions.write().await;
            functions.remove(name);
        }

        // Cleanup containers
        self.cleanup_function_containers(name).await?;

        Ok(())
    }

    /// Invoke a function
    async fn invoke(&self, function_name: &str, input: &str) -> Result<InvocationResult> {
        let request_id = format!("req-{}", uuid_v4());
        let start = Instant::now();

        self.metrics.total_invocations.fetch_add(1, Ordering::Relaxed);

        // Get function definition
        let func = {
            let functions = self.functions.read().await;
            functions.get(function_name).cloned()
        };

        let func = match func {
            Some(f) => f,
            None => {
                self.metrics.failed_invocations.fetch_add(1, Ordering::Relaxed);
                return Ok(InvocationResult {
                    request_id,
                    success: false,
                    output: String::new(),
                    error: Some(format!("Function '{}' not found", function_name)),
                    duration_ms: start.elapsed().as_millis() as u64,
                    cold_start: false,
                });
            }
        };

        // Stub mode: simulate execution without real containers
        if self.stub_mode {
            return self.invoke_stub(&request_id, &func, input, start).await;
        }

        // Real mode: use actual containers
        self.invoke_real(&request_id, function_name, &func, input, start).await
    }

    /// Stub mode invocation (simulated, no real containers)
    async fn invoke_stub(
        &self,
        request_id: &str,
        func: &Function,
        input: &str,
        start: Instant,
    ) -> Result<InvocationResult> {
        // Simulate cold/warm start randomly
        static INVOCATION_COUNT: AtomicU64 = AtomicU64::new(0);
        let count = INVOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
        let cold_start = count % 3 == 0; // Every 3rd invocation is a "cold start"

        if cold_start {
            self.metrics.cold_starts.fetch_add(1, Ordering::Relaxed);
            println!("❄️  [STUB] Cold start for {}", func.name);
            // Simulate cold start latency
            tokio::time::sleep(Duration::from_millis(100)).await;
        } else {
            self.metrics.warm_starts.fetch_add(1, Ordering::Relaxed);
            println!("♨️  [STUB] Warm start for {}", func.name);
        }

        // Simulate execution time
        tokio::time::sleep(Duration::from_millis(10 + (count % 50))).await;

        let duration_ms = start.elapsed().as_millis() as u64;
        self.metrics.total_duration_ms.fetch_add(duration_ms, Ordering::Relaxed);
        self.metrics.successful_invocations.fetch_add(1, Ordering::Relaxed);

        // Generate simulated output based on function
        let output = match func.runtime {
            FunctionRuntime::Shell => {
                format!(r#"{{"result": "Hello, {}!", "function": "{}", "stub": true}}"#, input, func.name)
            }
            _ => {
                format!(r#"{{"result": "Executed {}", "input": "{}", "stub": true}}"#, func.name, input)
            }
        };

        Ok(InvocationResult {
            request_id: request_id.to_string(),
            success: true,
            output,
            error: None,
            duration_ms,
            cold_start,
        })
    }

    /// Real mode invocation (uses actual containers)
    async fn invoke_real(
        &self,
        request_id: &str,
        function_name: &str,
        func: &Function,
        input: &str,
        start: Instant,
    ) -> Result<InvocationResult> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            ShimError::runtime("Container runtime not available")
        })?;

        // Try to get a warm container first
        let (container_id, cold_start) = match self.get_warm_container(function_name).await {
            Some(id) => {
                self.metrics.warm_starts.fetch_add(1, Ordering::Relaxed);
                println!("♨️  Warm start for {}: {}", function_name, id);
                (id, false)
            }
            None => {
                self.metrics.cold_starts.fetch_add(1, Ordering::Relaxed);
                println!("❄️  Cold start for {}", function_name);
                let id = self.create_function_container(func, input).await?;
                (id, true)
            }
        };

        // Execute the function
        let timeout_duration = Duration::from_millis(func.timeout_ms);
        let result = timeout(
            timeout_duration,
            self.execute_in_container(&container_id, func, input),
        )
        .await;

        let duration_ms = start.elapsed().as_millis() as u64;
        self.metrics.total_duration_ms.fetch_add(duration_ms, Ordering::Relaxed);

        // Return container to pool or cleanup
        match &result {
            Ok(Ok(_)) => {
                self.return_to_pool(function_name, &container_id).await;
                self.metrics.successful_invocations.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                // Cleanup on error
                let _ = runtime.force_delete(&container_id).await;
                self.metrics.failed_invocations.fetch_add(1, Ordering::Relaxed);
            }
        }

        match result {
            Ok(Ok(output)) => Ok(InvocationResult {
                request_id: request_id.to_string(),
                success: true,
                output,
                error: None,
                duration_ms,
                cold_start,
            }),
            Ok(Err(e)) => Ok(InvocationResult {
                request_id: request_id.to_string(),
                success: false,
                output: String::new(),
                error: Some(format!("Execution error: {}", e)),
                duration_ms,
                cold_start,
            }),
            Err(_) => Ok(InvocationResult {
                request_id: request_id.to_string(),
                success: false,
                output: String::new(),
                error: Some("Function timeout".to_string()),
                duration_ms,
                cold_start,
            }),
        }
    }

    /// Get a warm container from the pool
    async fn get_warm_container(&self, function_name: &str) -> Option<String> {
        let mut pool = self.container_pool.write().await;
        if let Some(containers) = pool.get_mut(function_name) {
            if let Some(mut container) = containers.pop() {
                container.last_used = Instant::now();
                container.invocations += 1;
                return Some(container.container_id);
            }
        }
        None
    }

    /// Return a container to the pool
    async fn return_to_pool(&self, function_name: &str, container_id: &str) {
        let mut pool = self.container_pool.write().await;
        let containers = pool.entry(function_name.to_string()).or_insert_with(Vec::new);

        if containers.len() < self.config.max_containers_per_function {
            containers.push(WarmContainer {
                container_id: container_id.to_string(),
                function_name: function_name.to_string(),
                created_at: Instant::now(),
                last_used: Instant::now(),
                invocations: 1,
            });
            println!("♻️  Returned container to pool: {} (pool size: {})", container_id, containers.len());
        } else {
            // Pool full, delete container
            if let Some(runtime) = &self.runtime {
                let _ = runtime.force_delete(container_id).await;
            }
            println!("🗑️  Pool full, deleted container: {}", container_id);
        }
    }

    /// Create a new container for a function
    async fn create_function_container(&self, func: &Function, input: &str) -> Result<String> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            ShimError::runtime("Container runtime not available")
        })?;

        let container_id = format!("fn-{}-{}", func.name, &uuid_v4()[..8]);

        // Build environment variables
        let mut env: Vec<String> = func
            .env
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        env.push(format!("INPUT={}", input));
        env.push(format!("FUNCTION_NAME={}", func.name));

        let command = func.runtime.command(&func.handler, &func.code);

        let config = ContainerConfig {
            id: container_id.clone(),
            rootfs: std::env::temp_dir().join("serverless-rootfs"),
            command,
            env,
            working_dir: "/".to_string(),
            resources: ResourceLimits {
                memory: Some(func.memory_mb * 1024 * 1024),
                cpu: Some(1.0),
                ..Default::default()
            },
            ..Default::default()
        };

        // Ensure rootfs exists (in real impl, this would be the function's image)
        let _ = std::fs::create_dir_all(&config.rootfs);

        runtime.create(config).await?;
        Ok(container_id)
    }

    /// Execute code in a container
    async fn execute_in_container(
        &self,
        container_id: &str,
        func: &Function,
        _input: &str,
    ) -> Result<String> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            ShimError::runtime("Container runtime not available")
        })?;

        // Start the container
        runtime.start(container_id).await?;

        // In a real implementation, we'd capture stdout/stderr
        // For this example, we'll simulate execution
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Stop the container
        runtime.stop(container_id).await?;

        // Return simulated output
        Ok(format!(
            "{{\"result\": \"Function '{}' executed successfully\"}}",
            func.name
        ))
    }

    /// Cleanup containers for a specific function
    async fn cleanup_function_containers(&self, function_name: &str) -> Result<()> {
        let containers_to_delete: Vec<String> = {
            let mut pool = self.container_pool.write().await;
            pool.remove(function_name)
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.container_id)
                .collect()
        };

        if let Some(runtime) = &self.runtime {
            for container_id in containers_to_delete {
                let _ = runtime.force_delete(&container_id).await;
            }
        }

        Ok(())
    }

    /// Cleanup idle containers
    async fn cleanup_idle_containers(&self) {
        let now = Instant::now();
        let idle_timeout = self.config.container_idle_timeout;

        let mut pool = self.container_pool.write().await;
        let mut total_cleaned = 0;

        for (_, containers) in pool.iter_mut() {
            let before = containers.len();
            containers.retain(|c| {
                let idle = now.duration_since(c.last_used) < idle_timeout;
                if !idle {
                    // Delete the container (fire and forget)
                    let id = c.container_id.clone();
                    let _ = std::thread::spawn(move || {
                        // In real code, we'd properly clean up
                        println!("🧹 Cleaned up idle container: {}", id);
                    });
                }
                idle
            });
            total_cleaned += before - containers.len();
        }

        if total_cleaned > 0 {
            println!("🧹 Cleaned up {} idle containers", total_cleaned);
        }
    }

    /// Get platform metrics
    fn get_metrics(&self) -> String {
        format!(
            r#"{{
  "total_invocations": {},
  "successful": {},
  "failed": {},
  "cold_starts": {},
  "warm_starts": {},
  "avg_duration_ms": {:.2}
}}"#,
            self.metrics.total_invocations.load(Ordering::Relaxed),
            self.metrics.successful_invocations.load(Ordering::Relaxed),
            self.metrics.failed_invocations.load(Ordering::Relaxed),
            self.metrics.cold_starts.load(Ordering::Relaxed),
            self.metrics.warm_starts.load(Ordering::Relaxed),
            {
                let total = self.metrics.total_invocations.load(Ordering::Relaxed);
                let duration = self.metrics.total_duration_ms.load(Ordering::Relaxed);
                if total > 0 {
                    duration as f64 / total as f64
                } else {
                    0.0
                }
            }
        )
    }
}

// ============================================================================
// HTTP Server
// ============================================================================

async fn handle_request(
    platform: Arc<ServerlessPlatform>,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, String) {
    match (method, path) {
        // Health check
        ("GET", "/health") => (200, r#"{"status": "ok"}"#.to_string()),

        // Metrics
        ("GET", "/metrics") => (200, platform.get_metrics()),

        // List functions
        ("GET", "/functions") => {
            let functions = platform.list_functions().await;
            let json = format!(r#"{{"functions": {:?}}}"#, functions);
            (200, json)
        }

        // Register function
        ("POST", "/functions") => {
            // Parse JSON body (simplified parsing)
            let name = extract_json_field(body, "name").unwrap_or("unnamed".to_string());
            let runtime = extract_json_field(body, "runtime").unwrap_or("shell".to_string());
            let handler = extract_json_field(body, "handler").unwrap_or("main".to_string());
            let code = extract_json_field(body, "code").unwrap_or_default();

            let func = Function {
                name: name.clone(),
                runtime: FunctionRuntime::from_str(&runtime),
                handler,
                code,
                timeout_ms: 30000,
                memory_mb: 128,
                env: HashMap::new(),
            };

            match platform.register_function(func).await {
                Ok(_) => (201, format!(r#"{{"message": "Function '{}' registered"}}"#, name)),
                Err(e) => (500, format!(r#"{{"error": "{}"}}"#, e)),
            }
        }

        // Invoke function
        _ if method == "POST" && path.starts_with("/invoke/") => {
            let function_name = &path[8..]; // Strip "/invoke/"
            let input = extract_json_field(body, "input").unwrap_or_default();

            match platform.invoke(function_name, &input).await {
                Ok(result) => {
                    let json = format!(
                        r#"{{
  "request_id": "{}",
  "success": {},
  "output": {},
  "error": {},
  "duration_ms": {},
  "cold_start": {}
}}"#,
                        result.request_id,
                        result.success,
                        serde_json_value(&result.output),
                        result.error.as_ref().map(|e| format!("\"{}\"", e)).unwrap_or("null".to_string()),
                        result.duration_ms,
                        result.cold_start
                    );
                    if result.success {
                        (200, json)
                    } else {
                        (500, json)
                    }
                }
                Err(e) => (500, format!(r#"{{"error": "{}"}}"#, e)),
            }
        }

        // Delete function
        _ if method == "DELETE" && path.starts_with("/functions/") => {
            let function_name = &path[11..]; // Strip "/functions/"
            match platform.delete_function(function_name).await {
                Ok(_) => (200, format!(r#"{{"message": "Function '{}' deleted"}}"#, function_name)),
                Err(e) => (500, format!(r#"{{"error": "{}"}}"#, e)),
            }
        }

        // Not found
        _ => (404, r#"{"error": "Not found"}"#.to_string()),
    }
}

async fn run_http_server(platform: Arc<ServerlessPlatform>, port: u16) -> Result<()> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await
        .map_err(|e| ShimError::runtime(format!("Failed to bind: {}", e)))?;

    println!("🚀 Serverless platform running on http://0.0.0.0:{}", port);
    println!("\nAPI Endpoints:");
    println!("  GET  /health              - Health check");
    println!("  GET  /metrics             - Platform metrics");
    println!("  GET  /functions           - List functions");
    println!("  POST /functions           - Register function");
    println!("  POST /invoke/:name        - Invoke function");
    println!("  DELETE /functions/:name   - Delete function");
    println!();

    loop {
        let (mut socket, addr) = listener.accept().await
            .map_err(|e| ShimError::runtime(format!("Accept failed: {}", e)))?;

        let platform = Arc::clone(&platform);

        tokio::spawn(async move {
            let (reader, mut writer) = socket.split();
            let mut reader = BufReader::new(reader);
            let mut request_line = String::new();

            // Read request line
            if reader.read_line(&mut request_line).await.is_err() {
                return;
            }

            let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
            if parts.len() < 2 {
                return;
            }

            let method = parts[0];
            let path = parts[1];

            // Read headers
            let mut content_length = 0;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).await.is_err() {
                    return;
                }
                if header.trim().is_empty() {
                    break;
                }
                if header.to_lowercase().starts_with("content-length:") {
                    content_length = header[15..].trim().parse().unwrap_or(0);
                }
            }

            // Read body
            let mut body = vec![0u8; content_length];
            if content_length > 0 {
                if tokio::io::AsyncReadExt::read_exact(&mut reader, &mut body).await.is_err() {
                    return;
                }
            }
            let body = String::from_utf8_lossy(&body).to_string();

            // Handle request
            let (status, response_body) = handle_request(platform, method, path, &body).await;

            // Send response
            let response = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status,
                status_text(status),
                response_body.len(),
                response_body
            );

            let _ = writer.write_all(response.as_bytes()).await;

            println!("{} {} {} - {} bytes", addr, method, path, response_body.len());
        });
    }
}

// ============================================================================
// Utilities
// ============================================================================

fn uuid_v4() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:032x}", now)
}

fn extract_json_field(json: &str, field: &str) -> Option<String> {
    let pattern = format!(r#""{}":"#, field);
    if let Some(start) = json.find(&pattern) {
        let rest = &json[start + pattern.len()..];
        let rest = rest.trim_start();
        
        if rest.starts_with('"') {
            // String value
            let rest = &rest[1..];
            if let Some(end) = rest.find('"') {
                return Some(rest[..end].to_string());
            }
        } else {
            // Non-string value
            let end = rest.find(|c| c == ',' || c == '}').unwrap_or(rest.len());
            return Some(rest[..end].trim().to_string());
        }
    }
    None
}

fn serde_json_value(s: &str) -> String {
    if s.starts_with('{') || s.starts_with('[') {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║          🚀 Serverless Platform (libcrun-shim)            ║");
    println!("╚═══════════════════════════════════════════════════════════╝");
    println!();

    // Initialize platform
    let platform = Arc::new(ServerlessPlatform::new().await?);
    
    if platform.stub_mode {
        println!("📋 Mode: STUB (simulated execution - great for demos!)\n");
    } else {
        println!("📋 Mode: REAL (actual container execution)\n");
    }

    // Register some example functions
    platform
        .register_function(Function {
            name: "hello".to_string(),
            runtime: FunctionRuntime::Shell,
            handler: "main".to_string(),
            code: r#"echo "Hello, $INPUT!""#.to_string(),
            timeout_ms: 5000,
            memory_mb: 64,
            env: HashMap::new(),
        })
        .await?;

    platform
        .register_function(Function {
            name: "add".to_string(),
            runtime: FunctionRuntime::Shell,
            handler: "main".to_string(),
            code: r#"echo "Result: $((1 + 2))""#.to_string(),
            timeout_ms: 5000,
            memory_mb: 64,
            env: HashMap::new(),
        })
        .await?;

    platform
        .register_function(Function {
            name: "timestamp".to_string(),
            runtime: FunctionRuntime::Shell,
            handler: "main".to_string(),
            code: r#"date +%s"#.to_string(),
            timeout_ms: 5000,
            memory_mb: 64,
            env: HashMap::new(),
        })
        .await?;

    println!("📝 Registered {} example functions\n", 3);

    // Start cleanup task
    let platform_for_cleanup = Arc::clone(&platform);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            platform_for_cleanup.cleanup_idle_containers().await;
        }
    });

    // Run HTTP server
    run_http_server(platform, 3000).await?;

    Ok(())
}

