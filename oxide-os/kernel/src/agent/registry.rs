use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use super::{AgentId, AgentState, AgentConfig};
use crate::println;

pub struct Agent {
    pub id: AgentId,
    pub config: AgentConfig,
    pub state: AgentState,
    pub parent: Option<AgentId>,
    pub children: Vec<AgentId>,
    pub restart_count: u32,
    pub max_restarts: u32,
}

pub struct AgentRegistry {
    agents: BTreeMap<AgentId, Agent>,
    name_index: BTreeMap<String, AgentId>,
}

impl AgentRegistry {
    pub const fn new() -> Self {
        AgentRegistry { agents: BTreeMap::new(), name_index: BTreeMap::new() }
    }

    pub fn register(&mut self, agent: Agent) {
        let name = agent.config.name.clone();
        let id = agent.id;
        println!("[agent] Registered '{}' (id: {})", name, id);
        self.name_index.insert(name, id);
        self.agents.insert(id, agent);
    }

    pub fn unregister(&mut self, id: AgentId) -> Option<Agent> {
        if let Some(agent) = self.agents.remove(&id) {
            self.name_index.remove(&agent.config.name);
            Some(agent)
        } else {
            None
        }
    }

    pub fn get(&self, id: AgentId) -> Option<&Agent> {
        self.agents.get(&id)
    }

    pub fn get_mut(&mut self, id: AgentId) -> Option<&mut Agent> {
        self.agents.get_mut(&id)
    }

    pub fn find_by_name(&self, name: &str) -> Option<AgentId> {
        self.name_index.get(name).copied()
    }

    pub fn count(&self) -> usize {
        self.agents.len()
    }

    pub fn all_ids(&self) -> Vec<AgentId> {
        self.agents.keys().copied().collect()
    }

    pub fn print_tree(&self) {
        let roots: Vec<&Agent> = self.agents.values().filter(|a| a.parent.is_none()).collect();
        for root in roots {
            self.print_subtree(root, 0);
        }
    }

    fn print_subtree(&self, agent: &Agent, depth: usize) {
        for _ in 0..depth {
            crate::print!("  ");
        }
        println!("|- {} [id:{}, {:?}, restarts:{}]",
            agent.config.name, agent.id, agent.state, agent.restart_count);
        for &child_id in &agent.children {
            if let Some(child) = self.agents.get(&child_id) {
                self.print_subtree(child, depth + 1);
            }
        }
    }
}

pub static REGISTRY: Mutex<AgentRegistry> = Mutex::new(AgentRegistry::new());
