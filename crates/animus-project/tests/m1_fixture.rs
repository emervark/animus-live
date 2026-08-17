//! Generates and guards `spec/fixtures/m1-rigged/`: the corpus whose IDs
//! are hostile on purpose.
//!
//! Every other fixture allocates IDs sequentially, so its `BoneId`s happen
//! to be small and ordered — which means a reader that treats an ID as an
//! index passes against them anyway. This fixture's IDs are sparse, large
//! and out of order (`bones` insert 907 before 350, joints run 512/77/2003),
//! so any load-path or bake-path code that confuses an ID with a dense
//! index breaks against it instead of surviving until a real project finds
//! it. Spec §7.2 names this the mistake that fails silently: vertices
//! following the wrong limb rather than crashing.
//!
//! Built with the real codec like the other fixtures, never hand-edited;
//! the committed copy is exact bytes.

use animus_core::doc::*;
use animus_core::ids::{AssetId, BoneId, JointId, LayerId, PuppetId};
use animus_project::{AssetStore, save};
use glam::Vec2;
use std::path::{Path, PathBuf};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("spec")
        .join("fixtures")
        .join("m1-rigged")
}

// Sparse, unordered, and far from any enumeration index.
const ASSET: AssetId = AssetId(41);
const LAYER: LayerId = LayerId(9000);
const PUPPET: PuppetId = PuppetId(613);
const J_ROOT: JointId = JointId(512);
const J_MID: JointId = JointId(77);
const J_TIP: JointId = JointId(2003);
const B_LOWER: BoneId = BoneId(907);
const B_UPPER: BoneId = BoneId(350);

fn build_fixture(assets_root: &Path) -> Project {
    let mut p = Project::new("M1 Rigged Corpus");
    p.meta.created_utc = "2026-08-17T00:00:00Z".to_string();
    p.meta.modified_utc = "2026-08-17T00:00:00Z".to_string();

    let scratch = tempfile::tempdir().unwrap();
    let texture_src = scratch.path().join("puppet-texture.png");
    std::fs::write(&texture_src, b"fixture texture bytes, not a real PNG").unwrap();
    let mut store = AssetStore::new(assets_root);
    let mut asset = store.import(&texture_src, AssetKind::Image, ASSET).unwrap();
    asset.width = Some(160);
    asset.height = Some(160);
    p.assets.insert(ASSET, asset);

    let mut mp = MeshPuppet::empty(ASSET);
    // A small real mesh: 6 vertices, 4 triangles.
    mp.mesh.positions = vec![
        Vec2::new(60.0, 20.0),
        Vec2::new(100.0, 20.0),
        Vec2::new(60.0, 80.0),
        Vec2::new(100.0, 80.0),
        Vec2::new(60.0, 140.0),
        Vec2::new(100.0, 140.0),
    ];
    mp.mesh.uvs = mp
        .mesh
        .positions
        .iter()
        .map(|v| *v / Vec2::new(160.0, 160.0))
        .collect();
    mp.mesh.triangles = vec![0, 1, 2, 2, 1, 3, 2, 3, 4, 4, 3, 5];

    let mut skeleton = SkeletonData::default();
    for (id, name, y, pinned) in [
        (J_ROOT, "root", 20.0, true),
        (J_MID, "mid", 80.0, false),
        (J_TIP, "tip", 140.0, false),
    ] {
        skeleton.joints.insert(
            id,
            Joint {
                id,
                name: name.to_string(),
                rest: Vec2::new(80.0, y),
                rest_angle: 0.0,
                inv_mass: if pinned { 0.0 } else { 1.0 },
                pinned,
            },
        );
    }
    // Deliberately: the LOWER bone (higher id) inserts FIRST, so dense
    // index 0 belongs to BoneId 907 and index 1 to BoneId 350.
    skeleton.bones.insert(
        B_LOWER,
        Bone {
            id: B_LOWER,
            name: "lower".to_string(),
            a: J_MID,
            b: J_TIP,
            rest_length: None,
            stiffness: 0.7,
            damping: 0.1,
            length_mul: 1.0,
            attach_radius: 50.0,
        },
    );
    skeleton.bones.insert(
        B_UPPER,
        Bone {
            id: B_UPPER,
            name: "upper".to_string(),
            a: J_ROOT,
            b: J_MID,
            rest_length: None,
            stiffness: 0.9,
            damping: 0.15,
            length_mul: 1.0,
            attach_radius: 45.0,
        },
    );
    mp.skeleton = skeleton;
    mp.attachments = animus_core::skeleton::auto_attach(&mp.mesh, &mp.skeleton);
    assert!(
        !mp.attachments.entries.is_empty(),
        "the fixture must carry real attachments or it guards nothing"
    );

    let mut layer = Layer::new(LAYER, "Puppet");
    layer.depth = 0.02;
    layer.contents.push(PUPPET);
    p.layer_data.insert(LAYER, layer);
    p.layers.push(LAYER);
    p.puppets.insert(
        PUPPET,
        Puppet {
            id: PUPPET,
            name: "rigged".to_string(),
            kind: PuppetKind::Mesh(mp),
        },
    );

    // The watermark sits above every hand-picked ID, as a real allocator
    // would leave it.
    p.next_id = 9001;
    p
}

#[test]
fn m1_fixture_matches_the_committed_copy() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("M1.animus");
    let project = build_fixture(&root);
    save(&project, &root).unwrap();
    let generated = std::fs::read_to_string(root.join("project.json")).unwrap();

    let committed_path = fixture_dir().join("project.json");
    let committed = std::fs::read_to_string(&committed_path).unwrap_or_else(|e| {
        panic!(
            "failed to read committed fixture at {}: {e}. \
             Run the ignored `regenerate_m1_fixture` test to create it.",
            committed_path.display()
        )
    });

    assert_eq!(
        generated, committed,
        "spec/fixtures/m1-rigged/project.json is stale relative to what this codec \
         produces. If the change was intentional, run the ignored \
         `regenerate_m1_fixture` test."
    );
}

#[test]
fn the_fixture_loads_and_its_hostile_ids_resolve_correctly() {
    // The point of the sparse IDs: after a load, the dense bone order must
    // come from insertion order, and looking a bone up by ID must land on
    // the right dense slot. If any load-path code used the ID as an index,
    // BoneId(907) would explode or land on nothing.
    let project = animus_project::load(&fixture_dir()).expect("the committed fixture loads");

    let PuppetKind::Mesh(mp) = &project.puppets[&PUPPET].kind else {
        panic!("mesh puppet");
    };
    let rig = animus_core::solver::CompiledRig::build(&mp.skeleton, &project.solver);
    assert_eq!(rig.bone_count(), 2);
    assert_eq!(
        rig.bone_index(B_LOWER),
        Some(0),
        "lower (id 907) inserted first, so dense index 0"
    );
    assert_eq!(rig.bone_index(B_UPPER), Some(1));

    // And the bake addresses those dense indices, not the raw IDs.
    let baked =
        animus_core::skeleton::bake_influences(&mp.attachments, &rig, mp.mesh.positions.len())
            .expect("bake");
    let max_index = baked
        .joint_index
        .iter()
        .flat_map(|v| v.iter())
        .max()
        .copied()
        .unwrap_or(0);
    assert!(
        max_index <= 1,
        "a palette index above 1 means a BoneId leaked through as an index (saw {max_index})"
    );
}

/// Maintenance tool: overwrites the committed fixture on purpose.
#[test]
#[ignore = "maintenance tool: regenerates the committed m1-rigged fixture on purpose"]
fn regenerate_m1_fixture() {
    let dir = fixture_dir();
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    let project = build_fixture(&dir);
    save(&project, &dir).unwrap();
    println!("regenerated {}", dir.display());
}
