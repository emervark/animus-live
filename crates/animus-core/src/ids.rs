//! Stable identifiers for document entities.
//!
//! IDs are allocated monotonically and **never reused**, so a stale
//! reference is detectably dangling rather than silently pointing at a
//! different object. `0` is reserved as an "unset" sentinel and is never
//! handed out.
//!
//! ## Wire shape: a number in value position, a string in key position
//!
//! An ID is a plain JSON integer wherever it appears as a *value* (a
//! `Layer`'s `id` field, a puppet's `texture` field, and so on). But
//! several of the maps in this format — `Project::assets`,
//! `Project::layer_data`, `Project::puppets`, and a skeleton's `joints`
//! and `bones` — are keyed *by* ID, and JSON object keys are always
//! strings; there is no other representation available. So the same ID
//! type must also deserialize from a decimal numeric string (`"10"`) when
//! it appears as a map key, and this is not a JSON quirk isolated to one
//! part of the codec: it's a hard requirement of the format itself, and
//! `spec/animus-project-format-v1.md` §4 documents both forms as
//! conformant.
//!
//! That's why `$name` below is *not* `#[derive(Deserialize)]` with
//! `#[serde(transparent)]`: deriving would delegate straight to `u64`'s
//! `Deserialize`, which accepts only a bare number. That happens to work
//! when `serde_json`'s own `Deserializer` drives a map key directly — its
//! map-key deserializer parses a string key like `"2"` into `u64` for
//! us — but it silently breaks for an ID used as a map key *inside* an
//! internally- or adjacently-tagged enum (e.g. `SkeletonData`'s
//! `joints`/`bones` maps, reached through `PuppetKind`): those are
//! deserialized a second time from serde's buffered `Content`
//! representation, which has no such string-to-number coercion, and fail
//! with "invalid type: string ..., expected u64" on every such key. The
//! hand-written `Deserialize` impl below accepts both a bare number and a
//! numeric string in every context, which is what actually lets this
//! format round-trip. See `ids::tests` for the map-key-in-a-tagged-enum
//! regression this guards.

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

        // Accepts both a bare number and a numeric string; see the
        // module-level doc comment above for why both are required, not
        // just tolerated.
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

    #[test]
    fn ids_deserialize_from_a_bare_number_and_from_a_numeric_string() {
        // Both forms are conformant per spec/animus-project-format-v1.md
        // §4: a bare number in value position, a numeric string in key
        // position (JSON object keys are always strings).
        assert_eq!(serde_json::from_str::<JointId>("2").unwrap(), JointId(2));
        assert_eq!(
            serde_json::from_str::<JointId>("\"2\"").unwrap(),
            JointId(2)
        );
    }

    /// Minimal reproduction of the shape that actually broke: an
    /// ID-keyed map (like `SkeletonData::joints`) nested inside an
    /// internally-tagged enum (like `PuppetKind`). JSON object keys are
    /// always strings, so `joints` necessarily serializes its `JointId`
    /// keys as `"2"`. When *this* enum's content is deserialized, serde
    /// buffers it into its private `Content` representation first and
    /// re-drives deserialization from that buffer — which has no
    /// string-to-number coercion for map keys, unlike `serde_json`'s
    /// `Deserializer` driving a top-level map directly.
    ///
    /// Against the old `#[derive(Deserialize)]` + `#[serde(transparent)]`
    /// impl (delegating straight to `u64`'s `Deserialize`, which accepts
    /// only a bare number) this failed with "invalid type: string \"2\",
    /// expected u64". Run against that derive, this test is RED; it is
    /// the regression guard for the hand-written impl above.
    #[test]
    fn an_id_used_as_a_map_key_inside_a_tagged_enum_round_trips() {
        use indexmap::IndexMap;

        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum Kind {
            Mesh { joints: IndexMap<JointId, String> },
        }

        let mut joints = IndexMap::new();
        joints.insert(JointId(2), "root".to_string());
        let value = Kind::Mesh { joints };

        let json = serde_json::to_string(&value).unwrap();
        assert!(
            json.contains("\"2\":"),
            "the map key must serialize as a JSON string, or this test isn't \
             reproducing the shape that broke: {json}"
        );

        let back: Kind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, value);
    }
}
