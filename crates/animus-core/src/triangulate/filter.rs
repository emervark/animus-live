//! Post-triangulation filtering.
//!
//! The CDT (`cdt.rs`) guarantees every ring segment survives as a mesh
//! edge, but it still triangulates the whole convex hull of the inserted
//! points — including the interior of holes and the concave "notch" of a
//! non-convex outline like an L-shape. This is a genuine correctness step,
//! not a leftover from a pre-CDT design: without it, a hole's interior and
//! a concave exterior both come back tiled with triangles.

use crate::silhouette::Ring;
use crate::silhouette::topology::point_in_polygon;
use glam::Vec2;

/// True if `p` lies inside at least one outer ring and inside no hole
/// ring. Used both to keep interior Poisson-disc samples on-shape
/// (`points.rs`) and to filter triangle centroids (below).
///
/// Point-in-polygon itself is `silhouette::topology::point_in_polygon`
/// (widened to `pub(crate)` for this), not a second copy of the same
/// ray-casting predicate — `silhouette` and `triangulate` must agree on
/// what "inside" means, or hole classification and triangle filtering
/// could disagree about the same point.
pub(super) fn inside_shape(p: Vec2, rings: &[Ring]) -> bool {
    let in_outer = rings
        .iter()
        .filter(|r| !r.is_hole)
        .any(|r| point_in_polygon(p, &r.points));
    if !in_outer {
        return false;
    }
    !rings
        .iter()
        .filter(|r| r.is_hole)
        .any(|r| point_in_polygon(p, &r.points))
}

/// Drops triangles whose centroid is outside every outer ring or inside
/// any hole, and drops degenerate (near-zero-area) triangles. Surviving
/// triangles are re-wound CCW.
pub(super) fn filter_triangles(
    positions: &[Vec2],
    triangles: &[[u32; 3]],
    rings: &[Ring],
) -> Vec<[u32; 3]> {
    triangles
        .iter()
        .filter_map(|&[a, b, c]| {
            let (pa, pb, pc) = (
                positions[a as usize],
                positions[b as usize],
                positions[c as usize],
            );
            let cross = (pb - pa).perp_dot(pc - pa);
            if cross.abs() < 1e-3 {
                return None; // degenerate
            }
            let centroid = (pa + pb + pc) / 3.0;
            if !inside_shape(centroid, rings) {
                return None; // inside a hole, or in the concave exterior
            }
            Some(if cross < 0.0 { [a, c, b] } else { [a, b, c] })
        })
        .collect()
}
