use x86_64::structures::paging::PhysFrame;
use x86_64::PhysAddr;
use spin::Mutex;
use crate::memory::PAGE_SIZE;
use crate::println;

/// Bitmap-based physical frame allocator.
/// Each bit represents one 4 KiB frame: 0 = free, 1 = allocated.
pub struct BitmapFrameAllocator {
    bitmap: &'static mut [u8],
    total_frames: usize,
    next_free: usize,
}

impl BitmapFrameAllocator {
    /// Safety: bitmap_storage must point to valid memory of size total_frames/8.
    pub unsafe fn new(bitmap_storage: *mut u8, total_frames: usize) -> Self {
        let bitmap_size = (total_frames + 7) / 8;
        let bitmap = unsafe { core::slice::from_raw_parts_mut(bitmap_storage, bitmap_size) };
        // Mark all frames as allocated initially
        bitmap.fill(0xFF);

        BitmapFrameAllocator {
            bitmap,
            total_frames,
            next_free: 0,
        }
    }

    pub fn mark_region_free(&mut self, start_frame: usize, count: usize) {
        for i in start_frame..start_frame + count {
            if i < self.total_frames {
                self.clear_bit(i);
            }
        }
    }

    pub fn mark_region_used(&mut self, start_frame: usize, count: usize) {
        for i in start_frame..start_frame + count {
            if i < self.total_frames {
                self.set_bit(i);
            }
        }
    }

    pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
        for i in self.next_free..self.total_frames {
            if !self.is_set(i) {
                self.set_bit(i);
                self.next_free = i + 1;
                let addr = PhysAddr::new((i as u64) * PAGE_SIZE);
                return Some(PhysFrame::containing_address(addr));
            }
        }
        // Wrap around
        for i in 0..self.next_free {
            if !self.is_set(i) {
                self.set_bit(i);
                self.next_free = i + 1;
                let addr = PhysAddr::new((i as u64) * PAGE_SIZE);
                return Some(PhysFrame::containing_address(addr));
            }
        }
        None
    }

    pub fn free_frame(&mut self, frame: PhysFrame) {
        let index = frame.start_address().as_u64() as usize / PAGE_SIZE as usize;
        self.clear_bit(index);
        if index < self.next_free {
            self.next_free = index;
        }
    }

    pub fn stats(&self) -> (usize, usize) {
        let used: usize = self.bitmap.iter().map(|b| b.count_ones() as usize).sum();
        (self.total_frames - used, self.total_frames)
    }

    fn is_set(&self, index: usize) -> bool {
        let byte = index / 8;
        let bit = index % 8;
        (self.bitmap[byte] >> bit) & 1 == 1
    }

    fn set_bit(&mut self, index: usize) {
        let byte = index / 8;
        let bit = index % 8;
        self.bitmap[byte] |= 1 << bit;
    }

    fn clear_bit(&mut self, index: usize) {
        let byte = index / 8;
        let bit = index % 8;
        self.bitmap[byte] &= !(1 << bit);
    }
}

pub static FRAME_ALLOCATOR: Mutex<Option<BitmapFrameAllocator>> = Mutex::new(None);

/// Initialize the frame allocator from Limine memory map entries.
pub fn init(entries: &[&limine::memmap::Entry], hhdm_offset: u64) {
    // Only track up to the highest usable address (avoids huge bitmap for MMIO regions)
    let max_addr = entries
        .iter()
        .filter(|e| e.type_ == limine::memmap::MEMMAP_USABLE)
        .map(|entry| entry.base + entry.length)
        .max()
        .unwrap_or(0);

    let total_frames = (max_addr / PAGE_SIZE) as usize;
    let bitmap_size = (total_frames + 7) / 8;

    println!("[memory] Total physical frames: {} ({} MiB)", total_frames, total_frames * 4096 / 1024 / 1024);

    // Find a usable region large enough for the bitmap
    let bitmap_region = entries
        .iter()
        .filter(|e| e.type_ == limine::memmap::MEMMAP_USABLE)
        .find(|e| e.length as usize >= bitmap_size)
        .expect("No usable region large enough for frame allocator bitmap");

    let bitmap_phys = bitmap_region.base;
    let bitmap_virt = (bitmap_phys + hhdm_offset) as *mut u8;

    let mut allocator = unsafe { BitmapFrameAllocator::new(bitmap_virt, total_frames) };

    // Mark usable regions as free
    for entry in entries.iter() {
        if entry.type_ == limine::memmap::MEMMAP_USABLE {
            let start_frame = (entry.base / PAGE_SIZE) as usize;
            let count = (entry.length / PAGE_SIZE) as usize;
            allocator.mark_region_free(start_frame, count);
        }
    }

    // Mark the bitmap itself as used
    let bitmap_start_frame = (bitmap_phys / PAGE_SIZE) as usize;
    let bitmap_frame_count = ((bitmap_size as u64 + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
    allocator.mark_region_used(bitmap_start_frame, bitmap_frame_count);

    // Mark kernel and bootloader-reclaimable regions as used (protect kernel memory)
    for entry in entries.iter() {
        let is_protected = entry.type_ == limine::memmap::MEMMAP_BOOTLOADER_RECLAIMABLE
            || entry.type_ == limine::memmap::MEMMAP_EXECUTABLE_AND_MODULES;
        if is_protected {
            let start_frame = (entry.base / PAGE_SIZE) as usize;
            let count = (entry.length / PAGE_SIZE) as usize;
            if start_frame < total_frames {
                allocator.mark_region_used(start_frame, count);
            }
        }
    }

    let (free, total) = allocator.stats();
    println!("[memory] Frame allocator: {}/{} frames free ({} MiB free)", free, total, free * 4096 / 1024 / 1024);

    *FRAME_ALLOCATOR.lock() = Some(allocator);
}
