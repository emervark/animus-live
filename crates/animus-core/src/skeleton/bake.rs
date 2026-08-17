//! Baking the authored, unbounded [`AttachmentTable`] down into the fixed
//! 4-influence palette the GPU skinning pipeline consumes. See spec §7.2.

use crate::doc::AttachmentTable;
use crate::ids::BoneId;
use crate::solver::CompiledRig;
use std::cmp::Ordering;

/// Bevy's `MAX_JOINTS`. In Animus Live those palette entries are **bones**,
/// not `Joint`s (see the `skeleton` module doc), so this counts bones.
///
/// **It is not a hardware-independent ceiling, and this crate does not
/// enforce it as one.** Reading `bevy_pbr/src/render/skin.rs`, `MAX_JOINTS`
/// bounds the *fallback uniform-buffer path*, taken only when
/// `storage_buffers_are_unsupported(limits)`. On any GPU with storage
/// buffers Bevy uses a growable allocator with no such cap: the M0-1 spike
/// ran 257 and 512 bones clean on two different discrete GPUs
/// (`docs/spikes/m0-1-skinned-mesh.md`).
///
/// So a bake over this count is **reported, not refused**. Refusing would be
/// wrong on capable hardware; refusing silently at 256 on uniform-buffer-only
/// hardware would also be wrong, because that path's real failure mode is
/// still untested. The caller warns and lets the render path speak.
pub const UNIFORM_PATH_BONE_LIMIT: usize = 256;

/// GPU vertex attributes carry exactly four influences per vertex.
pub const MAX_INFLUENCES: usize = 4;

/// The fixed-size GPU skinning palette baked from an [`AttachmentTable`]:
/// each vertex's top four influences, renormalized to sum to 1.0.
///
/// **Naming note — read this before touching either field.** `joint_index`
/// is named to match Bevy's `Mesh::ATTRIBUTE_JOINT_INDEX`, which it feeds
/// directly, and Bevy's skinning palette calls its entries "joints". But
/// the values stored here are **bone indices into [`CompiledRig`]**, not
/// indices into this crate's `Joint` type. A `Joint` is an unoriented
/// point mass in the spring graph; a `Bone` spans two joints and has the
/// A→B rest frame that `Attachment::local` is recorded in and that
/// skinning actually rotates. `MAX_SKIN_BONES` is a limit on bones, not
/// joints, for the same reason.
#[derive(Debug, Clone)]
pub struct BakedInfluences {
    /// Per vertex, up to four **bone** indices into `CompiledRig`. Unused
    /// slots (when a vertex has fewer than four influences) are padded
    /// with index 0 and weight 0.0.
    pub joint_index: Vec<[u16; 4]>,
    /// Per vertex, the weight for each corresponding `joint_index` entry.
    /// Sums to 1.0 for any vertex with at least one influence.
    pub joint_weight: Vec<[f32; 4]>,
    /// How many bones this palette addresses. Compare against
    /// [`UNIFORM_PATH_BONE_LIMIT`] via
    /// [`exceeds_uniform_bone_limit`](BakedInfluences::exceeds_uniform_bone_limit)
    /// to decide whether to warn.
    pub bone_count: usize,
    /// The largest fraction of any single vertex's authored weight that
    /// was dropped by keeping only its top four influences, across all
    /// vertices. 0.0 if no vertex had more than four attachments. Surface
    /// this to the artist ("vertex 812 lost 18% of its influence") rather
    /// than silently discarding it.
    pub max_dropped_mass: f32,
}

impl BakedInfluences {
    /// True when this rig would exceed Bevy's uniform-buffer skinning path.
    /// Not a failure — a reason to warn. See [`UNIFORM_PATH_BONE_LIMIT`].
    pub fn exceeds_uniform_bone_limit(&self) -> bool {
        self.bone_count > UNIFORM_PATH_BONE_LIMIT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BakeError {
    /// `att` names a bone `rig` does not contain. This is not malformed
    /// user input to shrug off — `BoneId`s are allocated from the
    /// project-wide `IdAlloc` sequence, so a dangling one means the
    /// `AttachmentTable` and the `CompiledRig` it's being baked against
    /// were built from different (or out-of-sync) skeletons, which is a
    /// caller bug, not something to silently under-weight a vertex over.
    #[error("attachment on vertex {vertex} names bone {bone}, which is not in this rig")]
    UnknownBone { vertex: u32, bone: BoneId },
}

/// Bakes `att` against `rig`'s bones into a fixed 4-influence-per-vertex
/// GPU palette, for a mesh of `vertex_count` vertices.
///
/// Each vertex's influences are sorted by weight descending, with **bone
/// index ascending as the tie-break** — required for the bake to be
/// deterministic when two bones tie on weight, since an arbitrary tie
/// order would let the top-4 selection (and therefore the baked mesh)
/// change between otherwise-identical runs. The top four survive and are
/// renormalized to sum to 1.0; a vertex with no attachments at all bakes
/// to all-zero indices and weights rather than panicking.
///
/// Each `Attachment::bone` is resolved to its dense index via
/// `CompiledRig::bone_index` — never treated as a dense index itself,
/// since `BoneId`s come from the project-wide `IdAlloc` sequence and are
/// not 0-based or contiguous per puppet.
///
/// Returns `Err(BakeError::UnknownBone)` if `att` names a bone `rig` does
/// not contain. A bone count above [`UNIFORM_PATH_BONE_LIMIT`] is *not* an
/// error — see that constant.
pub fn bake_influences(
    att: &AttachmentTable,
    rig: &CompiledRig,
    vertex_count: usize,
) -> Result<BakedInfluences, BakeError> {
    let bone_count = rig.bone_a.len();

    let mut per_vertex: Vec<Vec<(u32, f32)>> = vec![Vec::new(); vertex_count];
    for entry in &att.entries {
        let vertex = entry.vertex as usize;
        if vertex >= vertex_count {
            continue;
        }
        let bone_index = rig.bone_index(entry.bone).ok_or(BakeError::UnknownBone {
            vertex: entry.vertex,
            bone: entry.bone,
        })?;
        per_vertex[vertex].push((bone_index, entry.weight));
    }

    let mut joint_index = Vec::with_capacity(vertex_count);
    let mut joint_weight = Vec::with_capacity(vertex_count);
    let mut max_dropped_mass: f32 = 0.0;

    for mut influences in per_vertex {
        influences.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });

        let total: f32 = influences.iter().map(|(_, w)| w).sum();
        influences.truncate(MAX_INFLUENCES);
        let kept_sum: f32 = influences.iter().map(|(_, w)| w).sum();

        if total > 0.0 {
            let dropped = (total - kept_sum) / total;
            if dropped > max_dropped_mass {
                max_dropped_mass = dropped;
            }
        }

        let mut idx = [0u16; MAX_INFLUENCES];
        let mut wt = [0.0f32; MAX_INFLUENCES];
        for (slot, (bone_index, weight)) in influences.iter().enumerate() {
            idx[slot] = *bone_index as u16;
            wt[slot] = if kept_sum > 0.0 {
                weight / kept_sum
            } else {
                0.0
            };
        }

        joint_index.push(idx);
        joint_weight.push(wt);
    }

    Ok(BakedInfluences {
        joint_index,
        joint_weight,
        bone_count,
        max_dropped_mass,
    })
}
