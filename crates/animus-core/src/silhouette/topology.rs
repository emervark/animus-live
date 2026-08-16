//! Ring classification, winding normalization, and self-intersection
//! cleanup.

use crate::silhouette::Ring;
use glam::Vec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::float::simplify::SimplifyShape;

/// Signed area of a closed polygon (implicit edge from the last point back
/// to the first) via the shoelace formula.
///
/// Image space is Y-down. This is the *un-flipped* shoelace formula: no
/// axis correction is applied. In that space, this is positive for a ring
/// that runs counter-clockwise as drawn on screen, negative for clockwise.
/// [`build_rings`] normalizes outer rings to positive and holes to
/// negative, so downstream code (including Task 11's triangulator) can rely
/// on that sign without re-deriving it.
pub fn signed_area(points: &[Vec2]) -> f32 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    for i in 0..n {
        let p0 = points[i];
        let p1 = points[(i + 1) % n];
        sum += p0.x * p1.y - p1.x * p0.y;
    }
    sum * 0.5
}

/// Ray-casting point-in-polygon test (even-odd rule).
fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let n = poly.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let pi = poly[i];
        let pj = poly[j];
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

/// A point guaranteed to be strictly interior to `ring` (not just one of
/// its vertices, which can sit exactly on another ring's edge and make
/// point-in-polygon results for *that* other ring undefined).
///
/// Uses the centroid of the first non-degenerate triangle fanned from
/// `ring[0]` — cheap, and for any simple polygon with at least 3 non
/// collinear points in a row, that centroid is strictly inside the
/// triangle and (for a simple polygon) strictly inside the polygon too.
fn interior_point(ring: &[Vec2]) -> Vec2 {
    let n = ring.len();
    for i in 1..n.saturating_sub(1) {
        let (a, b, c) = (ring[0], ring[i], ring[i + 1]);
        let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if cross.abs() > f32::EPSILON {
            return (a + b + c) / 3.0;
        }
    }
    // Degenerate (collinear) ring: fall back to the first vertex. This
    // can't happen for anything that survived the min-area filter, but
    // never panic here.
    ring.first().copied().unwrap_or(Vec2::ZERO)
}

/// Runs a single contour through `i_overlay`'s self-union (a shape unioned
/// with itself under a non-zero fill rule) to remove self-intersections
/// that RDP's line-simplification can introduce. If the union yields
/// multiple disjoint pieces, keeps the outer contour of the largest one by
/// area. Falls back to the input unchanged if the union is degenerate
/// (shouldn't happen for a ring with >= 3 points, but never panic here).
fn self_union_largest(points: &[Vec2]) -> Vec<Vec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let contour: Vec<[f32; 2]> = points.iter().map(|p| [p.x, p.y]).collect();
    let shapes = contour.simplify_shape(FillRule::NonZero);

    let mut best: Option<(f32, Vec<Vec2>)> = None;
    for shape in &shapes {
        let Some(outer) = shape.first() else { continue };
        let pts: Vec<Vec2> = outer.iter().map(|p| Vec2::new(p[0], p[1])).collect();
        let area = signed_area(&pts).abs();
        if best.as_ref().is_none_or(|(a, _)| area > *a) {
            best = Some((area, pts));
        }
    }

    match best {
        Some((_, pts)) => pts,
        None => points.to_vec(),
    }
}

/// Turns simplified (but still raw: unclassified, arbitrary-winding, and
/// possibly self-intersecting) polygons into final [`Ring`]s.
///
/// Follows the brief's pipeline order:
/// 1. Drop rings below `min_area`.
/// 2. Classify each remaining ring by containment-depth *parity*: count how
///    many other candidate rings contain it (via a point known to be
///    strictly interior to it, not just one of its vertices), and take the
///    parity of that count — even means outer, odd means hole. A single
///    "contained in anything => hole" check is wrong past one level of
///    nesting: an island sitting inside a hole inside an outer body is
///    contained in *two* rings, and must come back as outer, not a hole.
/// 3. Normalize winding to the convention documented on [`signed_area`].
/// 4. Run each ring through self-union to remove self-intersections.
/// 5. Sort: outer rings first (descending area), then holes.
///
/// Winding is re-checked and, if necessary, re-flipped after step 4 as
/// well: `i_overlay` is free to return its own canonical orientation for a
/// shape's outer contour, which need not match the orientation this crate
/// picked in step 3.
pub(super) fn build_rings(raw: Vec<Vec<Vec2>>, min_area: f32) -> Vec<Ring> {
    let candidates: Vec<Vec<Vec2>> = raw
        .into_iter()
        .filter(|r| signed_area(r).abs() >= min_area)
        .collect();

    let n = candidates.len();
    let interior: Vec<Vec2> = candidates.iter().map(|c| interior_point(c)).collect();
    let mut is_hole = vec![false; n];
    for i in 0..n {
        let depth = (0..n)
            .filter(|&j| j != i && point_in_polygon(interior[i], &candidates[j]))
            .count();
        is_hole[i] = depth % 2 == 1;
    }

    let mut rings: Vec<Ring> = Vec::with_capacity(n);
    for (i, pts) in candidates.into_iter().enumerate() {
        let hole = is_hole[i];
        let target_positive = !hole;

        let mut pts = pts;
        if (signed_area(&pts) > 0.0) != target_positive {
            pts.reverse();
        }

        let mut cleaned = self_union_largest(&pts);
        if (signed_area(&cleaned) > 0.0) != target_positive {
            cleaned.reverse();
        }

        rings.push(Ring {
            points: cleaned,
            is_hole: hole,
        });
    }

    rings.sort_by(|a, b| match (a.is_hole, b.is_hole) {
        (false, true) => std::cmp::Ordering::Less,
        (true, false) => std::cmp::Ordering::Greater,
        _ => {
            let aa = signed_area(&a.points).abs();
            let ab = signed_area(&b.points).abs();
            ab.partial_cmp(&aa).unwrap_or(std::cmp::Ordering::Equal)
        }
    });

    rings
}
