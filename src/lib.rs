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
use smash::lib::LuaConst;

mod acmd;
mod callbacks;
mod hooks;
mod nro_hook;
mod nx;
mod rtld;
mod scripts;
mod status;
mod unwind;

#[derive(Clone)]
pub enum LuaConstant {
    Symbolic(LuaConst),
    Evaluated(i32)
}

impl LuaConstant {
    pub fn get(&self) -> i32 {
        match self {
            LuaConstant::Symbolic(symbolic) => **symbolic,
            LuaConstant::Evaluated(evaluated) => *evaluated
        }
    }
}

// I've copy pasted this from jugeeya so much
#[macro_export]
macro_rules! c_str {
    ($l:tt) => {
        [$l.as_bytes(), "\u{0}".as_bytes()].concat().as_ptr();
    }
}

fn nro_load(info: &NroInfo) {
    callbacks::nro_load(info);   
    hooks::nro_load(info);
    acmd::nro_load(info);
    status::nro_load(info);
}

fn nro_unload(info: &NroInfo) {
    callbacks::nro_unload(info);
    hooks::nro_unload(info);
    acmd::nro_unload(info);
    status::nro_unload(info);
}

#[skyline::main(name = "smashline_hook")]
pub fn main() {
    nro_hook::install();
    nro_hook::add_nro_load_hook(nro_load);
    nro_hook::add_nro_unload_hook(nro_unload);
    
    status::install();
    unwind::install();
}
