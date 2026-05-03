use super::{AgentId, RestartPolicy, registry::REGISTRY};
use crate::println;

/// Called when a child agent dies. Applies parent's restart policy.
pub fn notify_child_death(parent_id: AgentId, dead_child_id: AgentId) {
    let (policy, restart_count, max_restarts) = {
        let registry = REGISTRY.lock();
        match registry.get(parent_id) {
            Some(p) => (p.config.restart_policy, p.restart_count, p.max_restarts),
            None => return,
        }
    };

    if restart_count >= max_restarts {
        println!("[supervisor] Agent {} exceeded max restarts ({}), escalating", parent_id, max_restarts);
        escalate(parent_id);
        return;
    }

    match policy {
        RestartPolicy::RestartOne => {
            println!("[supervisor] RestartOne: would restart child {} of parent {}", dead_child_id, parent_id);
            // Actual restart requires storing the entry function — deferred to runtime
            increment_restart_count(parent_id);
        }
        RestartPolicy::RestartAll => {
            println!("[supervisor] RestartAll: would restart all children of parent {}", parent_id);
            increment_restart_count(parent_id);
        }
        RestartPolicy::Escalate => {
            println!("[supervisor] Escalating failure of child {} to parent {}", dead_child_id, parent_id);
            escalate(parent_id);
        }
        RestartPolicy::Permanent => {
            println!("[supervisor] Permanent: child {} will not be restarted", dead_child_id);
        }
    }
}

fn increment_restart_count(agent_id: AgentId) {
    let mut registry = REGISTRY.lock();
    if let Some(agent) = registry.get_mut(agent_id) {
        agent.restart_count += 1;
    }
}

fn escalate(agent_id: AgentId) {
    let parent = {
        let registry = REGISTRY.lock();
        registry.get(agent_id).and_then(|a| a.parent)
    };

    println!("[supervisor] Escalating: killing agent {}", agent_id);
    let _ = super::lifecycle::kill(agent_id);

    if let Some(grandparent) = parent {
        notify_child_death(grandparent, agent_id);
    }
}
