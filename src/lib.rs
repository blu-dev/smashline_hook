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
mod loader;
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

pub static mut COMMON_MEMORY_INFO: Option<nx::QueryMemoryResult> = None;

fn nro_load(info: &NroInfo) {
    callbacks::nro_load(info);   
    hooks::nro_load(info);
    acmd::nro_load(info);
    status::nro_load(info);
    if info.name == "common" {
        unsafe {
            COMMON_MEMORY_INFO = Some(nx::svc::query_memory((*info.module.ModuleObject).module_base as usize).expect("Unable to query common memory info."));
        }
    }
}

fn nro_unload(info: &NroInfo) {
    scripts::clear_loaded_agent(info);
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
    if cfg!(feature = "development") {
        unsafe {
            loader::load_development_plugin();
            std::thread::spawn(|| {
                let mut load_flag = false;
                skyline::nn::hid::InitializeNpad();
                loop {
                    const KEY_L: u32 = 1 << 6;
                    const KEY_R: u32 = 1 << 7;
                    const KEY_DUP: u32 = 1 << 13;
                    const BUTTON_COMBO: u64 = (KEY_L | KEY_R | KEY_DUP) as u64;
                    use skyline::nn::hid::*;
                    if load_flag {
                        std::thread::sleep(std::time::Duration::from_secs(5));
                        load_flag = false;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let mut npad_state = NpadHandheldState::default();
                    GetNpadHandheldState(&mut npad_state, &0x20);
                    if (npad_state.Buttons & BUTTON_COMBO) == BUTTON_COMBO {
                        loader::load_development_plugin();
                        load_flag = true;
                        continue;
                    } 
                    for x in 0..8 {
                        GetNpadFullKeyState(&mut npad_state, &x);
                        if (npad_state.Buttons & BUTTON_COMBO) == BUTTON_COMBO {
                            loader::load_development_plugin();
                            load_flag = true;
                            break;
                        }
                        GetNpadJoyDualState(&mut npad_state, &x);
                        if (npad_state.Buttons & BUTTON_COMBO) == BUTTON_COMBO {
                            loader::load_development_plugin();
                            load_flag = true;
                            break;
                        }
                        GetNpadGcState(&mut npad_state, &x);
                        if (npad_state.Buttons & BUTTON_COMBO) == BUTTON_COMBO {
                            loader::load_development_plugin();
                            load_flag = true;
                            break;
                        }
                    }
                }
            });
        }
    }
}
