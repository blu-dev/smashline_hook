use skyline::nro::NroInfo;
use smash::phx::Hash40;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(PartialEq, Clone, Copy)]
pub enum Category {
    ACMD_GAME,
    ACMD_EFFECT,
    ACMD_SOUND,
    ACMD_EXPRESSION
}

pub struct ScriptInfo {
    pub script: Hash40,
    pub original: Option<&'static mut *const extern "C" fn()>,
    pub low_priority: bool,
    pub bind_fn: *const extern "C" fn(),
    pub backup: *const extern "C" fn() // serves same purpose as `original` except for guaranteeing something on uninstallation
}

impl ScriptInfo {
    pub fn transfer(&mut self) -> Self {
        Self {
            script: self.script,
            original: self.original.take(),
            low_priority: self.low_priority,
            bind_fn: self.bind_fn,
            backup: self.backup
        }
    }
}

impl PartialEq<Hash40> for ScriptInfo {
    fn eq(&self, other: &Hash40) -> bool {
        self.script == *other
    }
}

impl PartialEq for ScriptInfo {
    fn eq(&self, other: &ScriptInfo) -> bool {
        self.script == other.script
    }
}

impl Eq for ScriptInfo {}

impl Hash for ScriptInfo {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.script.hash(state);
    }
}

unsafe impl Sync for ScriptInfo {}
unsafe impl Send for ScriptInfo {}

lazy_static! {
    pub static ref GAME_SCRIPTS: Mutex<HashMap<Hash40, Vec<ScriptInfo>>> = Mutex::new(HashMap::new());
    pub static ref EFFECT_SCRIPTS: Mutex<HashMap<Hash40, Vec<ScriptInfo>>> = Mutex::new(HashMap::new());
    pub static ref SOUND_SCRIPTS: Mutex<HashMap<Hash40, Vec<ScriptInfo>>> = Mutex::new(HashMap::new());
    pub static ref EXPRESSION_SCRIPTS: Mutex<HashMap<Hash40, Vec<ScriptInfo>>> = Mutex::new(HashMap::new());
}

pub fn nro_load(info: &NroInfo) {
    static CATEGORIES: &[Category] = &[
        Category::ACMD_GAME,
        Category::ACMD_EFFECT,
        Category::ACMD_SOUND,
        Category::ACMD_EXPRESSION
    ];
    match info.name {
        "common" | "item" | "" => {},
        _ => {
            for cat in CATEGORIES.iter() {
                crate::scripts::patch_create_agent_animcmd(info, *cat);
            }
        }
    }
}

pub fn nro_unload(info: &NroInfo) {
    
}

pub unsafe fn remove_acmd_scripts(range: (usize, usize)) {
    crate::scripts::remove_live_acmd_scripts(range);

    let locks = &mut [
        GAME_SCRIPTS.lock(),
        EFFECT_SCRIPTS.lock(),
        SOUND_SCRIPTS.lock(),
        EXPRESSION_SCRIPTS.lock()
    ];

    let (begin, end) = range;

    for scripts in locks.iter_mut() {
        for (_, script_list) in scripts.iter_mut() {
            let mut new_vec = Vec::with_capacity(script_list.len());
            for script in script_list.iter_mut() {
                let as_usize = script.bind_fn as *const () as usize;
                if !(begin <= as_usize && as_usize < end) {
                    new_vec.push(script.transfer());
                }
            }
            *script_list = new_vec;
        }
    }
}

#[no_mangle]
pub extern "Rust" fn replace_acmd_script(agent: Hash40, script: Hash40, original: Option<&'static mut *const extern "C" fn()>, category: Category, low_priority: bool, bind_fn: *const extern "C" fn()) {
    crate::unwind::register_skyline_plugin(bind_fn as usize);
    
    let mut info = ScriptInfo { script, original, low_priority, bind_fn, backup: 0 as _ };

    unsafe {
        crate::scripts::install_live_acmd_scripts(agent, category, &mut info);
    }
    
    let mut map = match category {
        Category::ACMD_GAME => GAME_SCRIPTS.lock(),
        Category::ACMD_EFFECT => EFFECT_SCRIPTS.lock(),
        Category::ACMD_SOUND => SOUND_SCRIPTS.lock(),
        Category::ACMD_EXPRESSION => EXPRESSION_SCRIPTS.lock()
    };

    if let Some(script_list) = map.get_mut(&agent) {
        for script_info in script_list.iter_mut() {
            if script_info.script == info.script {
                if script_info.low_priority {
                    *script_info = info;
                } else {
                    println!("[smashline::acmd] ACMD script already replaced with high priority | Agent: {:#x}, Script: {:#x}", agent.hash, info.script.hash);
                }
                return;
            } 
        }
        script_list.push(info);
    } else {
        map.insert(agent, vec![info]);
    }
}