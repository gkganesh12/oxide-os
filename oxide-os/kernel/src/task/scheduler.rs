//! Multi-level priority scheduler with preemptive context switching.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;
use super::{Task, TaskId, TaskState};
use super::context::{CpuContext, context_switch};
use crate::println;

/// Fixed-size context storage. Contexts live here (not inside the Task struct)
/// so pointers remain stable when tasks move between queues.
const MAX_TASKS: usize = 64;

use core::cell::UnsafeCell;

#[repr(transparent)]
struct ContextArray(UnsafeCell<[CpuContext; MAX_TASKS]>);
unsafe impl Sync for ContextArray {}

#[repr(transparent)]
struct ContextCell(UnsafeCell<CpuContext>);
unsafe impl Sync for ContextCell {}

static CONTEXTS: ContextArray = ContextArray(UnsafeCell::new([CpuContext::empty(); MAX_TASKS]));
static IDLE_CONTEXT: ContextCell = ContextCell(UnsafeCell::new(CpuContext::empty()));

/// Three-level priority queue scheduler.
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

    /// Add a task to the ready queue. Stores initial context in fixed array.
    pub fn spawn(&mut self, task: Task) {
        println!("[sched] Spawned task {} '{}' ({:?})", task.id, task.name, task.priority);
        let id = task.id as usize;
        assert!(id < MAX_TASKS, "too many tasks");
        // Store initial context in fixed array
        unsafe { (*CONTEXTS.0.get())[id] = task.context; }
        let pri = task.priority as usize;
        self.queues[pri].push_back(task);
    }

    /// Pick the next highest-priority ready task.
    fn pick_next(&mut self) -> Option<Task> {
        for queue in self.queues.iter_mut() {
            if let Some(mut task) = queue.pop_front() {
                task.state = TaskState::Running;
                return Some(task);
            }
        }
        None
    }

    /// Reschedule: put current back in queue, pick next.
    pub fn schedule(&mut self) {
        if let Some(mut current) = self.current.take() {
            if current.state == TaskState::Running {
                current.state = TaskState::Ready;
                let pri = current.priority as usize;
                self.queues[pri].push_back(current);
            } else if current.state == TaskState::Blocked {
                self.blocked.push(current);
            }
        }
        self.current = self.pick_next();
    }

    /// Get the current task's ID (or 0 for idle).
    pub fn current_id(&self) -> usize {
        self.current.as_ref().map(|t| t.id as usize).unwrap_or(0)
    }

    /// Block the current task. It will be moved to the blocked list on next schedule.
    pub fn block_current(&mut self) -> Option<TaskId> {
        if let Some(ref mut task) = self.current {
            task.state = TaskState::Blocked;
            Some(task.id)
        } else {
            None
        }
    }

    /// Unblock a task by ID — move it from blocked to ready.
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

    /// Kill the current task.
    pub fn kill_current(&mut self) -> Option<TaskId> {
        if let Some(mut task) = self.current.take() {
            task.state = TaskState::Dead;
            let id = task.id;
            println!("[sched] Task {} '{}' killed", id, task.name);
            Some(id)
        } else {
            None
        }
    }

    /// Get current task info.
    pub fn current_task(&self) -> Option<&Task> {
        self.current.as_ref()
    }

    /// Total task count.
    pub fn task_count(&self) -> usize {
        let queued: usize = self.queues.iter().map(|q| q.len()).sum();
        queued + self.blocked.len() + if self.current.is_some() { 1 } else { 0 }
    }

    pub fn print_stats(&self) {
        println!("[sched] Tasks: {} (RT={}, Normal={}, BG={}, blocked={}, running={})",
            self.task_count(),
            self.queues[0].len(),
            self.queues[1].len(),
            self.queues[2].len(),
            self.blocked.len(),
            if self.current.is_some() { 1 } else { 0 },
        );
    }
}

pub static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());

/// Called from timer interrupt to trigger preemption.
pub fn timer_tick() {
    let (old_id, new_id) = {
        let mut sched = match SCHEDULER.try_lock() {
            Some(s) => s,
            None => return,
        };

        let old_id = sched.current_id();
        sched.schedule();
        let new_id = sched.current_id();

        if old_id == new_id || new_id == 0 {
            return; // Same task or no task
        }

        (old_id, new_id)
    };
    // Lock dropped — safe to switch

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
