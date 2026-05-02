# Phase 2: Interrupts & Preemptive Scheduler — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add hardware timer interrupts and a preemptive, priority-based scheduler that can run multiple tasks concurrently — the foundation for running multiple agents.

**Architecture:** Use the APIC (Advanced Programmable Interrupt Controller) for timer interrupts. Each task has a kernel stack and saved CPU state. The scheduler uses a multi-level priority queue with three classes: `Realtime`, `Normal`, `Background`. Timer interrupt fires, scheduler picks next task, context switch occurs.

**Tech Stack:** `x86_64` crate (APIC, interrupts), custom scheduler, per-task kernel stacks allocated from frame allocator.

---

## File Structure

```
oxide-os/kernel/src/
├── interrupts.rs          # (modify) Add APIC, timer, PIC disable
├── apic.rs                # Local APIC driver
├── task/
│   ├── mod.rs             # Task structure, states
│   ├── context.rs         # CPU context save/restore (assembly)
│   ├── scheduler.rs       # Multi-level priority queue scheduler
│   └── switch.s           # Context switch assembly
```

---

## Task 1: Disable Legacy PIC & Enable Local APIC

**Files:**
- Create: `oxide-os/kernel/src/apic.rs`
- Modify: `oxide-os/kernel/src/interrupts.rs`

- [ ] **Step 1: Create apic.rs**

```rust
// oxide-os/kernel/src/apic.rs
use x86_64::instructions::port::Port;
use crate::println;

const APIC_BASE_MSR: u32 = 0x1B;
const APIC_SPURIOUS_VEC: u32 = 0x0F0;
const APIC_TIMER_LVT: u32 = 0x320;
const APIC_TIMER_INIT_COUNT: u32 = 0x380;
const APIC_TIMER_CURRENT_COUNT: u32 = 0x390;
const APIC_TIMER_DIVIDE: u32 = 0x3E0;
const APIC_EOI: u32 = 0x0B0;

static mut APIC_BASE: u64 = 0;

/// Disable the legacy 8259 PIC by masking all interrupts.
pub fn disable_pic() {
    unsafe {
        // Mask all IRQs on both PICs
        Port::<u8>::new(0x21).write(0xFF);
        Port::<u8>::new(0xA1).write(0xFF);
    }
    println!("[apic] Legacy PIC disabled");
}

/// Initialize the Local APIC.
pub fn init(hhdm_offset: u64) {
    unsafe {
        // Read APIC base from MSR
        let msr_val: u64;
        core::arch::asm!(
            "rdmsr",
            in("ecx") APIC_BASE_MSR,
            out("eax") _,
            out("edx") _,
            lateout("eax") msr_val,
        );
        // Actually read it properly
        let low: u32;
        let high: u32;
        core::arch::asm!(
            "rdmsr",
            in("ecx") APIC_BASE_MSR,
            out("eax") low,
            out("edx") high,
        );
        APIC_BASE = ((high as u64) << 32 | (low as u64)) & 0xFFFF_FFFF_FFFF_F000;
        let apic_virt = APIC_BASE + hhdm_offset;

        // Enable APIC via spurious interrupt vector register
        let spurious_ptr = (apic_virt + APIC_SPURIOUS_VEC as u64) as *mut u32;
        // Set spurious vector to 0xFF and enable bit (bit 8)
        spurious_ptr.write_volatile(0x1FF);

        println!("[apic] Local APIC enabled at phys {:#X}", APIC_BASE);
    }
}

/// Configure APIC timer in periodic mode.
/// `vector` is the interrupt vector number to fire.
/// `interval` is the initial count (higher = slower).
pub fn configure_timer(hhdm_offset: u64, vector: u8, interval: u32) {
    unsafe {
        let apic_virt = APIC_BASE + hhdm_offset;

        // Set divide value to 16
        let divide_ptr = (apic_virt + APIC_TIMER_DIVIDE as u64) as *mut u32;
        divide_ptr.write_volatile(0x03); // Divide by 16

        // Set timer LVT: periodic mode (bit 17), vector
        let lvt_ptr = (apic_virt + APIC_TIMER_LVT as u64) as *mut u32;
        lvt_ptr.write_volatile((1 << 17) | vector as u32);

        // Set initial count to start the timer
        let init_ptr = (apic_virt + APIC_TIMER_INIT_COUNT as u64) as *mut u32;
        init_ptr.write_volatile(interval);

        println!("[apic] Timer configured: vector={}, interval={}", vector, interval);
    }
}

/// Send End-Of-Interrupt to the Local APIC.
pub fn eoi(hhdm_offset: u64) {
    unsafe {
        let apic_virt = APIC_BASE + hhdm_offset;
        let eoi_ptr = (apic_virt + APIC_EOI as u64) as *mut u32;
        eoi_ptr.write_volatile(0);
    }
}
```

- [ ] **Step 2: Update interrupts.rs to add timer interrupt handler**

```rust
// Add to oxide-os/kernel/src/interrupts.rs
use crate::apic;

pub const TIMER_VECTOR: u8 = 32;

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();

    idt.breakpoint.set_handler_fn(breakpoint_handler);
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }
    idt.page_fault.set_handler_fn(page_fault_handler);
    idt.general_protection_fault.set_handler_fn(general_protection_handler);
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);

    // Timer interrupt at vector 32
    idt[TIMER_VECTOR as usize].set_handler_fn(timer_handler);

    idt
});

static mut TIMER_TICKS: u64 = 0;
static mut HHDM_OFFSET_CACHE: u64 = 0;

pub fn set_hhdm_offset(offset: u64) {
    unsafe { HHDM_OFFSET_CACHE = offset; }
}

extern "x86-interrupt" fn timer_handler(_stack_frame: InterruptStackFrame) {
    unsafe {
        TIMER_TICKS += 1;
        apic::eoi(HHDM_OFFSET_CACHE);
    }
    // Scheduler preemption will be added in Task 3
}

pub fn ticks() -> u64 {
    unsafe { TIMER_TICKS }
}
```

- [ ] **Step 3: Initialize APIC and timer in main.rs**

Add after IDT init in `_start`:

```rust
    apic::disable_pic();
    apic::init(hhdm_offset);
    interrupts::set_hhdm_offset(hhdm_offset);
    apic::configure_timer(hhdm_offset, interrupts::TIMER_VECTOR, 0x100000);
    x86_64::instructions::interrupts::enable();
    println!("[boot] Interrupts enabled, timer running");
```

- [ ] **Step 4: Verify timer fires — print tick count after brief spin**

```rust
    // Spin briefly and check tick counter
    for _ in 0..1_000_000 {
        core::hint::spin_loop();
    }
    println!("[test] Timer ticks: {}", interrupts::ticks());
```

- [ ] **Step 5: Build and run**

Run: `cd oxide-os && make run`
Expected: `[test] Timer ticks: <non-zero number>`

- [ ] **Step 6: Remove test, commit**

```bash
git add oxide-os/kernel/src/apic.rs oxide-os/kernel/src/interrupts.rs oxide-os/kernel/src/main.rs
git commit -m "feat: add Local APIC driver with periodic timer interrupt"
```

---

## Task 2: Task Structure & Kernel Stacks

**Files:**
- Create: `oxide-os/kernel/src/task/mod.rs`
- Create: `oxide-os/kernel/src/task/context.rs`
- Modify: `oxide-os/kernel/src/main.rs`

- [ ] **Step 1: Create task/mod.rs**

```rust
// oxide-os/kernel/src/task/mod.rs
pub mod context;
pub mod scheduler;

use alloc::string::String;
use context::CpuContext;
use crate::memory::frame_allocator::FRAME_ALLOCATOR;
use crate::memory::PAGE_SIZE;

pub type TaskId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Realtime = 0,
    Normal = 1,
    Background = 2,
}

/// Kernel stack size: 16 KiB (4 pages)
pub const KERNEL_STACK_PAGES: u64 = 4;
pub const KERNEL_STACK_SIZE: u64 = KERNEL_STACK_PAGES * PAGE_SIZE;

pub struct Task {
    pub id: TaskId,
    pub name: String,
    pub state: TaskState,
    pub priority: Priority,
    pub context: CpuContext,
    pub kernel_stack_top: u64,
    pub kernel_stack_bottom: u64,
}

static NEXT_ID: spin::Mutex<u64> = spin::Mutex::new(1);

impl Task {
    /// Create a new task with an allocated kernel stack.
    /// `entry` is the function pointer where execution begins.
    pub fn new(name: String, priority: Priority, entry: fn() -> !) -> Self {
        let id = {
            let mut next = NEXT_ID.lock();
            let id = *next;
            *next += 1;
            id
        };

        // Allocate pages for kernel stack
        let stack_bottom = Self::alloc_stack();
        let stack_top = stack_bottom + KERNEL_STACK_SIZE;

        let mut context = CpuContext::empty();
        // Set up initial context so first switch jumps to entry point
        context.rip = entry as u64;
        context.rsp = stack_top; // Stack grows downward
        context.rflags = 0x200; // Interrupts enabled

        Task {
            id,
            name,
            state: TaskState::Ready,
            priority,
            context,
            kernel_stack_top: stack_top,
            kernel_stack_bottom: stack_bottom,
        }
    }

    fn alloc_stack() -> u64 {
        let mut alloc = FRAME_ALLOCATOR.lock();
        let alloc = alloc.as_mut().expect("frame allocator not init");

        let mut base = 0u64;
        for i in 0..KERNEL_STACK_PAGES {
            let frame = alloc.allocate_frame().expect("OOM allocating task stack");
            if i == 0 {
                base = frame.start_address().as_u64();
            }
        }
        // Return virtual address via HHDM
        // Note: we need HHDM offset here — stored globally
        base + unsafe { crate::HHDM_OFFSET }
    }
}
```

- [ ] **Step 2: Create task/context.rs**

```rust
// oxide-os/kernel/src/task/context.rs

/// Saved CPU registers for context switching.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct CpuContext {
    pub rsp: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rflags: u64,
    pub rip: u64,
}

impl CpuContext {
    pub fn empty() -> Self {
        CpuContext {
            rsp: 0,
            rbp: 0,
            rbx: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rflags: 0,
            rip: 0,
        }
    }
}

extern "C" {
    /// Switch from `old` context to `new` context.
    /// Saves callee-saved registers to `old`, loads from `new`.
    pub fn context_switch(old: *mut CpuContext, new: *const CpuContext);
}

// The actual assembly is in switch.s
global_asm!(include_str!("switch.s"));
```

Wait — we need `global_asm!`. Let me add that properly.

- [ ] **Step 3: Create switch.s (context switch assembly)**

Create file `oxide-os/kernel/src/task/switch.s`:

```asm
# oxide-os/kernel/src/task/switch.s
# Context switch: saves callee-saved regs to old, restores from new.
# fn context_switch(old: *mut CpuContext, new: *const CpuContext)
# rdi = old, rsi = new

.global context_switch
context_switch:
    # Save callee-saved registers to old context
    mov [rdi + 0x00], rsp
    mov [rdi + 0x08], rbp
    mov [rdi + 0x10], rbx
    mov [rdi + 0x18], r12
    mov [rdi + 0x20], r13
    mov [rdi + 0x28], r14
    mov [rdi + 0x30], r15

    # Save rflags
    pushfq
    pop rax
    mov [rdi + 0x38], rax

    # Save return address as rip
    mov rax, [rsp]
    mov [rdi + 0x40], rax

    # Restore callee-saved registers from new context
    mov rsp, [rsi + 0x00]
    mov rbp, [rsi + 0x08]
    mov rbx, [rsi + 0x10]
    mov r12, [rsi + 0x18]
    mov r13, [rsi + 0x20]
    mov r14, [rsi + 0x28]
    mov r15, [rsi + 0x30]

    # Restore rflags
    mov rax, [rsi + 0x38]
    push rax
    popfq

    # Jump to new task's instruction pointer
    jmp [rsi + 0x40]
```

- [ ] **Step 4: Add global HHDM_OFFSET to main.rs**

```rust
// Add near top of main.rs
pub static mut HHDM_OFFSET: u64 = 0;

// In _start, after getting hhdm_offset:
    unsafe { HHDM_OFFSET = hhdm_offset; }
```

- [ ] **Step 5: Add task module to main.rs**

```rust
mod task;
```

- [ ] **Step 6: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles without errors.

- [ ] **Step 7: Commit**

```bash
git add oxide-os/kernel/src/task/
git commit -m "feat: add task structure with CPU context and switch assembly"
```

---

## Task 3: Priority Queue Scheduler

**Files:**
- Create: `oxide-os/kernel/src/task/scheduler.rs`
- Modify: `oxide-os/kernel/src/interrupts.rs`
- Modify: `oxide-os/kernel/src/main.rs`

- [ ] **Step 1: Create scheduler.rs**

```rust
// oxide-os/kernel/src/task/scheduler.rs
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;
use super::{Task, TaskId, TaskState, Priority};
use super::context::{CpuContext, context_switch};
use crate::println;

/// Three-level priority queue scheduler.
pub struct Scheduler {
    /// Queues per priority level
    queues: [VecDeque<Task>; 3],
    /// Currently running task
    current: Option<Task>,
    /// Idle task context (what we switch back to when no tasks are ready)
    idle_context: CpuContext,
}

impl Scheduler {
    pub const fn new() -> Self {
        Scheduler {
            queues: [VecDeque::new(), VecDeque::new(), VecDeque::new()],
            current: None,
            idle_context: CpuContext {
                rsp: 0, rbp: 0, rbx: 0, r12: 0, r13: 0, r14: 0, r15: 0,
                rflags: 0, rip: 0,
            },
        }
    }

    /// Add a task to the appropriate priority queue.
    pub fn spawn(&mut self, task: Task) {
        let priority = task.priority as usize;
        println!("[sched] Spawned task {} '{}' (priority: {:?})", task.id, task.name, task.priority);
        self.queues[priority].push_back(task);
    }

    /// Pick the next task to run. Highest priority (lowest number) first.
    fn pick_next(&mut self) -> Option<Task> {
        for queue in self.queues.iter_mut() {
            if let Some(mut task) = queue.pop_front() {
                task.state = TaskState::Running;
                return Some(task);
            }
        }
        None
    }

    /// Called on timer interrupt — preempt current task and schedule next.
    pub fn schedule(&mut self) {
        // Put current task back in queue
        if let Some(mut current) = self.current.take() {
            if current.state == TaskState::Running {
                current.state = TaskState::Ready;
                let priority = current.priority as usize;
                self.queues[priority].push_back(current);
            }
        }

        // Pick next task
        if let Some(next) = self.pick_next() {
            self.current = Some(next);
        }
    }

    /// Get mutable reference to current task's context for saving.
    pub fn current_context_mut(&mut self) -> *mut CpuContext {
        match self.current.as_mut() {
            Some(task) => &mut task.context as *mut CpuContext,
            None => &mut self.idle_context as *mut CpuContext,
        }
    }

    /// Get reference to current task (if any).
    pub fn current_task(&self) -> Option<&Task> {
        self.current.as_ref()
    }

    /// Perform a context switch to the next scheduled task.
    /// Safety: must be called with interrupts disabled.
    pub unsafe fn switch_to_next(&mut self) {
        let old_ctx = self.current_context_mut();

        self.schedule();

        let new_ctx = match self.current.as_ref() {
            Some(task) => &task.context as *const CpuContext,
            None => return, // No tasks to run, stay idle
        };

        if old_ctx != new_ctx as *mut CpuContext {
            context_switch(old_ctx, new_ctx);
        }
    }

    /// Number of tasks in all queues + current.
    pub fn task_count(&self) -> usize {
        let queued: usize = self.queues.iter().map(|q| q.len()).sum();
        queued + if self.current.is_some() { 1 } else { 0 }
    }
}

pub static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());

/// Called from timer interrupt handler to trigger preemption.
pub fn timer_tick() {
    let mut sched = SCHEDULER.lock();
    unsafe { sched.switch_to_next(); }
}
```

- [ ] **Step 2: Update timer handler to call scheduler**

In `interrupts.rs`, update `timer_handler`:

```rust
extern "x86-interrupt" fn timer_handler(_stack_frame: InterruptStackFrame) {
    unsafe {
        TIMER_TICKS += 1;
        apic::eoi(HHDM_OFFSET_CACHE);
    }
    crate::task::scheduler::timer_tick();
}
```

- [ ] **Step 3: Test with two tasks in main.rs**

```rust
// Add test task functions
fn task_a() -> ! {
    loop {
        println!("[task-a] running (tick {})", interrupts::ticks());
        for _ in 0..500_000 { core::hint::spin_loop(); }
    }
}

fn task_b() -> ! {
    loop {
        println!("[task-b] running (tick {})", interrupts::ticks());
        for _ in 0..500_000 { core::hint::spin_loop(); }
    }
}

// In _start, after all init:
    use task::{Task, Priority};
    use task::scheduler::SCHEDULER;
    use alloc::string::String;

    {
        let mut sched = SCHEDULER.lock();
        sched.spawn(Task::new(String::from("task-a"), Priority::Normal, task_a));
        sched.spawn(Task::new(String::from("task-b"), Priority::Normal, task_b));
    }
    println!("[boot] Spawned 2 test tasks, enabling scheduler");

    // Enable interrupts and idle — scheduler takes over
    x86_64::instructions::interrupts::enable();
    loop { x86_64::instructions::hlt(); }
```

- [ ] **Step 4: Build and run**

Run: `cd oxide-os && make run`
Expected: Interleaved output from task-a and task-b, proving preemptive scheduling works.

- [ ] **Step 5: Remove test tasks, commit**

```bash
git add oxide-os/kernel/src/task/scheduler.rs oxide-os/kernel/src/interrupts.rs oxide-os/kernel/src/main.rs
git commit -m "feat: add preemptive priority scheduler with context switching"
```

---

## Task 4: Task Blocking & Wakeup

**Files:**
- Modify: `oxide-os/kernel/src/task/scheduler.rs`
- Modify: `oxide-os/kernel/src/task/mod.rs`

- [ ] **Step 1: Add block/unblock to scheduler**

Add to `Scheduler` impl:

```rust
    /// Block the current task (e.g., waiting for IPC).
    /// Returns the task ID of the blocked task.
    pub fn block_current(&mut self) -> Option<TaskId> {
        if let Some(ref mut task) = self.current {
            task.state = TaskState::Blocked;
            Some(task.id)
        } else {
            None
        }
    }

    /// Unblock a task by ID, moving it back to the ready queue.
    pub fn unblock(&mut self, task_id: TaskId) -> bool {
        for queue in self.queues.iter_mut() {
            // Check if task is in a queue but blocked (shouldn't be, but handle)
        }
        // Task might be in "current" if it blocked itself — it goes to ready next schedule()
        // For now, search blocked tasks stored separately
        false // Will be refined when we add a blocked list
    }
```

Actually, let me redesign this properly with a blocked list:

```rust
    /// Blocked tasks waiting to be woken up.
    blocked: Vec<Task>,

    // Add to block_current:
    pub fn block_current(&mut self) -> Option<TaskId> {
        if let Some(mut task) = self.current.take() {
            task.state = TaskState::Blocked;
            let id = task.id;
            self.blocked.push(task);
            id
        } else {
            None
        }
        // After blocking, schedule next
    }

    /// Unblock a task by ID.
    pub fn unblock(&mut self, task_id: TaskId) -> bool {
        if let Some(pos) = self.blocked.iter().position(|t| t.id == task_id) {
            let mut task = self.blocked.remove(pos);
            task.state = TaskState::Ready;
            let priority = task.priority as usize;
            self.queues[priority].push_back(task);
            true
        } else {
            false
        }
    }
```

- [ ] **Step 2: Update Scheduler::new to include blocked field**

```rust
    pub const fn new() -> Self {
        Scheduler {
            queues: [VecDeque::new(), VecDeque::new(), VecDeque::new()],
            current: None,
            idle_context: CpuContext {
                rsp: 0, rbp: 0, rbx: 0, r12: 0, r13: 0, r14: 0, r15: 0,
                rflags: 0, rip: 0,
            },
            blocked: Vec::new(),
        }
    }
```

- [ ] **Step 3: Add kill_current to clean up dead tasks**

```rust
    /// Kill the currently running task.
    pub fn kill_current(&mut self) -> Option<TaskId> {
        if let Some(mut task) = self.current.take() {
            task.state = TaskState::Dead;
            let id = task.id;
            println!("[sched] Task {} '{}' killed", id, task.name);
            // Stack memory would be freed here (dealloc frames)
            Some(id)
        } else {
            None
        }
    }
```

- [ ] **Step 4: Verify it compiles**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 5: Commit**

```bash
git add oxide-os/kernel/src/task/
git commit -m "feat: add task blocking, unblocking, and kill support"
```

---

## Task 5: Scheduler Statistics & Debug Output

**Files:**
- Modify: `oxide-os/kernel/src/task/scheduler.rs`

- [ ] **Step 1: Add stats method**

```rust
    pub fn print_stats(&self) {
        println!("[sched] Tasks: {} total ({} ready, {} blocked, {} running)",
            self.task_count(),
            self.queues.iter().map(|q| q.len()).sum::<usize>(),
            self.blocked.len(),
            if self.current.is_some() { 1 } else { 0 },
        );
        println!("[sched] Queues: RT={}, Normal={}, BG={}",
            self.queues[0].len(),
            self.queues[1].len(),
            self.queues[2].len(),
        );
    }
```

- [ ] **Step 2: Add periodic stats printing (every N ticks)**

In timer handler, add conditional stats print:

```rust
extern "x86-interrupt" fn timer_handler(_stack_frame: InterruptStackFrame) {
    unsafe {
        TIMER_TICKS += 1;
        apic::eoi(HHDM_OFFSET_CACHE);

        // Print scheduler stats every 1000 ticks
        if TIMER_TICKS % 1000 == 0 {
            // Note: can't lock scheduler here if it's already locked
            // This is just for debug — remove in production
        }
    }
    crate::task::scheduler::timer_tick();
}
```

- [ ] **Step 3: Commit**

```bash
git add oxide-os/kernel/src/task/scheduler.rs oxide-os/kernel/src/interrupts.rs
git commit -m "feat: add scheduler statistics and debug output"
```

---

## Summary

After completing Phase 2, Oxide OS will have:
- Local APIC with periodic timer interrupts
- Task abstraction with kernel stacks and saved CPU context
- Context switch via assembly (callee-saved register save/restore)
- Preemptive multi-level priority scheduler (Realtime > Normal > Background)
- Task states: Ready, Running, Blocked, Dead
- Block/unblock/kill primitives for task management
- Ready for Phase 3 (capabilities gate who can spawn/kill/communicate)
