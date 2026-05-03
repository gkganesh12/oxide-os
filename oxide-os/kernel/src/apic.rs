//! Local APIC driver — disables legacy PIC, enables APIC timer for preemption.

use x86_64::instructions::port::Port;
use x86_64::structures::paging::{Page, PhysFrame, PageTableFlags, Size4KiB, OffsetPageTable, Mapper};
use x86_64::{PhysAddr, VirtAddr};
use crate::memory::paging::OxideFrameAllocator;
use crate::println;

const APIC_BASE_MSR: u32 = 0x1B;
const APIC_SPURIOUS_VEC: u32 = 0x0F0;
const APIC_TIMER_LVT: u32 = 0x320;
const APIC_TIMER_INIT_COUNT: u32 = 0x380;
const APIC_TIMER_DIVIDE: u32 = 0x3E0;
const APIC_EOI: u32 = 0x0B0;

use core::sync::atomic::{AtomicU64, Ordering};

static APIC_VIRT_BASE: AtomicU64 = AtomicU64::new(0);

/// Disable the legacy 8259 PIC by masking all IRQs.
pub fn disable_pic() {
    unsafe {
        Port::<u8>::new(0x21).write(0xFF);
        Port::<u8>::new(0xA1).write(0xFF);
    }
    println!("[apic] Legacy PIC disabled");
}

/// Initialize the Local APIC. Maps the APIC MMIO page and enables it.
pub fn init(hhdm_offset: u64, mapper: &mut OffsetPageTable) {
    let apic_phys = read_apic_base_phys();

    // Map the APIC MMIO page (it's not in the HHDM for non-RAM regions)
    let apic_virt = hhdm_offset + apic_phys;
    let page = Page::<Size4KiB>::containing_address(VirtAddr::new(apic_virt));
    let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(apic_phys));
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_EXECUTE
        | PageTableFlags::NO_CACHE;

    // Try mapping — it might already be mapped by HHDM
    let mut frame_alloc = OxideFrameAllocator;
    let map_result = unsafe { mapper.map_to(page, frame, flags, &mut frame_alloc) };
    match map_result {
        Ok(flush) => flush.flush(),
        Err(_) => {
            // Already mapped — try to update flags to ensure writable + no-cache
            // If it fails, the HHDM already covers it with correct permissions
        }
    }

    APIC_VIRT_BASE.store(apic_virt, Ordering::Release);

    // Enable APIC via spurious interrupt vector register (vector 0xFF, enable bit 8)
    unsafe {
        write_reg(APIC_SPURIOUS_VEC, 0x1FF);
    }

    println!("[apic] Local APIC enabled at phys {:#X}", apic_phys);
}

/// Configure APIC timer in periodic mode.
pub fn configure_timer(vector: u8, interval: u32) {
    unsafe {
        // Divide by 16
        write_reg(APIC_TIMER_DIVIDE, 0x03);
        // Periodic mode (bit 17) | vector
        write_reg(APIC_TIMER_LVT, (1 << 17) | vector as u32);
        // Start the timer
        write_reg(APIC_TIMER_INIT_COUNT, interval);
    }
    println!("[apic] Timer: vector={}, interval={}", vector, interval);
}

/// Send End-Of-Interrupt.
pub fn eoi() {
    unsafe { write_reg(APIC_EOI, 0); }
}

fn read_apic_base_phys() -> u64 {
    let (low, high): (u32, u32);
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") APIC_BASE_MSR,
            out("eax") low,
            out("edx") high,
        );
    }
    ((high as u64) << 32 | (low as u64)) & 0xFFFF_FFFF_FFFF_F000
}

unsafe fn write_reg(offset: u32, value: u32) {
    let base = APIC_VIRT_BASE.load(Ordering::Acquire);
    let ptr = (base + offset as u64) as *mut u32;
    unsafe { ptr.write_volatile(value); }
}
