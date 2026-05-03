use alloc::vec::Vec;
use spin::Mutex;

use super::{CapError, PermissionBits, ResourceRef};
use crate::println;
use crate::task::TaskId;

pub type CapId = u64;

#[derive(Debug, Clone)]
pub struct Capability {
    pub id: CapId,
    pub resource: ResourceRef,
    pub permissions: PermissionBits,
    pub owner: TaskId,
    pub parent_cap: Option<CapId>,
    pub revoked: bool,
}

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

    /// Create a root capability (no parent). Used at system init.
    pub fn create_root(
        &mut self,
        owner: TaskId,
        resource: ResourceRef,
        permissions: PermissionBits,
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
        };
        let idx = id as usize;
        if self.capabilities.len() <= idx {
            self.capabilities.resize_with(idx + 1, || None);
        }
        self.capabilities[idx] = Some(cap);
        let stored = self.capabilities[idx].as_ref().unwrap();
        println!("[cap] Created root #{} for task {} -> {:?} {}", id, owner, stored.resource, stored.permissions);
        id
    }

    /// Look up a capability. Returns error if not found or revoked.
    pub fn get(&self, id: CapId) -> Result<&Capability, CapError> {
        match self.capabilities.get(id as usize) {
            Some(Some(cap)) if !cap.revoked => Ok(cap),
            Some(Some(_)) => Err(CapError::Revoked),
            _ => Err(CapError::NotFound),
        }
    }

    /// Validate: task owns this cap AND it has the required permissions.
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
        Ok(cap)
    }

    /// Delegate: create a child capability with equal or fewer permissions.
    pub fn delegate(
        &mut self,
        source_id: CapId,
        from: TaskId,
        to: TaskId,
        permissions: PermissionBits,
        resource_override: Option<ResourceRef>,
    ) -> Result<CapId, CapError> {
        let source = self.get(source_id)?.clone();
        if source.owner != from {
            return Err(CapError::NotOwner);
        }
        if !source.permissions.can_delegate_to(permissions) {
            return Err(CapError::InvalidDelegation);
        }

        let resource = match resource_override {
            Some(ref r) if r.is_subset_of(&source.resource) => r.clone(),
            Some(_) => return Err(CapError::InvalidDelegation),
            None => source.resource.clone(),
        };

        let id = self.next_id;
        self.next_id += 1;
        let cap = Capability {
            id,
            resource,
            permissions,
            owner: to,
            parent_cap: Some(source_id),
            revoked: false,
        };
        let idx = id as usize;
        if self.capabilities.len() <= idx {
            self.capabilities.resize_with(idx + 1, || None);
        }
        self.capabilities[idx] = Some(cap);
        println!("[cap] Delegated #{} -> #{} (task {} -> {})", source_id, id, from, to);
        Ok(id)
    }

    /// Revoke a capability and all descendants (cascading).
    pub fn revoke(&mut self, cap_id: CapId, revoker: TaskId) -> Result<usize, CapError> {
        // Verify revoker owns this cap OR owns the parent
        {
            let cap = self.get(cap_id)?;
            let authorized = cap.owner == revoker
                || cap.parent_cap.map_or(false, |pid| {
                    self.capabilities
                        .get(pid as usize)
                        .and_then(|c| c.as_ref())
                        .map_or(false, |p| p.owner == revoker)
                });
            if !authorized {
                return Err(CapError::NotOwner);
            }
        }
        let count = self.revoke_recursive(cap_id);
        println!("[cap] Revoked #{} ({} capabilities cascaded)", cap_id, count);
        Ok(count)
    }

    fn revoke_recursive(&mut self, cap_id: CapId) -> usize {
        if let Some(Some(cap)) = self.capabilities.get_mut(cap_id as usize) {
            if cap.revoked {
                return 0;
            }
            cap.revoked = true;
        } else {
            return 0;
        }

        let children: Vec<CapId> = self
            .capabilities
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

        let mut count = 1;
        for child_id in children {
            count += self.revoke_recursive(child_id);
        }
        count
    }

    pub fn active_count(&self) -> usize {
        self.capabilities
            .iter()
            .filter(|c| c.as_ref().map_or(false, |cap| !cap.revoked))
            .count()
    }
}

pub static CAP_TABLE: Mutex<CapabilityTable> = Mutex::new(CapabilityTable::new());
