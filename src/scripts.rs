use std::collections::HashMap;

use aarch64_decode::*;
use skyline::nro::NroInfo;
use smash::phx::Hash40;
use smash::app::{BattleObject, BattleObjectModuleAccessor};
use smash::lua_State;
use smash::lua2cpp::L2CAgentBase;
use smash::lib::L2CValue;

use parking_lot::Mutex;

use crate::hooks::lazy_symbol_replace;
use crate::acmd::{Category, GAME_SCRIPTS, EFFECT_SCRIPTS, SOUND_SCRIPTS, EXPRESSION_SCRIPTS};
use crate::status::STATUS_SCRIPTS;
use Category::*;

macro_rules! generate_new_create_agent {
    ($(($func_name:ident, $agent_infos:ident, $script_list:ident)),* $(,)?) => {
        $(
            generate_new_create_agent!($func_name, $agent_infos, $script_list);
        )*
    };
    ($func_name:ident, $agent_infos:ident, $script_list:ident) => {
        paste! {
            pub unsafe extern "C" fn [<create_agent_fighter_ $func_name>] (
                hash: Hash40,
                bobj: *mut BattleObject,
                boma: *mut BattleObjectModuleAccessor,
                state: *mut lua_State
            ) -> *mut L2CAgentBase {
                let infos = $agent_infos.lock();
                for (_, info) in infos.iter() {
                    if info.hashes.contains(&hash) {
                        let agent = (info.original)(hash, bobj, boma, state);
                        let mut script_list = $script_list.lock();
                        if let Some(scripts) = script_list.get_mut(&Hash40::new("common")) {
                            for script_info in scripts.iter_mut() {
                                if let Some(original) = script_info.original.as_mut() {
                                    **original = 0 as _;
                                }
                                (*agent).sv_set_function_hash(std::mem::transmute(script_info.bind_fn), script_info.script);
                            }
                        }
                        if let Some(scripts) = script_list.get_mut(&hash) {
                            for script_info in scripts.iter_mut() {
                                if let Some(original) = script_info.original.as_mut() {
                                    let og_func = *(*agent).functions.get(&script_info.script).unwrap_or(&(0 as _));
                                    **original = std::mem::transmute(og_func);
                                }
                                (*agent).sv_set_function_hash(std::mem::transmute(script_info.bind_fn), script_info.script);
                            }
                        }
                        return agent;
                    }
                }
                return 0 as _;
            }
        }
    }
}

generate_new_create_agent!(
    (animcmd_game, GAME_CREATE_AGENTS, GAME_SCRIPTS),
    (animcmd_game_share, GAME_SHARE_CREATE_AGENTS, GAME_SCRIPTS),
    (animcmd_effect, EFFECT_CREATE_AGENTS, EFFECT_SCRIPTS),
    (animcmd_effect_share, EFFECT_SHARE_CREATE_AGENTS, EFFECT_SCRIPTS),
    (animcmd_sound, SOUND_CREATE_AGENTS, SOUND_SCRIPTS),
    (animcmd_sound_share, SOUND_SHARE_CREATE_AGENTS, SOUND_SCRIPTS),
    (animcmd_expression, EXPRESSION_CREATE_AGENTS, EXPRESSION_SCRIPTS),
    (animcmd_expression_share, EXPRESSION_SHARE_CREATE_AGENTS, EXPRESSION_SCRIPTS)
);

const STATUS_DTOR: usize = 15;
const STATUS_DEL_DTOR: usize = 16;
const STATUS_SET: usize = 17;
const STATUS_AGENT_HASH: usize = 18;

unsafe fn recreate_status_vtable(vtable: *const u64, hash: Hash40) -> *const u64 {
    let new_vtable = std::alloc::alloc(std::alloc::Layout::from_size_align(19 * 0x8, 0x8).unwrap()) as *mut u64;
    std::ptr::copy_nonoverlapping(vtable, new_vtable, 15);
    *new_vtable.add(STATUS_DTOR) = *vtable.add(0);
    *new_vtable.add(STATUS_DEL_DTOR) = *vtable.add(1);
    *new_vtable.add(STATUS_SET) = *vtable.add(9);
    *new_vtable.add(STATUS_AGENT_HASH) = hash.hash;
    *new_vtable.add(0) = status_agent_dtor as *const () as u64;
    *new_vtable.add(1) = status_agent_del_dtor as *const () as u64;
    *new_vtable.add(9) = set_status_scripts as *const () as u64;
    new_vtable
}

unsafe extern "C" fn status_agent_dtor(agent: *mut L2CAgentBase) {
    let callable: extern "C" fn(*mut L2CAgentBase) = std::mem::transmute(*((*agent).vtable as *const u64).add(STATUS_DTOR));
    callable(agent)
}

unsafe extern "C" fn status_agent_del_dtor(agent: *mut L2CAgentBase) {
    let callable: extern "C" fn(*mut L2CAgentBase) = std::mem::transmute(*((*agent).vtable as *const u64).add(STATUS_DEL_DTOR));
    callable(agent)
}

unsafe extern "C" fn set_status_scripts(agent: *mut L2CAgentBase) {
    let agent_hash = Hash40::new_raw(*((*agent).vtable as *const u64).add(STATUS_AGENT_HASH));
    let callable: extern "C" fn(*mut L2CAgentBase) = std::mem::transmute(*((*agent).vtable as *const u64).add(STATUS_SET));
    callable(agent);
    let mut scripts = STATUS_SCRIPTS.lock();
    if let Some(script_list) = scripts.get_mut(&agent_hash) {
        for script in script_list.iter_mut() {
            if let Some(original) = script.original.as_mut() {
                **original = (*agent).sv_get_status_func(
                    &L2CValue::I32(script.status.get()),
                    &L2CValue::I32(script.condition.get())
                ).get_ptr() as _;
            }
            let og = (*agent).sv_get_status_func(
                &L2CValue::I32(script.status.get()),
                &L2CValue::I32(script.condition.get())
            ).get_ptr();
            (*agent).sv_set_status_func(
                L2CValue::I32(script.status.get()),
                L2CValue::I32(script.condition.get()),
                std::mem::transmute(script.replacement)
            );
        }
    }
}

unsafe extern "C" fn create_agent_fighter_status_script(
    hash: Hash40,
    bobj: *mut BattleObject,
    boma: *mut BattleObjectModuleAccessor,
    state: *mut lua_State
) -> *mut L2CAgentBase {
    let agents = STATUS_CREATE_AGENTS.lock();
    for (module, info) in agents.iter() {
        if info.hashes.contains(&hash) {
            let agent = (info.original)(hash, bobj, boma, state);
            let new_vtable = recreate_status_vtable((*agent).vtable as *const u64, hash);
            (*agent).vtable = new_vtable as u64;
            let mut vtables = STATUS_VTABLES.lock();
            if let Some(vtables) = vtables.get_mut(module) {
                vtables.push(new_vtable as u64);
            } else {
                vtables.insert(module.clone(), vec![new_vtable as u64]);
            }
            return agent;
        }
    }
    0 as _
}

type StatusFunc = unsafe extern "C" fn(*mut L2CAgentBase);
type CreateAgentFunc = unsafe extern "C" fn(Hash40, *mut BattleObject, *mut BattleObjectModuleAccessor, *mut lua_State) -> *mut L2CAgentBase;

struct CreateAgentInfo {
    pub original: CreateAgentFunc,
    pub hashes: Vec<Hash40>
}

struct StatusCreateAgentInfo {
    pub agent: Hash40,
    pub set_status_func: StatusFunc,
    pub status_dtor: StatusFunc,
    pub status_del_dtor: StatusFunc
}

lazy_static! {
    static ref GAME_CREATE_AGENTS: Mutex<HashMap<String, CreateAgentInfo>> = Mutex::new(HashMap::new());
    static ref GAME_SHARE_CREATE_AGENTS: Mutex<HashMap<String, CreateAgentInfo>> = Mutex::new(HashMap::new());
    
    static ref EFFECT_CREATE_AGENTS: Mutex<HashMap<String, CreateAgentInfo>> = Mutex::new(HashMap::new());
    static ref EFFECT_SHARE_CREATE_AGENTS: Mutex<HashMap<String, CreateAgentInfo>> = Mutex::new(HashMap::new());
    
    static ref SOUND_CREATE_AGENTS: Mutex<HashMap<String, CreateAgentInfo>> = Mutex::new(HashMap::new());
    static ref SOUND_SHARE_CREATE_AGENTS: Mutex<HashMap<String, CreateAgentInfo>> = Mutex::new(HashMap::new());
    
    static ref EXPRESSION_CREATE_AGENTS: Mutex<HashMap<String, CreateAgentInfo>> = Mutex::new(HashMap::new());
    static ref EXPRESSION_SHARE_CREATE_AGENTS: Mutex<HashMap<String, CreateAgentInfo>> = Mutex::new(HashMap::new());

    static ref STATUS_CREATE_AGENTS: Mutex<HashMap<String, CreateAgentInfo>> = Mutex::new(HashMap::new());
    static ref STATUS_VTABLES: Mutex<HashMap<String, Vec<u64>>> = Mutex::new(HashMap::new());
}

fn get_movk_offset(instr: u32) -> u32 {
    (instr & 0x0060_0000) >> 21
}

pub fn read_hashes_from_executable(mut address: *const u32) -> Vec<Hash40> {
    let mut hashes = Vec::new();
    let mut cpu = [0; 31];

    unsafe {
        while let Some(instr) = aarch64_decode::decode_a64(*address) {
            match instr {
                Instr::Ret64RBranchReg => {
                    break;
                },
                Instr::Movz64Movewide { imm16: imm, Rd: dst, .. } => {
                    cpu[dst as usize] = imm as u64;
                },
                Instr::Movk64Movewide { imm16: part, Rd: dst, .. } => {
                    let offset = get_movk_offset(*address) * 16;
                    cpu[dst as usize] |= (part as u64) << offset;
                },
                Instr::Subs64AddsubShift{ Rm: src, Rn: _, Rd: _, .. } => {
                    hashes.push(Hash40::new_raw(cpu[src as usize]));
                },
                _ => {}
            }
            address = address.add(1);
        }
    }

    hashes
}

fn format_create_agent_symbol(fighter: &str, func: &str) -> String {
    let func_name = format!("create_agent_fighter_{}_{}", func, fighter);
    format!("_ZN7lua2cpp{}{}EN3phx6Hash40EPN3app12BattleObjectEPNS2_26BattleObjectModuleAccessorEP9lua_State", func_name.len(), func_name)
}

pub fn patch_create_agent_animcmd(info: &NroInfo, category: Category) {
    static mut ORIGINAL: *const extern "C" fn() = 0 as _; // to fulfill requirements of lazy_symbol_replace
    let mut original_opt: Option<&'static mut *const extern "C" fn()> = unsafe { Some(&mut ORIGINAL) };
    macro_rules! patch_category {
        ($cat:ident, $unshared_create_agents:ident, $shared_create_agents:ident, $unshared:expr, $shared:expr) => {
            paste! {
                let $cat = format_create_agent_symbol(info.name, $unshared);
                let [<$cat _share>] = format_create_agent_symbol(info.name, $shared);
                unsafe {
                    ORIGINAL = 0 as _;
                    lazy_symbol_replace(info.module.ModuleObject, $cat.as_str(), [<create_agent_fighter_animcmd_ $cat>] as *const _, original_opt.as_mut());
                    if !ORIGINAL.is_null() {
                        let mut map = $unshared_create_agents.lock();
                        map.insert(
                            String::from(info.name),
                            CreateAgentInfo {
                                original: std::mem::transmute(ORIGINAL),
                                hashes: read_hashes_from_executable(ORIGINAL as *const u32)
                            }
                        );
                    }
                    ORIGINAL = 0 as _;
                    lazy_symbol_replace(info.module.ModuleObject, [<$cat _share>].as_str(), [<create_agent_fighter_animcmd_ $cat _share>] as *const _, original_opt.as_mut());
                    if !ORIGINAL.is_null() {
                        let mut map = $shared_create_agents.lock();
                        map.insert(
                            String::from(info.name),
                            CreateAgentInfo {
                                original: std::mem::transmute(ORIGINAL),
                                hashes: read_hashes_from_executable(ORIGINAL as *const u32)
                            }
                        );
                    }
                }
            }
        }
    }

    match category {
        ACMD_GAME => {
            patch_category!(game, GAME_CREATE_AGENTS, GAME_SHARE_CREATE_AGENTS, "animcmd_game", "animcmd_game_share");
        },
        ACMD_EFFECT => {
            patch_category!(effect, EFFECT_CREATE_AGENTS, EFFECT_SHARE_CREATE_AGENTS, "animcmd_effect", "animcmd_effect_share");
        },
        ACMD_SOUND => {
            patch_category!(sound, SOUND_CREATE_AGENTS, SOUND_SHARE_CREATE_AGENTS, "animcmd_sound", "animcmd_sound_share");
        },
        ACMD_EXPRESSION => {
            patch_category!(expression, EXPRESSION_CREATE_AGENTS, EXPRESSION_SHARE_CREATE_AGENTS, "animcmd_expression", "animcmd_expression_share");
        }
    }
}

pub fn patch_create_agent_status(info: &NroInfo) {
    static mut ORIGINAL: *const extern "C" fn() = 0 as _; // to fulfill requirements of lazy_symbol_replace
    let mut original_opt: Option<&'static mut *const extern "C" fn()> = unsafe { Some(&mut ORIGINAL) };
    let status = format_create_agent_symbol(info.name, "status_script");
    unsafe {
        ORIGINAL = 0 as _;
        lazy_symbol_replace(info.module.ModuleObject, status.as_str(), create_agent_fighter_status_script as *const extern "C" fn(), original_opt.as_mut());
        if !ORIGINAL.is_null() {
            let mut agents = STATUS_CREATE_AGENTS.lock();
            agents.insert(
                String::from(info.name),
                CreateAgentInfo {
                    original: std::mem::transmute(ORIGINAL),
                    hashes: read_hashes_from_executable(ORIGINAL as *const u32)
                }
            );
        }
    }
}

pub fn release_status_vtables(info: &NroInfo) {
    let name = String::from(info.name);
    let mut vtables = STATUS_VTABLES.lock();
    if let Some(vtable_list) = vtables.remove(&name) {
        unsafe {
            let layout = std::alloc::Layout::from_size_align_unchecked(19 * 0x8, 0x8);
            for vtable in vtable_list.into_iter() {
                let vtable = vtable as *mut u8;
                std::alloc::dealloc(vtable, layout);
            }
        }
    }
}