use core::fmt;

/// Permission bits for capabilities. Bitfield for fast validation.
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

    /// Check if delegation to `subset` is valid (must have DELEGATE permission and contain all target bits).
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
        // Display as readable permission string like [R|W|X|D]
        let mut parts = alloc::vec::Vec::new();
        if self.contains(Self::READ) {
            parts.push("R");
        }
        if self.contains(Self::WRITE) {
            parts.push("W");
        }
        if self.contains(Self::EXECUTE) {
            parts.push("X");
        }
        if self.contains(Self::DELEGATE) {
            parts.push("D");
        }
        if self.contains(Self::SPAWN) {
            parts.push("SPAWN");
        }
        if self.contains(Self::KILL) {
            parts.push("KILL");
        }
        if self.contains(Self::SUBSCRIBE) {
            parts.push("SUB");
        }
        if self.contains(Self::PUBLISH) {
            parts.push("PUB");
        }
        if self.contains(Self::CONNECT) {
            parts.push("CONN");
        }
        if parts.is_empty() {
            return write!(f, "[NONE]");
        }
        write!(f, "[{}]", parts.join("|"))
    }
}
