use alloc::string::String;
use alloc::vec::Vec;
use crate::task::TaskId;
use crate::capability::CapId;

/// A user-space process (future: wraps a task with address space isolation).
#[derive(Debug)]
pub struct Process {
    pub pid: TaskId,
    pub name: String,
    pub capabilities: Vec<CapId>,
    pub entry_point: u64,
}

impl Process {
    pub fn new(pid: TaskId, name: String, entry_point: u64) -> Self {
        Process { pid, name, capabilities: Vec::new(), entry_point }
    }
}
