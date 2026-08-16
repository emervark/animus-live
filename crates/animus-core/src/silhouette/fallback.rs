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
        while lower.len() >= 2
            && (lower[lower.len() - 1] - lower[lower.len() - 2])
                .perp_dot(p - lower[lower.len() - 2])
                <= 0.0
        {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<Vec2> = Vec::new();
    for &p in points.iter().rev() {
        while upper.len() >= 2
            && (upper[upper.len() - 1] - upper[upper.len() - 2])
                .perp_dot(p - upper[upper.len() - 2])
                <= 0.0
        {
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

/// Tiles the opaque-pixel bounding box into a regular grid of square outer
/// [`Ring`]s, each `cell_size` px on a side (rows/columns at the far edge
/// clipped to the bounding box). `cell_size <= 0.0` falls back to a quarter
/// of the box's longer side (at least 1px), so a caller passing
/// `interior_spacing_px` unmodified always gets a sane tiling.
///
/// Unlike [`bounding_box_ring`]'s single quad, a grid of many small quads
/// gives the CDT real internal edges to deform along — this is the
/// distinct "bounding-box grid" fallback of spec §6.2, not a second name
/// for a plain bounding box.
///
/// Always succeeds (returns `Vec::new()`) on an image with no opaque
/// pixels; never panics.
pub fn grid_ring(img: &RgbaImage, threshold: u8, cell_size: f32) -> Vec<Ring> {
    let points = opaque_points(img, threshold);
    if points.is_empty() {
        return Vec::new();
    }
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for p in &points {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }

    let cell = if cell_size > 0.0 {
        cell_size
    } else {
        ((max_x - min_x).max(max_y - min_y) / 4.0).max(1.0)
    };

    let mut rings = Vec::new();
    let mut y = min_y;
    while y < max_y {
        let y1 = (y + cell).min(max_y);
        let mut x = min_x;
        while x < max_x {
            let x1 = (x + cell).min(max_x);
            let mut corners = vec![
                Vec2::new(x, y),
                Vec2::new(x1, y),
                Vec2::new(x1, y1),
                Vec2::new(x, y1),
            ];
            if signed_area(&corners) < 0.0 {
                corners.reverse();
            }
            rings.push(Ring {
                points: corners,
                is_hole: false,
            });
            x += cell;
        }
        y += cell;
    }
    rings
}
