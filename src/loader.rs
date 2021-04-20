use std::{mem::MaybeUninit, path::{Path, PathBuf}};
use skyline::{nn, libc};
use nn::ro::{self, NroHeader, NrrHeader, RegistrationInfo, Module};
use parking_lot::Mutex;

use crate::c_str;

macro_rules! align_up {
    ($x:expr, $a:expr) => {
        ((($x) + (($a)-1)) & !(($a)-1))
    }
}

struct DevelopmentPlugin {
    // pub path: PathBuf,
    pub nro_module: Module,
    pub nrr_info: RegistrationInfo,
    // pub bss_section: *mut u8,
    // pub bss_size: usize 
}

unsafe impl Send for DevelopmentPlugin {}
unsafe impl Sync for DevelopmentPlugin {}

extern "C" {
    #[allow(non_snake_case)]
    #[link_name = "\u{1}_ZN2nn2ro20UnregisterModuleInfoEPNS0_16RegistrationInfoE"]
    fn UnregisterModuleInfo(info: *mut RegistrationInfo);
}

lazy_static! {
    static ref LOADED_DEVELOPMENT_PLUGIN: Mutex<Option<DevelopmentPlugin>> = Mutex::new(None);
}

const NRR_SIZE: usize = std::mem::size_of::<NrrHeader>();

impl DevelopmentPlugin {
    pub unsafe fn new(path: &str) -> Option<Self> {
        let path = PathBuf::from(path);
        let file_path = path.as_path();
        if !file_path.exists() {
            println!("[smashline::loader] Development plugin file not found");
            return None;
        }

        let nro_image = match std::fs::read(file_path) {
            Ok(data) => data,
            Err(_) => {
                println!("[smashline::loader] Failed to read development plugin");
                return None;
            }
        };

        let nro_image = {
            let new_mem = libc::memalign(0x1000, nro_image.len()) as *mut u8;
            std::ptr::copy_nonoverlapping(nro_image.as_ptr(), new_mem, nro_image.len());
            new_mem as *const libc::c_void
        };

        let mut bss_size = 0u64;
        let rc = nn::ro::GetBufferSize(&mut bss_size, nro_image);
        if rc != 0 {
            println!("[smashline::loader] Failed to read buffer size from development plugin ({:#x}). Is it a valid NRO?", rc);
            libc::free(nro_image as *mut libc::c_void);
            return None;
        }
        let bss_size = bss_size as usize;

        let nro_header = nro_image as *const NroHeader;
        let mut hash = [0u8; 0x20];
        nn::crypto::GenerateSha256Hash(hash.as_mut_ptr() as _, 0x20, nro_header as _, (*nro_header).size as u64);
        
        let nrr_size = align_up!(NRR_SIZE + 0x20, 0x1000);
        let nrr_image = libc::memalign(0x1000, nrr_size) as *mut u8;
        libc::memset(nrr_image as _, 0x0, nrr_size);

        let program_id = skyline::info::get_program_id();

        {
            let nrr_header = &mut *(nrr_image as *mut NrrHeader);
            nrr_header.magic = 0x3052524E;
            nrr_header.program_id = ro::ProgramId { value: program_id };
            nrr_header.size = nrr_size as u32;
            nrr_header.type_ = 0;
            nrr_header.hashes_offset = NRR_SIZE as u32;
            nrr_header.num_hashes = 1;
        }

        libc::memcpy(nrr_image.add(NRR_SIZE) as _, hash.as_ptr() as _, 0x20);
        let mut nrr_info = MaybeUninit::uninit();
        let rc = ro::RegisterModuleInfo(nrr_info.as_mut_ptr(), nrr_image as _);
        if rc != 0 {
            println!("[smashline::loader] Failed to register NRR ({:#x})", rc);
            libc::free(nro_image as _);
            libc::free(nrr_image as _);
            return None;
        } else {
            println!("[smashline::loader] Loading development plugin...");
        }
        let nrr_info = nrr_info.assume_init();

        let bss_section = libc::memalign(0x1000, bss_size);
        let mut nro_module = MaybeUninit::uninit();
        let rc = ro::LoadModule(nro_module.as_mut_ptr(), nro_image, bss_section, bss_size as u64, ro::BindFlag_BindFlag_Now as i32);
        if rc == 0 {
            println!("[smashline::loader] Successfuly loaded development plugin");
        } else {
            println!("[smashline::loader] Failed to load development plugin ({:#x})", rc);
        }
        let nro_module = nro_module.assume_init();

        if rc == 0 {
            let bss_section = bss_section as *mut u8;
    
            Some(Self {
                // path,
                nro_module,
                nrr_info,
                // bss_section,
                // bss_size
            })
        } else {
            // we don't free on success since it's owned by RO now
            libc::free(bss_section as _);
            libc::free(nro_image as _);
            libc::free(nrr_image as _);
            None
        }

    }

    pub unsafe fn install(&self) {
        let mut install_fn = 0usize;
        let rc = ro::LookupModuleSymbol(&mut install_fn, &self.nro_module, c_str!("smashline_install"));
        if rc != 0 {
            panic!("Smashline development plugin does not export 'smashline_install'");
        } else {
            let callable: extern "Rust" fn() = std::mem::transmute(install_fn);
            callable()
        }
    }

    pub unsafe fn uninstall(&mut self) {
        let mut uninstall_fn = 0usize;
        let rc = ro::LookupModuleSymbol(&mut uninstall_fn, &self.nro_module, c_str!("smashline_uninstall"));
        
        if rc != 0 || uninstall_fn == 0 {
            println!("[smashline::loader] Development plugin does not export 'smashline_uninstall', continuing with default uninstallation.");
        } else {
            let callable: extern "Rust" fn() = std::mem::transmute(uninstall_fn);
            callable();
            println!("[smashline::loader] Development plugin's uninstall routine called, continuing with default uninstallation.");
        }

        let mem_info = crate::nx::svc::query_memory((*self.nro_module.ModuleObject).module_base as usize).expect("Smashline unable to query memory for development plugin on uninstall.");
        let range = (mem_info.mem_info.base_address, mem_info.mem_info.base_address + mem_info.mem_info.size);

        crate::callbacks::remove_fighter_resets(range);
        crate::callbacks::remove_agent_resets(range);
        crate::callbacks::remove_fighter_frame_callbacks(range);
        crate::callbacks::remove_weapon_frame_callbacks(range);
        crate::callbacks::remove_fighter_init_callbacks(range);
        crate::callbacks::remove_agent_init_callbacks(range);
        crate::acmd::remove_acmd_scripts(range);
        crate::status::remove_status_scripts(range);
        crate::unwind::unregister_skyline_plugin(range.0);

        println!("[smashline::loader] Unloading the development plugin...");
        ro::UnloadModule(&mut self.nro_module);
        UnregisterModuleInfo(&mut self.nrr_info);
    }
}

// The following code has been adapted by Skyline (https://github.com/skyline-dev/skyline/blob/master/source/skyline/plugin/PluginManager.cpp)
// Only one development plugin is allowed at a time
pub unsafe fn load_development_plugin() {
    let mut loaded = LOADED_DEVELOPMENT_PLUGIN.lock();
    if let Some(mut plugin) = loaded.take() {
        plugin.uninstall();
        std::mem::forget(plugin);
    }
    // hardcode the path here. would like to use rom but nnsdk caches the rom contents when it's mounted ig
    if let Some(plugin) = DevelopmentPlugin::new("sd:/atmosphere/contents/01006A800016E000/romfs/smashline/development.nro") {
        plugin.install();
        *loaded = Some(plugin);
    }
}