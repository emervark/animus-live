//! Poisson-disc interior point sampling (Bridson's algorithm), seeded by a
//! small xorshift PRNG.
//!
//! Poisson-disc rather than a regular grid: a lattice produces visible row
//! artifacts as the mesh deforms — the eye picks up the grid structure
//! moving with the puppet. Poisson-disc gives "random but never too
//! close together" points, which stays looking organic under deformation.
//!
//! No `rand` dependency: `MeshSource::Auto` must be able to regenerate an
//! identical mesh from the same `seed`, and a ~20-line xorshift is enough
//! for that without pulling in a crate.

use super::filter::inside_shape;
use crate::silhouette::Ring;
use glam::Vec2;

/// A small, fast, deterministic PRNG (xorshift64, with a final multiply to
/// improve output mixing — "xorshift64*"). Not cryptographic; only needs
/// to be reproducible given a seed and reasonably well distributed.
struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        // The all-zero state is a fixed point of xorshift; substitute a
        // fixed nonzero constant so seed == 0 still produces a real stream.
        Self {
            state: if seed == 0 { 0x9E3779B97F4A7C15 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform float in `[0, 1)`.
    fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform float in `[lo, hi)`.
    fn next_range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next_f32() * (hi - lo)
    }
}

struct Bbox {
    min: Vec2,
    max: Vec2,
}

/// Bounding box of every outer ring's points (holes don't extend the
/// sampling domain).
fn outer_bbox(rings: &[Ring]) -> Option<Bbox> {
    let mut min = Vec2::splat(f32::MAX);
    let mut max = Vec2::splat(f32::MIN);
    let mut any = false;
    for r in rings.iter().filter(|r| !r.is_hole) {
        for &p in &r.points {
            min = min.min(p);
            max = max.max(p);
            any = true;
        }
    }
    any.then_some(Bbox { min, max })
}

/// Bridson's Poisson-disc sampling over the union of `rings`' outer
/// boundaries minus holes, with minimum spacing `spacing`. Deterministic:
/// the same `rings`, `spacing` and `seed` always produce the same points
/// in the same order.
pub fn poisson_disc(rings: &[Ring], spacing: f32, seed: u64) -> Vec<Vec2> {
    if spacing <= 0.0 {
        return Vec::new();
    }
    let Some(bbox) = outer_bbox(rings) else {
        return Vec::new();
    };
    let (w, h) = (bbox.max.x - bbox.min.x, bbox.max.y - bbox.min.y);
    if w <= 0.0 || h <= 0.0 {
        return Vec::new();
    }

    let mut rng = Xorshift64::new(seed);

    // Background grid: cell size spacing/sqrt(2) guarantees at most one
    // accepted point per cell, so a 5x5 neighborhood search is sufficient
    // to find every point that could violate the minimum spacing.
    let cell = spacing / std::f32::consts::SQRT_2;
    let cols = ((w / cell).floor() as isize + 1).max(1);
    let rows = ((h / cell).floor() as isize + 1).max(1);
    let mut grid: Vec<Option<usize>> = vec![None; (cols * rows) as usize];

    let cell_of = |p: Vec2| -> (isize, isize) {
        (
            ((p.x - bbox.min.x) / cell).floor() as isize,
            ((p.y - bbox.min.y) / cell).floor() as isize,
        )
    };

    let mut points: Vec<Vec2> = Vec::new();
    let mut active: Vec<usize> = Vec::new();

    // Rejection-sample a first point that actually lies in the shape (the
    // outer rings' union minus holes), not just somewhere in the bbox.
    let mut first = None;
    for _ in 0..2000 {
        let cand = Vec2::new(
            rng.next_range(bbox.min.x, bbox.max.x),
            rng.next_range(bbox.min.y, bbox.max.y),
        );
        if inside_shape(cand, rings) {
            first = Some(cand);
            break;
        }
    }
    let Some(first) = first else {
        return Vec::new();
    };
    points.push(first);
    active.push(0);
    let (fx, fy) = cell_of(first);
    grid[(fy * cols + fx) as usize] = Some(0);

    const K: u32 = 30; // candidates tried per active point, Bridson's default

    while !active.is_empty() {
        let pick = ((rng.next_f32() * active.len() as f32) as usize).min(active.len() - 1);
        let origin = points[active[pick]];

        let mut placed = false;
        for _ in 0..K {
            let angle = rng.next_range(0.0, std::f32::consts::TAU);
            let radius = rng.next_range(spacing, 2.0 * spacing);
            let cand = origin + Vec2::new(angle.cos(), angle.sin()) * radius;

            if cand.x < bbox.min.x
                || cand.x > bbox.max.x
                || cand.y < bbox.min.y
                || cand.y > bbox.max.y
            {
                continue;
            }
            if !inside_shape(cand, rings) {
                continue;
            }

            let (ccx, ccy) = cell_of(cand);
            let mut ok = true;
            'search: for gy in (ccy - 2).max(0)..=(ccy + 2).min(rows - 1) {
                for gx in (ccx - 2).max(0)..=(ccx + 2).min(cols - 1) {
                    if let Some(idx) = grid[(gy * cols + gx) as usize]
                        && points[idx].distance(cand) < spacing
                    {
                        ok = false;
                        break 'search;
                    }
                }
            }

            if ok {
                let new_idx = points.len();
                points.push(cand);
                active.push(new_idx);
                grid[(ccy * cols + ccx) as usize] = Some(new_idx);
                placed = true;
                break;
            }
        }

        if !placed {
            active.swap_remove(pick);
        }
    }

    points
}
