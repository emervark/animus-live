//! The cutout-mesh puppet: image, mesh, spring skeleton, and the vertex
//! attachments binding one to the other. See spec §4.3.

use crate::doc::solver_cfg::SolverConfig;
use crate::ids::{AssetId, BoneId, JointId};
use glam::Vec2;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshPuppet {
    pub texture: AssetId,
    /// Where the silhouette's alpha comes from.
    ///
    /// One mode exists today (`UseImageAlpha`). The field is serialized
    /// anyway, from the first save, so adding a mode later is a new *value*
    /// in an existing field rather than a schema change needing a
    /// migration. `#[serde(default)]` also means projects written before
    /// this field existed still load.
    #[serde(default)]
    pub matte: MatteParams,
    pub mesh: MeshData,
    pub skeleton: SkeletonData,
    pub attachments: AttachmentTable,
    pub material: MaterialCfg,
    /// Per-puppet override of the project solver. `None` means "use
    /// `Project::solver`".
    #[serde(default)]
    pub solver_override: Option<SolverConfig>,
}

impl MeshPuppet {
    /// An empty puppet bound to `texture`: no vertices, no skeleton, no
    /// attachments, default material, no solver override. Callers build
    /// it up from here (import a silhouette, auto-mesh, rig).
    pub fn empty(texture: AssetId) -> Self {
        Self {
            texture,
            matte: MatteParams::default(),
            mesh: MeshData::default(),
            skeleton: SkeletonData::default(),
            attachments: AttachmentTable::default(),
            material: MaterialCfg::default(),
            solver_override: None,
        }
    }
}

/// How a puppet's alpha is obtained.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatteParams {
    #[serde(default)]
    pub mode: MatteMode,
}

/// The alpha source. Additive by design: new variants are new values in an
/// existing field, so old projects keep loading and no migration is needed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatteMode {
    /// Use the image's own alpha channel. The only mode M1 implements.
    #[default]
    UseImageAlpha,
}

// ── Mesh: structure of arrays ───────────────────────────────────────────
//
// Positions, UVs and triangle indices are stored as parallel arrays (and
// flat index triples) rather than a `Vec<Vertex>`/`Vec<Face>`, because a
// puppet mesh carries 10^3-10^4 vertices and every one of positions, UVs
// and skinning data is accessed as a dense array in the hot mesh-build and
// solver paths.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MeshData {
    /// Rest positions, IMAGE SPACE: pixels, origin top-left, Y down. Never
    /// flipped here — that happens once, at the render boundary (spec §7.1).
    pub positions: Vec<Vec2>,
    /// Normalized 0..1, Y down — already matches wgpu's convention.
    pub uvs: Vec<Vec2>,
    /// Flat index triples, CCW in image space.
    pub triangles: Vec<u32>,
    /// Provenance, so "re-run auto-mesh" is reproducible.
    pub source: MeshSource,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshSource {
    #[default]
    Manual,
    Auto(AutoMeshParams),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoMeshParams {
    pub alpha_threshold: u8,
    pub close_radius: u32,
    pub rdp_epsilon_px: f32,
    pub min_region_area_px: f32,
    pub interior_spacing_px: f32,
    pub mode: AutoMeshMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoMeshMode {
    Silhouette,
    ConvexHull,
    BoundingBox,
    Grid,
}

// ── Skeleton: a GRAPH of springs, NOT a hierarchy ───────────────────────
//
// A `Bone` names its two `JointId` endpoints directly; there is no
// parent/child relationship between bones anywhere in this type. Posing
// is whatever the spring solver settles on, not a chain of transforms.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkeletonData {
    pub joints: IndexMap<JointId, Joint>,
    pub bones: IndexMap<BoneId, Bone>,
}

/// A point mass in the spring graph. Has a position; has no meaningful
/// orientation of its own — nothing defines which way a joint "faces".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Joint {
    pub id: JointId,
    pub name: String,
    /// Image space.
    pub rest: Vec2,
    /// Radians, image space. Unused by skinning (a `Bone`'s A→B direction
    /// supplies orientation there); retained for future tools such as IK
    /// hints or driven rotations.
    #[serde(default)]
    pub rest_angle: f32,
    /// 0.0 means pinned (infinite mass).
    pub inv_mass: f32,
    #[serde(default)]
    pub pinned: bool,
}

/// A spring between two joints. Its A→B direction is also the frame that
/// `Attachment::local` is expressed in, and the frame skinning rotates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bone {
    pub id: BoneId,
    pub name: String,
    pub a: JointId,
    pub b: JointId,
    /// `None` means: compute from the joints' rest positions.
    #[serde(default)]
    pub rest_length: Option<f32>,
    pub stiffness: f32,
    /// Reserved for a future per-bone damping model; ignored by the v1
    /// reference solver, which applies only `SolverConfig::global_damping`
    /// (see `CompiledRig::build`). Implementing per-bone damping is a
    /// solver change, not a document-model one.
    pub damping: f32,
    /// Squash/stretch, animatable. 1.0 is rest length.
    #[serde(default = "one")]
    pub length_mul: f32,
    pub attach_radius: f32,
}

// ── Attachments: authored truth, unbounded influence count ─────────────
//
// This table is what the user actually authored; it may bind a vertex to
// more bones than the GPU's 4-influence limit allows. Baking down to a
// bounded, renormalized skinning palette is a separate, later step (spec
// §7.2) — this type does not enforce that limit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttachmentTable {
    /// Sorted by `(vertex, bone)` for deterministic output.
    pub entries: Vec<Attachment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub vertex: u32,
    pub bone: BoneId,
    pub weight: f32,
    /// The vertex's rest position in THIS bone's local frame, recorded at
    /// bind time.
    pub local: Vec2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialCfg {
    /// Multiplies the texture; alpha scales overall opacity on top of
    /// `Layer::opacity`.
    pub tint: [f32; 4],
    /// `Blend` (soft, correct AA edges, never occludes 3D) or `Mask` (hard
    /// cutout that both occludes and is occluded by 3D). See spec §7.4.
    pub alpha_mode: AlphaModeCfg,
}

impl Default for MaterialCfg {
    fn default() -> Self {
        Self {
            tint: [1.0, 1.0, 1.0, 1.0],
            alpha_mode: AlphaModeCfg::Blend,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaModeCfg {
    Blend,
    Mask,
}
