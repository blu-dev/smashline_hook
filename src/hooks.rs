use std::collections::HashMap;

use parking_lot::Mutex;

use skyline::nro::NroInfo;
use nnsdk::root::rtld::ModuleObject;

use crate::rtld;

struct HookCtx {
    pub symbol: String,
    pub replace: *const extern "C" fn(),
    pub original: Option<&'static mut *const extern "C" fn()>
}

unsafe impl Send for HookCtx {}
unsafe impl Sync for HookCtx {}

lazy_static::lazy_static! {
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

unsafe fn lazy_symbol_replace(module_object: *mut ModuleObject, symbol: &str, replace: *const extern "C" fn(), original: Option<&mut &'static mut *const extern "C" fn()>) {
    let sym = rtld::get_symbol_by_name(module_object, symbol);
    if sym.is_null() {
        println!("[smashline::hooks] Unable to find symbol {} to hook.", symbol);
    } else {
        let base = (*module_object).module_base;
        let difference = (replace as u64) - base;
        if let Some(original) = original {
            **original = ((*sym).st_value + base) as *const extern "C" fn();
        }
        skyline::patching::sky_memcpy(&(*sym).st_value as *const u64 as *const _, &difference as *const u64 as *const _, 8);
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