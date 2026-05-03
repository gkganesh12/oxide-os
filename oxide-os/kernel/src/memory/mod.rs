pub mod frame_allocator;

use x86_64::PhysAddr;

pub const PAGE_SIZE: u64 = 4096;

/// Convert a physical address to a virtual address using the HHDM offset.
pub fn phys_to_virt(phys: PhysAddr, hhdm_offset: u64) -> *mut u8 {
    (phys.as_u64() + hhdm_offset) as *mut u8
}
