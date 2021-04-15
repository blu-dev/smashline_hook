use std::collections::HashMap;

use aarch64_decode::*;
use skyline::nro::NroInfo;
use smash::phx::Hash40;
use smash::app::{BattleObject, BattleObjectModuleAccessor};
use smash::lua_State;
use smash::lua2cpp::L2CAgentBase;

use parking_lot::Mutex;

use crate::hooks::lazy_symbol_replace;
use crate::acmd::{Category, GAME_SCRIPTS, EFFECT_SCRIPTS, SOUND_SCRIPTS, EXPRESSION_SCRIPTS};
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
                                (*agent).sv_set_function_hash(Some(std::mem::transmute(script_info.bind_fn)), script_info.script.hash);
                            }
                        }
                        if let Some(scripts) = script_list.get_mut(&hash) {
                            for script_info in scripts.iter_mut() {
                                if let Some(original) = script_info.original.as_mut() {
                                    let og_func = *(*agent).functions.get(&script_info.script).unwrap_or(&(0 as _));
                                    **original = std::mem::transmute(og_func);
                                }
                                println!("setting func has {}", script_info.bind_fn as u64);
                                (*agent).sv_set_function_hash(Some(std::mem::transmute(script_info.bind_fn)), script_info.script.hash);
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

type CreateAgentFunc = unsafe extern "C" fn(Hash40, *mut BattleObject, *mut BattleObjectModuleAccessor, *mut lua_State) -> *mut L2CAgentBase;

struct CreateAgentInfo {
    pub original: CreateAgentFunc,
    pub hashes: Vec<Hash40>
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