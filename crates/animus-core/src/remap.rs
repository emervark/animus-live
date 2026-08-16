//! Vertex index remapping.
//!
//! Deleting mesh vertices invalidates every stored vertex index in the
//! puppet — triangles, attachments, selection, pins. `IndexRemap` is the
//! single object that describes such a deletion, and `Remappable` is how
//! every referrer applies it.
//!
//! The safety property is enforced in `MeshPuppet::remove_vertices`,
//! which destructures `Self` exhaustively **without `..`**. Adding a new
//! field that stores vertex indices will fail to compile until it is
//! handled there. Do not add `..` to that destructuring.

/// Describes a vertex deletion: which old indices survive, and where they moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRemap {
    old_to_new: Vec<Option<u32>>,
    new_len: u32,
}

impl IndexRemap {
    /// Build a remap for deleting `victims` from a mesh of `old_len` vertices.
    /// Victims may be unsorted, duplicated, or out of range.
    pub fn from_deletions(old_len: u32, victims: &[u32]) -> Self {
        let mut doomed = vec![false; old_len as usize];
        for &v in victims {
            if (v as usize) < doomed.len() {
                doomed[v as usize] = true;
            }
        }
        let mut old_to_new = Vec::with_capacity(old_len as usize);
        let mut next = 0u32;
        for is_doomed in doomed {
            if is_doomed {
                old_to_new.push(None);
            } else {
                old_to_new.push(Some(next));
                next += 1;
            }
        }
        Self {
            old_to_new,
            new_len: next,
        }
    }

    /// The new index for an old one, or `None` if it was deleted.
    pub fn map(&self, old: u32) -> Option<u32> {
        self.old_to_new.get(old as usize).copied().flatten()
    }

    pub fn is_deleted(&self, old: u32) -> bool {
        self.map(old).is_none()
    }

    /// Vertex count after the deletion.
    pub fn new_len(&self) -> u32 {
        self.new_len
    }

    pub fn old_len(&self) -> u32 {
        self.old_to_new.len() as u32
    }
}

/// Implemented by every type that stores a vertex index.
pub trait Remappable {
    fn remap_vertices(&mut self, r: &IndexRemap);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_shifts_survivors_down_and_marks_victims() {
        // 5 vertices, delete 1 and 3 -> survivors 0,2,4 become 0,1,2
        let r = IndexRemap::from_deletions(5, &[1, 3]);
        assert_eq!(r.map(0), Some(0));
        assert_eq!(r.map(1), None);
        assert_eq!(r.map(2), Some(1));
        assert_eq!(r.map(3), None);
        assert_eq!(r.map(4), Some(2));
        assert_eq!(r.new_len(), 3);
    }

    #[test]
    fn duplicate_and_unsorted_victims_are_handled() {
        let r = IndexRemap::from_deletions(4, &[3, 1, 1, 3]);
        assert_eq!(r.new_len(), 2);
        assert_eq!(r.map(0), Some(0));
        assert_eq!(r.map(2), Some(1));
    }

    #[test]
    fn out_of_range_victims_are_ignored() {
        let r = IndexRemap::from_deletions(3, &[99]);
        assert_eq!(r.new_len(), 3);
    }

    #[test]
    fn deleting_nothing_is_the_identity() {
        let r = IndexRemap::from_deletions(3, &[]);
        for i in 0..3 {
            assert_eq!(r.map(i), Some(i));
        }
    }
}
