//! Fallback ring builders that never fail on an image with at least one
//! opaque pixel, so a bad silhouette never blocks the artist.

use crate::silhouette::{Ring, signed_area};
use glam::Vec2;
use image::RgbaImage;

fn opaque_points(img: &RgbaImage, threshold: u8) -> Vec<Vec2> {
    img.enumerate_pixels()
        .filter(|(_, _, p)| p.0[3] >= threshold)
        .map(|(x, y, _)| Vec2::new(x as f32, y as f32))
        .collect()
}

fn cross(o: Vec2, a: Vec2, b: Vec2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Monotone-chain convex hull over the given points.
fn convex_hull(mut points: Vec<Vec2>) -> Vec<Vec2> {
    points.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap()
            .then(a.y.partial_cmp(&b.y).unwrap())
    });
    points.dedup();
    let n = points.len();
    if n < 3 {
        return points;
    }

    let mut lower: Vec<Vec2> = Vec::new();
    for &p in &points {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<Vec2> = Vec::new();
    for &p in points.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Convex hull of the opaque pixels, as a CCW outer [`Ring`].
///
/// Always succeeds on any image containing at least one opaque pixel
/// (degenerate inputs with 0-2 distinct opaque pixel positions just yield a
/// degenerate "ring" with fewer than 3 points).
pub fn convex_hull_ring(img: &RgbaImage, threshold: u8) -> Ring {
    let points = opaque_points(img, threshold);
    let mut hull = convex_hull(points);
    if signed_area(&hull) < 0.0 {
        hull.reverse();
    }
    Ring {
        points: hull,
        is_hole: false,
    }
}

/// Axis-aligned bounding box of the opaque pixels, as a CCW outer [`Ring`].
///
/// Always succeeds on any image containing at least one opaque pixel.
pub fn bounding_box_ring(img: &RgbaImage, threshold: u8) -> Ring {
    let points = opaque_points(img, threshold);
    if points.is_empty() {
        return Ring {
            points: Vec::new(),
            is_hole: false,
        };
    }
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for p in &points {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    let mut corners = vec![
        Vec2::new(min_x, min_y),
        Vec2::new(max_x, min_y),
        Vec2::new(max_x, max_y),
        Vec2::new(min_x, max_y),
    ];
    if signed_area(&corners) < 0.0 {
        corners.reverse();
    }
    Ring {
        points: corners,
        is_hole: false,
    }
}
