#![feature(proc_macro_hygiene)]
#![feature(asm)]
#![allow(unused_imports)]

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate bitflags;

use skyline::{hook, install_hook};

mod hooks;
mod nro_hook;
mod nx;
mod rtld;
mod unwind;

// I've copy pasted this from jugeeya so much
#[macro_export]
macro_rules! c_str {
    ($l:tt) => {
        [$l.as_bytes(), "\u{0}".as_bytes()].concat().as_ptr();
    }
}

static mut ORIGINAL: *const extern "C" fn() -> bool = 0 as _;

unsafe extern "C" fn realloc_hook() -> bool {
    let callable: extern "C" fn() -> bool = std::mem::transmute(ORIGINAL);
    let ret = callable();
    println!("{}", ret);
    ret
}

#[skyline::main(name = "smashline_hook")]
pub fn main() {
    nro_hook::install();
    nro_hook::add_nro_load_hook(hooks::nro_load);
    nro_hook::add_nro_unload_hook(hooks::nro_unload);

    unwind::install();
}
