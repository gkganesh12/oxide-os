use linked_list_allocator::LockedHeap;
use x86_64::structures::paging::{Page, PageTableFlags, OffsetPageTable, Size4KiB};
use x86_64::VirtAddr;
use crate::memory::paging;
use crate::println;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Kernel heap starts at this virtual address (chosen to avoid Limine's HHDM region).
pub const HEAP_START: u64 = 0xFFFF_A000_0000_0000;
/// Initial heap size: 1 MiB.
pub const HEAP_SIZE: u64 = 1024 * 1024;

/// Initialize the kernel heap by mapping pages and initializing the allocator.
pub fn init(mapper: &mut OffsetPageTable) {
    let heap_start = VirtAddr::new(HEAP_START);
    let heap_end = heap_start + HEAP_SIZE;

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

    let start_page = Page::<Size4KiB>::containing_address(heap_start);
    let end_page = Page::<Size4KiB>::containing_address(heap_end - 1u64);

    let mut page_count = 0u64;
    let mut current = start_page;
    while current <= end_page {
        paging::alloc_and_map(mapper, current, flags);
        current = Page::containing_address(current.start_address() + 4096u64);
        page_count += 1;
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE as usize);
    }

    println!(
        "[heap] Kernel heap initialized: {} KiB ({} pages) at {:#X}",
        HEAP_SIZE / 1024,
        page_count,
        HEAP_START
    );
}
