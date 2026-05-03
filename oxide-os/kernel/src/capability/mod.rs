pub mod permissions;
pub mod resource;
pub mod table;

pub use permissions::PermissionBits;
pub use resource::ResourceRef;
pub use table::{CapId, Capability, CapabilityTable, CAP_TABLE};

#[derive(Debug)]
pub enum CapError {
    NotFound,
    InsufficientPermissions,
    InvalidDelegation,
    Revoked,
    NotOwner,
}
