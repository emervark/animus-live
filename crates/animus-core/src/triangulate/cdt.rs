//! Constrained Delaunay triangulation via `spade` 2.15.
//!
//! # `spade` API, confirmed against docs.rs/spade/2.15 before writing this
//! file (task brief Step 1)
//!
//! - **Construct**: `ConstrainedDelaunayTriangulation<V>` where `V:
//!   HasPosition`. `HasPosition` requires `type Scalar` and
//!   `fn position(&self) -> Point2<Self::Scalar>`. Construction is
//!   `ConstrainedDelaunayTriangulation::<V>::new() -> Self` (an inherent
//!   associated function; `with_capacity(..)` also exists but is not
//!   needed here).
//! - **Insert a vertex, get a handle back** (via the `Triangulation`
//!   trait): `fn insert(&mut self, vertex: V) -> Result<FixedVertexHandle,
//!   InsertionError>`. `InsertionError` has three variants — `TooSmall`,
//!   `TooLarge`, `NAN` — none reachable from finite in-range pixel
//!   coordinates, but the `Result` is still propagated, not unwrapped.
//! - **Add a constraint edge between two handles**: there are two
//!   relevant methods.
//!   - `fn add_constraint(&mut self, from: FixedVertexHandle, to:
//!     FixedVertexHandle) -> bool` — **panics** if the new edge
//!     intersects an existing constraint edge.
//!   - `fn try_add_constraint(&mut self, from: FixedVertexHandle, to:
//!     FixedVertexHandle) -> Vec<FixedDirectedEdgeHandle>` — the
//!     non-panicking sibling. Leaves the triangulation unchanged and
//!     returns an **empty `Vec`** if the new edge would intersect an
//!     existing constraint edge (or if `from == to`). We use this one and
//!     treat an empty result (after excluding `from == to` ourselves) as
//!     [`TriangulateError::ConstraintFailed`].
//! - **Iterate the resulting faces**: `fn inner_faces(&self) ->
//!   InnerFaceIterator<..>`, skipping the outer face. Each yielded
//!   `FaceHandle<InnerTag, ..>` has `fn positions(&self) -> [Point2<V::
//!   Scalar>; 3]`, already in CCW order — exactly the raw (px, py) triples
//!   we need; no separate vertex-handle-to-index bookkeeping is required.
//! - **Error type on intersecting constraint edges**: as above, `spade`
//!   does not have a dedicated error enum for this — `try_add_constraint`
//!   signals it by returning an empty `Vec` rather than an `Err`.

use crate::silhouette::Ring;
use crate::triangulate::TriangulateError;
use glam::Vec2;
use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, Triangulation};
use std::collections::HashMap;

/// Thin wrapper so `glam::Vec2` (which `spade` has no impl for) can serve
/// as CDT vertex data.
struct VertexData(Vec2);

impl HasPosition for VertexData {
    type Scalar = f32;

    fn position(&self) -> Point2<f32> {
        Point2::new(self.0.x, self.0.y)
    }
}

/// The CDT's raw output: every inserted position (ring points + interior
/// Poisson-disc points that survived as a face corner) and every inner
/// face as an index triple. Unfiltered — still includes triangles inside
/// holes and in concave exterior regions; see `filter`.
pub(super) struct RawMesh {
    pub positions: Vec<Vec2>,
    pub triangles: Vec<[u32; 3]>,
}

/// Bit-exact position dedup: `FaceHandle::positions()` returns copies of
/// the exact `f32`s stored at insertion, so two faces sharing a vertex
/// return bit-identical `Point2`s for it — no epsilon comparison needed.
fn dedup_index(p: Vec2, positions: &mut Vec<Vec2>, index_of: &mut HashMap<(u32, u32), u32>) -> u32 {
    let key = (p.x.to_bits(), p.y.to_bits());
    *index_of.entry(key).or_insert_with(|| {
        positions.push(p);
        (positions.len() - 1) as u32
    })
}

/// Builds the CDT: ring points as constrained boundary vertices (every
/// ring segment, including the closing edge, becomes a constraint edge so
/// the silhouette outline is guaranteed to survive as mesh edges), then
/// `interior` points as free (unconstrained) vertices.
pub(super) fn build(rings: &[Ring], interior: &[Vec2]) -> Result<RawMesh, TriangulateError> {
    let mut cdt: ConstrainedDelaunayTriangulation<VertexData> =
        ConstrainedDelaunayTriangulation::new();

    for ring in rings {
        let pts = &ring.points;
        let n = pts.len();
        if n < 3 {
            continue; // degenerate ring, nothing to constrain
        }
        let mut handles = Vec::with_capacity(n);
        for &p in pts {
            let h = cdt
                .insert(VertexData(p))
                .map_err(|e| TriangulateError::InsertionFailed(format!("{e:?}")))?;
            handles.push(h);
        }
        for i in 0..n {
            let (a, b) = (handles[i], handles[(i + 1) % n]); // includes the closing edge
            if a == b {
                continue;
            }
            if cdt.try_add_constraint(a, b).is_empty() {
                return Err(TriangulateError::ConstraintFailed);
            }
        }
    }

    for &p in interior {
        cdt.insert(VertexData(p))
            .map_err(|e| TriangulateError::InsertionFailed(format!("{e:?}")))?;
    }

    let mut positions = Vec::new();
    let mut index_of = HashMap::new();
    let mut triangles = Vec::new();
    for face in cdt.inner_faces() {
        let corners = face.positions();
        let mut tri = [0u32; 3];
        for (i, pt) in corners.iter().enumerate() {
            tri[i] = dedup_index(Vec2::new(pt.x, pt.y), &mut positions, &mut index_of);
        }
        triangles.push(tri);
    }

    Ok(RawMesh {
        positions,
        triangles,
    })
}
