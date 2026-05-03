use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags,
    PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};
use crate::memory::frame_allocator::FRAME_ALLOCATOR;

/// Frame allocator wrapper for use with x86_64 crate's Mapper trait.
pub struct OxideFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for OxideFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        FRAME_ALLOCATOR
            .lock()
            .as_mut()
            .and_then(|alloc| alloc.allocate_frame())
    }
}

/// Initialize the OffsetPageTable using Limine's HHDM.
/// Safety: hhdm_offset must be correct and level 4 table must be valid.
pub unsafe fn init(hhdm_offset: u64) -> OffsetPageTable<'static> {
    let level_4_table = unsafe { active_level_4_table(hhdm_offset) };
    unsafe { OffsetPageTable::new(level_4_table, VirtAddr::new(hhdm_offset)) }
}

unsafe fn active_level_4_table(hhdm_offset: u64) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (frame, _flags) = Cr3::read();
    let phys = frame.start_address();
    let virt = VirtAddr::new(phys.as_u64() + hhdm_offset);
    let table: *mut PageTable = virt.as_mut_ptr();
    unsafe { &mut *table }
}

/// Allocate a new frame and map a virtual page to it.
pub fn alloc_and_map(
    mapper: &mut OffsetPageTable,
    page: Page<Size4KiB>,
    flags: PageTableFlags,
) -> PhysFrame<Size4KiB> {
    let frame = FRAME_ALLOCATOR
        .lock()
        .as_mut()
        .expect("frame allocator not initialized")
        .allocate_frame()
        .expect("out of physical memory");

    let mut frame_allocator = OxideFrameAllocator;
    unsafe {
        mapper
            .map_to(page, frame, flags, &mut frame_allocator)
            .expect("map_to failed")
            .flush();
    }
    frame
}
