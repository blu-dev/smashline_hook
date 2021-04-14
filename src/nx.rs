
// The following code is taken from the WIP skyline rust-rewrite to assist in symbol hooking
// the static modules

#[derive(Debug)]
#[repr(transparent)]
pub struct NxResult(u32);

#[repr(u32)]
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
    pub struct MemoryAttribute : u32 {
        const LOCKED        = 1 << 0;
        const IPC_LOCKED    = 1 << 1;
        const DEVICE_SHARED = 1 << 2;
        const UNCACHED      = 1 << 3;
    }
}

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
pub struct PageInfo {
    pub flags: u32
}

pub struct QueryMemoryResult {
    pub mem_info: MemoryInfo,
    pub page_info: PageInfo
}

use std::mem::MaybeUninit;

pub mod svc {
    use super::*;
    pub fn query_memory(address: usize) -> Result<QueryMemoryResult, NxResult> {
        let res: NxResult;
        let mem_info = MaybeUninit::<MemoryInfo>::uninit();
        let svc_result = unsafe {
            let mem_info_ptr = mem_info.as_ptr();
            let page_info: PageInfo;
            asm!(
                "svc 0x6"
                : "={w0}" (res), "={w1}" (page_info)
                : "{x0}" (mem_info_ptr), "{x2}" (address)
            );
            QueryMemoryResult {
                mem_info: mem_info.assume_init(),
                page_info
            }
        };
        match res.0 {
            0 => Ok(svc_result),
            _ => Err(res)
        }
    }
}