use skyline::{nn, libc};
use nnsdk::root::Elf64_Sym;
use nnsdk::root::rtld::ModuleObject;

use crate::c_str;

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
        let sym = &mut *module_object.dynsym.offset(i as isize);
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