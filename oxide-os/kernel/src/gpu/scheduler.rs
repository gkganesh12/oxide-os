use alloc::collections::BinaryHeap;
use alloc::vec::Vec;
use core::cmp::Ordering;
use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use crate::agent::AgentId;
use crate::capability::{CapId, CAP_TABLE, PermissionBits};
use crate::println;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferencePriority {
    Urgent = 0,
    Normal = 1,
    Batch = 2,
}

pub type RequestId = u64;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InferenceRequest {
    pub id: RequestId,
    pub agent_id: AgentId,
    pub model_id: alloc::string::String,
    pub priority: InferencePriority,
    pub deadline_tick: Option<u64>,
    pub submitted_tick: u64,
}

impl Ord for InferenceRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        // Lower priority number = higher priority. Tie-break by deadline then submit time.
        (self.priority as u8).cmp(&(other.priority as u8))
            .then_with(|| match (self.deadline_tick, other.deadline_tick) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => self.submitted_tick.cmp(&other.submitted_tick),
            })
            .reverse() // BinaryHeap is max-heap, we want min
    }
}

impl PartialOrd for InferenceRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub struct GpuScheduler {
    queue: BinaryHeap<InferenceRequest>,
    current: Option<InferenceRequest>,
    total_completed: u64,
    total_failed: u64,
}

impl GpuScheduler {
    pub const fn new() -> Self {
        GpuScheduler { queue: BinaryHeap::new(), current: None, total_completed: 0, total_failed: 0 }
    }

    pub fn submit(&mut self, agent_id: AgentId, model_id: alloc::string::String, priority: InferencePriority, deadline_tick: Option<u64>) -> RequestId {
        let id = NEXT_REQUEST_ID.fetch_add(1, AtomicOrdering::Relaxed);
        self.queue.push(InferenceRequest {
            id, agent_id, model_id, priority, deadline_tick,
            submitted_tick: crate::interrupts::ticks(),
        });
        id
    }

    pub fn dequeue(&mut self) -> Option<InferenceRequest> {
        let req = self.queue.pop()?;
        self.current = Some(req.clone());
        Some(req)
    }

    pub fn complete_current(&mut self) {
        if self.current.take().is_some() { self.total_completed += 1; }
    }

    pub fn fail_current(&mut self) {
        if self.current.take().is_some() { self.total_failed += 1; }
    }

    pub fn expire_deadlines(&mut self, current_tick: u64) -> usize {
        let mut expired = 0;
        let remaining: Vec<InferenceRequest> = self.queue.drain()
            .filter(|req| {
                if let Some(d) = req.deadline_tick {
                    if current_tick > d { expired += 1; return false; }
                }
                true
            }).collect();
        for r in remaining { self.queue.push(r); }
        self.total_failed += expired as u64;
        expired
    }

    pub fn queue_length(&self) -> usize { self.queue.len() }

    pub fn stats(&self) -> (u64, u64, usize) {
        (self.total_completed, self.total_failed, self.queue.len())
    }
}

pub static GPU_SCHEDULER: Mutex<GpuScheduler> = Mutex::new(GpuScheduler::new());

/// Capability-gated inference request submission.
pub fn submit_request(agent_id: AgentId, model_id: alloc::string::String, priority: InferencePriority, deadline_tick: Option<u64>, cap_id: CapId) -> Result<RequestId, &'static str> {
    CAP_TABLE.lock().validate(cap_id, agent_id, PermissionBits::EXECUTE)
        .map_err(|_| "insufficient inference capability")?;
    let id = GPU_SCHEDULER.lock().submit(agent_id, model_id, priority, deadline_tick);
    Ok(id)
}

pub fn init() {
    println!("[gpu] Inference scheduler initialized (priority queue with deadlines)");
}
