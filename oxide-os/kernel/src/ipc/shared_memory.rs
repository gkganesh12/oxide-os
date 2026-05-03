use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::task::TaskId;
use crate::capability::{CapId, CAP_TABLE, PermissionBits, ResourceRef};
use crate::memory::frame_allocator::FRAME_ALLOCATOR;
use crate::memory::PAGE_SIZE;
use crate::println;
use super::IpcError;

pub type SharedRegionId = u64;

#[derive(Debug)]
pub struct SharedRegion {
    pub id: SharedRegionId,
    pub phys_base: u64,
    pub size: u64,
    pub owner: TaskId,
    pub grants: Vec<TaskId>,
}

static NEXT_REGION_ID: AtomicU64 = AtomicU64::new(1);
static SHARED_REGIONS: Mutex<BTreeMap<SharedRegionId, SharedRegion>> = Mutex::new(BTreeMap::new());

/// Create a shared memory region. Allocates physical frames.
pub fn create_region(owner: TaskId, size: u64) -> Result<SharedRegionId, IpcError> {
    let page_count = (size + PAGE_SIZE - 1) / PAGE_SIZE;

    let phys_base = {
        let mut alloc = FRAME_ALLOCATOR.lock();
        let alloc = alloc.as_mut().ok_or(IpcError::RecipientNotFound)?;
        let first = alloc.allocate_frame().ok_or(IpcError::RecipientNotFound)?;
        // Allocate remaining (non-contiguous in phys, but we track the base)
        for _ in 1..page_count {
            alloc.allocate_frame().ok_or(IpcError::RecipientNotFound)?;
        }
        first.start_address().as_u64()
    };

    let id = NEXT_REGION_ID.fetch_add(1, Ordering::Relaxed);

    let region = SharedRegion {
        id,
        phys_base,
        size: page_count * PAGE_SIZE,
        owner,
        grants: Vec::new(),
    };

    println!("[shm] Created region #{}: {} pages at phys {:#X} (owner: task {})", id, page_count, phys_base, owner);

    // Create a capability for the owner
    {
        let mut table = CAP_TABLE.lock();
        table.create_root(
            owner,
            ResourceRef::Memory { base: phys_base, length: page_count * PAGE_SIZE },
            PermissionBits::READ.union(PermissionBits::WRITE).union(PermissionBits::DELEGATE),
        );
    }

    SHARED_REGIONS.lock().insert(id, region);
    Ok(id)
}

/// Grant access to another task (requires DELEGATE on the region).
pub fn grant_access(
    region_id: SharedRegionId,
    granter: TaskId,
    grantee: TaskId,
    granter_cap: CapId,
) -> Result<CapId, IpcError> {
    {
        let table = CAP_TABLE.lock();
        table.validate(granter_cap, granter, PermissionBits::DELEGATE)
            .map_err(|_| IpcError::CapabilityDenied)?;
    }

    let mut regions = SHARED_REGIONS.lock();
    let region = regions.get_mut(&region_id).ok_or(IpcError::RecipientNotFound)?;
    region.grants.push(grantee);

    let cap_id = {
        let mut table = CAP_TABLE.lock();
        table.create_root(
            grantee,
            ResourceRef::Memory { base: region.phys_base, length: region.size },
            PermissionBits::READ,
        )
    };

    println!("[shm] Granted region #{} to task {} (cap #{})", region_id, grantee, cap_id);
    Ok(cap_id)
}
