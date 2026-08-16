//! In-house marching squares over a binary mask.
//!
//! Written in-house rather than pulling in the `contour` crate: that crate
//! has been unmaintained since April 2024, and the topology step (Task 10
//! Step 7) needs direct control over ring output — in particular, being
//! able to trust that a hole boundary and its outer boundary come out as
//! two separate, independently-oriented rings.
//!
//! # Sampling convention
//!
//! The mask is treated as a scalar field sampled at pixel *centres*,
//! addressed here by their integer pixel coordinates. A "cell" sits between
//! four adjacent pixel-coordinate corners `(cx,cy)`, `(cx+1,cy)`,
//! `(cx,cy+1)`, `(cx+1,cy+1)`. Boundary crossings land at the midpoints of
//! cell edges, e.g. `(cx+0.5, cy)`. Pixels outside the image are treated as
//! "outside" (0), so the traced boundary of a fully-opaque `w`x`h` image
//! runs from `-0.5` to `w - 0.5`, i.e. it has width `w` — it hugs the true
//! edges of the image, not the centres of the border pixels.
//!
//! # Saddle resolution (cases 5 and 10)
//!
//! Configurations 5 (`1010`: TL and BR "inside", TR and BL "outside") and
//! 10 (the complement) are ambiguous: the same four corners admit two
//! different connectivities. This implementation resolves every cell,
//! saddle or not, with one uniform rule (see [`trace_rings`] below): walk
//! the four corners in a fixed cyclic order (TL, TR, BR, BL) and pair each
//! "inside→outside" transition with the next "outside→inside" transition
//! encountered going around that cycle. For a saddle this produces two
//! segments that each cut off one of the two "outside" corners individually
//! — equivalent to treating the two diagonal "inside" corners as connected
//! through the cell centre. This is applied identically to every cell, so
//! it can't produce inconsistent, self-intersecting rings.

use glam::Vec2;
use image::GrayImage;
use std::collections::HashMap;

#[derive(Clone, Copy)]
enum Event {
    Start(Vec2),
    End(Vec2),
}

/// Traces closed rings from a binary mask (pixel value `>= threshold` is
/// "inside"). Returned rings are in image space (Y down), each a closed
/// polygon with no duplicated first/last point, in unspecified (not
/// necessarily consistent) winding — [`crate::silhouette::topology`]
/// normalizes winding afterwards.
pub fn trace_rings(mask: &GrayImage, threshold: u8) -> Vec<Vec<Vec2>> {
    let (w, h) = mask.dimensions();
    let inside = |x: i64, y: i64| -> u8 {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            0
        } else if mask.get_pixel(x as u32, y as u32).0[0] >= threshold {
            1
        } else {
            0
        }
    };

    let mut segments: Vec<(Vec2, Vec2)> = Vec::new();

    for cy in -1..h as i64 {
        for cx in -1..w as i64 {
            let tl = inside(cx, cy);
            let tr = inside(cx + 1, cy);
            let br = inside(cx + 1, cy + 1);
            let bl = inside(cx, cy + 1);
            if tl == tr && tr == br && br == bl {
                continue; // uniform cell: no boundary passes through it
            }

            let n = Vec2::new(cx as f32 + 0.5, cy as f32);
            let e = Vec2::new(cx as f32 + 1.0, cy as f32 + 0.5);
            let s = Vec2::new(cx as f32 + 0.5, cy as f32 + 1.0);
            let west = Vec2::new(cx as f32, cy as f32 + 0.5);

            // Walk corners TL -> TR -> BR -> BL -> (TL), the four edges
            // between consecutive corners being N, E, S, W respectively.
            let corner_pairs = [(tl, tr, n), (tr, br, e), (br, bl, s), (bl, tl, west)];

            let mut events: Vec<Event> = Vec::with_capacity(4);
            for (v1, v2, mid) in corner_pairs {
                if v1 == 1 && v2 == 0 {
                    events.push(Event::Start(mid));
                } else if v1 == 0 && v2 == 1 {
                    events.push(Event::End(mid));
                }
            }

            // Transitions around a cyclic 0/1 sequence strictly alternate
            // direction, so `events` alternates Start/End regardless of
            // where it starts. Pairing each Start with the very next event
            // (wrapping) is therefore always a Start-End pair.
            let count = events.len();
            for i in 0..count {
                if let Event::Start(p) = events[i]
                    && let Event::End(q) = events[(i + 1) % count]
                {
                    segments.push((p, q));
                }
            }
        }
    }

    assemble_rings(&segments)
}

/// Half-integer coordinates (all crossing points land on a 0.5-spaced
/// grid), hashed exactly by doubling and rounding to the nearest integer.
fn key(p: Vec2) -> (i64, i64) {
    ((p.x * 2.0).round() as i64, (p.y * 2.0).round() as i64)
}

fn assemble_rings(segments: &[(Vec2, Vec2)]) -> Vec<Vec<Vec2>> {
    let mut next: HashMap<(i64, i64), Vec2> = HashMap::with_capacity(segments.len());
    for &(s, e) in segments {
        next.insert(key(s), e);
    }

    let mut visited: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    let mut rings = Vec::new();

    for &(start, _) in segments {
        let k0 = key(start);
        if visited.contains(&k0) {
            continue;
        }
        let mut ring = Vec::new();
        let mut cur = start;
        loop {
            let k = key(cur);
            if !visited.insert(k) {
                break; // defensive: shouldn't happen for well-formed input
            }
            ring.push(cur);
            match next.get(&k) {
                Some(&nxt) if key(nxt) == k0 => break, // closed the loop
                Some(&nxt) => cur = nxt,
                None => break, // defensive: dangling edge, shouldn't happen
            }
        }
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }

    rings
}
