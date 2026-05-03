#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod gdt;
mod interrupts;
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
fn panic(_info: &PanicInfo) -> ! {
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

    // Print memory map information.
    if let Some(response) = MEMMAP_REQUEST.response() {
        let entries = response.entries();
        let mut usable_bytes: u64 = 0;
        for entry in entries {
            if entry.type_ == limine::memmap::MEMMAP_USABLE {
                usable_bytes += entry.length;
            }
        }
        println!(
            "[boot] Memory map: {} entries, {} MiB usable.",
            entries.len(),
            usable_bytes / (1024 * 1024)
        );
    } else {
        println!("[boot] WARNING: No memory map response from bootloader.");
    }

    println!("[boot] Kernel halted.");
    hcf();
}

/// Halt and catch fire: disable interrupts and loop forever.
fn hcf() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
