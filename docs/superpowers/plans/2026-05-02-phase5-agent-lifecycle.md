# Phase 5: Agent Lifecycle Management — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Promote "agents" from generic tasks to first-class kernel entities with full lifecycle management: spawn, configure, monitor, restart. Implement supervision trees with Erlang-style restart policies.

**Architecture:** An Agent wraps a Task with additional metadata (system prompt, model binding, tools, context store). The Supervisor agent sits at the root, managing child agents. Restart policies determine behavior on failure. The kernel tracks parent-child relationships and propagates death notifications.

**Tech Stack:** Builds on Phase 2 (scheduler/tasks), Phase 3 (capabilities), Phase 4 (IPC).

---

## File Structure

```
oxide-os/kernel/src/
├── agent/
│   ├── mod.rs              # Agent structure, states, public API
│   ├── lifecycle.rs        # Spawn, kill, restart logic
│   ├── supervisor.rs       # Supervision tree, restart policies
│   └── registry.rs         # Global agent registry (lookup by name/id)
```

---

## Task 1: Agent Structure & Registry

**Files:**
- Create: `oxide-os/kernel/src/agent/mod.rs`
- Create: `oxide-os/kernel/src/agent/registry.rs`

- [ ] **Step 1: Create agent/mod.rs**

```rust
// oxide-os/kernel/src/agent/mod.rs
pub mod lifecycle;
pub mod supervisor;
pub mod registry;

use alloc::string::String;
use alloc::vec::Vec;
use crate::task::TaskId;
use crate::capability::CapId;

pub type AgentId = TaskId; // Agents are backed by tasks, same ID space

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Initializing,
    Idle,
    Running,
    Waiting,
    Suspended,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Restart only the failed child.
    RestartOne,
    /// Restart all children if one fails.
    RestartAll,
    /// Don't restart — escalate failure to parent.
    Escalate,
    /// Never restart.
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
    pub max_cpu_ticks: u64,     // 0 = unlimited
    pub max_messages: usize,    // Mailbox limit
    pub max_children: usize,    // Max child agents
}

impl Default for ResourceLimits {
    fn default() -> Self {
        ResourceLimits {
            max_memory_bytes: 16 * 1024 * 1024, // 16 MiB
            max_cpu_ticks: 0,
            max_messages: 256,
            max_children: 32,
        }
    }
}

/// Configuration passed when spawning an agent.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub name: String,
    pub system_prompt: Option<String>,
    pub model: ModelBinding,
    pub tools: Vec<String>,
    pub capabilities: Vec<CapId>,
    pub restart_policy: RestartPolicy,
    pub resource_limits: ResourceLimits,
    pub enable_context_store: bool,
}

/// Runtime state of an agent (stored alongside its Task).
#[derive(Debug)]
pub struct Agent {
    pub id: AgentId,
    pub config: AgentConfig,
    pub state: AgentState,
    pub parent: Option<AgentId>,
    pub children: Vec<AgentId>,
    pub restart_count: u32,
    pub max_restarts: u32,       // Max restarts before giving up (default: 5)
}

impl Agent {
    pub fn new(id: AgentId, config: AgentConfig, parent: Option<AgentId>) -> Self {
        Agent {
            id,
            config,
            state: AgentState::Initializing,
            parent,
            children: Vec::new(),
            restart_count: 0,
            max_restarts: 5,
        }
    }
}
```

- [ ] **Step 2: Create agent/registry.rs**

```rust
// oxide-os/kernel/src/agent/registry.rs
use alloc::collections::BTreeMap;
use alloc::string::String;
use spin::Mutex;
use super::{Agent, AgentId};
use crate::println;

/// Global agent registry — lookup by ID or name.
pub struct AgentRegistry {
    agents: BTreeMap<AgentId, Agent>,
    name_index: BTreeMap<String, AgentId>,
}

impl AgentRegistry {
    pub const fn new() -> Self {
        AgentRegistry {
            agents: BTreeMap::new(),
            name_index: BTreeMap::new(),
        }
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
}

pub static REGISTRY: Mutex<AgentRegistry> = Mutex::new(AgentRegistry::new());
```

- [ ] **Step 3: Add agent module to main.rs**

```rust
mod agent;
```

- [ ] **Step 4: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 5: Commit**

```bash
git add oxide-os/kernel/src/agent/
git commit -m "feat: add agent structure, config, and registry"
```

---

## Task 2: Agent Spawn & Kill

**Files:**
- Create: `oxide-os/kernel/src/agent/lifecycle.rs`

- [ ] **Step 1: Create lifecycle.rs**

```rust
// oxide-os/kernel/src/agent/lifecycle.rs
use alloc::string::String;
use alloc::vec::Vec;
use super::{Agent, AgentConfig, AgentId, AgentState, registry::REGISTRY};
use crate::task::{Task, Priority, TaskId};
use crate::task::scheduler::SCHEDULER;
use crate::capability::{CapId, CAP_TABLE, PermissionBits, ResourceRef};
use crate::ipc::message;
use crate::println;

/// Spawn a new agent.
/// `parent_id` is the spawning agent (or None for root/init).
/// `entry` is the function the agent's task will execute.
/// Returns the new agent's ID.
pub fn spawn(
    parent_id: Option<AgentId>,
    config: AgentConfig,
    entry: fn() -> !,
    spawn_cap: Option<CapId>,
) -> Result<AgentId, &'static str> {
    // Validate spawn capability (if parent is specified)
    if let Some(parent) = parent_id {
        let cap = spawn_cap.ok_or("spawn requires a capability")?;
        let table = CAP_TABLE.lock();
        table.validate(cap, parent, PermissionBits::SPAWN)
            .map_err(|_| "insufficient spawn capability")?;
    }

    // Check parent's child limit
    if let Some(parent) = parent_id {
        let registry = REGISTRY.lock();
        if let Some(parent_agent) = registry.get(parent) {
            if parent_agent.children.len() >= parent_agent.config.resource_limits.max_children {
                return Err("parent has reached max children");
            }
        }
    }

    // Map priority from agent config (default: Normal)
    let priority = Priority::Normal;

    // Create the underlying task
    let capabilities = config.capabilities.clone();
    let name = config.name.clone();
    let task = Task::new(name.clone(), priority, entry, capabilities);
    let agent_id = task.id;

    // Register mailbox for IPC
    message::register_mailbox(agent_id);

    // Create the agent
    let mut agent = Agent::new(agent_id, config, parent_id);
    agent.state = AgentState::Running;

    // Register agent
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

/// Kill an agent and clean up.
/// Notifies parent. Cascades to children based on restart policy.
pub fn kill(agent_id: AgentId, killer_cap: Option<CapId>, killer: Option<TaskId>) -> Result<(), &'static str> {
    // Validate kill capability
    if let Some(cap) = killer_cap {
        if let Some(killer_id) = killer {
            let table = CAP_TABLE.lock();
            table.validate(cap, killer_id, PermissionBits::KILL)
                .map_err(|_| "insufficient kill capability")?;
        }
    }

    // Get agent info before removing
    let (parent_id, children) = {
        let mut registry = REGISTRY.lock();
        let agent = registry.get_mut(agent_id).ok_or("agent not found")?;
        agent.state = AgentState::Dead;
        (agent.parent, agent.children.clone())
    };

    // Kill all children
    for child_id in &children {
        let _ = kill(*child_id, None, None); // Recursive kill, no cap needed (kernel-initiated)
    }

    // Remove from parent's children list
    if let Some(parent) = parent_id {
        let mut registry = REGISTRY.lock();
        if let Some(parent_agent) = registry.get_mut(parent) {
            parent_agent.children.retain(|&id| id != agent_id);
        }
    }

    // Clean up task
    {
        let mut sched = SCHEDULER.lock();
        sched.kill_current(); // Only works if it's the current task
        // TODO: kill by ID for non-current tasks
    }

    // Clean up mailbox
    message::unregister_mailbox(agent_id);

    // Unregister agent
    REGISTRY.lock().unregister(agent_id);

    println!("[agent] Killed agent {} (children killed: {})", agent_id, children.len());

    // Notify parent for potential restart
    if let Some(parent) = parent_id {
        super::supervisor::notify_child_death(parent, agent_id);
    }

    Ok(())
}

/// Suspend an agent (pause execution).
pub fn suspend(agent_id: AgentId) -> Result<(), &'static str> {
    let mut registry = REGISTRY.lock();
    let agent = registry.get_mut(agent_id).ok_or("agent not found")?;
    agent.state = AgentState::Suspended;
    // Block the underlying task
    let mut sched = SCHEDULER.lock();
    sched.block_current();
    Ok(())
}

/// Resume a suspended agent.
pub fn resume(agent_id: AgentId) -> Result<(), &'static str> {
    let mut registry = REGISTRY.lock();
    let agent = registry.get_mut(agent_id).ok_or("agent not found")?;
    agent.state = AgentState::Running;
    let mut sched = SCHEDULER.lock();
    sched.unblock(agent_id);
    Ok(())
}
```

- [ ] **Step 2: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add oxide-os/kernel/src/agent/lifecycle.rs
git commit -m "feat: add agent spawn, kill, suspend, resume"
```

---

## Task 3: Supervision Tree & Restart Policies

**Files:**
- Create: `oxide-os/kernel/src/agent/supervisor.rs`

- [ ] **Step 1: Create supervisor.rs**

```rust
// oxide-os/kernel/src/agent/supervisor.rs
use super::{AgentId, AgentState, RestartPolicy, registry::REGISTRY};
use crate::println;

/// Called when a child agent dies. Applies the parent's restart policy.
pub fn notify_child_death(parent_id: AgentId, dead_child_id: AgentId) {
    let (policy, children, restart_count, max_restarts) = {
        let registry = REGISTRY.lock();
        let parent = match registry.get(parent_id) {
            Some(p) => p,
            None => return, // Parent already dead
        };
        (
            parent.config.restart_policy,
            parent.children.clone(),
            parent.restart_count,
            parent.max_restarts,
        )
    };

    // Check if we've exceeded max restarts
    if restart_count >= max_restarts {
        println!("[supervisor] Agent {} exceeded max restarts ({}), escalating",
            parent_id, max_restarts);
        escalate(parent_id);
        return;
    }

    match policy {
        RestartPolicy::RestartOne => {
            println!("[supervisor] RestartOne: restarting child {} of parent {}",
                dead_child_id, parent_id);
            restart_child(parent_id, dead_child_id);
        }
        RestartPolicy::RestartAll => {
            println!("[supervisor] RestartAll: restarting all children of parent {}",
                parent_id);
            for &child_id in &children {
                if child_id != dead_child_id {
                    // Kill other children first
                    let _ = super::lifecycle::kill(child_id, None, None);
                }
            }
            // Restart all (including the dead one)
            for &child_id in &children {
                restart_child(parent_id, child_id);
            }
        }
        RestartPolicy::Escalate => {
            println!("[supervisor] Escalating failure of {} to parent's parent", dead_child_id);
            escalate(parent_id);
        }
        RestartPolicy::Permanent => {
            println!("[supervisor] Permanent policy: child {} will not be restarted", dead_child_id);
        }
    }

    // Increment parent's restart counter
    let mut registry = REGISTRY.lock();
    if let Some(parent) = registry.get_mut(parent_id) {
        parent.restart_count += 1;
    }
}

/// Restart a specific child agent.
fn restart_child(parent_id: AgentId, child_id: AgentId) {
    // Get the child's config from registry (if still there)
    let config = {
        let registry = REGISTRY.lock();
        registry.get(child_id).map(|a| a.config.clone())
    };

    if let Some(config) = config {
        println!("[supervisor] Restarting agent '{}' (id: {})", config.name, child_id);
        // Note: In a real implementation, we'd need to store the entry function
        // or have a generic agent entry point that reads config from context store.
        // For now, we log the restart intention.
        // TODO: store entry point in AgentConfig or use generic agent runtime
    } else {
        println!("[supervisor] Cannot restart agent {}: config not found", child_id);
    }
}

/// Escalate failure: kill the parent and notify the grandparent.
fn escalate(agent_id: AgentId) {
    let parent_of_parent = {
        let registry = REGISTRY.lock();
        registry.get(agent_id).and_then(|a| a.parent)
    };

    println!("[supervisor] Escalating: killing agent {}", agent_id);
    let _ = super::lifecycle::kill(agent_id, None, None);

    if let Some(grandparent) = parent_of_parent {
        notify_child_death(grandparent, agent_id);
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add oxide-os/kernel/src/agent/supervisor.rs
git commit -m "feat: add supervision tree with restart policies"
```

---

## Task 4: Agent Status & Monitoring

**Files:**
- Modify: `oxide-os/kernel/src/agent/registry.rs`

- [ ] **Step 1: Add monitoring methods to registry**

```rust
    /// Get a summary of all agents for monitoring.
    pub fn status_all(&self) -> Vec<AgentStatus> {
        self.agents.values().map(|a| AgentStatus {
            id: a.id,
            name: a.config.name.clone(),
            state: a.state,
            parent: a.parent,
            children_count: a.children.len(),
            restart_count: a.restart_count,
        }).collect()
    }

    /// Print agent tree to serial.
    pub fn print_tree(&self) {
        // Find root agents (no parent)
        let roots: Vec<&Agent> = self.agents.values()
            .filter(|a| a.parent.is_none())
            .collect();

        for root in roots {
            self.print_subtree(root, 0);
        }
    }

    fn print_subtree(&self, agent: &Agent, depth: usize) {
        let indent = "  ".repeat(depth);
        crate::println!("{}├─ {} [id:{}, state:{:?}, restarts:{}]",
            indent, agent.config.name, agent.id, agent.state, agent.restart_count);
        for &child_id in &agent.children {
            if let Some(child) = self.agents.get(&child_id) {
                self.print_subtree(child, depth + 1);
            }
        }
    }
```

Add the status struct:

```rust
#[derive(Debug)]
pub struct AgentStatus {
    pub id: AgentId,
    pub name: String,
    pub state: super::AgentState,
    pub parent: Option<AgentId>,
    pub children_count: usize,
    pub restart_count: u32,
}
```

- [ ] **Step 2: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add oxide-os/kernel/src/agent/registry.rs
git commit -m "feat: add agent monitoring, status, and tree printing"
```

---

## Summary

After Phase 5, Oxide OS has:
- Agents as first-class kernel entities (wrapping tasks with rich metadata)
- Agent lifecycle: spawn, kill, suspend, resume
- AgentConfig with system prompt, model binding, tools, resource limits
- Global agent registry with name-based lookup
- Supervision trees: parent-child relationships
- Restart policies: RestartOne, RestartAll, Escalate, Permanent
- Max restart counter (prevents infinite restart loops)
- Agent status monitoring and tree visualization
- Ready for Phase 6 (networking enables agents to call external LLM APIs)
