//! Post-triangulation filtering.
//!
//! The CDT (`cdt.rs`) guarantees every ring segment survives as a mesh
//! edge, but it still triangulates the whole convex hull of the inserted
//! points — including the interior of holes and the concave "notch" of a
//! non-convex outline like an L-shape. This is a genuine correctness step,
//! not a leftover from a pre-CDT design: without it, a hole's interior and
//! a concave exterior both come back tiled with triangles.

use crate::silhouette::Ring;
use glam::Vec2;

/// Ray-casting point-in-polygon (even-odd rule). Deliberately re-implemented
/// here rather than reused: the equivalent helper in
/// `silhouette::topology` is private to that module.
pub(super) fn point_in_ring(p: Vec2, ring: &[Vec2]) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let pi = ring[i];
        let pj = ring[j];
        if (pi.y > p.y) != (pj.y > p.y) {
            let x_at_p_y = (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x;
            if p.x < x_at_p_y {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// True if `p` lies inside at least one outer ring and inside no hole
/// ring. Used both to keep interior Poisson-disc samples on-shape
/// (`points.rs`) and to filter triangle centroids (below).
pub(super) fn inside_shape(p: Vec2, rings: &[Ring]) -> bool {
    let in_outer = rings
        .iter()
        .filter(|r| !r.is_hole)
        .any(|r| point_in_ring(p, &r.points));
    if !in_outer {
        return false;
    }
    !rings
        .iter()
        .filter(|r| r.is_hole)
        .any(|r| point_in_ring(p, &r.points))
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
