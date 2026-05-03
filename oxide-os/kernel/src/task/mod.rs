//! Task (thread) abstraction for the Oxide OS scheduler.
//!
//! Each task has:
//! - A unique ID
//! - Its own kernel stack (contiguous, with a guard page)
//! - A saved CPU context for context switching
//! - Priority level and scheduling state

pub mod context;
pub mod scheduler;

use alloc::string::String;
use alloc::vec::Vec;
use context::CpuContext;
use x86_64::structures::paging::{Page, PageTableFlags, OffsetPageTable, Size4KiB};
use x86_64::VirtAddr;
use crate::capability::CapId;
use crate::memory::frame_allocator::FRAME_ALLOCATOR;
use crate::memory::paging;
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

/// Kernel stack size: 16 KiB (4 pages) + 1 guard page
const STACK_PAGES: u64 = 4;
const STACK_SIZE: u64 = STACK_PAGES * PAGE_SIZE;
/// Virtual address region for task stacks. Each task gets (1 guard + 4 stack) pages.
/// Task N's stack lives at STACK_REGION_BASE + N * STACK_SLOT_SIZE
const STACK_REGION_BASE: u64 = 0xFFFF_B000_0000_0000;
const STACK_SLOT_SIZE: u64 = (STACK_PAGES + 1) * PAGE_SIZE; // guard + stack

pub struct Task {
    pub id: TaskId,
    pub name: String,
    pub state: TaskState,
    pub priority: Priority,
    pub context: CpuContext,
    pub stack_top: u64,
    /// Physical frame addresses used for this task's stack (for freeing on death).
    pub stack_frames: Vec<u64>,
    pub capabilities: Vec<CapId>,
}

/// Free a dead task's resources (stack frames, mailbox, task ID).
/// Called by the scheduler when cleaning up a Dead task.
pub fn cleanup_dead_task(task: &Task) {
    use x86_64::structures::paging::PhysFrame;
    use x86_64::PhysAddr;

    // Return stack frames to allocator
    {
        let mut alloc = FRAME_ALLOCATOR.lock();
        if let Some(alloc) = alloc.as_mut() {
            for &phys_addr in &task.stack_frames {
                let frame = PhysFrame::containing_address(PhysAddr::new(phys_addr));
                alloc.free_frame(frame);
            }
        }
    }

    // Unregister mailbox
    crate::ipc::message::unregister_mailbox(task.id);

    // Recycle the task ID
    ID_ALLOCATOR.lock().free(task.id);
}

/// Task ID allocator with recycling. IDs are reused after tasks die.
struct IdAllocator {
    next: u64,
    free_list: Vec<u64>,
}

impl IdAllocator {
    const fn new() -> Self {
        IdAllocator { next: 1, free_list: Vec::new() }
    }

    fn alloc(&mut self) -> u64 {
        if let Some(id) = self.free_list.pop() {
            id
        } else {
            let id = self.next;
            self.next += 1;
            id
        }
    }

    fn free(&mut self, id: u64) {
        self.free_list.push(id);
    }
}

static ID_ALLOCATOR: spin::Mutex<IdAllocator> = spin::Mutex::new(IdAllocator::new());

impl Task {
    /// Create a new task with a properly mapped, contiguous kernel stack.
    ///
    /// The stack layout for task N:
    /// ```text
    /// [guard page - unmapped] [stack page 0] [stack page 1] [stack page 2] [stack page 3]
    ///                         ^                                                            ^
    ///                    stack_bottom                                                  stack_top (RSP starts here)
    /// ```
    pub fn new(
        name: String,
        priority: Priority,
        entry: fn() -> !,
        mapper: &mut OffsetPageTable,
    ) -> Self {
        let id = ID_ALLOCATOR.lock().alloc();

        // Calculate this task's stack virtual address
        let slot_base = STACK_REGION_BASE + (id as u64) * STACK_SLOT_SIZE;
        // First page is the guard page (NOT mapped — access causes page fault)
        let stack_bottom = slot_base + PAGE_SIZE; // Skip guard page
        let stack_top = stack_bottom + STACK_SIZE;

        // Map stack pages (contiguous in virtual address space)
        let flags = PageTableFlags::PRESENT
            | PageTableFlags::WRITABLE
            | PageTableFlags::NO_EXECUTE;

        let mut stack_frames = Vec::with_capacity(STACK_PAGES as usize);
        for i in 0..STACK_PAGES {
            let page_addr = VirtAddr::new(stack_bottom + i * PAGE_SIZE);
            let page = Page::<Size4KiB>::containing_address(page_addr);
            let frame = paging::alloc_and_map(mapper, page, flags);
            stack_frames.push(frame.start_address().as_u64());
        }
        // Guard page is intentionally NOT mapped — any access triggers page fault

        // Set up initial context.
        // The task's initial RIP points to `task_entry_trampoline`.
        // The actual entry function pointer is stored in R12 (callee-saved,
        // survives context_switch). The trampoline enables interrupts and
        // then calls the real entry function.
        let context = CpuContext {
            rsp: stack_top,
            rbp: 0,
            rbx: 0,
            r12: entry as u64, // Real entry fn stored here
            r13: 0,
            r14: 0,
            r15: 0,
            rip: task_entry_trampoline as u64,
        };

        Task {
            id,
            name,
            state: TaskState::Ready,
            priority,
            context,
            stack_top,
            stack_frames,
            capabilities: Vec::new(),
        }
    }

    /// Check if this task holds a specific capability.
    pub fn has_capability(&self, cap_id: CapId) -> bool {
        self.capabilities.contains(&cap_id)
    }

    /// Grant a capability to this task.
    pub fn grant_capability(&mut self, cap_id: CapId) {
        if !self.capabilities.contains(&cap_id) {
            self.capabilities.push(cap_id);
        }
    }

    /// Remove a capability from this task's set.
    pub fn revoke_capability(&mut self, cap_id: CapId) {
        self.capabilities.retain(|&id| id != cap_id);
    }

    /// Remove all revoked capabilities from the task's set (garbage collection).
    pub fn gc_capabilities(&mut self) {
        use crate::capability::CAP_TABLE;
        let table = CAP_TABLE.lock();
        self.capabilities.retain(|&id| table.get(id).is_ok());
    }
}

/// Trampoline for fresh tasks. Called by context_switch when a task runs for
/// the first time. Enables interrupts (which were disabled by yield_now)
/// then calls the real entry function stored in R12.
#[unsafe(naked)]
extern "C" fn task_entry_trampoline() -> ! {
    core::arch::naked_asm!(
        "sti",          // Enable interrupts
        "call r12",     // Call the real entry function (stored by Task::new)
        "ud2",          // Should never return — entry is fn() -> !
    );
}
