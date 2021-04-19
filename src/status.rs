use smash::lib::{lua_const::*, LuaConst, L2CValue};
use smash::phx::Hash40;
use smash::lua2cpp::*;
use std::collections::HashMap;
use parking_lot::Mutex;
use skyline::nro::NroInfo;
use crate::LuaConstant;

lazy_static! {
    pub static ref STATUS_SCRIPTS: Mutex<HashMap<Hash40, Vec<StatusInfo>>> = Mutex::new(HashMap::new());
    pub static ref COMMON_STATUS_SCRIPTS: Mutex<HashMap<Hash40, Vec<StatusInfo>>> = Mutex::new(HashMap::new());
}

pub struct StatusInfo {
    pub status: LuaConstant,
    pub condition: LuaConstant,
    pub original: Option<&'static mut *const extern "C" fn()>,
    pub low_priority: bool,
    pub replacement: *const extern "C" fn(),
    pub backup: *const extern "C" fn()
}

impl StatusInfo {
    pub fn transfer(&mut self) -> Self {
        Self {
            status: self.status.clone(),
            condition: self.condition.clone(),
            original: self.original.take(),
            low_priority: self.low_priority,
            replacement: self.replacement,
            backup: self.backup
        }
    }
}

unsafe impl Sync for StatusInfo {}
unsafe impl Send for StatusInfo {}

static mut CONSTANT_RESOLVER: Option<fn(&LuaConstant, &LuaConstant) -> bool> = None;

fn const_resolver(this: &LuaConstant, that: &LuaConstant) -> bool {
    let this = this.get();
    let that = that.get();
    this == that
}

static mut ORIGINAL: *const extern "C" fn() = 0 as _;

extern "C" fn sub_set_fighter_common_table_replace(fighter: &mut L2CFighterCommon) {
    let original: extern "C" fn(&mut L2CFighterCommon) = unsafe { std::mem::transmute(ORIGINAL) };
    original(fighter);
    let mut scripts = COMMON_STATUS_SCRIPTS.lock();
    if let Some(script_list) = scripts.get_mut(&Hash40::new("common")) {
        for script in script_list.iter_mut() {
            if script.status.get() != FIGHTER_STATUS_KIND_NONE {
                unsafe {
                    let og = fighter.sv_get_status_func(
                        &L2CValue::I32(script.status.get()),
                        &L2CValue::I32(script.condition.get()),
                    ).get_ptr() as *const extern "C" fn();
                    if let Some(original) = script.original.as_mut() {
                        **original = og;
                    }
                    script.backup = og;
                    fighter.sv_set_status_func(
                        L2CValue::I32(script.status.get()),
                        L2CValue::I32(script.condition.get()),
                        std::mem::transmute(script.replacement)
                    );
                }
            }
        }
    }
}

pub fn install() {
    unsafe {
        crate::hooks::replace_symbol("common", "_ZN7lua2cpp16L2CFighterCommon28sub_set_fighter_common_tableEv", sub_set_fighter_common_table_replace as *const extern "C" fn(), Some(&mut ORIGINAL));
    }
}

pub fn nro_load(info: &NroInfo) {
    match info.name {
        "common" => {
            // On common load we need to resolve all of the statuses added before the const table was filled in
            // this way people can just do a "one and done" approach like they can with status scripts
            let script_locks = &mut [
                STATUS_SCRIPTS.lock(),
                COMMON_STATUS_SCRIPTS.lock()
            ];
            unsafe {
                CONSTANT_RESOLVER = Some(const_resolver);
            }
            for lock in script_locks.iter_mut() {
                for (agent, info) in lock.iter_mut() {
                    let mut high_priority: Vec<StatusInfo> = Vec::new();
                    let mut low_priority = Vec::new();
                    for status_info in info.iter_mut() {
                        if status_info.low_priority {
                            low_priority.push(status_info.transfer());
                        } else {
                            let mut is_unique = true;
                            for high_info in high_priority.iter() {
                                if const_resolver(&high_info.status, &status_info.status) && const_resolver(&high_info.condition, &status_info.condition) {
                                    println!("[smashline::status] Status script already replaced with high priority | Status: {:#x}, condition: {:#x}", high_info.status.get(), high_info.condition.get());
                                    is_unique = false;
                                    break;
                                }
                            }
                            if is_unique {
                                high_priority.push(status_info.transfer());
                            }
                        }
                    }
    
                    // I'm sorry y'all, it has to be done
                    let mut output: Vec<StatusInfo> = Vec::new();
    
                    for low_info in low_priority.into_iter() {
                        let mut is_unique = true;
                        for high_info in high_priority.iter() {
                            if const_resolver(&high_info.status, &low_info.status) && const_resolver(&high_info.condition, &low_info.condition) {
                                println!("[smashline::status] Status script already replaced with high priority | Status: {:#x}, condition: {:#x}", high_info.status.get(), high_info.condition.get());
                                is_unique = false;
                                break;
                            }
                        }
                        if is_unique {
                            for output_info in output.iter_mut() {
                                if const_resolver(&low_info.status, &output_info.status) && const_resolver(&low_info.condition, &output_info.status) {
                                    *output_info = low_info;
                                    break;
                                }
                            }
                        }
                    }
                    output.reserve(high_priority.len());
                    for info in high_priority.into_iter() {
                        output.push(info);
                    }
                    *info = output;
                }
            }
        },
        "item" | "" => {},
        _ => {
            crate::scripts::patch_create_agent_status(info);
        }
    }
}

pub fn nro_unload(info: &NroInfo) {
    crate::scripts::release_status_vtables(info);
}

pub unsafe fn remove_status_scripts(range: (usize, usize)) {
    crate::scripts::remove_live_status_scripts(range);

    let range = range.0..range.1;
    let mut scripts = STATUS_SCRIPTS.lock();
    let mut common_scripts = COMMON_STATUS_SCRIPTS.lock();
    for (_, script_list) in scripts.iter_mut() {
        let mut new_script_list = Vec::with_capacity(script_list.len());
        for script in script_list.iter_mut() {
            if !range.contains(&(script.replacement as usize)) {
                new_script_list.push(script.transfer());
            }
        }
        *script_list = new_script_list;
    }
    for (_, script_list) in common_scripts.iter_mut() {
        let mut new_script_list = Vec::with_capacity(script_list.len());
        for script in script_list.iter_mut() {
            if !range.contains(&(script.replacement as usize)) {
                new_script_list.push(script.transfer());
            }
        }
        *script_list = new_script_list;
    }
}

#[no_mangle]
pub extern "Rust" fn replace_status_script(agent: Hash40, status: LuaConstant, condition: LuaConstant, original: Option<&'static mut *const extern "C" fn()>, low_priority: bool, replacement: *const extern "C" fn()) {
    let mut info = StatusInfo {
        status,
        condition,
        original,
        low_priority,
        replacement,
        backup: 0 as _
    };

    let mut scripts = STATUS_SCRIPTS.lock();
    unsafe {
        if let Some(common_module) = crate::COMMON_MEMORY_INFO.as_ref() {
            crate::scripts::install_live_status_scripts(agent, &mut info, common_module, false);
        }
    }

    if let Some(script_list) = scripts.get_mut(&agent) {
        unsafe {
            if let Some(resolver) = CONSTANT_RESOLVER.as_ref() {
                for script in script_list.iter_mut() {
                    if (resolver)(&script.status, &info.status) && (resolver)(&script.condition, &info.condition) {
                        if script.low_priority {
                            *script = info;
                        } else {
                            println!("[smashline::status] Status script already replaced with high priority | Status: {:#x}, condition: {:#x}", script.status.get(), script.condition.get());
                        }
                        return;
                    }
                }
            }
            script_list.push(info);
        }
    } else {
        scripts.insert(agent, vec![info]);
    }
}

#[no_mangle]
pub extern "Rust" fn replace_common_status_script(status: LuaConstant, condition: LuaConstant, original: Option<&'static mut *const extern "C" fn()>, replacement: *const extern "C" fn()) {
    let mut info = StatusInfo {
        status,
        condition,
        original,
        low_priority: false,
        replacement,
        backup: 0 as _
    };

    let mut scripts = COMMON_STATUS_SCRIPTS.lock();
    unsafe {
        if let Some(common_module) = crate::COMMON_MEMORY_INFO.as_ref() {
            crate::scripts::install_live_status_scripts(Hash40::new("common"), &mut info, common_module, true);
        }
    }
    if let Some(script_list) = scripts.get_mut(&Hash40::new("common")) {
        unsafe {
            if let Some(resolver) = CONSTANT_RESOLVER.as_ref() {
                for script in script_list.iter_mut() {
                    if (resolver)(&script.status, &info.status) && (resolver)(&script.condition, &info.condition) {
                        if script.low_priority {
                            *script = info;
                        } else {
                            println!("[smashline::status] Common status script already replaced with high priority | Status: {:#x}, condition: {:#x}", script.status.get(), script.condition.get());
                        }
                        return;
                    }
                }
            }
            script_list.push(info);
        }
    } else {
        scripts.insert(Hash40::new("common"), vec![info]);
    }
}