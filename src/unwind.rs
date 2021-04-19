use skyline::{nn, libc};
use skyline::hooks::InlineCtx;

use parking_lot::Mutex;

use crate::c_str;
use crate::nx::{self, svc};

const UNW_STEP_END: u64 = 0;
const UNW_STEP_SUCCESS: u64 = 1;

const _UA_SEARCH_PHASE: u64 = 1;

const _URC_FATAL_PHASE2_ERROR: u64 = 2;
const _URC_FATAL_PHASE1_ERROR: u64 = 3;
const _URC_HANDLER_FOUND: u64 = 6;
const _URC_INSTALL_CONTEXT: u64 = 7;

extern "C" {
    #[link_name = "\u{1}_Unwind_GetIP"]
    fn _Unwind_GetIP(context: *const u64) -> u64;
    #[link_name = "\u{1}_Unwind_SetIP"]
    fn _Unwind_SetIP(context: *const u64, ip: u64);
}

static STEP_WITH_DWARF_SEARCH_CODE: &[u8] = &[
    0xfc, 0x0f, 0x1a, 0xf8, 0xfa, 0x67, 0x01, 0xa9,
    0xf8, 0x5f, 0x02, 0xa9, 0xf6, 0x57, 0x03, 0xa9,
    0xf4, 0x4f, 0x04, 0xa9, 0xfd, 0x7b, 0x05, 0xa9,
    0xfd, 0x43, 0x01, 0x91, 0xff, 0x83, 0x22, 0xd1,
    0xe8, 0x03, 0x02, 0xaa, 0xf3, 0x03, 0x03, 0xaa,
    0xf5, 0x03, 0x01, 0xaa, 0xa2, 0x03, 0x02, 0xd1
];

static SET_INFO_BASED_ON_IP_SEARCH_CODE: &[u8] = &[
    0xfc, 0x57, 0xbd, 0xa9, 0xf4, 0x4f, 0x01, 0xa9,
    0xfd, 0x7b, 0x02, 0xa9, 0xfd, 0x83, 0x00, 0x91,
    0xff, 0x03, 0x1b, 0xd1, 0x08, 0x00, 0x40, 0xf9,
    0xf4, 0x03, 0x01, 0x2a
];

static UNWIND_CURSOR_STEP_SEARCH_CODE: &[u8] = &[
    0xf4, 0x4f, 0xbe, 0xa9, 0xfd, 0x7b, 0x01, 0xa9,
    0xfd, 0x43, 0x00, 0x91, 0x08, 0xa0, 0x49, 0x39,
    0xc8, 0x00, 0x00, 0x34
];

static BAD_INFO_CHECK_SEARCH_CODE: &[u8] = &[
    0x00, 0x01, 0x3f, 0xd6, 0x68, 0x02, 0x40, 0xf9,
    0xe0, 0x03, 0x13, 0xaa, 0xe1, 0x03, 0x1f, 0x2a,
    0x08, 0x35, 0x40, 0xf9, 0x00, 0x01, 0x3f, 0xd6
];

static mut STEP_WITH_DWARF: *const extern "C" fn(*mut u64, u64, *mut u64, *mut u64) -> u64 = 0 as _;
static mut SET_INFO_BASED_ON_IP_REGISTER: *const extern "C" fn(*mut u64, bool) = 0 as _;
static mut UNWIND_CURSOR_STEP_ADDRESS: usize = 0;
static mut BAD_INFO_CHECK_ADDRESS: usize = 0;

static OFFSET_INIT: std::sync::Once = std::sync::Once::new();
static CUSTOM_EH_MEM: Mutex<Vec<nx::QueryMemoryResult>> = Mutex::new(Vec::new());

#[skyline::hook(replace = libc::abort)]
fn abort_hook() -> ! {
    println!("[smashline::unwind | Fatal Error] abort() has been called. Flushing logger.");
    std::thread::sleep(std::time::Duration::from_millis(500));

    call_original!()
}

#[skyline::hook(replace = libc::fwrite)]
fn fwrite_hook(c_str: *const libc::c_char, size: libc::size_t, count: libc::size_t, file: *mut libc::c_void) -> libc::size_t {
    unsafe {
        print!("{}", skyline::from_c_str(c_str));
    }

    call_original!(c_str, size, count, file)
}

unsafe fn step_with_dwarf(address_space: *mut u64, ip: u64, unwind_info: *mut u64, registers: *mut u64) -> u64 {
    if STEP_WITH_DWARF.is_null() {
        panic!("step_with_dwarf is null.");
    }
    let callable: extern "C" fn(*mut u64, u64, *mut u64, *mut u64) -> u64 = std::mem::transmute(STEP_WITH_DWARF);
    callable(address_space, ip, unwind_info, registers)
}

unsafe fn set_info_based_on_ip_register(arg1: *mut u64, arg2: bool) {
    if SET_INFO_BASED_ON_IP_REGISTER.is_null() {
        panic!("set_info_based_on_ip_register is null.");
    }
    let callable: extern "C" fn(*mut u64, bool) = std::mem::transmute(SET_INFO_BASED_ON_IP_REGISTER);
    callable(arg1, arg2)
}

fn is_custom_eh_mem(ip: usize) -> bool {
    let custom_mems = CUSTOM_EH_MEM.lock();
    for mem in custom_mems.iter() {
        if mem.mem_info.base_address <= ip && ip < (mem.mem_info.base_address + mem.mem_info.size) {
            return true;
        }
    }
    false
}

unsafe fn byte_search(start: *const u32, want: u32, distance: usize) -> Option<*const u32> {
    for x in 0..distance {
        let cur = start.add(x);
        if *cur == want {
            return Some(cur);
        }
    }
    None
}

#[allow(unused_assignments)]
unsafe extern "C" fn custom_eh_personality(version: i32, actions: u64, _: u64, _: *mut u64, context: *mut u64) -> u64 {
    let mut ret = _URC_FATAL_PHASE1_ERROR;
    if version != 1 {
        panic!("Custom EH personality routine called in the wrong context.");
    }
    if actions & _UA_SEARCH_PHASE != 0 {
        ret = _URC_HANDLER_FOUND;
    } else {
        let ip = _Unwind_GetIP(context);
        if !is_custom_eh_mem(ip as usize) {
            panic!("Custom EH personality routine said it found an exception handler, but it is not inside a skyline plugin.");
        }
        let unwind_info_ptr = context.add(0x44);
        let landing_pad = *unwind_info_ptr.add(1) + 4;
        if !is_custom_eh_mem(landing_pad as usize) {
            panic!("Custom EH personality routine found landing pad, but it is not inside a skyline plugin.");
        }
        _Unwind_SetIP(context, landing_pad);
        ret = _URC_INSTALL_CONTEXT;
    }
    ret
}

#[skyline::hook(replace = UNWIND_CURSOR_STEP_ADDRESS)]
unsafe fn step_replace(this: *mut u64) -> u64 {
    if *(this as *const bool).offset(0x268) {
        return UNW_STEP_END;
    }

    let address_space = *this.add(1) as *mut u64;
    let ip = _Unwind_GetIP(this);
    let unwind_info = *this.add(0x4B) as *mut u64;
    let registers = this.add(2);

    let result = step_with_dwarf(address_space, ip, unwind_info, registers);
    if result == UNW_STEP_SUCCESS {
        let ip = _Unwind_GetIP(this) as usize;
        if is_custom_eh_mem(ip) {
            let pc_end = byte_search(ip as *const u32, 0xB000B1E5, 0x2000).expect("Stack unwinding passing through skyline plugin with no eh marker.");
            let unwind_info_ptr = this.add(0x44);
            *unwind_info_ptr.add(0) = ip as u64;
            *unwind_info_ptr.add(1) = pc_end as u64;
            *unwind_info_ptr.add(3) = std::mem::transmute(custom_eh_personality as *const ());
        } else {
            set_info_based_on_ip_register(this, true);
            if *(this as *const bool).offset(0x268) {
                return UNW_STEP_END;
            }
        }
    }

    result
}

#[skyline::hook(replace = BAD_INFO_CHECK_ADDRESS, inline)]
unsafe fn prevent_bad_info_check(ctx: &mut InlineCtx) {
    fn stub() {}
    let ip = _Unwind_GetIP(*ctx.registers[0].x.as_ref() as *const u64) as usize;
    if is_custom_eh_mem(ip) {
        *ctx.registers[8].x.as_mut() = std::mem::transmute(stub as *const ());
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[inline(never)]
pub fn register_skyline_plugin(addr: usize) {
    let mem_info = svc::query_memory(addr).expect("Unable to query memory for skyline plugin.");
    let mut custom_mem = CUSTOM_EH_MEM.lock();
    for plugin in custom_mem.iter() {
        if plugin.mem_info.base_address == mem_info.mem_info.base_address { 
            return;
        }
    }
    custom_mem.push(mem_info);
}

#[inline(never)]
pub fn unregister_skyline_plugin(base_addr: usize) {
    let mut custom_mem = CUSTOM_EH_MEM.lock();
    let mut new_mems = Vec::with_capacity(custom_mem.len());
    for plugin in custom_mem.iter() {
        if plugin.mem_info.base_address != base_addr {
            new_mems.push(*plugin);
        }
    }
    *custom_mem = new_mems;
}

pub fn install() {
    OFFSET_INIT.call_once(|| {
        unsafe {
            let mut unwind_resume = 0usize;
            let result = nn::ro::LookupSymbol(&mut unwind_resume, c_str!("_Unwind_Resume"));
            if result != 0 || unwind_resume == 0 {
                panic!("Failed to lookup symbol for \"_Unwind_Resume\"");
            }
    
            let nnsdk_memory = svc::query_memory(unwind_resume).expect("Failed to locate the start of nnSdk in memory.");
            let nnsdk_text_range = std::slice::from_raw_parts(nnsdk_memory.mem_info.base_address as *const u8, nnsdk_memory.mem_info.size);

            STEP_WITH_DWARF = (find_subsequence(nnsdk_text_range, STEP_WITH_DWARF_SEARCH_CODE).expect("Unable to locate stepWithDwarf in nnSdk.") + nnsdk_memory.mem_info.base_address) as _;
            SET_INFO_BASED_ON_IP_REGISTER = (find_subsequence(nnsdk_text_range, SET_INFO_BASED_ON_IP_SEARCH_CODE).expect("Unable to locate setInfoBasedOnIPRegister in nnSdk.") + nnsdk_memory.mem_info.base_address) as _;
            UNWIND_CURSOR_STEP_ADDRESS = find_subsequence(nnsdk_text_range, UNWIND_CURSOR_STEP_SEARCH_CODE).expect("Unable to locate UnwindCursor::Step in nnSdk.") + nnsdk_memory.mem_info.base_address;
            BAD_INFO_CHECK_ADDRESS = find_subsequence(nnsdk_text_range, BAD_INFO_CHECK_SEARCH_CODE).expect("Unable to locate badInfoCheck in nnSdk.") + nnsdk_memory.mem_info.base_address + 0x14;
        }
    });

    skyline::install_hooks!(
        abort_hook,
        fwrite_hook,
        step_replace,
        prevent_bad_info_check
    );
}