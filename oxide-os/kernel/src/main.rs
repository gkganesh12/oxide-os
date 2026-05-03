#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod allocator;
mod gdt;
mod interrupts;
mod memory;
mod serial;

use core::panic::PanicInfo;
use limine::BaseRevision;
use limine::request::{HhdmRequest, MemmapRequest, StackSizeRequest};

/// Request markers and base revision for Limine bootloader protocol.
#[used]
#[unsafe(link_section = ".limine_requests_start")]
static LIMINE_REQUESTS_START: limine::RequestsStartMarker = limine::RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".limine_requests_end")]
static LIMINE_REQUESTS_END: limine::RequestsEndMarker = limine::RequestsEndMarker::new();

/// Base revision: request revision 3 (supported by Limine v8).
#[used]
#[unsafe(link_section = ".limine_requests")]
static BASE_REVISION: BaseRevision = BaseRevision::with_revision(3);

/// Request a 64 KiB stack.
#[used]
#[unsafe(link_section = ".limine_requests")]
static STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest::new(64 * 1024);

/// Request the Higher-Half Direct Map offset.
#[used]
#[unsafe(link_section = ".limine_requests")]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

/// Request the physical memory map.
#[used]
#[unsafe(link_section = ".limine_requests")]
static MEMMAP_REQUEST: MemmapRequest = MemmapRequest::new();

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("KERNEL PANIC: {}", info);
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    // Write to QEMU debug exit port to confirm kernel entry
    unsafe {
        core::arch::asm!("out dx, al", in("dx") 0xf4u16, in("al") 0x21u8);
    }

    // Initialize serial output first so we can print diagnostics.
    serial::init();

    println!("=== Oxide OS v0.1.0 ===");

    // Verify the bootloader supports our requested base revision.
    if !BASE_REVISION.is_supported() {
        println!("[boot] ERROR: Limine base revision not supported!");
        hcf();
    }
    println!("[boot] Limine base revision supported.");

    gdt::init();
    println!("[boot] GDT initialized.");

    interrupts::init();
    println!("[boot] IDT initialized.");

    // Initialize memory subsystem.
    let hhdm_offset = HHDM_REQUEST.response()
        .expect("HHDM response missing")
        .offset;

    if let Some(response) = MEMMAP_REQUEST.response() {
        let entries = response.entries();
        println!("[boot] Memory map: {} entries.", entries.len());
        memory::frame_allocator::init(entries, hhdm_offset);
    } else {
        panic!("Memory map not available");
    }

    let mut mapper = unsafe { memory::paging::init(hhdm_offset) };
    println!("[boot] Page tables initialized.");

    allocator::init(&mut mapper);

    println!("[boot] Kernel halted.");
    hcf();
}

/// Halt and catch fire: disable interrupts and loop forever.
fn hcf() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
