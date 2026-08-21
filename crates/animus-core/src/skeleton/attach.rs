//! Radius-falloff auto-attach: which bones influence which vertices, and
//! by how much. See spec §7.2.

use crate::doc::{Attachment, AttachmentTable, MeshData, SkeletonData};
use glam::Vec2;

/// Exponent on the inverse-distance weight. Higher values concentrate weight
/// closer to the bone; fixed here rather than exposed as a per-bone knob.
const FALLOFF: f32 = 2.0;

/// Keeps the weight finite for a vertex sitting exactly on a bone, in image
/// pixels.
const NEAR: f32 = 1.0;

/// Binds every mesh vertex to every bone within reach of it, weighted by
/// distance to the bone *segment* (not the infinite line through it), then
/// normalizes each vertex's weights to sum to 1.0.
///
/// **Every vertex gets at least one bone**, as long as the skeleton has one.
/// A vertex outside every radius falls back to its nearest bone at full
/// weight, and the reason is not tidiness: GPU skinning multiplies a
/// vertex by the sum of its weights, so a vertex with no bones is a vertex
/// multiplied by zero — it collapses onto the puppet's origin and drags a
/// spike of triangles into the middle of the drawing. A hand-drawn rig
/// leaves stray vertices out of reach constantly (a fingertip, an ear), and
/// the tool must not answer that with a hole in the artwork.
///
/// The fallback also costs nothing at rest: one bone at weight 1.0
/// reproduces the bind pose exactly, so an unreached fingertip simply
/// follows the limb nearest to it once the rig moves.
///
/// `entries` is returned sorted by `(vertex, bone)` so the table
/// serializes deterministically.
pub fn auto_attach(mesh: &MeshData, skel: &SkeletonData) -> AttachmentTable {
    let mut entries = Vec::new();

    for (vertex, &pos) in mesh.positions.iter().enumerate() {
        let mut candidates: Vec<Attachment> = Vec::new();
        let mut nearest: Option<(f32, Attachment)> = None;

        for bone in skel.bones.values() {
            // A radius of zero is an explicit "this bone holds nothing", so
            // it is not a fallback candidate either.
            if bone.attach_radius <= 0.0 {
                continue;
            }
            let (Some(a), Some(b)) = (skel.joints.get(&bone.a), skel.joints.get(&bone.b)) else {
                // A dangling bone reference; skip it rather than panic (the
                // solver's `CompiledRig::build` treats this the same way).
                continue;
            };

            let a_pos = a.rest;
            let b_pos = b.rest;
            let axis = b_pos - a_pos;
            let len = axis.length();
            if len < f32::EPSILON {
                continue;
            }
            let x_axis = axis / len;
            let y_axis = Vec2::new(-x_axis.y, x_axis.x);

            let rel = pos - a_pos;
            let local = Vec2::new(rel.dot(x_axis), rel.dot(y_axis));

            // Distance to the SEGMENT: clamp the projection onto the axis
            // to [0, len] before measuring, so a vertex beyond either
            // endpoint measures to that endpoint rather than to the
            // line's extension.
            let t = local.x.clamp(0.0, len);
            let closest = a_pos + x_axis * t;
            let dist = (pos - closest).length();

            let attachment = Attachment {
                vertex: vertex as u32,
                bone: bone.id,
                weight: 1.0,
                local,
            };
            if nearest.as_ref().is_none_or(|(best, _)| dist < *best) {
                nearest = Some((dist, attachment.clone()));
            }

            if dist <= bone.attach_radius {
                // **Weighted by absolute distance, not by distance as a
                // fraction of the radius.** The normalised form is only
                // comparable between bones whose radii match: a far bone with
                // a generous radius scored higher than a near bone with a
                // tight one, so a fingertip could be owned by the chest and
                // stay behind when the arm moved. The radius decides *which*
                // bones may claim a vertex; distance decides which one wins.
                candidates.push(Attachment {
                    weight: 1.0 / (dist + NEAR).powf(FALLOFF),
                    ..attachment
                });
            }
        }

        let sum: f32 = candidates.iter().map(|a| a.weight).sum();
        if sum > 0.0 {
            for c in &mut candidates {
                c.weight /= sum;
            }
            entries.extend(candidates);
        } else if let Some((_, fallback)) = nearest {
            entries.push(fallback);
        }
    }

    entries.sort_by(|a, b| a.vertex.cmp(&b.vertex).then(a.bone.cmp(&b.bone)));

    AttachmentTable { entries }
}
