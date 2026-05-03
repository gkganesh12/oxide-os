#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

extern crate alloc;

mod allocator;
mod apic;
mod capability;
mod gdt;
mod interrupts;
mod ipc;
mod memory;
mod qemu;
mod serial;
mod task;

use core::panic::PanicInfo;
use limine::BaseRevision;
use limine::request::{HhdmRequest, MemmapRequest, StackSizeRequest};

#[used]
#[unsafe(link_section = ".limine_requests_start")]
static LIMINE_REQUESTS_START: limine::RequestsStartMarker = limine::RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".limine_requests_end")]
static LIMINE_REQUESTS_END: limine::RequestsEndMarker = limine::RequestsEndMarker::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static BASE_REVISION: BaseRevision = BaseRevision::with_revision(3);

#[used]
#[unsafe(link_section = ".limine_requests")]
static STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest::new(64 * 1024);

#[used]
#[unsafe(link_section = ".limine_requests")]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static MEMMAP_REQUEST: MemmapRequest = MemmapRequest::new();

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    panic_println!("KERNEL PANIC: {}", info);
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    serial::init();

    println!();
    println!("  ╔══════════════════════════════════════╗");
    println!("  ║        Oxide OS v0.1.0               ║");
    println!("  ║   Agent-Native Microkernel (Rust)    ║");
    println!("  ╚══════════════════════════════════════╝");
    println!();

    assert!(BASE_REVISION.is_supported());
    println!("[boot] Limine protocol OK");

    gdt::init();
    println!("[boot] GDT loaded");

    interrupts::init();
    println!("[boot] IDT loaded");

    let hhdm_offset = HHDM_REQUEST.response()
        .expect("HHDM response missing")
        .offset;

    if let Some(response) = MEMMAP_REQUEST.response() {
        memory::frame_allocator::init(response.entries(), hhdm_offset);
    } else {
        panic!("Memory map not available");
    }

    let mut mapper = unsafe { memory::paging::init(hhdm_offset) };
    println!("[boot] Page tables ready");

    allocator::init(&mut mapper);

    // Initialize APIC and timer
    apic::disable_pic();
    apic::init(hhdm_offset, &mut mapper);
    apic::configure_timer(interrupts::TIMER_VECTOR, 0x20000);
    x86_64::instructions::interrupts::enable();
    println!("[boot] APIC timer running, interrupts enabled");

    // Spawn tasks
    {
        use task::{Task, Priority};
        use task::scheduler::SCHEDULER;
        use alloc::string::String;

        let mut sched = SCHEDULER.lock();
        sched.spawn(Task::new(String::from("task-a"), Priority::Normal, task_a, &mut mapper));
        sched.spawn(Task::new(String::from("task-b"), Priority::Normal, task_b, &mut mapper));
        sched.spawn(Task::new(String::from("task-c"), Priority::Realtime, task_c, &mut mapper));
        sched.print_stats();
    }

    println!("[boot] Scheduler active");
    println!();

    // Idle loop — yields to tasks when reschedule is pending
    loop {
        x86_64::instructions::hlt(); // Sleep until next interrupt
        if task::scheduler::should_reschedule() {
            task::scheduler::yield_now();
        }
    }
}

// --- Test Tasks ---

fn task_a() -> ! {
    let mut count = 0u64;
    loop {
        count += 1;
        if count % 1_000_000 == 0 {
            println!("[task-a] tick={} iterations={}", interrupts::ticks(), count);
        }
        // Cooperative reschedule check — ensures preemption even without true interrupt-return patching
        if task::scheduler::should_reschedule() {
            task::scheduler::yield_now();
        }
    }
}

fn task_b() -> ! {
    let mut count = 0u64;
    loop {
        count += 1;
        if count % 1_000_000 == 0 {
            println!("[task-b] tick={} iterations={}", interrupts::ticks(), count);
        }
        if task::scheduler::should_reschedule() {
            task::scheduler::yield_now();
        }
    }
}

/// Realtime priority task — should run before Normal tasks when both are ready.
fn task_c() -> ! {
    let mut count = 0u64;
    loop {
        count += 1;
        if count % 2_000_000 == 0 {
            println!("[task-c RT] tick={} iterations={}", interrupts::ticks(), count);
        }
        if task::scheduler::should_reschedule() {
            task::scheduler::yield_now();
        }
    }
}
