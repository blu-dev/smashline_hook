#![feature(proc_macro_hygiene)]
#![feature(asm)]
#![allow(unused_imports)]
#![feature(const_if_match)]

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate paste;

use skyline::{hook, install_hook};
use skyline::nro::NroInfo;

mod acmd;
mod hooks;
mod nro_hook;
mod nx;
mod rtld;
mod scripts;
mod unwind;

// I've copy pasted this from jugeeya so much
#[macro_export]
macro_rules! c_str {
    ($l:tt) => {
        [$l.as_bytes(), "\u{0}".as_bytes()].concat().as_ptr();
    }
}

fn nro_load(info: &NroInfo) {
    hooks::nro_load(info);
    acmd::nro_load(info);
}

fn nro_unload(info: &NroInfo) {
    hooks::nro_unload(info);
    acmd::nro_unload(info);
}

#[skyline::main(name = "smashline_hook")]
pub fn main() {
    nro_hook::install();
    nro_hook::add_nro_load_hook(nro_load);
    nro_hook::add_nro_unload_hook(nro_unload);

    unwind::install();
}
