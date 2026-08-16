//! Ramer-Douglas-Peucker simplification for closed rings.

use glam::Vec2;

/// Perpendicular distance from `p` to the infinite line through `a` and `b`
/// (falls back to point-to-point distance if `a == b`).
fn perpendicular_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len = ab.length();
    if len < f32::EPSILON {
        return (p - a).length();
    }
    // |ab x ap| / |ab|
    let ap = p - a;
    ab.perp_dot(ap).abs() / len
}

/// Standard recursive RDP over an *open* polyline; keeps both endpoints.
fn rdp_open(points: &[Vec2], epsilon: f32) -> Vec<Vec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    let mut max_dist = 0.0f32;
    let mut split = 0usize;
    for (i, &p) in points.iter().enumerate().take(points.len() - 1).skip(1) {
        let d = perpendicular_distance(p, first, last);
        if d > max_dist {
            max_dist = d;
            split = i;
        }
    }
    if max_dist > epsilon {
        let mut left = rdp_open(&points[..=split], epsilon);
        let right = rdp_open(&points[split..], epsilon);
        left.pop(); // drop duplicate join point before appending `right`
        left.extend(right);
        left
    } else {
        vec![first, last]
    }
}

/// Simplifies a *closed* ring (no duplicated first/last point) with RDP.
///
/// Naively running RDP on the ring's point array as if it were an open
/// polyline would make the result depend on which point the array happens
/// to start at — that point and its opposite would always survive
/// untouched. Instead, this finds the two mutually most distant points on
/// the ring, splits it into two open halves at those points, simplifies
/// each half independently, and stitches the results back into a closed
/// ring. The split points are geometric properties of the ring's shape, not
/// of its array order, so the result is start-index-independent.
pub(super) fn simplify_closed_ring(ring: &[Vec2], epsilon: f32) -> Vec<Vec2> {
    let n = ring.len();
    if n < 4 {
        return ring.to_vec();
    }

    let mut best = (0.0f32, 0usize, 1usize);
    for i in 0..n {
        for j in (i + 1)..n {
            let d = ring[i].distance_squared(ring[j]);
            if d > best.0 {
                best = (d, i, j);
            }
        }
    }
    let (_, i, j) = best;

    let path_a: Vec<Vec2> = ring[i..=j].to_vec();
    let mut path_b: Vec<Vec2> = ring[j..].to_vec();
    path_b.extend_from_slice(&ring[..=i]);

    let mut sa = rdp_open(&path_a, epsilon);
    let mut sb = rdp_open(&path_b, epsilon);

    // sa: ring[i] .. ring[j], sb: ring[j] .. ring[i]. Drop each path's last
    // point (which duplicates the other path's first point) so the
    // concatenation is a closed ring with no repeated vertex.
    sa.pop();
    sb.pop();
    sa.extend(sb);
    sa
}
