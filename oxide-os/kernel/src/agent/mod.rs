pub mod registry;
pub mod lifecycle;
pub mod supervisor;

use alloc::string::String;
use alloc::vec::Vec;
use crate::task::TaskId;
use crate::capability::CapId;

pub type AgentId = TaskId; // Agents are backed by tasks — same ID space

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Initializing,
    Running,
    Waiting,
    Suspended,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    RestartOne,
    RestartAll,
    Escalate,
    Permanent,
}

#[derive(Debug, Clone)]
pub enum ModelBinding {
    Local { model_id: String },
    Remote { endpoint: String, api_key_cap: CapId },
    Auto { preference: Vec<String> },
}

#[derive(Debug, Clone)]
pub struct ResourceLimits {
    pub max_memory_bytes: u64,
    pub max_children: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        ResourceLimits { max_memory_bytes: 16 * 1024 * 1024, max_children: 32 }
    }
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub name: String,
    pub system_prompt: Option<String>,
    pub model: ModelBinding,
    pub tools: Vec<String>,
    pub capabilities: Vec<CapId>,
    pub restart_policy: RestartPolicy,
    pub resource_limits: ResourceLimits,
}
