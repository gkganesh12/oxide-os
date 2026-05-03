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

    // Set up IPC test: create capabilities and spawn tasks that communicate
    let (sender_cap, receiver_cap) = {
        use capability::{CAP_TABLE, PermissionBits, ResourceRef};
        let mut table = CAP_TABLE.lock();
        // Cap for task 1 (sender) to write to task 2 (receiver)
        let sc = table.create_root(1, ResourceRef::Agent(2), PermissionBits::WRITE.union(PermissionBits::DELEGATE));
        // Cap for task 2 (receiver) — can read
        let rc = table.create_root(2, ResourceRef::Agent(1), PermissionBits::READ);
        (sc, rc)
    };
    let _ = (sender_cap, receiver_cap); // Used by tasks via global knowledge of their IDs

    // Register mailboxes before spawning tasks
    ipc::message::register_mailbox(1);
    ipc::message::register_mailbox(2);
    ipc::message::register_mailbox(3);

    // Spawn tasks
    {
        use task::{Task, Priority};
        use task::scheduler::SCHEDULER;
        use alloc::string::String;

        let mut sched = SCHEDULER.lock();
        sched.spawn(Task::new(String::from("sender"), Priority::Normal, task_sender, &mut mapper));
        sched.spawn(Task::new(String::from("receiver"), Priority::Normal, task_receiver, &mut mapper));
        sched.spawn(Task::new(String::from("worker"), Priority::Background, task_worker, &mut mapper));
        sched.print_stats();
    }

    println!("[boot] Scheduler active — IPC test running");
    println!();

    // Idle loop — yields to tasks when reschedule is pending
    loop {
        x86_64::instructions::hlt(); // Sleep until next interrupt
        if task::scheduler::should_reschedule() {
            task::scheduler::yield_now();
        }
    }
}

// --- IPC Test Tasks ---

/// Sender task: sends messages to the receiver every ~200 ticks.
fn task_sender() -> ! {
    use ipc::{MessageType, message};

    // Get our capability to talk to task 2 (receiver)
    // Task 1's cap to write to task 2 is cap #1 (created in boot)
    let my_cap: u64 = 1;
    let mut msg_count = 0u32;

    // Wait a bit for receiver to start
    let mut warmup = 0u64;
    loop {
        warmup += 1;
        if warmup > 500_000 { break; }
        if task::scheduler::should_reschedule() { task::scheduler::yield_now(); }
    }

    loop {
        msg_count += 1;
        let payload = alloc::format!("Hello #{}", msg_count).into_bytes();

        match message::send(1, 2, MessageType::Data, payload, None, None, my_cap) {
            Ok(msg_id) => {
                println!("[sender] Sent message #{} (id={}) to receiver", msg_count, msg_id.0);
            }
            Err(e) => {
                println!("[sender] ERROR sending: {:?}", e);
            }
        }

        // Wait ~200 ticks before next message
        let target = interrupts::ticks() + 200;
        while interrupts::ticks() < target {
            if task::scheduler::should_reschedule() { task::scheduler::yield_now(); }
        }
    }
}

/// Receiver task: polls for messages and prints them.
fn task_receiver() -> ! {
    use ipc::message;

    let mut received = 0u32;

    loop {
        // Check for messages
        if let Some(msg) = message::receive(2) {
            received += 1;
            let text = core::str::from_utf8(&msg.payload).unwrap_or("<binary>");
            println!("[receiver] Got message from task {}: \"{}\" (total: {})", msg.sender, text, received);
        }

        if task::scheduler::should_reschedule() {
            task::scheduler::yield_now();
        }
    }
}

/// Background worker — just counts to prove Background priority works.
fn task_worker() -> ! {
    let mut count = 0u64;
    loop {
        count += 1;
        if count % 5_000_000 == 0 {
            println!("[worker BG] tick={} alive, iterations={}", interrupts::ticks(), count);
        }
        if task::scheduler::should_reschedule() {
            task::scheduler::yield_now();
        }
    }
}
