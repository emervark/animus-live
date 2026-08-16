//! Radius-falloff auto-attach: which bones influence which vertices, and
//! by how much. See spec §7.2.

use crate::doc::{Attachment, AttachmentTable, MeshData, SkeletonData};
use glam::Vec2;

/// Exponent on the linear radius falloff `(1.0 - dist / radius)`. Higher
/// values concentrate weight closer to the bone; the spec fixes this at
/// 2.0 rather than exposing it as a per-bone tuning knob.
const FALLOFF: f32 = 2.0;

/// Binds every mesh vertex to every bone within reach of it, weighted by
/// distance to the bone *segment* (not the infinite line through it), then
/// normalizes each vertex's weights to sum to 1.0.
///
/// A vertex with no bone within `attach_radius` gets no entries at all —
/// callers must not assume every vertex appears in the returned table.
///
/// `entries` is returned sorted by `(vertex, bone)` so the table
/// serializes deterministically.
pub fn auto_attach(mesh: &MeshData, skel: &SkeletonData) -> AttachmentTable {
    let mut entries = Vec::new();

    for (vertex, &pos) in mesh.positions.iter().enumerate() {
        let mut candidates: Vec<Attachment> = Vec::new();

        for bone in skel.bones.values() {
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

            if dist <= bone.attach_radius {
                let weight = (1.0 - dist / bone.attach_radius).powf(FALLOFF);
                candidates.push(Attachment {
                    vertex: vertex as u32,
                    bone: bone.id,
                    weight,
                    local,
                });
            }
        }

        let sum: f32 = candidates.iter().map(|a| a.weight).sum();
        if sum > 0.0 {
            for c in &mut candidates {
                c.weight /= sum;
            }
            entries.extend(candidates);
        }
    }

    entries.sort_by(|a, b| a.vertex.cmp(&b.vertex).then(a.bone.cmp(&b.bone)));

    AttachmentTable { entries }
}
