//! Multi-level priority scheduler with preemptive context switching.
//!
//! Design:
//! - Timer interrupt sets NEED_RESCHEDULE flag and returns normally (iretq).
//! - After the interrupt returns, the task checks the flag and calls `yield_now()`.
//! - `yield_now()` performs the actual context switch at function-call level.
//! - This avoids leaking interrupt frames on the stack.
//!
//! For preemption of tasks that don't voluntarily check the flag, we inject
//! a check into the timer handler's return path. The interrupt modifies the
//! saved RIP on the stack to point to a trampoline that does the switch.
//!
//! Simpler approach (current): the timer handler directly calls schedule_next()
//! which does the context switch. The key insight is that we call it AFTER eoi
//! and the x86-interrupt handler's own register save/restore handles the interrupt
//! frame correctly — we save/restore the CALLEE-SAVED regs separately.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};
use super::{Task, TaskId, TaskState};
use super::context::{CpuContext, context_switch};
use crate::println;

// --- Context Storage ---
// Contexts are stored in a fixed array so pointers remain stable
// even when Task structs move between scheduler queues.
const MAX_TASKS: usize = 256;

#[repr(transparent)]
struct ContextStore(UnsafeCell<[CpuContext; MAX_TASKS]>);
unsafe impl Sync for ContextStore {}

#[repr(transparent)]
struct SingleContext(UnsafeCell<CpuContext>);
unsafe impl Sync for SingleContext {}

static CONTEXTS: ContextStore = ContextStore(UnsafeCell::new([CpuContext::empty(); MAX_TASKS]));
static IDLE_CONTEXT: SingleContext = SingleContext(UnsafeCell::new(CpuContext::empty()));

/// Flag: set by timer, cleared by yield_now
static NEED_RESCHEDULE: AtomicBool = AtomicBool::new(false);

// --- Scheduler ---

pub struct Scheduler {
    queues: [VecDeque<Task>; 3],
    current: Option<Task>,
    blocked: Vec<Task>,
}

impl Scheduler {
    pub const fn new() -> Self {
        Scheduler {
            queues: [VecDeque::new(), VecDeque::new(), VecDeque::new()],
            current: None,
            blocked: Vec::new(),
        }
    }

    /// Spawn a task. Stores its initial context in the fixed array.
    pub fn spawn(&mut self, task: Task) {
        let id = task.id as usize;
        assert!(id < MAX_TASKS, "task ID {} exceeds MAX_TASKS {}", id, MAX_TASKS);

        unsafe { (*CONTEXTS.0.get())[id] = task.context; }

        println!("[sched] Spawned task {} '{}' (priority: {:?})", task.id, task.name, task.priority);
        let pri = task.priority as usize;
        self.queues[pri].push_back(task);
    }

    /// Pick the next highest-priority ready task (strict priority).
    fn pick_next(&mut self) -> Option<Task> {
        for queue in self.queues.iter_mut() {
            if let Some(mut task) = queue.pop_front() {
                task.state = TaskState::Running;
                return Some(task);
            }
        }
        None
    }

    /// Fair pick: try to pick a task different from `last_id`.
    /// Searches all queues for a different task first. Falls back to same task
    /// only if it's the only one ready.
    fn pick_next_fair(&mut self, last_id: usize) -> Option<Task> {
        // First pass: find any task that isn't `last_id`
        for queue in self.queues.iter_mut() {
            // Find first task in this queue that isn't last_id
            if let Some(pos) = queue.iter().position(|t| t.id as usize != last_id) {
                let mut task = queue.remove(pos).unwrap();
                task.state = TaskState::Running;
                return Some(task);
            }
        }
        // Second pass: all ready tasks are `last_id` (or no tasks). Pick normally.
        self.pick_next()
    }

    /// Put current task back and pick next. Returns (old_id, new_id).
    fn reschedule(&mut self) -> (usize, usize) {
        let old_id = self.current.as_ref().map(|t| t.id as usize).unwrap_or(0);

        // Put current back into appropriate queue
        if let Some(mut current) = self.current.take() {
            match current.state {
                TaskState::Running => {
                    current.state = TaskState::Ready;
                    let pri = current.priority as usize;
                    self.queues[pri].push_back(current);
                }
                TaskState::Blocked => {
                    self.blocked.push(current);
                }
                TaskState::Dead => {
                    println!("[sched] Task {} '{}' cleaned up (freed {} frames)",
                        current.id, current.name, current.stack_frames.len());
                    super::cleanup_dead_task(&current);
                    // Task is dropped here, freeing heap allocations (name, capabilities vec)
                }
                TaskState::Ready => {
                    let pri = current.priority as usize;
                    self.queues[pri].push_back(current);
                }
            }
        }

        // Pick next task. Use round-robin across ALL priority levels:
        // each yield rotates to the next ready task regardless of priority.
        // Priority is respected at spawn time (RT tasks go first initially)
        // and when multiple tasks become ready simultaneously.
        self.current = self.pick_next_fair(old_id);

        let new_id = self.current.as_ref().map(|t| t.id as usize).unwrap_or(0);
        (old_id, new_id)
    }

    /// Block the current task and immediately yield CPU.
    pub fn block_current(&mut self) {
        if let Some(ref mut task) = self.current {
            task.state = TaskState::Blocked;
        }
        // The actual context switch happens when we call do_switch after dropping lock
    }

    /// Unblock a task by ID — move from blocked to ready queue.
    pub fn unblock(&mut self, task_id: TaskId) -> bool {
        if let Some(pos) = self.blocked.iter().position(|t| t.id == task_id) {
            let mut task = self.blocked.remove(pos);
            task.state = TaskState::Ready;
            let pri = task.priority as usize;
            self.queues[pri].push_back(task);
            true
        } else {
            false
        }
    }

    /// Kill the current task. It will be cleaned up on next reschedule.
    pub fn kill_current(&mut self) {
        if let Some(ref mut task) = self.current {
            task.state = TaskState::Dead;
        }
    }

    /// Get current task's ID.
    pub fn current_id(&self) -> usize {
        self.current.as_ref().map(|t| t.id as usize).unwrap_or(0)
    }

    /// Get current task reference.
    pub fn current_task(&self) -> Option<&Task> {
        self.current.as_ref()
    }

    /// Total active tasks.
    pub fn task_count(&self) -> usize {
        let queued: usize = self.queues.iter().map(|q| q.len()).sum();
        queued + self.blocked.len() + if self.current.is_some() { 1 } else { 0 }
    }

    pub fn print_stats(&self) {
        println!("[sched] Tasks: {} (RT={}, Normal={}, BG={}, blocked={}, current={})",
            self.task_count(),
            self.queues[0].len(),
            self.queues[1].len(),
            self.queues[2].len(),
            self.blocked.len(),
            self.current.as_ref().map(|t| t.id).unwrap_or(0),
        );
    }
}

pub static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());

// --- Public API ---

/// Called from timer interrupt. Signals that a reschedule is needed.
/// Does NOT perform the context switch inside the interrupt handler.
pub fn timer_tick() {
    NEED_RESCHEDULE.store(true, Ordering::Release);
}

/// Voluntary yield — call this when:
/// - A task wants to give up its time slice
/// - After blocking the current task
/// - After killing the current task
/// - From the idle loop when a reschedule is pending
///
/// This performs the actual context switch at function-call level (not inside ISR).
pub fn yield_now() {
    // Disable interrupts during the switch to prevent nested scheduling
    x86_64::instructions::interrupts::disable();

    NEED_RESCHEDULE.store(false, Ordering::Release);

    let (old_id, new_id) = {
        let mut sched = SCHEDULER.lock();
        let result = sched.reschedule();
        result
    };
    // Lock is dropped here

    if old_id != new_id && new_id != 0 {
        unsafe {
            let old_ctx = if old_id == 0 {
                IDLE_CONTEXT.0.get() as *mut CpuContext
            } else {
                &mut (*CONTEXTS.0.get())[old_id] as *mut CpuContext
            };
            let new_ctx = &(*CONTEXTS.0.get())[new_id] as *const CpuContext;
            context_switch(old_ctx, new_ctx);
        }
    }

    // Re-enable interrupts (we're back on this task's stack now)
    x86_64::instructions::interrupts::enable();
}

/// Check if a reschedule is pending (called from idle loop and cooperative points).
pub fn should_reschedule() -> bool {
    NEED_RESCHEDULE.load(Ordering::Acquire)
}

/// Block current task and yield immediately.
pub fn block_and_yield() {
    x86_64::instructions::interrupts::disable();
    {
        let mut sched = SCHEDULER.lock();
        sched.block_current();
    }
    yield_now(); // yield_now will re-enable interrupts
}

/// Kill current task and yield immediately.
pub fn exit_current() -> ! {
    x86_64::instructions::interrupts::disable();
    {
        let mut sched = SCHEDULER.lock();
        sched.kill_current();
    }
    yield_now();
    // Should never reach here — we switched away and this task is dead
    unreachable!("dead task resumed");
}
