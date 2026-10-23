//! Capability flags advertised by the host runtime.
//!
//! Scripts query capabilities to branch cleanly on host support instead of relying on
//! hidden platform divergence. The set is bitflag-style without a dependency on the
//! `bitflags` crate.

/// A single host capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Audio playback is available.
    AudioPlayback,
    /// Physics raycast queries are available.
    PhysicsRaycast,
    /// Spot lights are available.
    LightingSpot,
    /// Filesystem read access is available (native runtime only).
    FsRead,
    /// Hot reload of script sources is available.
    ScriptHotReload,
}

/// A set of capabilities. Backed by a `Vec` for portability; capability counts are
/// expected to stay small.
#[derive(Debug, Default, Clone)]
pub struct Capabilities {
    flags: Vec<Capability>,
}

impl Capabilities {
    /// Empty capability set.
    pub const fn empty() -> Self {
        Self { flags: Vec::new() }
    }

    /// Build from a slice.
    pub fn from_slice(flags: &[Capability]) -> Self {
        Self { flags: flags.to_vec() }
    }

    /// Add a capability (idempotent).
    pub fn insert(&mut self, cap: Capability) {
        if !self.flags.contains(&cap) {
            self.flags.push(cap);
        }
    }

    /// Returns `true` if the capability is present.
    pub fn contains(&self, cap: Capability) -> bool {
        self.flags.contains(&cap)
    }

    /// Iterate over all capabilities.
    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.flags.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_contains_nothing() {
        assert!(!Capabilities::empty().contains(Capability::AudioPlayback));
    }

    #[test]
    fn insert_is_idempotent() {
        let mut caps = Capabilities::empty();
        caps.insert(Capability::FsRead);
        caps.insert(Capability::FsRead);
        assert_eq!(caps.iter().count(), 1);
        assert!(caps.contains(Capability::FsRead));
    }
}
