use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Tell cargo to invalidate the built crate whenever the wrapper changes
    println!("cargo:rerun-if-changed=wrapper.h");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bindings_file = out_path.join("bindings.rs");

    // Try to find libcrun via pkg-config
    let libcrun_available = pkg_config::Config::new().probe("libcrun").is_ok();

    if libcrun_available {
        println!("cargo:rustc-link-lib=crun");
        println!("cargo:warning=libcrun found! Using real FFI bindings.");

        // Generate real bindings from libcrun headers
        let bindings = bindgen::Builder::default()
            .header("wrapper.h")
            // Allowlist libcrun functions
            .allowlist_function("libcrun_.*")
            .allowlist_type("libcrun_.*")
            // Allowlist container and context types
            .allowlist_type(".*container.*")
            .allowlist_type(".*context.*")
            .allowlist_type(".*error.*")
            // Set C standard
            .clang_arg("-std=c11")
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .generate()
            .expect("Unable to generate bindings from libcrun headers");

        bindings
            .write_to_file(&bindings_file)
            .expect("Couldn't write bindings!");
    } else {
        // If libcrun is not available, create stub bindings
        println!("cargo:warning=libcrun not found via pkg-config, using stub bindings");
        println!("cargo:warning=To enable real libcrun support, install libcrun-dev (Ubuntu/Debian) or crun-devel (Fedora)");

        let stub_bindings = r#"
// Stub bindings for libcrun when not available
// Install libcrun-dev (Ubuntu/Debian) or crun-devel (Fedora) to enable real bindings
use std::os::raw::{c_int, c_char};

#[repr(C)]
pub struct libcrun_container_s {
    _unused: [u8; 0],
}

pub type libcrun_container_t = libcrun_container_s;

#[repr(C)]
pub struct libcrun_context_s {
    _unused: [u8; 0],
}

pub type libcrun_context_t = libcrun_context_s;

#[repr(C)]
pub struct libcrun_error_s {
    _unused: [u8; 0],
}

pub type libcrun_error_t = libcrun_error_s;

#[no_mangle]
pub extern "C" fn libcrun_error_release(_err: *mut *mut libcrun_error_t) {
    // Stub: not implemented
}

#[no_mangle]
pub extern "C" fn libcrun_container_load_from_memory(
    _config_json: *const c_char,
    _err: *mut *mut libcrun_error_t
) -> *mut libcrun_container_t {
    std::ptr::null_mut() // Stub: not implemented
}

#[no_mangle]
pub extern "C" fn libcrun_container_create(
    _context: *mut libcrun_context_t,
    _container: *mut libcrun_container_t,
    _id: *const c_char,
    _err: *mut *mut libcrun_error_t
) -> c_int {
    -1 // Stub: not implemented
}

#[no_mangle]
pub extern "C" fn libcrun_container_start(
    _context: *mut libcrun_context_t,
    _container: *mut libcrun_container_t,
    _id: *const c_char,
    _err: *mut *mut libcrun_error_t
) -> c_int {
    -1 // Stub: not implemented
}

#[no_mangle]
pub extern "C" fn libcrun_container_kill(
    _context: *mut libcrun_context_t,
    _container: *mut libcrun_container_t,
    _id: *const c_char,
    _signal: c_int,
    _err: *mut *mut libcrun_error_t
) -> c_int {
    -1 // Stub: not implemented
}

#[no_mangle]
pub extern "C" fn libcrun_container_delete(
    _context: *mut libcrun_context_t,
    _container: *mut libcrun_container_t,
    _id: *const c_char,
    _err: *mut *mut libcrun_error_t
) -> c_int {
    -1 // Stub: not implemented
}

#[no_mangle]
pub extern "C" fn libcrun_container_state(
    _context: *mut libcrun_context_t,
    _container: *mut libcrun_container_t,
    _id: *const c_char,
    _err: *mut *mut libcrun_error_t
) -> c_int {
    -1 // Stub: not implemented
}

#[no_mangle]
pub extern "C" fn libcrun_container_free(_container: *mut libcrun_container_t) {
    // Stub: not implemented
}

#[no_mangle]
pub extern "C" fn libcrun_context_new(_err: *mut *mut libcrun_error_t) -> *mut libcrun_context_t {
    std::ptr::null_mut() // Stub: not implemented
}

#[no_mangle]
pub extern "C" fn libcrun_context_free(_context: *mut libcrun_context_t) {
    // Stub: not implemented
}
"#;

        fs::write(&bindings_file, stub_bindings).expect("Couldn't write stub bindings!");
    }
}
