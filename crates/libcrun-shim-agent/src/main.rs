use libcrun_shim_proto::*;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};

fn main() {
    // Listen on a Unix socket for RPC requests
    let listener = UnixListener::bind("/tmp/libcrun-shim.sock").unwrap();
    
    println!("Agent listening on /tmp/libcrun-shim.sock");
    
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                std::thread::spawn(|| handle_client(stream));
            }
            Err(e) => {
                eprintln!("Connection error: {}", e);
            }
        }
    }
}

fn handle_client(mut stream: UnixStream) {
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
                
                let response = handle_request(request);
                let _ = stream.write_all(&serialize_response(&response));
            }
            Err(e) => {
                eprintln!("Read error: {}", e);
                break;
            }
        }
    }
}

fn handle_request(request: Request) -> Response {
    match request {
        Request::Create(req) => {
            // TODO: Actually call libcrun here
            println!("Creating container: {}", req.id);
            Response::Created(req.id)
        }
        Request::Start(id) => {
            println!("Starting container: {}", id);
            Response::Started
        }
        Request::Stop(id) => {
            println!("Stopping container: {}", id);
            Response::Stopped
        }
        Request::Delete(id) => {
            println!("Deleting container: {}", id);
            Response::Deleted
        }
        Request::List => {
            println!("Listing containers");
            Response::List(vec![])
        }
    }
}

