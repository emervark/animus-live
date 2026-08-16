//! The worked example `spec/animus-project-format-v1.md` points readers at.
//!
//! Prose describing a wire format drifts from the code that actually
//! produces it — that's exactly what happened with `Transform2Or3` and
//! `MeshSource` before this file existed. This test regenerates a small
//! but non-trivial project (two layers — one flat 2D transform, one
//! spatial 3D transform — and a mesh puppet with a skeleton, an
//! attachment, and `MeshSource::Auto` provenance) by running the real
//! codec, and asserts the result is byte-for-byte identical to the
//! committed fixture the spec cites. Any future change to the wire format
//! that isn't also applied to the fixture (and the spec prose describing
//! it) fails this test instead of silently invalidating the spec.

use animus_core::doc::*;
use animus_core::ids::{AssetId, BoneId, JointId, LayerId, PuppetId};
use animus_project::{AssetStore, save};
use glam::{Quat, Vec2, Vec3};
use std::path::{Path, PathBuf};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("spec")
        .join("fixtures")
        .join("sample-project")
}

/// Build the worked-example project. `assets_root` is the `AssetStore`
/// root the puppet's texture is imported into through the real
/// `AssetStore` — so the asset's `sha256` in the fixture is a real hash of
/// real bytes, not a hand-typed placeholder. Callers should pass the same
/// directory they later `save` the project to, so the resulting project
/// directory is self-contained (`project.json` plus a real `assets/`
/// tree it can actually resolve), not just a JSON file naming a hash that
/// exists nowhere on disk. The staging file the source bytes are written
/// to before import lives in its own scratch tempdir, not `assets_root`,
/// so nothing but `assets/` ends up alongside `project.json`.
fn build_fixture(assets_root: &Path) -> Project {
    let mut p = Project::new("Fixture Puppet Show");
    p.meta.created_utc = "2026-08-16T00:00:00Z".to_string();
    p.meta.modified_utc = "2026-08-16T00:00:00Z".to_string();

    // One content-addressed texture asset, imported through the real store.
    let scratch = tempfile::tempdir().unwrap();
    let texture_src = scratch.path().join("hero-source.png");
    std::fs::write(&texture_src, b"a small fixture texture, not a real PNG").unwrap();
    let mut store = AssetStore::new(assets_root);
    let texture_id = AssetId(p.alloc_id());
    let texture = store
        .import(&texture_src, AssetKind::Image, texture_id)
        .unwrap();
    p.assets.insert(texture_id, texture);

    // A two-joint, one-bone skeleton with a single attachment.
    let joint_root = JointId(p.alloc_id());
    let joint_tip = JointId(p.alloc_id());
    let bone_spine = BoneId(p.alloc_id());

    let mut skeleton = SkeletonData::default();
    skeleton.joints.insert(
        joint_root,
        Joint {
            id: joint_root,
            name: "root".to_string(),
            rest: Vec2::new(100.0, 400.0),
            rest_angle: 0.0,
            inv_mass: 0.0,
            pinned: true,
        },
    );
    skeleton.joints.insert(
        joint_tip,
        Joint {
            id: joint_tip,
            name: "tip".to_string(),
            rest: Vec2::new(100.0, 100.0),
            rest_angle: 0.0,
            inv_mass: 1.0,
            pinned: false,
        },
    );
    skeleton.bones.insert(
        bone_spine,
        Bone {
            id: bone_spine,
            name: "spine".to_string(),
            a: joint_root,
            b: joint_tip,
            rest_length: None,
            stiffness: 0.8,
            damping: 0.1,
            length_mul: 1.0,
            attach_radius: 40.0,
        },
    );

    let mesh = MeshData {
        positions: vec![
            Vec2::new(80.0, 100.0),
            Vec2::new(120.0, 100.0),
            Vec2::new(100.0, 400.0),
        ],
        uvs: vec![
            Vec2::new(0.4, 0.0),
            Vec2::new(0.6, 0.0),
            Vec2::new(0.5, 1.0),
        ],
        triangles: vec![0, 1, 2],
        source: MeshSource::Auto(AutoMeshParams {
            alpha_threshold: 16,
            close_radius: 2,
            rdp_epsilon_px: 1.5,
            min_region_area_px: 24.0,
            interior_spacing_px: 20.0,
            mode: AutoMeshMode::Silhouette,
        }),
    };

    let attachments = AttachmentTable {
        entries: vec![Attachment {
            vertex: 2,
            bone: bone_spine,
            weight: 1.0,
            local: Vec2::new(0.0, 300.0),
        }],
    };

    let mut mesh_puppet = MeshPuppet::empty(texture_id);
    mesh_puppet.mesh = mesh;
    mesh_puppet.skeleton = skeleton;
    mesh_puppet.attachments = attachments;

    let puppet_id = PuppetId(p.alloc_id());
    p.puppets.insert(
        puppet_id,
        Puppet {
            id: puppet_id,
            name: "Hero".to_string(),
            kind: PuppetKind::Mesh(mesh_puppet),
        },
    );

    // Background: default (flat 2D) transform.
    let bg_id = LayerId(p.alloc_id());
    let mut background = Layer::new(bg_id, "Background");
    background.depth = -10.0;
    p.layers.push(bg_id);
    p.layer_data.insert(bg_id, background);

    // Hero: spatial (3D) transform, hosting the mesh puppet.
    let hero_id = LayerId(p.alloc_id());
    let mut hero_layer = Layer::new(hero_id, "Hero");
    hero_layer.transform = Transform2Or3::Spatial {
        translation: Vec3::new(0.0, 0.0, 0.0),
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };
    hero_layer.contents.push(puppet_id);
    p.layers.push(hero_id);
    p.layer_data.insert(hero_id, hero_layer);

    p
}

#[test]
fn spec_worked_example_matches_the_committed_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Fixture.animus");
    let project = build_fixture(&root);
    save(&project, &root).unwrap();

    let generated = std::fs::read_to_string(root.join("project.json")).unwrap();

    let committed_path = fixture_dir().join("project.json");
    let committed = std::fs::read_to_string(&committed_path).unwrap_or_else(|e| {
        panic!(
            "failed to read committed spec fixture at {}: {e}. \
             Run the ignored `regenerate_spec_fixture` test to create it.",
            committed_path.display()
        )
    });

    assert_eq!(
        generated, committed,
        "spec/fixtures/sample-project/project.json is stale relative to the wire \
         format this codec actually produces. If the format changed on purpose, \
         run the ignored `regenerate_spec_fixture` test to update the fixture, and \
         update spec/animus-project-format-v1.md's prose to match."
    );
}

/// Maintenance tool, not part of CI: overwrites the committed fixture with
/// whatever this codec currently produces. Run explicitly
/// (`cargo test -p animus-project --test spec_fixture regenerate_spec_fixture
/// -- --ignored`) only when the wire format changed on purpose and the spec
/// prose has already been updated to match; otherwise this would silently
/// launder a real drift into the "normative" example.
#[test]
#[ignore = "maintenance tool: regenerates the committed spec fixture on purpose"]
fn regenerate_spec_fixture() {
    let target = fixture_dir();
    std::fs::create_dir_all(&target).unwrap();
    let project = build_fixture(&target);
    save(&project, &target).unwrap();
}
