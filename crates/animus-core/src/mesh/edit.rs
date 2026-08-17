//! Vertex deletion: the one place in the crate that removes mesh vertices.
//!
//! `MeshData::remove_vertices_internal` is `pub(crate)` on purpose — using
//! it alone leaves attachments (and any future vertex-index referrer)
//! dangling. `MeshPuppet::remove_vertices` is the only public deletion
//! path; see its doc comment for how the compile-time safety property
//! works.

use crate::doc::{AttachmentTable, MeshData, MeshPuppet};
use crate::remap::{IndexRemap, Remappable};

impl MeshData {
    /// Private: use `MeshPuppet::remove_vertices`, which also updates
    /// every other referrer. Calling this alone leaves attachments dangling.
    pub(crate) fn remove_vertices_internal(&mut self, victims: &[u32]) -> IndexRemap {
        let r = IndexRemap::from_deletions(self.positions.len() as u32, victims);

        let mut positions = Vec::with_capacity(r.new_len() as usize);
        let mut uvs = Vec::with_capacity(r.new_len() as usize);
        for old in 0..r.old_len() {
            if r.map(old).is_some() {
                positions.push(self.positions[old as usize]);
                // `uvs` is not guaranteed to be the same length as
                // `positions`: `invariants::validate` treats
                // `UvCountMismatch` as reportable but non-fatal, so a
                // hand-edited or third-party project.json can load with a
                // short (or missing) `uvs` array. `.get` + a default keeps
                // this compaction infallible instead of indexing straight
                // into a possibly-shorter array.
                uvs.push(self.uvs.get(old as usize).copied().unwrap_or_default());
            }
        }
        self.positions = positions;
        self.uvs = uvs;

        // A triangle touching a deleted vertex is dropped, never repaired.
        let mut tris = Vec::with_capacity(self.triangles.len());
        for tri in self.triangles.chunks_exact(3) {
            if let (Some(a), Some(b), Some(c)) = (r.map(tri[0]), r.map(tri[1]), r.map(tri[2])) {
                tris.extend_from_slice(&[a, b, c]);
            }
        }
        self.triangles = tris;

        r
    }
}

impl Remappable for AttachmentTable {
    fn remap_vertices(&mut self, r: &IndexRemap) {
        self.entries.retain_mut(|a| match r.map(a.vertex) {
            Some(new) => {
                a.vertex = new;
                true
            }
            None => false,
        });
    }
}

impl MeshPuppet {
    /// The ONLY public way to delete vertices.
    ///
    /// The destructuring below is exhaustive **on purpose**. Do not add
    /// `..`. If you add a field that stores vertex indices, this stops
    /// compiling until you handle it — which is the entire point.
    pub fn remove_vertices(&mut self, victims: &[u32]) -> IndexRemap {
        let MeshPuppet {
            texture: _,
            matte: _, // an alpha source, no vertex indices
            mesh,
            skeleton: _, // stores JointIds and BoneIds, never vertex indices
            attachments,
            material: _,
            solver_override: _,
        } = self;

        let r = mesh.remove_vertices_internal(victims);
        attachments.remap_vertices(&r);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::*;
    use crate::ids::{AssetId, BoneId};
    use glam::Vec2;

    fn quad() -> MeshData {
        // 0---1
        // | \ |
        // 2---3
        MeshData {
            positions: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(0.0, 10.0),
                Vec2::new(10.0, 10.0),
            ],
            uvs: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(1.0, 1.0),
            ],
            triangles: vec![0, 2, 3, 0, 3, 1],
            source: MeshSource::Manual,
        }
    }

    #[test]
    fn deleting_a_vertex_drops_its_triangles_and_reindexes_the_rest() {
        let mut m = quad();
        let r = m.remove_vertices_internal(&[1]);
        assert_eq!(m.positions.len(), 3);
        assert_eq!(m.uvs.len(), 3, "uvs must stay parallel to positions");
        // Triangle [0,3,1] referenced the victim and is gone.
        // Triangle [0,2,3] survives, remapped to [0,1,2].
        assert_eq!(m.triangles, vec![0, 1, 2]);
        assert_eq!(r.new_len(), 3);
    }

    #[test]
    fn a_short_uvs_array_does_not_panic_on_vertex_deletion() {
        // `invariants::validate` treats `UvCountMismatch` as reportable but
        // non-fatal, so a hand-edited or third-party project.json can load
        // cleanly with fewer uvs than positions. Deleting a vertex must not
        // panic on that document.
        let mut m = quad();
        m.uvs.pop(); // now shorter than positions
        let r = m.remove_vertices_internal(&[1]);
        assert_eq!(m.positions.len(), 3);
        assert_eq!(m.uvs.len(), 3, "uvs must stay parallel to positions");
        assert_eq!(r.new_len(), 3);
    }

    #[test]
    fn attachments_to_deleted_vertices_are_dropped_and_the_rest_reindexed() {
        let mut table = AttachmentTable {
            entries: vec![
                Attachment {
                    vertex: 0,
                    bone: BoneId(1),
                    weight: 1.0,
                    local: Vec2::ZERO,
                },
                Attachment {
                    vertex: 1,
                    bone: BoneId(1),
                    weight: 0.5,
                    local: Vec2::ZERO,
                },
                Attachment {
                    vertex: 3,
                    bone: BoneId(2),
                    weight: 0.7,
                    local: Vec2::ZERO,
                },
            ],
        };
        let r = IndexRemap::from_deletions(4, &[1]);
        table.remap_vertices(&r);

        assert_eq!(table.entries.len(), 2, "the attachment on vertex 1 is gone");
        assert_eq!(table.entries[0].vertex, 0);
        assert_eq!(table.entries[1].vertex, 2, "vertex 3 shifted down to 2");
        assert_eq!(table.entries[1].weight, 0.7, "weights survive unchanged");
    }

    #[test]
    fn remove_vertices_updates_every_referrer_at_once() {
        let mut mp = MeshPuppet::empty(AssetId(1));
        mp.mesh = quad();
        mp.attachments.entries.push(Attachment {
            vertex: 3,
            bone: BoneId(1),
            weight: 1.0,
            local: Vec2::ZERO,
        });
        mp.remove_vertices(&[0]);
        assert_eq!(mp.mesh.positions.len(), 3);
        assert_eq!(mp.attachments.entries[0].vertex, 2, "3 shifted down to 2");
        for t in &mp.mesh.triangles {
            assert!(
                (*t as usize) < mp.mesh.positions.len(),
                "no dangling triangle index"
            );
        }
    }
}
