
// The following code is taken from the WIP skyline rust-rewrite to assist in symbol hooking
// the static modules

#[derive(Debug)]
#[repr(transparent)]
pub struct NxResult(u32);

#[repr(u32)]
#[derive(Copy, Clone)]
pub enum MemoryState {
    Free             = 0x00,
    Io               = 0x01,
    Static           = 0x02,
    Code             = 0x03,
    CodeData         = 0x04,
    Normal           = 0x05,
    Shared           = 0x06,
    Alias            = 0x07,
    AliasCode        = 0x08,
    AliasCodeData    = 0x09,
    Ipc              = 0x0A,
    Stack            = 0x0B,
    ThreadLocal      = 0x0C,
    Transfered       = 0x0D,
    SharedTransfered = 0x0E,
    SharedCode       = 0x0F,
    Inaccessible     = 0x10,
    NonSecureIpc     = 0x11,
    NonDeviceIpc     = 0x12,
    Kernel           = 0x13,
    GeneratedCode    = 0x14,
    CodeOut          = 0x15,
}

bitflags! {
    #[repr(C)]
    pub struct MemoryPermission : u32 {
        const NONE          = 0;
        const READ          = 1 << 0;
        const WRITE         = 1 << 1;
        const EXECUTE       = 1 << 2;
        const DONT_CARE     = 1 << 28;

        const READ_WRITE    = Self::READ.bits | Self::WRITE.bits;
        const READ_EXECUTE  = Self::READ.bits | Self::EXECUTE.bits;
    }
}

bitflags! {
    #[repr(C)]
    pub struct MemoryAttribute : u32 {
        const LOCKED        = 1 << 0;
        const IPC_LOCKED    = 1 << 1;
        const DEVICE_SHARED = 1 << 2;
        const UNCACHED      = 1 << 3;
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub struct MemoryInfo {
    pub base_address: usize,
    pub size: usize,
    pub state: MemoryState,
    pub attribute: MemoryAttribute,
    pub permission: MemoryPermission,
    pub device_refcount: u32,
    pub ipc_refcount: u32,
    pub padding: u32
}

#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct PageInfo {
    pub flags: u32
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct QueryMemoryResult {
    pub mem_info: MemoryInfo,
    pub page_info: PageInfo
}

use std::mem::MaybeUninit;

pub mod svc {
    use super::*;

    #[inline(always)]
    pub extern "C" fn query_memory(address: usize) -> Result<QueryMemoryResult, NxResult> {
        let res: NxResult;
        let mut mem_info: MemoryInfo = unsafe { std::mem::zeroed() };
        let svc_result = unsafe {
            let mem_info_ptr = &mut mem_info as *mut MemoryInfo;;
            let mut page_info: PageInfo = PageInfo { flags: 0 };
            asm!("svc 0x6" : "={w0}" (res), "={w1}" (page_info) : "{x0}" (mem_info_ptr), "{x2}" (address) : : "volatile");
            QueryMemoryResult {
                mem_info: mem_info,
                page_info
            }
        };
        match res.0 {
            0 => Ok(svc_result),
            _ => Err(res)
        }
    }
}