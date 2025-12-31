use std::env;
use std::path::PathBuf;
use std::fs;

fn main() {
    // Tell cargo to invalidate the built crate whenever the wrapper changes
    println!("cargo:rerun-if-changed=wrapper.h");
    
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bindings_file = out_path.join("bindings.rs");
    
    // Try to find libcrun via pkg-config
    let libcrun_available = pkg_config::Config::new().probe("libcrun").is_ok();
    
    if libcrun_available {
        println!("cargo:rustc-link-lib=crun");
        
        // Generate real bindings
        let bindings = bindgen::Builder::default()
            .header("wrapper.h")
            .allowlist_function("crun_container_.*")
            .allowlist_type("crun_container.*")
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .generate()
            .expect("Unable to generate bindings");
        
        bindings
            .write_to_file(&bindings_file)
            .expect("Couldn't write bindings!");
    } else {
        // If libcrun is not available, create stub bindings
        println!("cargo:warning=libcrun not found via pkg-config, using stub bindings");
        
        let stub_bindings = r#"
// Stub bindings for libcrun when not available
use std::os::raw::{c_int, c_char};

#[repr(C)]
pub struct crun_container_s {
    _unused: [u8; 0],
}

pub type crun_container_t = crun_container_s;

#[repr(C)]
pub struct crun_runtime_s {
    _unused: [u8; 0],
}

pub type crun_runtime_t = crun_runtime_s;

#[no_mangle]
pub extern "C" fn crun_container_create(_container: *mut crun_container_t, _id: *const c_char) -> c_int {
    -1 // Not implemented
}

#[no_mangle]
pub extern "C" fn crun_container_start(_container: *mut crun_container_t, _id: *const c_char) -> c_int {
    -1 // Not implemented
}

#[no_mangle]
pub extern "C" fn crun_container_kill(_container: *mut crun_container_t, _id: *const c_char, _signal: c_int) -> c_int {
    -1 // Not implemented
}

#[no_mangle]
pub extern "C" fn crun_container_delete(_container: *mut crun_container_t, _id: *const c_char) -> c_int {
    -1 // Not implemented
}

#[no_mangle]
pub extern "C" fn crun_container_list(_containers: *mut *mut crun_container_t, _count: *mut usize) -> c_int {
    -1 // Not implemented
}
"#;
        
        fs::write(&bindings_file, stub_bindings)
            .expect("Couldn't write stub bindings!");
    }
}

