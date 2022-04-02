use skyline::nro::{self, Callback, NroInfo};
use skyline::{nn, libc};

pub fn add_nro_load_hook(callback: Callback) {
    match nro::add_hook(callback) {
        Err(_) => {
            panic!("smashline failed to add NRO callback because libnro_hook.nro is missing!");
        },
        _ => {}
    }
}

pub fn add_nro_unload_hook(callback: Callback) {
    match nro::add_unload_hook(callback) {
        Err(_) => {
            panic!("smashline failed to add NRO callback because libnro_hook.nro is missing!");
        }
        _ => {}
    }
}

extern "C" {
    fn nro_request_bind_now();
}

pub fn install() {
    unsafe {
        nro_request_bind_now();
    }
}