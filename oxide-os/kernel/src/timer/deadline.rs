use alloc::collections::BinaryHeap;
use alloc::vec::Vec;
use core::cmp::Ordering;
use spin::Mutex;
use crate::task::TaskId;
use crate::task::scheduler::SCHEDULER;
use crate::println;
use core::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

static NEXT_CALLBACK: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DeadlineEntry {
    pub deadline_tick: u64,
    pub task_id: TaskId,
    pub callback_id: u64,
}

// Min-heap: earliest deadline first
impl Ord for DeadlineEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other.deadline_tick.cmp(&self.deadline_tick)
    }
}
impl PartialOrd for DeadlineEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct DeadlineQueue {
    heap: BinaryHeap<DeadlineEntry>,
}

impl DeadlineQueue {
    pub const fn new() -> Self {
        DeadlineQueue { heap: BinaryHeap::new() }
    }

    pub fn schedule(&mut self, task_id: TaskId, deadline_tick: u64) -> u64 {
        let id = NEXT_CALLBACK.fetch_add(1, AtomicOrdering::Relaxed);
        self.heap.push(DeadlineEntry { deadline_tick, task_id, callback_id: id });
        id
    }

    pub fn cancel(&mut self, callback_id: u64) {
        let entries: Vec<DeadlineEntry> = self.heap.drain().filter(|e| e.callback_id != callback_id).collect();
        for e in entries {
            self.heap.push(e);
        }
    }

    pub fn check_expired(&mut self, current_tick: u64) -> usize {
        let mut fired = 0;
        while let Some(entry) = self.heap.peek() {
            if entry.deadline_tick > current_tick {
                break;
            }
            let entry = self.heap.pop().unwrap();
            if let Some(mut sched) = SCHEDULER.try_lock() {
                sched.unblock(entry.task_id);
            }
            fired += 1;
        }
        fired
    }

    pub fn pending_count(&self) -> usize {
        self.heap.len()
    }
}

pub static DEADLINES: Mutex<DeadlineQueue> = Mutex::new(DeadlineQueue::new());

pub fn schedule(task_id: TaskId, deadline_tick: u64) -> u64 {
    DEADLINES.lock().schedule(task_id, deadline_tick)
}

pub fn cancel(callback_id: u64) {
    DEADLINES.lock().cancel(callback_id);
}

/// Called from timer interrupt to fire expired deadlines.
pub fn tick(current_tick: u64) {
    if let Some(mut q) = DEADLINES.try_lock() {
        q.check_expired(current_tick);
    }
}

pub fn init() {
    println!("[timer] Deadline queue initialized");
}
