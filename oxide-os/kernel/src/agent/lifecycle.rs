use super::{AgentId, AgentState, AgentConfig, registry::{Agent, REGISTRY}};
use crate::task::{Task, Priority};
use crate::task::scheduler::SCHEDULER;
use crate::ipc::message;
use crate::println;
use x86_64::structures::paging::OffsetPageTable;

/// Spawn a new agent. Creates the underlying task and registers the agent.
pub fn spawn(
    parent_id: Option<AgentId>,
    config: AgentConfig,
    entry: fn() -> !,
    mapper: &mut OffsetPageTable,
) -> Result<AgentId, &'static str> {
    // Check parent's child limit
    if let Some(parent) = parent_id {
        let registry = REGISTRY.lock();
        if let Some(parent_agent) = registry.get(parent) {
            if parent_agent.children.len() >= parent_agent.config.resource_limits.max_children {
                return Err("parent has reached max children");
            }
        }
    }

    let name = config.name.clone();
    let priority = Priority::Normal;

    // Create the underlying kernel task
    let task = Task::new(name.clone(), priority, entry, mapper);
    let agent_id = task.id;

    // Register mailbox for IPC
    message::register_mailbox(agent_id);

    // Register the agent
    let agent = Agent {
        id: agent_id,
        config,
        state: AgentState::Running,
        parent: parent_id,
        children: alloc::vec::Vec::new(),
        restart_count: 0,
        max_restarts: 5,
    };

    REGISTRY.lock().register(agent);

    // Add as child of parent
    if let Some(parent) = parent_id {
        let mut registry = REGISTRY.lock();
        if let Some(parent_agent) = registry.get_mut(parent) {
            parent_agent.children.push(agent_id);
        }
    }

    // Schedule the task
    SCHEDULER.lock().spawn(task);

    println!("[agent] Spawned '{}' (id: {}, parent: {:?})", name, agent_id, parent_id);
    Ok(agent_id)
}

/// Kill an agent and notify its parent for supervision.
pub fn kill(agent_id: AgentId) -> Result<(), &'static str> {
    let (parent_id, children) = {
        let mut registry = REGISTRY.lock();
        let agent = registry.get_mut(agent_id).ok_or("agent not found")?;
        agent.state = AgentState::Dead;
        (agent.parent, agent.children.clone())
    };

    // Kill all children recursively
    for child_id in &children {
        let _ = kill(*child_id);
    }

    // Remove from parent's children list
    if let Some(parent) = parent_id {
        let mut registry = REGISTRY.lock();
        if let Some(parent_agent) = registry.get_mut(parent) {
            parent_agent.children.retain(|&id| id != agent_id);
        }
    }

    // Unregister
    REGISTRY.lock().unregister(agent_id);

    println!("[agent] Killed {} (children: {})", agent_id, children.len());

    // Notify parent for potential restart
    if let Some(parent) = parent_id {
        super::supervisor::notify_child_death(parent, agent_id);
    }

    Ok(())
}

/// Suspend an agent.
pub fn suspend(agent_id: AgentId) -> Result<(), &'static str> {
    let mut registry = REGISTRY.lock();
    let agent = registry.get_mut(agent_id).ok_or("agent not found")?;
    agent.state = AgentState::Suspended;
    Ok(())
}

/// Resume a suspended agent.
pub fn resume(agent_id: AgentId) -> Result<(), &'static str> {
    let mut registry = REGISTRY.lock();
    let agent = registry.get_mut(agent_id).ok_or("agent not found")?;
    agent.state = AgentState::Running;
    Ok(())
}
