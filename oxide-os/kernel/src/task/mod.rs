//! Task (thread) abstraction for the Oxide OS scheduler.

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

/// Kernel stack: 16 KiB (4 pages)
const KERNEL_STACK_PAGES: u64 = 4;
const KERNEL_STACK_SIZE: u64 = KERNEL_STACK_PAGES * PAGE_SIZE;

pub struct Task {
    pub id: TaskId,
    pub name: String,
    pub state: TaskState,
    pub priority: Priority,
    pub context: CpuContext,
    pub kernel_stack_bottom: u64,
    pub kernel_stack_top: u64,
}

static NEXT_ID: spin::Mutex<u64> = spin::Mutex::new(1);

impl Task {
    /// Create a new task. `entry` is where execution begins.
    /// `hhdm_offset` is needed to convert physical stack frames to virtual addresses.
    pub fn new(name: String, priority: Priority, entry: fn() -> !, hhdm_offset: u64) -> Self {
        let id = {
            let mut next = NEXT_ID.lock();
            let current = *next;
            *next += 1;
            current
        };

        // Allocate physical frames for kernel stack
        let stack_phys = {
            let mut alloc = FRAME_ALLOCATOR.lock();
            let alloc = alloc.as_mut().expect("frame allocator not init");
            let first = alloc.allocate_frame().expect("OOM: task stack");
            // Allocate remaining pages (they'll be contiguous-ish from bitmap)
            for _ in 1..KERNEL_STACK_PAGES {
                alloc.allocate_frame().expect("OOM: task stack");
            }
            first.start_address().as_u64()
        };

        let stack_bottom = stack_phys + hhdm_offset;
        let stack_top = stack_bottom + KERNEL_STACK_SIZE;

        // Set up initial context: when we switch to this task, it jumps to `entry`
        let context = CpuContext {
            rsp: stack_top,
            rbp: 0,
            rbx: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rip: entry as u64,
        };

        Task {
            id,
            name,
            state: TaskState::Ready,
            priority,
            context,
            kernel_stack_bottom: stack_bottom,
            kernel_stack_top: stack_top,
        }
    }
}
