use libcrun_shim_proto::*;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, RwLock};

#[cfg(target_os = "linux")]
use libcrun_sys::safe as crun;

// Container state in the agent
#[derive(Clone)]
struct ContainerState {
    id: String,
    rootfs: String,
    command: Vec<String>,
    env: Vec<String>,
    working_dir: String,
    status: String,
    pid: Option<u32>,
}

// Shared state for the agent
struct AgentState {
    containers: RwLock<HashMap<String, ContainerState>>,
}

impl AgentState {
    fn new() -> Self {
        Self {
            containers: RwLock::new(HashMap::new()),
        }
    }
}

fn main() {
    // Remove old socket if it exists
    let _ = std::fs::remove_file("/tmp/libcrun-shim.sock");
    
    // Listen on a Unix socket for RPC requests
    let listener = UnixListener::bind("/tmp/libcrun-shim.sock")
        .expect("Failed to bind to socket");
    
    println!("Agent listening on /tmp/libcrun-shim.sock");
    
    // Create shared state
    let state = Arc::new(AgentState::new());
    
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state_clone = Arc::clone(&state);
                std::thread::spawn(move || handle_client(stream, state_clone));
            }
            Err(e) => {
                eprintln!("Connection error: {}", e);
            }
        }
    }
}

fn handle_client(mut stream: UnixStream, state: Arc<AgentState>) {
    let mut buffer = vec![0u8; 4096];
    
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break, // Connection closed
            Ok(n) => {
                let request = match deserialize_request(&buffer[..n]) {
                    Ok(req) => req,
                    Err(e) => {
                        let response = Response::Error(format!("Parse error: {}", e));
                        let _ = stream.write_all(&serialize_response(&response));
                        continue;
                    }
                };
                
                let response = handle_request(request, &state);
                if let Err(e) = stream.write_all(&serialize_response(&response)) {
                    eprintln!("Write error: {}", e);
                    break;
                }
            }
            Err(e) => {
                eprintln!("Read error: {}", e);
                break;
            }
        }
    }
}

fn handle_request(request: Request, state: &AgentState) -> Response {
    match request {
        Request::Create(req) => {
            // Validate request
            if req.id.is_empty() {
                return Response::Error("Container ID cannot be empty".to_string());
            }
            if req.command.is_empty() {
                return Response::Error("Command cannot be empty".to_string());
            }
            
            // Check if container already exists
            {
                let containers = state.containers.read().unwrap();
                if containers.contains_key(&req.id) {
                    return Response::Error(format!("Container '{}' already exists", req.id));
                }
            }
            
            // TODO: Actually call libcrun here
            // In a real implementation:
            //   let container = crun::container_create(...)?;
            //   crun::container_create(&container, &req.id)?;
            
            println!("Creating container: {}", req.id);
            
            let container_state = ContainerState {
                id: req.id.clone(),
                rootfs: req.rootfs,
                command: req.command,
                env: req.env,
                working_dir: req.working_dir,
                status: "Created".to_string(),
                pid: None,
            };
            
            state.containers.write().unwrap().insert(req.id.clone(), container_state);
            Response::Created(req.id)
        }
        Request::Start(id) => {
            let mut containers = state.containers.write().unwrap();
            let container = containers.get_mut(&id);
            
            match container {
                None => Response::Error(format!("Container '{}' not found", id)),
                Some(c) => {
                    if c.status == "Running" {
                        Response::Error(format!("Container '{}' is already running", id))
                    } else if c.status == "Stopped" {
                        Response::Error(format!("Container '{}' is stopped and cannot be restarted", id))
                    } else {
                        // TODO: Actually call libcrun here
                        // In a real implementation:
                        //   let container = get_container(&id)?;
                        //   crun::container_start(&container, &id)?;
                        
                        println!("Starting container: {}", id);
                        c.status = "Running".to_string();
                        c.pid = Some(std::process::id()); // Placeholder
                        Response::Started
                    }
                }
            }
        }
        Request::Stop(id) => {
            let mut containers = state.containers.write().unwrap();
            let container = containers.get_mut(&id);
            
            match container {
                None => Response::Error(format!("Container '{}' not found", id)),
                Some(c) => {
                    if c.status != "Running" {
                        Response::Error(format!("Container '{}' is not running", id))
                    } else {
                        // TODO: Actually call libcrun here
                        // In a real implementation:
                        //   let container = get_container(&id)?;
                        //   crun::container_kill(&container, &id, libc::SIGTERM)?;
                        
                        println!("Stopping container: {}", id);
                        c.status = "Stopped".to_string();
                        c.pid = None;
                        Response::Stopped
                    }
                }
            }
        }
        Request::Delete(id) => {
            let mut containers = state.containers.write().unwrap();
            let container = containers.get(&id);
            
            match container {
                None => Response::Error(format!("Container '{}' not found", id)),
                Some(c) => {
                    if c.status == "Running" {
                        Response::Error(format!("Cannot delete running container '{}'. Stop it first.", id))
                    } else {
                        // TODO: Actually call libcrun here
                        // In a real implementation:
                        //   let container = get_container(&id)?;
                        //   crun::container_delete(&container, &id)?;
                        
                        println!("Deleting container: {}", id);
                        containers.remove(&id);
                        Response::Deleted
                    }
                }
            }
        }
        Request::List => {
            let containers = state.containers.read().unwrap();
            let list: Vec<ContainerInfoProto> = containers
                .values()
                .map(|c| ContainerInfoProto {
                    id: c.id.clone(),
                    status: c.status.clone(),
                    pid: c.pid,
                })
                .collect();
            
            Response::List(list)
        }
    }
}

