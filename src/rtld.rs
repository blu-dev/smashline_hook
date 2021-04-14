// This code has been adapted from Thog's oss-rtld implementation
// https://github.com/Thog/oss-rtld/

use skyline::{nn, libc};
use nnsdk::root::Elf64_Sym;
use nnsdk::root::rtld::ModuleObject;

use crate::c_str;
use crate::nx::{self, svc};

unsafe fn gen_rtld_elf_hash(mut name: *const libc::c_char) -> u32 {
    let mut elf_hash = 0u32;
    let mut g;
    loop {
        if *name == 0 {
            break;
        }

        elf_hash = (elf_hash << 4) + (*name as u32);
        name = name.add(1);
        g = elf_hash & 0xF0000000;
        if g != 0 {
            elf_hash ^= g >> 24;
        }

        elf_hash &= !g;
    }

    elf_hash 
}

pub unsafe fn get_symbol_by_name(module_object: *const ModuleObject, name: &str) -> *const Elf64_Sym {
    let module_object = &*module_object;
    let name = c_str!(name);
    let hash = gen_rtld_elf_hash(name) as u32;
    let mut i = *module_object.hash_bucket.offset((hash % (module_object.hash_nbucket_value as u32)) as isize);
    while i != 0 {
        let sym = &*module_object.dynsym.offset(i as isize);
        let mut is_common = true;
        if sym.st_shndx != 0 {
            is_common = sym.st_shndx == 0xFFF2;
        }
        if !is_common && libc::strcmp(name, module_object.dynstr.offset(sym.st_name as isize)) == 0 {
            return module_object.dynsym.offset(i as isize);
        }
        i = *module_object.hash_chain.offset(i as isize);
    }
    0 as _
}

pub unsafe fn get_symbol_by_resolved_address(module_object: *const ModuleObject, address: usize) -> *const Elf64_Sym {
    // this is slow and should be avoided at all costs
    let module_object = &*module_object;
    let difference = (address as u64) - module_object.module_base;
    for i in (0..module_object.hash_nbucket_value) {
        let mut j = *module_object.hash_bucket.offset(i as isize);
        while j != 0 {
            let sym = &*module_object.dynsym.offset(j as isize);
            let mut is_common = true;
            if sym.st_shndx != 0 {
                is_common = sym.st_shndx == 0xFFF2;
            }
            if !is_common && difference == sym.st_value {
                return sym as *const Elf64_Sym;
            }
            j = *module_object.hash_chain.offset(j as isize);
        }
    }
    0 as _
}

#[derive(Clone, Copy)]
struct Mod0Header {
    pub reserved: u32,
    pub mod0_offset: u32
}
#[derive(Clone, Copy)]
struct Mod0 {
    pub magic: u32,
    pub dynamic_offset: u32,
    pub bss_start_offset: u32,
    pub bss_end_offset: u32,
    pub unwind_start_offset: u32,
    pub unwind_end_offset: u32,
    pub module_object_offset: u32
}

pub unsafe fn get_module_object_from_address(address: usize) -> Result<*mut ModuleObject, nx::NxResult> {
    let queried_mem = svc::query_memory(address)?;
    let header = *(queried_mem.mem_info.base_address as *const Mod0Header);
    let mod0_addr = queried_mem.mem_info.base_address + header.mod0_offset as usize;
    let mod0 = *(mod0_addr as *const Mod0);
    assert!(mod0.magic == 0x30444f4d);
    let module_object = (mod0_addr + mod0.module_object_offset as usize) as *mut ModuleObject;
    Ok(module_object)
}