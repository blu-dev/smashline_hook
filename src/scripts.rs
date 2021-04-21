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
use crate::status::{COMMON_STATUS_SCRIPTS, STATUS_SCRIPTS};
use crate::COMMON_MEMORY_INFO;
use Category::*;

macro_rules! generate_new_create_agent {
    ($(($func_name:ident, $agent_infos:ident, $script_list:ident, $cat:ident, $share:expr)),* $(,)?) => {
        $(
            generate_new_create_agent!($func_name, $agent_infos, $script_list, $cat, $share);
        )*
    };
    ($func_name:ident, $agent_infos:ident, $script_list:ident, $cat:ident, $share:expr) => {
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
                        LOADED_ACMD_AGENTS.lock().push(LoadedAcmdAgentInfo { agent: agent, hash: hash, category: $cat, is_share: $share });
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
                                let og_func = *(*agent).functions.get(&script_info.script).unwrap_or(&(0 as _));
                                script_info.backup = std::mem::transmute(og_func);
                                if let Some(original) = script_info.original.as_mut() {
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
    (animcmd_game, GAME_CREATE_AGENTS, GAME_SCRIPTS, ACMD_GAME, false),
    (animcmd_game_share, GAME_SHARE_CREATE_AGENTS, GAME_SCRIPTS, ACMD_GAME, true),
    (animcmd_effect, EFFECT_CREATE_AGENTS, EFFECT_SCRIPTS, ACMD_EFFECT, false),
    (animcmd_effect_share, EFFECT_SHARE_CREATE_AGENTS, EFFECT_SCRIPTS, ACMD_EFFECT, true),
    (animcmd_sound, SOUND_CREATE_AGENTS, SOUND_SCRIPTS, ACMD_SOUND, false),
    (animcmd_sound_share, SOUND_SHARE_CREATE_AGENTS, SOUND_SCRIPTS, ACMD_SOUND, true),
    (animcmd_expression, EXPRESSION_CREATE_AGENTS, EXPRESSION_SCRIPTS, ACMD_EXPRESSION, false),
    (animcmd_expression_share, EXPRESSION_SHARE_CREATE_AGENTS, EXPRESSION_SCRIPTS, ACMD_EXPRESSION, true)
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
            let og_func = (*agent).sv_get_status_func(
                &L2CValue::I32(script.status.get()),
                &L2CValue::I32(script.condition.get())
            ).get_ptr();
            if let Some(original) = script.original.as_mut() {
                **original = std::mem::transmute(og_func);
            }
            script.backup = std::mem::transmute(og_func);
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
            let mut loaded_agents = LOADED_STATUS_AGENTS.lock();
            loaded_agents.push(LoadedStatusAgentInfo { agent, hash });
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

#[derive(Copy, Clone)]
struct LoadedAcmdAgentInfo {
    pub agent: *mut L2CAgentBase,
    pub hash: Hash40,
    pub category: Category,
    pub is_share: bool
}

unsafe impl Sync for LoadedAcmdAgentInfo {}
unsafe impl Send for LoadedAcmdAgentInfo {}

struct StatusCreateAgentInfo {
    pub agent: Hash40,
    pub set_status_func: StatusFunc,
    pub status_dtor: StatusFunc,
    pub status_del_dtor: StatusFunc
}

#[derive(Copy, Clone)]
struct LoadedStatusAgentInfo {
    pub agent: *mut L2CAgentBase,
    pub hash: Hash40
}

unsafe impl Sync for LoadedStatusAgentInfo {}
unsafe impl Send for LoadedStatusAgentInfo {}

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

    static ref LOADED_ACMD_AGENTS: Mutex<Vec<LoadedAcmdAgentInfo>> = Mutex::new(Vec::new());
    static ref LOADED_STATUS_AGENTS: Mutex<Vec<LoadedStatusAgentInfo>> = Mutex::new(Vec::new());
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

pub fn install_live_acmd_scripts(agent_hash: Hash40, category: Category, info: &mut crate::acmd::ScriptInfo) {
    let agents = LOADED_ACMD_AGENTS.lock();
    for agent in agents.iter() {
        if agent.hash == agent_hash && agent.category == category {
            unsafe {
                let test_func = *((*agent.agent).vtable as *const usize).add(1);
                let original_module = crate::nx::svc::query_memory(test_func).expect("Smashline unable to query mem info from live agent.");
                let og_begin = original_module.mem_info.base_address;
                let og_end = og_begin + original_module.mem_info.size;
                let current = *(*agent.agent).functions.get(&info.script).unwrap_or(&(0 as _));
                if current == 0 as _ || (og_begin <= (current as usize) && (current as usize) < og_end) {
                    if let Some(original) = info.original.as_mut() {
                        **original = std::mem::transmute(current);
                    }
                    info.backup = std::mem::transmute(current);
                    (*agent.agent).sv_set_function_hash(std::mem::transmute(info.bind_fn), info.script);
                }
            }
        }
    }
}

pub unsafe fn install_live_status_scripts(agent_hash: Hash40, info: &mut crate::status::StatusInfo, common_module: &crate::nx::QueryMemoryResult, is_common: bool) {
    let agents = LOADED_STATUS_AGENTS.lock();
    for agent in agents.iter() {
        if agent.hash == agent_hash || is_common {
            let test_func = *((*agent.agent).vtable as *const usize).add(STATUS_DTOR);
            let original_module = crate::nx::svc::query_memory(test_func).expect("Smashline unable to query mem info from live agent.");
            let common = common_module.mem_info.base_address..common_module.mem_info.base_address + common_module.mem_info.size;
            let original = original_module.mem_info.base_address..original_module.mem_info.base_address + original_module.mem_info.size;
            let current = (*agent.agent).sv_get_status_func(
                &L2CValue::I32(info.status.get()),
                &L2CValue::I32(info.condition.get())
            ).get_ptr() as usize;
            // println!("{:#x} {:#x?} {:#x?}", current, common, original);
            if current == 0 || common.contains(&current) || (original.contains(&current) && !is_common) {
                if let Some(original) = info.original.as_mut() {
                    **original = std::mem::transmute(current);
                }
                info.backup = std::mem::transmute(current);
                (*agent.agent).sv_set_status_func(
                    L2CValue::I32(info.status.get()),
                    L2CValue::I32(info.condition.get()),
                    std::mem::transmute(info.replacement)
                );
            }
        }
    }
}

pub unsafe fn remove_live_acmd_scripts(range: (usize, usize)) {
    macro_rules! remove_scripts {
        ($agent:ident, $scripts:ident, $begin:ident, $end:ident) => {
            let scripts = $scripts.lock();
            if let Some(script_list) = scripts.get(&$agent.hash) {
                for script in script_list.iter() {
                    let as_usize = script.bind_fn as *const () as usize;
                    if $begin <= as_usize && as_usize < $end {
                        (*$agent.agent).sv_set_function_hash(
                            std::mem::transmute(script.backup),
                            script.script
                        );
                    }
                }
            }
        }
    }

    let (begin, end) = range;
    let agents = LOADED_ACMD_AGENTS.lock();
    for agent in agents.iter() {
        match agent.category {
            ACMD_GAME => {
                remove_scripts!(agent, GAME_SCRIPTS, begin, end);
            },
            ACMD_EFFECT => {
                remove_scripts!(agent, EFFECT_SCRIPTS, begin, end);
            },
            ACMD_SOUND => {
                remove_scripts!(agent, SOUND_SCRIPTS, begin, end);
            },
            ACMD_EXPRESSION => {
                remove_scripts!(agent, EXPRESSION_SCRIPTS, begin, end);
            }
        }
    }
}

pub unsafe fn remove_live_status_scripts(range: (usize, usize)) {
    let (begin, end) = range;
    let agents = LOADED_STATUS_AGENTS.lock();
    let scripts = STATUS_SCRIPTS.lock();
    let common_scripts = COMMON_STATUS_SCRIPTS.lock();
    for agent in agents.iter() {
        if let Some(script_list) = scripts.get(&agent.hash) {
            for script in script_list.iter() {
                let as_usize = script.replacement as *const () as usize;
                if begin <= as_usize && as_usize < end {
                    (*agent.agent).sv_set_status_func(
                        L2CValue::I32(script.status.get()),
                        L2CValue::I32(script.condition.get()),
                        std::mem::transmute(script.backup)
                    );
                }
            }
        }
        if let Some(script_list) = common_scripts.get(&Hash40::new("common")) {
            for script in script_list.iter() {
                let as_usize = script.replacement as *const () as usize;
                if begin <= as_usize && as_usize < end {
                    (*agent.agent).sv_set_status_func(
                        L2CValue::I32(script.status.get()),
                        L2CValue::I32(script.condition.get()),
                        std::mem::transmute(script.backup)
                    );
                }
            }
        }
    }
}

pub fn clear_loaded_agent(info: &NroInfo) {
    let possible_agent_hashes = [
        GAME_CREATE_AGENTS.lock(),
        GAME_SHARE_CREATE_AGENTS.lock(),
        EFFECT_CREATE_AGENTS.lock(),
        EFFECT_SHARE_CREATE_AGENTS.lock(),
        SOUND_CREATE_AGENTS.lock(),
        SOUND_SHARE_CREATE_AGENTS.lock(),
        EXPRESSION_CREATE_AGENTS.lock(),
        EXPRESSION_SHARE_CREATE_AGENTS.lock()
    ];
    let module_name = String::from(info.name);
    for hashes in possible_agent_hashes.iter() {
        if let Some(agent_hashes) = hashes.get(&module_name) {
            for hash in agent_hashes.hashes.iter() {
                let mut loaded_agents = LOADED_ACMD_AGENTS.lock();
                let mut new_agents = Vec::new();
                for agent in loaded_agents.iter() {
                    if agent.hash != *hash {
                        new_agents.push(*agent);
                    }
                }
                *loaded_agents = new_agents;
                let mut loaded_agents = LOADED_STATUS_AGENTS.lock();
                let mut new_agents = Vec::new();
                for agent in loaded_agents.iter() {
                    if agent.hash != *hash {
                        new_agents.push(*agent);
                    }
                }
                *loaded_agents = new_agents;
            }
        }
    }
}