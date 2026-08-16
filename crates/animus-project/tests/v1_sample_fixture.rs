//! Generates `spec/fixtures/v1_sample/`: a migration test corpus richer
//! than the spec's worked example.
//!
//! `spec/fixtures/sample-project/` (`spec_fixture.rs`) is normative for
//! the format spec, but it is deliberately small — a two-joint, one-bone
//! skeleton and a single-triangle mesh — because a worked example should
//! be easy to read alongside the prose. That leaves it thin as a
//! *migration* corpus: a future migration restructuring "how a skeleton's
//! bones reference joints" or "how attachments are keyed" has only one
//! bone and one attachment to prove itself against. This fixture adds a
//! three-bone skeleton (four joints, a real chain) with two attachments
//! on different bones, and a mesh with more than one triangle, so the
//! migration-fixture test in `migrations.rs` exercises those structures
//! too.
//!
//! Built with the real codec, the same way as `spec_fixture.rs`'s
//! `build_fixture`, so the asset's `sha256` is a real hash of real bytes
//! and the committed JSON is exactly what `save` produces — never
//! hand-edited.

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
        .join("v1_sample")
}

/// `assets_root` is the `AssetStore` root the puppet's texture is imported
/// into. Callers should pass the same directory they later `save` the
/// project to (see `spec_fixture.rs`'s `build_fixture` doc comment for
/// why) so the resulting project directory is self-contained.
fn build_fixture(assets_root: &Path) -> Project {
    let mut p = Project::new("Migration Corpus");
    p.meta.created_utc = "2026-08-16T00:00:00Z".to_string();
    p.meta.modified_utc = "2026-08-16T00:00:00Z".to_string();

    let scratch = tempfile::tempdir().unwrap();
    let texture_src = scratch.path().join("arm-source.png");
    std::fs::write(&texture_src, b"a small fixture texture, not a real PNG").unwrap();
    let mut store = AssetStore::new(assets_root);
    let texture_id = AssetId(p.alloc_id());
    let texture = store
        .import(&texture_src, AssetKind::Image, texture_id)
        .unwrap();
    p.assets.insert(texture_id, texture);

    // A four-joint, three-bone chain: shoulder -> elbow -> wrist -> hand.
    let joint_shoulder = JointId(p.alloc_id());
    let joint_elbow = JointId(p.alloc_id());
    let joint_wrist = JointId(p.alloc_id());
    let joint_hand = JointId(p.alloc_id());
    let bone_upper = BoneId(p.alloc_id());
    let bone_lower = BoneId(p.alloc_id());
    let bone_hand = BoneId(p.alloc_id());

    let mut skeleton = SkeletonData::default();
    skeleton.joints.insert(
        joint_shoulder,
        Joint {
            id: joint_shoulder,
            name: "shoulder".to_string(),
            rest: Vec2::new(100.0, 100.0),
            rest_angle: 0.0,
            inv_mass: 0.0,
            pinned: true,
        },
    );
    skeleton.joints.insert(
        joint_elbow,
        Joint {
            id: joint_elbow,
            name: "elbow".to_string(),
            rest: Vec2::new(100.0, 200.0),
            rest_angle: 0.0,
            inv_mass: 1.0,
            pinned: false,
        },
    );
    skeleton.joints.insert(
        joint_wrist,
        Joint {
            id: joint_wrist,
            name: "wrist".to_string(),
            rest: Vec2::new(100.0, 300.0),
            rest_angle: 0.0,
            inv_mass: 1.0,
            pinned: false,
        },
    );
    skeleton.joints.insert(
        joint_hand,
        Joint {
            id: joint_hand,
            name: "hand".to_string(),
            rest: Vec2::new(100.0, 380.0),
            rest_angle: 0.0,
            inv_mass: 1.0,
            pinned: false,
        },
    );
    skeleton.bones.insert(
        bone_upper,
        Bone {
            id: bone_upper,
            name: "upper_arm".to_string(),
            a: joint_shoulder,
            b: joint_elbow,
            rest_length: None,
            stiffness: 0.8,
            damping: 0.1,
            length_mul: 1.0,
            attach_radius: 40.0,
        },
    );
    skeleton.bones.insert(
        bone_lower,
        Bone {
            id: bone_lower,
            name: "forearm".to_string(),
            a: joint_elbow,
            b: joint_wrist,
            rest_length: None,
            stiffness: 0.7,
            damping: 0.1,
            length_mul: 1.0,
            attach_radius: 35.0,
        },
    );
    skeleton.bones.insert(
        bone_hand,
        Bone {
            id: bone_hand,
            name: "hand".to_string(),
            a: joint_wrist,
            b: joint_hand,
            rest_length: None,
            stiffness: 0.9,
            damping: 0.15,
            length_mul: 1.0,
            attach_radius: 20.0,
        },
    );

    // A quad strip: 6 vertices, 4 triangles, so there is more than one
    // triangle for a future migration to walk.
    let mesh = MeshData {
        positions: vec![
            Vec2::new(80.0, 100.0),
            Vec2::new(120.0, 100.0),
            Vec2::new(80.0, 200.0),
            Vec2::new(120.0, 200.0),
            Vec2::new(80.0, 300.0),
            Vec2::new(120.0, 300.0),
        ],
        uvs: vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 0.5),
            Vec2::new(1.0, 0.5),
            Vec2::new(0.0, 1.0),
            Vec2::new(1.0, 1.0),
        ],
        triangles: vec![0, 1, 2, 2, 1, 3, 2, 3, 4, 4, 3, 5],
        source: MeshSource::Auto(AutoMeshParams {
            alpha_threshold: 16,
            close_radius: 2,
            rdp_epsilon_px: 1.5,
            min_region_area_px: 24.0,
            interior_spacing_px: 20.0,
            mode: AutoMeshMode::Grid,
        }),
    };

    let attachments = AttachmentTable {
        entries: vec![
            Attachment {
                vertex: 2,
                bone: bone_upper,
                weight: 1.0,
                local: Vec2::new(0.0, 100.0),
            },
            Attachment {
                vertex: 4,
                bone: bone_lower,
                weight: 1.0,
                local: Vec2::new(0.0, 100.0),
            },
        ],
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
            name: "Arm".to_string(),
            kind: PuppetKind::Mesh(mesh_puppet),
        },
    );

    let bg_id = LayerId(p.alloc_id());
    let mut background = Layer::new(bg_id, "Background");
    background.depth = -10.0;
    p.layers.push(bg_id);
    p.layer_data.insert(bg_id, background);

    let arm_id = LayerId(p.alloc_id());
    let mut arm_layer = Layer::new(arm_id, "Arm");
    arm_layer.transform = Transform2Or3::Spatial {
        translation: Vec3::new(0.0, 0.0, 0.0),
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };
    arm_layer.contents.push(puppet_id);
    p.layers.push(arm_id);
    p.layer_data.insert(arm_id, arm_layer);

    p
}

#[test]
fn v1_sample_fixture_matches_the_committed_copy() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Arm.animus");
    let project = build_fixture(&root);
    save(&project, &root).unwrap();

    let generated = std::fs::read_to_string(root.join("project.json")).unwrap();

    let committed_path = fixture_dir().join("project.json");
    let committed = std::fs::read_to_string(&committed_path).unwrap_or_else(|e| {
        panic!(
            "failed to read committed fixture at {}: {e}. \
             Run the ignored `regenerate_v1_sample_fixture` test to create it.",
            committed_path.display()
        )
    });

    assert_eq!(
        generated, committed,
        "spec/fixtures/v1_sample/project.json is stale relative to what this codec \
         actually produces. If the change was intentional, run the ignored \
         `regenerate_v1_sample_fixture` test to update it."
    );
}

/// Maintenance tool, not part of CI: overwrites the committed fixture with
/// whatever this codec currently produces. Run explicitly
/// (`cargo test -p animus-project --test v1_sample_fixture
/// regenerate_v1_sample_fixture -- --ignored`).
#[test]
#[ignore = "maintenance tool: regenerates the committed v1_sample fixture on purpose"]
fn regenerate_v1_sample_fixture() {
    let target = fixture_dir();
    std::fs::create_dir_all(&target).unwrap();
    let project = build_fixture(&target);
    save(&project, &target).unwrap();
}
