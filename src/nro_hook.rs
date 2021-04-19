use skyline::nro::{self, Callback, NroInfo};
use skyline::{nn, libc};

#[cfg(feature = "standalone")]
use parking_lot::Mutex;

#[cfg(feature = "standalone")]
static LOAD_HOOKS: Mutex<Vec<Callback>> = Mutex::new(Vec::new());

#[cfg(feature = "standalone")]
static UNLOAD_HOOKS: Mutex<Vec<Callback>> = Mutex::new(Vec::new());

#[cfg(feature = "standalone")]
#[skyline::hook(replace = nn::ro::LoadModule)]
fn handle_load_module(
    out_module: &mut nn::ro::Module,
    image: *const libc::c_void,
    buffer: *mut libc::c_void,
    buffer_size: usize,
    _flag: i32
) -> i32 {
    let ret = call_original!(out_module, image, buffer, buffer_size, nn::ro::BindFlag_BindFlag_Now as i32); // no lazy binding
    let name = unsafe { 
        let c_str = &out_module.Name as *const _;
        let name_slice = core::slice::from_raw_parts(c_str, libc::strlen(c_str));
        match core::str::from_utf8(&name_slice) {
            Ok(v) => v.to_owned(),
            Err(e) => String::from("")
        }
    };
    if ret != 0 {
        println!("[smashline::nro_hook] nn::ro::LoadModule failed for module '{}'. Result code {:#x}", name, ret);
    } else {
        println!("[smashline::nro_hook] Module '{}' loaded.", name);
        let nro_info = NroInfo::new(&name, out_module);
        for hook in LOAD_HOOKS.lock().iter() {
            hook(&nro_info);
        }
    }
    ret
}

#[cfg(feature = "standalone")]
#[skyline::hook(replace = nn::ro::UnloadModule)]
fn handle_unload_module(in_module: &mut nn::ro::Module) {
    let name = unsafe { 
        let c_str = &in_module.Name as *const _;
        let name_slice = core::slice::from_raw_parts(c_str, libc::strlen(c_str));
        match core::str::from_utf8(&name_slice) {
            Ok(v) => v.to_owned(),
            Err(e) => String::from("")
        }
    };
    println!("[smashline::nro_hook] Module '{}' unloaded.", name);
    let nro_info = NroInfo::new(&name, in_module);
    for hook in UNLOAD_HOOKS.lock().iter() {
        hook(&nro_info);
    }
    call_original!(in_module);
}

#[cfg(feature = "standalone")]
#[no_mangle]
pub extern "Rust" fn add_nro_load_hook(callback: Callback) {
    let mut hooks = LOAD_HOOKS.lock();
    hooks.push(callback);
}

#[cfg(not(feature = "standalone"))]
pub fn add_nro_load_hook(callback: Callback) {
    match nro::add_hook(callback) {
        Err(_) => {
            panic!("smashline failed to add NRO callback because libnro_hook.nro is missing!");
        },
        _ => {}
    }
}

#[cfg(feature = "standalone")]
#[no_mangle]
pub extern "Rust" fn add_nro_unload_hook(callback: Callback) {
    let mut hooks = UNLOAD_HOOKS.lock();
    hooks.push(callback);
}

#[cfg(not(feature = "standalone"))]
pub fn add_nro_unload_hook(callback: Callback) {
    match nro::add_unload_hook(callback) {
        Err(_) => {
            panic!("smashline failed to add NRO callback because libnro_hook.nro is missing!");
        }
        _ => {}
    }
}

#[cfg(feature = "standalone")]
pub fn install() {
    if cfg!(feature = "standalone") {
        skyline::install_hooks!(
            handle_load_module,
            handle_unload_module
        );
    }
}

#[cfg(not(feature = "standalone"))]
pub fn install() {}