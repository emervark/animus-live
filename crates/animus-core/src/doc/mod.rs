//! The document model. This is the single source of truth for a show.
//!
//! Everything the renderer displays is a one-way projection of these
//! types. Nothing ever writes back into them from the scene graph.

mod asset;
pub mod command;
mod layer;
mod mesh_puppet;
mod model_puppet;
mod puppet;
mod solver_cfg;
mod stage;
pub mod undo;

pub use asset::{AssetKind, AssetRef};
pub use command::{
    AddLayer, AddPuppet, BoneParam, CommandError, DocChange, DocCommand, DuplicateLayer,
    ImportImage, ImportTarget, LayerPlacement, LayerScalar, MIN_LAYER_SCALE, MoveJointRest,
    PendingChanges, RemoveLayer, RemovePuppet, RenameLayer, ReorderLayers, ReplacePuppet,
    RotateJoint, SetBoneParam, SetJointMass, SetJointPinned, SetLayerLocked, SetLayerScalar,
    SetLayerVisible, SetSkeleton, SetSolverParam, SetStageCanvas, SolverParam, TransformLayer,
};
pub use layer::{BlendMode, Layer, Transform2Or3};
pub use mesh_puppet::{
    AlphaModeCfg, Attachment, AttachmentTable, AutoMeshMode, AutoMeshParams, Bone, Joint,
    MaterialCfg, MatteMode, MatteParams, MeshData, MeshPuppet, MeshSource, SkeletonData,
};
pub use model_puppet::{DrivenJoint, ModelNode, ModelPuppet};
pub use puppet::{Puppet, PuppetKind};
pub use solver_cfg::SolverConfig;
pub use stage::StageConfig;
pub use undo::{UndoStack, apply_command};

use crate::ids::*;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Bump on any breaking change to the on-disk format, and add a migration.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub meta: ProjectMeta,
    pub next_id: u64,
    pub assets: IndexMap<AssetId, AssetRef>,
    /// Paint order. Front of the Vec is the back of the scene.
    pub layers: Vec<LayerId>,
    pub layer_data: IndexMap<LayerId, Layer>,
    pub puppets: IndexMap<PuppetId, Puppet>,
    /// Typed in the signal-bus milestone. Kept as raw JSON for now so
    /// files written today still load once `Binding` exists: an untyped
    /// vec round-trips whatever shape a later version reads into it,
    /// whereas a typed field we got wrong now would need a migration.
    #[serde(default)]
    pub bindings: Vec<serde_json::Value>,
    pub solver: SolverConfig,
    pub stage: StageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub created_by: String,
    pub created_utc: String,
    pub modified_utc: String,
}

impl Project {
    pub fn new(name: &str) -> Self {
        let stamp = "1970-01-01T00:00:00Z".to_string(); // callers overwrite; core has no clock
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            meta: ProjectMeta {
                name: name.to_string(),
                created_by: concat!("animus ", env!("CARGO_PKG_VERSION")).to_string(),
                created_utc: stamp.clone(),
                modified_utc: stamp,
            },
            next_id: 1,
            assets: IndexMap::new(),
            layers: Vec::new(),
            layer_data: IndexMap::new(),
            puppets: IndexMap::new(),
            bindings: Vec::new(),
            solver: SolverConfig::default(),
            stage: StageConfig::default(),
        }
    }

    /// Allocate a new never-reused ID and persist the watermark.
    pub fn alloc_id(&mut self) -> u64 {
        let mut alloc = IdAlloc::from_next(self.next_id);
        let id = alloc.next();
        self.next_id = alloc.peek();
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_project_is_empty_and_current_schema() {
        let p = Project::new("Test Show");
        assert_eq!(p.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(p.meta.name, "Test Show");
        assert!(p.layers.is_empty());
        assert!(p.puppets.is_empty());
        assert!(p.assets.is_empty());
        assert_eq!(p.next_id, 1);
    }

    #[test]
    fn alloc_id_advances_next_id() {
        let mut p = Project::new("Test Show");
        let a = p.alloc_id();
        let b = p.alloc_id();
        assert_ne!(a, b);
        assert_eq!(p.next_id, 3);
    }

    #[test]
    fn solver_defaults_match_the_spec() {
        let s = SolverConfig::default();
        assert_eq!(s.hz, 120);
        assert_eq!(s.iterations, 8);
        assert_eq!(s.max_substeps_per_frame, 8);
        assert!(s.enabled);
        assert_eq!(s.gravity, glam::Vec2::ZERO);
    }

    #[test]
    fn bone_defaults_leave_length_mul_at_one() {
        let b = Bone {
            id: BoneId(1),
            name: "arm".into(),
            a: JointId(1),
            b: JointId(2),
            rest_length: None,
            stiffness: 0.8,
            damping: 0.1,
            length_mul: 1.0,
            attach_radius: 30.0,
        };
        assert_eq!(b.length_mul, 1.0);
        assert!(
            b.rest_length.is_none(),
            "None means: compute from rest positions"
        );
    }

    #[test]
    fn project_round_trips_through_json() {
        let mut p = Project::new("Round Trip");
        let lid = LayerId(p.alloc_id());
        p.layers.push(lid);
        p.layer_data.insert(lid, Layer::new(lid, "Background"));

        let json = serde_json::to_string_pretty(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.layers, p.layers);
        assert_eq!(back.layer_data[&lid].name, "Background");
        assert_eq!(back.next_id, p.next_id);
    }

    #[test]
    fn unknown_fields_are_ignored_for_forward_compatibility() {
        // A v1 reader must survive a file written by a later version that
        // added fields. deny_unknown_fields must stay OFF.
        let json = r#"{
            "schema_version": 1,
            "meta": { "name": "X", "created_by": "animus 0.1.0",
                      "created_utc": "2026-08-16T00:00:00Z",
                      "modified_utc": "2026-08-16T00:00:00Z" },
            "next_id": 1,
            "assets": {}, "layers": [], "layer_data": {}, "puppets": {},
            "bindings": [],
            "solver": { "hz": 120, "iterations": 8, "gravity": [0.0, 0.0],
                        "global_damping": 0.98, "max_substeps_per_frame": 8,
                        "enabled": true },
            "stage": { "canvas": [1920, 1080], "background": [0.0,0.0,0.0,1.0] },
            "a_field_from_the_future": 42
        }"#;
        let p: Project = serde_json::from_str(json).expect("must not reject unknown fields");
        assert_eq!(p.meta.name, "X");
    }

    /// The `mesh_puppet` types (spec §4.3 defers their shape to
    /// implementation) have no test elsewhere. This one constructs and
    /// JSON-round-trips each of them using only names reachable through
    /// `super::*` — i.e. `doc`'s public re-export surface, exactly what an
    /// external crate sees. It's a reachability check as much as a
    /// round-trip check: a type that's `pub` inside a privately-declared
    /// submodule but missing from `doc::mod`'s `pub use` list would fail to
    /// resolve here, the way `AlphaModeCfg` originally did.
    #[test]
    fn self_designed_helper_types_round_trip_through_json() {
        let material = MaterialCfg {
            tint: [1.0, 0.5, 0.25, 1.0],
            alpha_mode: AlphaModeCfg::Mask,
        };
        let back: MaterialCfg =
            serde_json::from_str(&serde_json::to_string(&material).unwrap()).unwrap();
        assert_eq!(back.alpha_mode, AlphaModeCfg::Mask);

        let flat = Transform2Or3::Flat {
            translation: glam::Vec2::new(1.0, 2.0),
            rotation: 0.5,
            scale: glam::Vec2::ONE,
        };
        let back: Transform2Or3 =
            serde_json::from_str(&serde_json::to_string(&flat).unwrap()).unwrap();
        assert!(matches!(back, Transform2Or3::Flat { .. }));

        let spatial = Transform2Or3::Spatial {
            translation: glam::Vec3::ZERO,
            rotation: glam::Quat::IDENTITY,
            scale: glam::Vec3::ONE,
        };
        let back: Transform2Or3 =
            serde_json::from_str(&serde_json::to_string(&spatial).unwrap()).unwrap();
        assert!(matches!(back, Transform2Or3::Spatial { .. }));

        let driven = DrivenJoint {
            node_name: "jaw".into(),
            channel: "mouth_open".into(),
        };
        let back: DrivenJoint =
            serde_json::from_str(&serde_json::to_string(&driven).unwrap()).unwrap();
        assert_eq!(back.node_name, "jaw");

        let kind = AssetKind::Gltf;
        let back: AssetKind = serde_json::from_str(&serde_json::to_string(&kind).unwrap()).unwrap();
        assert_eq!(back, AssetKind::Gltf);
    }
}
