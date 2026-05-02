# Phase 3: Capability System — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the capability-based security system — unforgeable kernel-managed tokens that mediate all resource access. No ambient authority.

**Architecture:** A kernel-global capability table stores all capabilities. Each task holds a set of capability IDs (indices). On every resource access (IPC, memory, network), the kernel validates the caller holds a valid capability with sufficient permissions. Capabilities can be delegated (subset) and revoked (cascading).

**Tech Stack:** Custom kernel data structures, HMAC-based validation, `alloc` collections.

---

## File Structure

```
oxide-os/kernel/src/
├── capability/
│   ├── mod.rs              # Public API, types
│   ├── table.rs            # Global capability table
│   ├── permissions.rs      # Permission bits and operations
│   └── resource.rs         # Resource reference types
```

---

## Task 1: Permission Bits & Resource Types

**Files:**
- Create: `oxide-os/kernel/src/capability/mod.rs`
- Create: `oxide-os/kernel/src/capability/permissions.rs`
- Create: `oxide-os/kernel/src/capability/resource.rs`

- [ ] **Step 1: Create capability/permissions.rs**

```rust
// oxide-os/kernel/src/capability/permissions.rs
use core::fmt;

/// Permission bits for capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionBits(u32);

impl PermissionBits {
    pub const NONE: Self = Self(0);
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const EXECUTE: Self = Self(1 << 2);
    pub const DELEGATE: Self = Self(1 << 3);
    pub const SPAWN: Self = Self(1 << 4);
    pub const KILL: Self = Self(1 << 5);
    pub const SUBSCRIBE: Self = Self(1 << 6);
    pub const PUBLISH: Self = Self(1 << 7);
    pub const CONNECT: Self = Self(1 << 8);

    /// Full permissions (all bits set).
    pub const ALL: Self = Self(0xFFFF_FFFF);

    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Check if `subset` is a valid delegation (equal or fewer permissions).
    pub fn can_delegate_to(self, subset: Self) -> bool {
        self.contains(Self::DELEGATE) && self.contains(subset)
    }

    pub fn raw(self) -> u32 {
        self.0
    }

    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

impl fmt::Display for PermissionBits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = alloc::vec::Vec::new();
        if self.contains(Self::READ) { parts.push("R"); }
        if self.contains(Self::WRITE) { parts.push("W"); }
        if self.contains(Self::EXECUTE) { parts.push("X"); }
        if self.contains(Self::DELEGATE) { parts.push("D"); }
        if self.contains(Self::SPAWN) { parts.push("SPAWN"); }
        if self.contains(Self::KILL) { parts.push("KILL"); }
        if self.contains(Self::SUBSCRIBE) { parts.push("SUB"); }
        if self.contains(Self::PUBLISH) { parts.push("PUB"); }
        if self.contains(Self::CONNECT) { parts.push("CONN"); }
        write!(f, "[{}]", parts.join("|"))
    }
}
```

- [ ] **Step 2: Create capability/resource.rs**

```rust
// oxide-os/kernel/src/capability/resource.rs
use alloc::string::String;
use crate::task::TaskId;

/// What a capability points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceRef {
    /// Another agent/task
    Agent(TaskId),
    /// A memory region (start address, length)
    Memory { base: u64, length: u64 },
    /// A network endpoint (host, port)
    Network { host: String, port: u16 },
    /// A storage path
    Storage { path: String },
    /// A pub/sub channel
    Channel { name: String },
    /// A tool (by name)
    Tool { name: String },
    /// A model (by ID)
    Model { model_id: String },
    /// Agent spawning permission
    AgentSpawn,
    /// System-level resource
    System { name: String },
}

impl ResourceRef {
    /// Check if this resource reference is a subset of `parent`.
    /// Used for delegation validation.
    pub fn is_subset_of(&self, parent: &ResourceRef) -> bool {
        match (self, parent) {
            // Same type, exact match
            (a, b) if a == b => true,
            // Network: child can restrict to same or narrower host
            (ResourceRef::Network { host: ch, port: cp },
             ResourceRef::Network { host: ph, port: pp }) => {
                (ph == "*" || ch == ph) && (pp == &0 || cp == pp)
            }
            // Storage: child path must be under parent path
            (ResourceRef::Storage { path: child },
             ResourceRef::Storage { path: parent_path }) => {
                child.starts_with(parent_path.as_str())
            }
            _ => false,
        }
    }
}
```

- [ ] **Step 3: Create capability/mod.rs**

```rust
// oxide-os/kernel/src/capability/mod.rs
pub mod permissions;
pub mod resource;
pub mod table;

pub use permissions::PermissionBits;
pub use resource::ResourceRef;
pub use table::{CapId, Capability, CapabilityTable, CAP_TABLE};

use crate::task::TaskId;

/// A handle that a task holds — an index into the capability table.
pub type CapHandle = u32;

/// Result type for capability operations.
#[derive(Debug)]
pub enum CapError {
    NotFound,
    InsufficientPermissions,
    InvalidDelegation,
    Revoked,
    Expired,
    NotOwner,
}
```

- [ ] **Step 4: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 5: Commit**

```bash
git add oxide-os/kernel/src/capability/
git commit -m "feat: add capability types — permissions, resources, errors"
```

---

## Task 2: Capability Table (Creation & Lookup)

**Files:**
- Create: `oxide-os/kernel/src/capability/table.rs`

- [ ] **Step 1: Create table.rs**

```rust
// oxide-os/kernel/src/capability/table.rs
use alloc::vec::Vec;
use spin::Mutex;
use super::{CapError, CapHandle, PermissionBits, ResourceRef};
use crate::task::TaskId;
use crate::println;

pub type CapId = u64;

#[derive(Debug, Clone)]
pub struct Capability {
    pub id: CapId,
    pub resource: ResourceRef,
    pub permissions: PermissionBits,
    pub owner: TaskId,
    pub parent_cap: Option<CapId>,
    pub revoked: bool,
    pub expiry: Option<u64>, // Tick-based expiry, None = no expiry
}

/// Global capability table — stores all capabilities in the system.
pub struct CapabilityTable {
    capabilities: Vec<Option<Capability>>,
    next_id: CapId,
}

impl CapabilityTable {
    pub const fn new() -> Self {
        CapabilityTable {
            capabilities: Vec::new(),
            next_id: 1,
        }
    }

    /// Create a new root capability (no parent). Used for system-level caps.
    pub fn create_root(
        &mut self,
        owner: TaskId,
        resource: ResourceRef,
        permissions: PermissionBits,
        expiry: Option<u64>,
    ) -> CapId {
        let id = self.next_id;
        self.next_id += 1;

        let cap = Capability {
            id,
            resource,
            permissions,
            owner,
            parent_cap: None,
            revoked: false,
            expiry,
        };

        // Store at index = id (grow vector as needed)
        let idx = id as usize;
        while self.capabilities.len() <= idx {
            self.capabilities.push(None);
        }
        self.capabilities[idx] = Some(cap);

        println!("[cap] Created root cap #{} for task {} -> {:?} {}",
            id, owner, resource, permissions);
        id
    }

    /// Look up a capability by ID.
    pub fn get(&self, id: CapId) -> Result<&Capability, CapError> {
        let idx = id as usize;
        match self.capabilities.get(idx) {
            Some(Some(cap)) => {
                if cap.revoked {
                    Err(CapError::Revoked)
                } else {
                    Ok(cap)
                }
            }
            _ => Err(CapError::NotFound),
        }
    }

    /// Validate that a task holds a capability with the required permissions.
    pub fn validate(
        &self,
        cap_id: CapId,
        task_id: TaskId,
        required: PermissionBits,
    ) -> Result<&Capability, CapError> {
        let cap = self.get(cap_id)?;
        if cap.owner != task_id {
            return Err(CapError::NotOwner);
        }
        if !cap.permissions.contains(required) {
            return Err(CapError::InsufficientPermissions);
        }
        // Check expiry
        if let Some(expiry) = cap.expiry {
            let current_tick = unsafe { crate::interrupts::ticks() };
            if current_tick > expiry {
                return Err(CapError::Expired);
            }
        }
        Ok(cap)
    }

    /// Total number of active (non-revoked) capabilities.
    pub fn active_count(&self) -> usize {
        self.capabilities
            .iter()
            .filter(|c| c.as_ref().map_or(false, |cap| !cap.revoked))
            .count()
    }
}

pub static CAP_TABLE: Mutex<CapabilityTable> = Mutex::new(CapabilityTable::new());
```

- [ ] **Step 2: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add oxide-os/kernel/src/capability/table.rs
git commit -m "feat: add capability table with creation and validation"
```

---

## Task 3: Capability Delegation

**Files:**
- Modify: `oxide-os/kernel/src/capability/table.rs`

- [ ] **Step 1: Add delegate method to CapabilityTable**

```rust
    /// Delegate a capability to another task with equal or fewer permissions.
    /// The delegated cap's parent is the source cap (for revocation chain).
    pub fn delegate(
        &mut self,
        source_cap_id: CapId,
        from_task: TaskId,
        to_task: TaskId,
        permissions: PermissionBits,
        resource_override: Option<ResourceRef>,
    ) -> Result<CapId, CapError> {
        // Validate source cap exists and is owned by from_task
        let source = self.get(source_cap_id)?.clone();
        if source.owner != from_task {
            return Err(CapError::NotOwner);
        }

        // Check delegation permission
        if !source.permissions.can_delegate_to(permissions) {
            return Err(CapError::InvalidDelegation);
        }

        // If resource is narrowed, validate it's a subset
        let resource = match resource_override {
            Some(ref r) => {
                if !r.is_subset_of(&source.resource) {
                    return Err(CapError::InvalidDelegation);
                }
                r.clone()
            }
            None => source.resource.clone(),
        };

        // Create delegated capability
        let id = self.next_id;
        self.next_id += 1;

        let cap = Capability {
            id,
            resource,
            permissions,
            owner: to_task,
            parent_cap: Some(source_cap_id),
            revoked: false,
            expiry: source.expiry, // Inherit expiry from parent
        };

        let idx = id as usize;
        while self.capabilities.len() <= idx {
            self.capabilities.push(None);
        }
        self.capabilities[idx] = Some(cap);

        println!("[cap] Delegated cap #{} -> #{} (task {} -> task {})",
            source_cap_id, id, from_task, to_task);
        Ok(id)
    }
```

- [ ] **Step 2: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add oxide-os/kernel/src/capability/table.rs
git commit -m "feat: add capability delegation with subset validation"
```

---

## Task 4: Capability Revocation (Cascading)

**Files:**
- Modify: `oxide-os/kernel/src/capability/table.rs`

- [ ] **Step 1: Add revoke method with cascading**

```rust
    /// Revoke a capability and all capabilities derived from it (cascading).
    /// Only the owner of the parent cap can revoke.
    pub fn revoke(&mut self, cap_id: CapId, revoker_task: TaskId) -> Result<usize, CapError> {
        // Validate the revoker owns the parent of this cap, or owns this cap directly
        {
            let cap = match self.capabilities.get(cap_id as usize) {
                Some(Some(cap)) => cap,
                _ => return Err(CapError::NotFound),
            };

            let authorized = cap.owner == revoker_task
                || cap.parent_cap.map_or(false, |parent_id| {
                    self.capabilities.get(parent_id as usize)
                        .and_then(|c| c.as_ref())
                        .map_or(false, |parent| parent.owner == revoker_task)
                });

            if !authorized {
                return Err(CapError::NotOwner);
            }
        }

        // Cascade: revoke this cap and all children
        let revoked_count = self.revoke_recursive(cap_id);
        println!("[cap] Revoked cap #{} and {} children (by task {})",
            cap_id, revoked_count - 1, revoker_task);
        Ok(revoked_count)
    }

    fn revoke_recursive(&mut self, cap_id: CapId) -> usize {
        // Mark this cap as revoked
        if let Some(Some(cap)) = self.capabilities.get_mut(cap_id as usize) {
            if cap.revoked {
                return 0; // Already revoked
            }
            cap.revoked = true;
        } else {
            return 0;
        }

        let mut count = 1;

        // Find all children (caps whose parent_cap == cap_id) and revoke them
        let children: Vec<CapId> = self.capabilities
            .iter()
            .filter_map(|slot| {
                slot.as_ref().and_then(|c| {
                    if c.parent_cap == Some(cap_id) && !c.revoked {
                        Some(c.id)
                    } else {
                        None
                    }
                })
            })
            .collect();

        for child_id in children {
            count += self.revoke_recursive(child_id);
        }

        count
    }
```

- [ ] **Step 2: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add oxide-os/kernel/src/capability/table.rs
git commit -m "feat: add cascading capability revocation"
```

---

## Task 5: Task Capability Set Integration

**Files:**
- Modify: `oxide-os/kernel/src/task/mod.rs`
- Modify: `oxide-os/kernel/src/main.rs`

- [ ] **Step 1: Add capability set to Task struct**

```rust
// In task/mod.rs, add to Task struct:
use alloc::vec::Vec;
use crate::capability::CapId;

pub struct Task {
    pub id: TaskId,
    pub name: String,
    pub state: TaskState,
    pub priority: Priority,
    pub context: CpuContext,
    pub kernel_stack_top: u64,
    pub kernel_stack_bottom: u64,
    pub capabilities: Vec<CapId>,  // Capability IDs this task holds
}

// Update Task::new to include capabilities parameter:
    pub fn new(name: String, priority: Priority, entry: fn() -> !, capabilities: Vec<CapId>) -> Self {
        // ... existing code ...
        Task {
            // ... existing fields ...
            capabilities,
        }
    }

    /// Check if this task holds a specific capability.
    pub fn has_capability(&self, cap_id: CapId) -> bool {
        self.capabilities.contains(&cap_id)
    }

    /// Grant a capability to this task.
    pub fn grant_capability(&mut self, cap_id: CapId) {
        if !self.capabilities.contains(&cap_id) {
            self.capabilities.push(cap_id);
        }
    }
```

- [ ] **Step 2: Add capability module to main.rs**

```rust
mod capability;
```

- [ ] **Step 3: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 4: Commit**

```bash
git add oxide-os/kernel/src/task/mod.rs oxide-os/kernel/src/capability/ oxide-os/kernel/src/main.rs
git commit -m "feat: integrate capability set into task structure"
```

---

## Summary

After Phase 3, Oxide OS has:
- Permission bits (Read, Write, Execute, Delegate, Spawn, Kill, Subscribe, Publish, Connect)
- Resource references (Agent, Memory, Network, Storage, Channel, Tool, Model)
- Global capability table with O(1) lookup by ID
- Root capability creation (for system init)
- Delegation with subset validation (permissions and resource narrowing)
- Cascading revocation (revoke a cap → all derived caps are revoked)
- Per-task capability sets
- Validation on every access (task must own cap with sufficient permissions)
- Ready for Phase 4 (IPC will validate capabilities on every message)
