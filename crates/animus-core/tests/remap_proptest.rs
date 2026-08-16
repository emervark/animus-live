//! Property tests for vertex deletion.
//!
//! This is the highest-value test suite in the project: a dangling vertex
//! index is silent corruption that surfaces as a crash or garbled mesh
//! long after the edit that caused it.

use animus_core::doc::*;
use animus_core::ids::{AssetId, BoneId};
use glam::Vec2;
use proptest::prelude::*;

fn arb_mesh() -> impl Strategy<Value = MeshData> {
    (3usize..40).prop_flat_map(|n| {
        let positions = prop::collection::vec(
            (0.0f32..1000.0, 0.0f32..1000.0).prop_map(|(x, y)| Vec2::new(x, y)),
            n,
        );
        let tris = prop::collection::vec((0..n as u32, 0..n as u32, 0..n as u32), 0..60);
        (positions, tris, Just(n)).prop_map(|(positions, tris, n)| {
            let uvs = positions.iter().map(|p| *p / 1000.0).collect();
            let triangles = tris
                .into_iter()
                // reject degenerate triangles up front; they are not what
                // this test is about
                .filter(|(a, b, c)| a != b && b != c && a != c)
                .flat_map(|(a, b, c)| [a, b, c])
                .collect();
            let _ = n;
            MeshData {
                positions,
                uvs,
                triangles,
                source: MeshSource::Manual,
            }
        })
    })
}

fn arb_puppet() -> impl Strategy<Value = MeshPuppet> {
    arb_mesh().prop_flat_map(|mesh| {
        let n = mesh.positions.len() as u32;
        let atts = prop::collection::vec((0..n, 1u64..5, 0.0f32..1.0), 0..30);
        (Just(mesh), atts).prop_map(|(mesh, atts)| {
            let mut mp = MeshPuppet::empty(AssetId(1));
            mp.mesh = mesh;
            mp.attachments.entries = atts
                .into_iter()
                .map(|(v, b, w)| Attachment {
                    vertex: v,
                    bone: BoneId(b),
                    weight: w,
                    local: Vec2::ZERO,
                })
                .collect();
            mp
        })
    })
}

proptest! {
    // `deletion_is_order_independent` draws `a`/`b` from a fixed `0..40`
    // range independent of the mesh's actual vertex count (as low as 3),
    // so `prop_assume!(a < n && b < n && a != b)` rejects often when `n`
    // is small. The default `max_global_rejects` (1024) can't absorb that
    // over 500 cases, so it's raised here; this doesn't change what's
    // asserted, only how many rejected samples proptest tolerates.
    #![proptest_config(ProptestConfig {
        cases: 500,
        max_global_rejects: 65536,
        ..ProptestConfig::default()
    })]

    /// After any deletion, nothing may reference a vertex that no longer exists.
    #[test]
    fn no_dangling_indices_after_deletion(
        mut mp in arb_puppet(),
        victims in prop::collection::vec(0u32..40, 0..10),
    ) {
        let n_before = mp.mesh.positions.len() as u32;
        let victims: Vec<u32> = victims.into_iter().filter(|v| *v < n_before).collect();

        mp.remove_vertices(&victims);
        let n_after = mp.mesh.positions.len() as u32;

        prop_assert_eq!(mp.mesh.uvs.len() as u32, n_after, "uvs stayed parallel");
        prop_assert_eq!(mp.mesh.triangles.len() % 3, 0, "triangles stayed whole");

        for t in &mp.mesh.triangles {
            prop_assert!(*t < n_after, "triangle index {} >= {}", t, n_after);
        }
        for a in &mp.attachments.entries {
            prop_assert!(a.vertex < n_after, "attachment vertex {} >= {}", a.vertex, n_after);
        }
    }

    /// Surviving vertices keep their positions. This catches an off-by-one
    /// in the compaction loop, which a dangling-index check alone would miss.
    #[test]
    fn survivors_keep_their_data(
        mut mp in arb_puppet(),
        victim in 0u32..40,
    ) {
        let n = mp.mesh.positions.len() as u32;
        prop_assume!(victim < n);

        let expected: Vec<Vec2> = mp.mesh.positions.iter().enumerate()
            .filter(|(i, _)| *i as u32 != victim)
            .map(|(_, p)| *p)
            .collect();

        mp.remove_vertices(&[victim]);
        prop_assert_eq!(mp.mesh.positions, expected);
    }

    /// Deleting {a, b} in one call equals deleting {a, b} in the other order.
    #[test]
    fn deletion_is_order_independent(
        mp in arb_puppet(),
        a in 0u32..40,
        b in 0u32..40,
    ) {
        let n = mp.mesh.positions.len() as u32;
        prop_assume!(a < n && b < n && a != b);

        let mut x = mp.clone();
        x.remove_vertices(&[a, b]);
        let mut y = mp.clone();
        y.remove_vertices(&[b, a]);

        prop_assert_eq!(x.mesh.positions, y.mesh.positions);
        prop_assert_eq!(x.mesh.triangles, y.mesh.triangles);
        // Full content, not just count: `IndexRemap::from_deletions` masks
        // victims by a boolean array keyed on vertex index (see remap.rs),
        // so the resulting remap -- and therefore every surviving
        // attachment's remapped vertex, bone, weight and local coords --
        // must be identical regardless of which order [a, b] was passed
        // in, not merely the same length.
        prop_assert_eq!(x.attachments.entries, y.attachments.entries);
    }
}
