use std::collections::HashMap;

use parking_lot::Mutex;

use skyline::nro::NroInfo;
use nnsdk::root::{Elf64_Sym, rtld::ModuleObject};

use crate::c_str;
use crate::rtld;

struct HookCtx {
    pub symbol: String,
    pub replace: *const extern "C" fn(),
    pub original: Option<&'static mut *const extern "C" fn()>
}

pub enum StaticSymbol {
    Resolved(usize),
    Unresolved(&'static str)
}

unsafe impl Send for HookCtx {}
unsafe impl Sync for HookCtx {}

lazy_static! {
    static ref SYMBOL_HOOKS: Mutex<HashMap<String, Vec<HookCtx>>> = Mutex::new(HashMap::new());
}

pub fn nro_load(nro_info: &NroInfo) {
    let mut map = SYMBOL_HOOKS.lock();
    if let Some(hooks) = map.get_mut(nro_info.name) {
        for hook in hooks.iter_mut() {
            unsafe {
                lazy_symbol_replace(nro_info.module.ModuleObject, hook.symbol.as_str(), hook.replace, hook.original.as_mut());
            }
        }
    }
}

pub fn nro_unload(nro_info: &NroInfo) {
    let mut map = SYMBOL_HOOKS.lock();
    if let Some(hooks) = map.get_mut(nro_info.name) {
        for hook in hooks.iter_mut() {
            if let Some(original) = hook.original.as_mut() {
                // change the original function to nullptr, leave it to the
                // smashline-macro implementation to check when calling original
                **original = 0 as _;
            }
        }
    }
}

pub unsafe fn lazy_symbol_replace(module_object: *mut ModuleObject, symbol: &str, replace: *const extern "C" fn(), original: Option<&mut &'static mut *const extern "C" fn()>) {
    let sym = rtld::get_symbol_by_name(module_object, symbol);
    if sym.is_null() {
        println!("[smashline::hooks] Unable to find symbol {} to replace", symbol);
    } else {
        symbol_replace(module_object, sym, replace, original);
    }
}

unsafe fn symbol_replace(module_object: *mut ModuleObject, symbol: *const Elf64_Sym, replace: *const extern "C" fn(), original: Option<&mut &'static mut *const extern "C" fn()>) {
    if !symbol.is_null() {
        let base = (*module_object).module_base;
        let difference = (replace as u64) - base;
        if let Some(original) = original {
            **original = ((*symbol).st_value + base) as *const extern "C" fn();
        }
        skyline::patching::sky_memcpy(&(*symbol).st_value as *const u64 as *const _, &difference as *const u64 as *const _, 8);
    }
}

#[no_mangle]
pub extern "Rust" fn replace_symbol(module: &str, symbol: &str, replace: *const extern "C" fn(), original: Option<&'static mut *const extern "C" fn()>) {
    let mut map = SYMBOL_HOOKS.lock();
    let hook_ctx = HookCtx {
        symbol: String::from(symbol),
        replace,
        original
    };
    if let Some(hooks) = map.get_mut(module) {
        hooks.push(hook_ctx);
    } else {
        map.insert(String::from(module), vec![hook_ctx]);
    }
}

#[no_mangle]
pub extern "Rust" fn replace_static_symbol(symbol: StaticSymbol, replace: *const extern "C" fn(), mut original: Option<&'static mut *const extern "C" fn()>) {
    unsafe {
        match symbol {
            StaticSymbol::Unresolved(sym) => {
                let mut symbol_addr = 0usize;
                let result = skyline::nn::ro::LookupSymbol(&mut symbol_addr, c_str!(sym));
                if result != 0 || symbol_addr == 0 {
                    panic!("Failed to lookup symbol \"{}\", is it really static?", sym);
                }
                let module_object = rtld::get_module_object_from_address(symbol_addr).expect("Failed to get module object from static symbol, is it really static?");
                lazy_symbol_replace(module_object, sym, replace, original.as_mut());
            },
            StaticSymbol::Resolved(addr) => {
                let module_object = rtld::get_module_object_from_address(addr).expect("Failed to get module object from static symbol, is it really static?");
                let sym = rtld::get_symbol_by_resolved_address(module_object, addr);
                if sym.is_null() {
                    println!("[smashline::hooks] Unable to replace static symbol with resolved address {:#x}", addr);
                } else {
                    symbol_replace(module_object, sym, replace, original.as_mut());
                }
            }
        }
    }
}