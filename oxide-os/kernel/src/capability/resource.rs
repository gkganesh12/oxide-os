use alloc::string::String;
use crate::task::TaskId;

/// What a capability grants access to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceRef {
    Agent(TaskId),
    Memory { base: u64, length: u64 },
    Network { host: String, port: u16 },
    Storage { path: String },
    Channel { name: String },
    Tool { name: String },
    Model { model_id: String },
    AgentSpawn,
    System { name: String },
}

impl ResourceRef {
    /// Check if `self` is a subset of `parent` (for delegation validation).
    pub fn is_subset_of(&self, parent: &ResourceRef) -> bool {
        match (self, parent) {
            (a, b) if a == b => true,
            (
                ResourceRef::Network {
                    host: ch,
                    port: cp,
                },
                ResourceRef::Network {
                    host: ph,
                    port: pp,
                },
            ) => (ph == "*" || ch == ph) && (*pp == 0 || cp == pp),
            (ResourceRef::Storage { path: child }, ResourceRef::Storage { path: parent_path }) => {
                child.starts_with(parent_path.as_str())
            }
            _ => false,
        }
    }
}
