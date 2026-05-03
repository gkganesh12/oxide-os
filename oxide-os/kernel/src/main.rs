#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

extern crate alloc;

mod agent;
mod allocator;
mod apic;
mod capability;
mod gdt;
mod interrupts;
mod ipc;
mod memory;
mod net;
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
    println!("  ║        Oxide OS v0.5.0               ║");
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

    net::init(hhdm_offset);

    // Initialize APIC and timer
    apic::disable_pic();
    apic::init(hhdm_offset, &mut mapper);
    apic::configure_timer(interrupts::TIMER_VECTOR, 0x20000);
    x86_64::instructions::interrupts::enable();
    println!("[boot] APIC timer running, interrupts enabled");

    // ═══════════════════════════════════════════════════════════════════
    //  DEMO: Agent Swarm with Capabilities, IPC, and Supervision
    // ═══════════════════════════════════════════════════════════════════

    println!("[demo] Setting up agent swarm...");
    println!();

    // 1. Create capabilities for the agent swarm
    //    - Supervisor (task 1) can spawn and communicate with all agents
    //    - Research agents (tasks 2,3) can write to aggregator (task 4)
    //    - Aggregator (task 4) can publish to the "results" channel
    {
        use capability::{CAP_TABLE, PermissionBits, ResourceRef};
        use alloc::string::String;

        let mut table = CAP_TABLE.lock();

        // Supervisor caps
        table.create_root(1, ResourceRef::AgentSpawn,
            PermissionBits::SPAWN.union(PermissionBits::KILL).union(PermissionBits::DELEGATE));

        // Research agent 1 → can write to aggregator (task 4)
        table.create_root(2, ResourceRef::Agent(4), PermissionBits::WRITE);

        // Research agent 2 → can write to aggregator (task 4)
        table.create_root(3, ResourceRef::Agent(4), PermissionBits::WRITE);

        // Aggregator → can publish to "results" channel
        table.create_root(4, ResourceRef::Channel { name: String::from("results") },
            PermissionBits::PUBLISH);

        // Aggregator → can read from any agent
        table.create_root(4, ResourceRef::Agent(0), PermissionBits::READ);
    }

    // 2. Create pub/sub channel
    ipc::channel::create(alloc::string::String::from("results"));

    // 3. Register mailboxes
    for id in 1..=4 {
        ipc::message::register_mailbox(id);
    }

    // 4. Spawn agents using the agent lifecycle system
    {
        use agent::{AgentConfig, ModelBinding, RestartPolicy, ResourceLimits};
        use alloc::string::String;
        use alloc::vec;

        // Supervisor agent
        let supervisor_config = AgentConfig {
            name: String::from("supervisor"),
            system_prompt: Some(String::from("Manage the research swarm")),
            model: ModelBinding::Auto { preference: vec![String::from("gpt-4")] },
            tools: vec![],
            capabilities: vec![1], // spawn+kill cap
            restart_policy: RestartPolicy::Permanent,
            resource_limits: ResourceLimits::default(),
        };
        agent::lifecycle::spawn(None, supervisor_config, agent_supervisor, &mut mapper)
            .expect("failed to spawn supervisor");

        // Research agent 1
        let research1_config = AgentConfig {
            name: String::from("researcher-1"),
            system_prompt: Some(String::from("Research AI safety papers")),
            model: ModelBinding::Remote { endpoint: String::from("https://api.openai.com/v1/chat"), api_key_cap: 0 },
            tools: vec![String::from("web-fetch")],
            capabilities: vec![2], // write to aggregator
            restart_policy: RestartPolicy::RestartOne,
            resource_limits: ResourceLimits::default(),
        };
        agent::lifecycle::spawn(Some(1), research1_config, agent_researcher_1, &mut mapper)
            .expect("failed to spawn researcher-1");

        // Research agent 2
        let research2_config = AgentConfig {
            name: String::from("researcher-2"),
            system_prompt: Some(String::from("Research ML optimization techniques")),
            model: ModelBinding::Remote { endpoint: String::from("https://api.openai.com/v1/chat"), api_key_cap: 0 },
            tools: vec![String::from("web-fetch")],
            capabilities: vec![3], // write to aggregator
            restart_policy: RestartPolicy::RestartOne,
            resource_limits: ResourceLimits::default(),
        };
        agent::lifecycle::spawn(Some(1), research2_config, agent_researcher_2, &mut mapper)
            .expect("failed to spawn researcher-2");

        // Aggregator agent
        let aggregator_config = AgentConfig {
            name: String::from("aggregator"),
            system_prompt: Some(String::from("Collect and summarize research findings")),
            model: ModelBinding::Auto { preference: vec![String::from("gpt-4")] },
            tools: vec![],
            capabilities: vec![4, 5], // publish + read
            restart_policy: RestartPolicy::RestartOne,
            resource_limits: ResourceLimits::default(),
        };
        agent::lifecycle::spawn(Some(1), aggregator_config, agent_aggregator, &mut mapper)
            .expect("failed to spawn aggregator");
    }

    // 5. Print the agent tree
    println!();
    println!("[demo] Agent supervision tree:");
    agent::registry::REGISTRY.lock().print_tree();
    println!();

    // Print scheduler stats
    task::scheduler::SCHEDULER.lock().print_stats();
    println!();
    println!("[demo] Swarm running — agents communicating via IPC");
    println!("═══════════════════════════════════════════════════════════");
    println!();

    // Idle loop
    loop {
        x86_64::instructions::hlt();
        if task::scheduler::should_reschedule() {
            task::scheduler::yield_now();
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Agent Entry Functions
// ═══════════════════════════════════════════════════════════════════════

/// Supervisor agent — monitors the swarm, prints status periodically.
fn agent_supervisor() -> ! {
    println!("[supervisor] Online — monitoring swarm");
    let mut report_num = 0u32;

    loop {
        // Print status every ~500 ticks
        let target = interrupts::ticks() + 500;
        while interrupts::ticks() < target {
            if task::scheduler::should_reschedule() { task::scheduler::yield_now(); }
        }

        report_num += 1;
        let agent_count = agent::registry::REGISTRY.lock().count();
        let task_count = task::scheduler::SCHEDULER.lock().task_count();
        println!("[supervisor] Status #{}: {} agents, {} tasks, tick={}",
            report_num, agent_count, task_count, interrupts::ticks());
    }
}

/// Research agent 1 — simulates finding research results and sending to aggregator.
fn agent_researcher_1() -> ! {
    use ipc::{MessageType, message};

    println!("[researcher-1] Starting research on AI safety...");
    let my_cap: u64 = 2; // Cap to write to aggregator (task 4)
    let mut finding_num = 0u32;

    // Initial delay
    let target = interrupts::ticks() + 100;
    while interrupts::ticks() < target {
        if task::scheduler::should_reschedule() { task::scheduler::yield_now(); }
    }

    loop {
        finding_num += 1;
        let payload = alloc::format!(
            "{{\"agent\":\"researcher-1\",\"finding\":{},\"topic\":\"AI alignment\"}}",
            finding_num
        ).into_bytes();

        match message::send(2, 4, MessageType::Data, payload, None, None, my_cap) {
            Ok(_) => println!("[researcher-1] Sent finding #{} to aggregator", finding_num),
            Err(e) => println!("[researcher-1] Send error: {:?}", e),
        }

        // Research takes time...
        let target = interrupts::ticks() + 300;
        while interrupts::ticks() < target {
            if task::scheduler::should_reschedule() { task::scheduler::yield_now(); }
        }
    }
}

/// Research agent 2 — different research topic, same pattern.
fn agent_researcher_2() -> ! {
    use ipc::{MessageType, message};

    println!("[researcher-2] Starting research on ML optimization...");
    let my_cap: u64 = 3; // Cap to write to aggregator (task 4)
    let mut finding_num = 0u32;

    // Staggered start
    let target = interrupts::ticks() + 200;
    while interrupts::ticks() < target {
        if task::scheduler::should_reschedule() { task::scheduler::yield_now(); }
    }

    loop {
        finding_num += 1;
        let payload = alloc::format!(
            "{{\"agent\":\"researcher-2\",\"finding\":{},\"topic\":\"gradient optimization\"}}",
            finding_num
        ).into_bytes();

        match message::send(3, 4, MessageType::Data, payload, None, None, my_cap) {
            Ok(_) => println!("[researcher-2] Sent finding #{} to aggregator", finding_num),
            Err(e) => println!("[researcher-2] Send error: {:?}", e),
        }

        // Different research cadence
        let target = interrupts::ticks() + 400;
        while interrupts::ticks() < target {
            if task::scheduler::should_reschedule() { task::scheduler::yield_now(); }
        }
    }
}

/// Aggregator agent — collects findings from researchers, logs summaries.
fn agent_aggregator() -> ! {
    use ipc::message;

    println!("[aggregator] Online — waiting for research findings...");
    let mut total_findings = 0u32;

    loop {
        // Check for incoming messages from researchers
        while let Some(msg) = message::receive(4) {
            total_findings += 1;
            let text = core::str::from_utf8(&msg.payload).unwrap_or("<binary>");
            println!("[aggregator] Received finding #{} from task {}: {}",
                total_findings, msg.sender, text);

            if total_findings % 5 == 0 {
                println!("[aggregator] *** Summary: {} total findings collected ***", total_findings);
            }
        }

        if task::scheduler::should_reschedule() {
            task::scheduler::yield_now();
        }
    }
}
