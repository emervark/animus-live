//! Stable identifiers for document entities.
//!
//! IDs are allocated monotonically and **never reused**, so a stale
//! reference is detectably dangling rather than silently pointing at a
//! different object. `0` is reserved as an "unset" sentinel and is never
//! handed out.

use serde::de::Visitor;
use serde::{Deserialize, Deserializer, Serialize};

macro_rules! define_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
            Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        // Not `#[derive(Deserialize)]` with `#[serde(transparent)]`: that
        // delegates straight to `u64`'s `Deserialize`, which only accepts
        // a JSON number. That's fine when `serde_json`'s own `Deserializer`
        // drives it directly — its map-key deserializer parses a string
        // key like `"2"` into `u64` for us. But an ID used as a map key
        // *inside* an internally- or adjacently-tagged enum (e.g. a
        // `SkeletonData` nested in `PuppetKind`) is deserialized a second
        // time from serde's buffered `Content` representation, which has
        // no such string-to-number coercion and fails with "invalid
        // type: string ..., expected u64" on every such key. Accepting
        // both a bare number and a numeric string here makes IDs work
        // as map keys in both contexts.
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct IdVisitor;

                impl<'de> Visitor<'de> for IdVisitor {
                    type Value = u64;

                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(f, "an integer or a string containing one")
                    }

                    fn visit_u64<E>(self, v: u64) -> Result<u64, E>
                    where
                        E: serde::de::Error,
                    {
                        Ok(v)
                    }

                    fn visit_i64<E>(self, v: i64) -> Result<u64, E>
                    where
                        E: serde::de::Error,
                    {
                        u64::try_from(v).map_err(E::custom)
                    }

                    fn visit_str<E>(self, v: &str) -> Result<u64, E>
                    where
                        E: serde::de::Error,
                    {
                        v.parse().map_err(E::custom)
                    }
                }

                deserializer.deserialize_any(IdVisitor).map($name)
            }
        }
    };
}

define_id!(
    /// Identifies a `Layer` within a `Project`.
    LayerId
);
define_id!(
    /// Identifies a `Puppet` within a `Project`.
    PuppetId
);
define_id!(
    /// Identifies a `Bone` within a `SkeletonData`.
    BoneId
);
define_id!(
    /// Identifies a `Joint` within a `SkeletonData`.
    JointId
);
define_id!(
    /// Identifies an `AssetRef` within a `Project`.
    AssetId
);
define_id!(
    /// Identifies a `Binding` within a `Project`.
    BindingId
);

/// Monotonic ID allocator. Serialized as `Project::next_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdAlloc {
    next: u64,
}

impl IdAlloc {
    /// A fresh allocator. The first ID handed out is 1.
    pub fn new() -> Self {
        Self { next: 1 }
    }

    /// Resume allocation for a project loaded from disk.
    pub fn from_next(next: u64) -> Self {
        Self { next: next.max(1) }
    }

    /// Allocate the next unused ID.
    ///
    /// Named `next` per the interface spec, not `Iterator::next`; this type
    /// deliberately does not implement `Iterator`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }

    /// The ID that would be allocated next, without allocating it.
    pub fn peek(&self) -> u64 {
        self.next
    }
}

impl Default for IdAlloc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_never_reused() {
        let mut alloc = IdAlloc::new();
        let a = LayerId(alloc.next());
        let b = LayerId(alloc.next());
        let c = LayerId(alloc.next());
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
        assert_eq!(alloc.peek(), 4, "first id is 1, so next unallocated is 4");
    }

    #[test]
    fn id_zero_is_never_allocated() {
        // 0 is reserved as a sentinel meaning "unset".
        let mut alloc = IdAlloc::new();
        assert_ne!(alloc.next(), 0);
    }

    #[test]
    fn alloc_resumes_from_a_loaded_project() {
        let mut alloc = IdAlloc::from_next(500);
        assert_eq!(alloc.next(), 500);
        assert_eq!(alloc.next(), 501);
    }

    #[test]
    fn ids_serialize_as_bare_numbers() {
        let json = serde_json::to_string(&PuppetId(42)).unwrap();
        assert_eq!(json, "42");
        let back: PuppetId = serde_json::from_str("42").unwrap();
        assert_eq!(back, PuppetId(42));
    }
}
