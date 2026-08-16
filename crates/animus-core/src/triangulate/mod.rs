//! Turns silhouette [`Ring`]s into a deformable triangle [`MeshData`].
//!
//! Pipeline: constrained Delaunay triangulation ([`cdt`]) over the ring
//! boundaries plus Poisson-disc-sampled interior points ([`points`]), then
//! centroid/degenerate filtering and winding normalization ([`filter`]),
//! then vertex compaction (reusing [`crate::remap::IndexRemap`] — see Task
//! 8, there is exactly one compaction path in this crate) and UV
//! assignment.

mod cdt;
mod filter;
mod points;

pub use points::poisson_disc;

use crate::doc::{AutoMeshParams, MeshData, MeshSource};
use crate::remap::IndexRemap;
use crate::silhouette::Ring;
use glam::Vec2;

#[derive(Debug, thiserror::Error)]
pub enum TriangulateError {
    /// A ring segment's constraint edge crosses another constraint edge —
    /// the silhouette is not a simple polygon after simplification. The
    /// caller walks the fallback ladder from spec §6.2 (smaller RDP
    /// epsilon, then `i_overlay` self-union, then convex hull, then
    /// bounding-box grid) so a bad silhouette never blocks the artist.
    #[error(
        "constraint edge insertion failed: the silhouette self-intersects after simplification"
    )]
    ConstraintFailed,
    /// `spade` rejected a vertex position outright (NaN, or out of its
    /// allowed coordinate range). Not reachable from ordinary pixel
    /// coordinates, but propagated rather than unwrapped.
    #[error("failed to insert a vertex into the triangulation: {0}")]
    InsertionFailed(String),
}

/// Fixed seed for interior Poisson-disc sampling. `triangulate` takes no
/// seed of its own: it must be deterministic given the same rings, params
/// and image size alone, so that re-running it (as `MeshSource::Auto`
/// does) reproduces an identical mesh. `poisson_disc` itself does take a
/// seed — this is just the one constant `triangulate` always calls it
/// with.
const AUTO_MESH_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// Turns silhouette `rings` into a triangle mesh: constrained Delaunay
/// triangulation over the ring boundaries plus Poisson-disc interior
/// points, filtered to the shape's interior, with UVs assigned from pixel
/// position (no Y flip — image space is already Y-down, matching wgpu's
/// UV convention).
pub fn triangulate(
    rings: &[Ring],
    params: &AutoMeshParams,
    img_size: (u32, u32),
) -> Result<MeshData, TriangulateError> {
    let interior = points::poisson_disc(rings, params.interior_spacing_px, AUTO_MESH_SEED);

    let raw = cdt::build(rings, &interior)?;
    let kept = filter::filter_triangles(&raw.positions, &raw.triangles, rings);

    // Compact away vertices no surviving triangle references. Reuses
    // Task 8's `IndexRemap` — the single vertex-compaction path in this
    // crate — rather than a second hand-rolled one.
    let mut referenced = vec![false; raw.positions.len()];
    for tri in &kept {
        for &i in tri {
            referenced[i as usize] = true;
        }
    }
    let victims: Vec<u32> = referenced
        .iter()
        .enumerate()
        .filter(|&(_, &r)| !r)
        .map(|(i, _)| i as u32)
        .collect();
    let remap = IndexRemap::from_deletions(raw.positions.len() as u32, &victims);

    let mut positions = vec![Vec2::ZERO; remap.new_len() as usize];
    for (old, p) in raw.positions.iter().enumerate() {
        if let Some(new) = remap.map(old as u32) {
            positions[new as usize] = *p;
        }
    }

    let mut triangles = Vec::with_capacity(kept.len() * 3);
    for tri in &kept {
        for &i in tri {
            triangles.push(
                remap
                    .map(i)
                    .expect("a kept triangle referenced a vertex the compaction deleted"),
            );
        }
    }

    let (w, h) = img_size;
    let uvs: Vec<Vec2> = positions
        .iter()
        .map(|p| *p / Vec2::new(w as f32, h as f32))
        .collect();

    Ok(MeshData {
        positions,
        uvs,
        triangles,
        source: MeshSource::Auto(params.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::silhouette::Ring;
    use glam::Vec2;

    fn square(size: f32) -> Ring {
        Ring {
            points: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(size, 0.0),
                Vec2::new(size, size),
                Vec2::new(0.0, size),
            ],
            is_hole: false,
        }
    }

    fn hole(cx: f32, cy: f32, r: f32) -> Ring {
        // CW winding for a hole.
        Ring {
            points: vec![
                Vec2::new(cx - r, cy - r),
                Vec2::new(cx - r, cy + r),
                Vec2::new(cx + r, cy + r),
                Vec2::new(cx + r, cy - r),
            ],
            is_hole: true,
        }
    }

    fn params(spacing: f32) -> crate::doc::AutoMeshParams {
        crate::doc::AutoMeshParams {
            alpha_threshold: 8,
            close_radius: 2,
            rdp_epsilon_px: 2.0,
            min_region_area_px: 64.0,
            interior_spacing_px: spacing,
            mode: crate::doc::AutoMeshMode::Silhouette,
        }
    }

    #[test]
    fn a_square_triangulates_into_a_valid_mesh() {
        let m = triangulate(&[square(100.0)], &params(25.0), (100, 100)).unwrap();
        assert!(!m.triangles.is_empty());
        assert_eq!(m.triangles.len() % 3, 0);
        assert_eq!(m.uvs.len(), m.positions.len());
        for i in &m.triangles {
            assert!((*i as usize) < m.positions.len());
        }
    }

    #[test]
    fn every_triangle_centroid_lies_inside_the_shape() {
        let m = triangulate(&[square(100.0)], &params(25.0), (100, 100)).unwrap();
        for t in m.triangles.chunks_exact(3) {
            let c = (m.positions[t[0] as usize]
                + m.positions[t[1] as usize]
                + m.positions[t[2] as usize])
                / 3.0;
            assert!(
                c.x > -0.01 && c.x < 100.01 && c.y > -0.01 && c.y < 100.01,
                "centroid {c:?} escaped the square"
            );
        }
    }

    #[test]
    fn no_triangle_lands_inside_a_hole() {
        let rings = vec![square(100.0), hole(50.0, 50.0, 20.0)];
        let m = triangulate(&rings, &params(15.0), (100, 100)).unwrap();
        for t in m.triangles.chunks_exact(3) {
            let c = (m.positions[t[0] as usize]
                + m.positions[t[1] as usize]
                + m.positions[t[2] as usize])
                / 3.0;
            let in_hole = c.x > 30.0 && c.x < 70.0 && c.y > 30.0 && c.y < 70.0;
            assert!(!in_hole, "triangle centroid {c:?} is inside the hole");
        }
    }

    #[test]
    fn the_boundary_survives_as_mesh_edges() {
        // This is what CDT buys us over plain Delaunay: an L-shaped
        // concavity must not be cut across.
        let l_shape = Ring {
            points: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(100.0, 0.0),
                Vec2::new(100.0, 40.0),
                Vec2::new(40.0, 40.0),
                Vec2::new(40.0, 100.0),
                Vec2::new(0.0, 100.0),
            ],
            is_hole: false,
        };
        let m = triangulate(&[l_shape], &params(20.0), (100, 100)).unwrap();
        for t in m.triangles.chunks_exact(3) {
            let c = (m.positions[t[0] as usize]
                + m.positions[t[1] as usize]
                + m.positions[t[2] as usize])
                / 3.0;
            // The notch is the region x>40 AND y>40.
            assert!(
                !(c.x > 41.0 && c.y > 41.0),
                "triangle centroid {c:?} spans the L-shape's notch"
            );
        }
    }

    #[test]
    fn no_zero_area_triangles_are_emitted() {
        let m = triangulate(&[square(100.0)], &params(25.0), (100, 100)).unwrap();
        for t in m.triangles.chunks_exact(3) {
            let (a, b, c) = (
                m.positions[t[0] as usize],
                m.positions[t[1] as usize],
                m.positions[t[2] as usize],
            );
            let cross = (b - a).perp_dot(c - a);
            assert!(cross.abs() > 1e-3, "degenerate triangle, cross = {cross}");
        }
    }

    #[test]
    fn uvs_are_normalized_pixel_coordinates_with_no_y_flip() {
        let m = triangulate(&[square(100.0)], &params(25.0), (200, 400)).unwrap();
        for (p, uv) in m.positions.iter().zip(&m.uvs) {
            assert!((uv.x - p.x / 200.0).abs() < 1e-5);
            assert!((uv.y - p.y / 400.0).abs() < 1e-5, "UVs must NOT flip in Y");
        }
    }

    #[test]
    fn total_mesh_area_matches_the_silhouette_area() {
        let rings = vec![square(100.0), hole(50.0, 50.0, 20.0)];
        let m = triangulate(&rings, &params(10.0), (100, 100)).unwrap();
        let mesh_area: f32 = m
            .triangles
            .chunks_exact(3)
            .map(|t| {
                let (a, b, c) = (
                    m.positions[t[0] as usize],
                    m.positions[t[1] as usize],
                    m.positions[t[2] as usize],
                );
                ((b - a).perp_dot(c - a) / 2.0).abs()
            })
            .sum();
        let want = 100.0 * 100.0 - 40.0 * 40.0; // 8400
        assert!(
            (mesh_area - want).abs() / want < 0.02,
            "mesh area {mesh_area} vs expected {want}"
        );
    }

    #[test]
    fn poisson_points_respect_the_minimum_spacing() {
        let pts = poisson_disc(&[square(200.0)], 20.0, 12345);
        assert!(pts.len() > 10);
        for (i, a) in pts.iter().enumerate() {
            for b in &pts[i + 1..] {
                assert!(
                    a.distance(*b) >= 20.0 * 0.99,
                    "points {a:?} and {b:?} are too close"
                );
            }
        }
    }

    #[test]
    fn poisson_sampling_is_reproducible_for_a_given_seed() {
        let a = poisson_disc(&[square(200.0)], 20.0, 7);
        let b = poisson_disc(&[square(200.0)], 20.0, 7);
        assert_eq!(a, b, "same seed must give the same points");
    }

    // Defensive-code coverage: `cdt::build`'s `n < 3` skip and the
    // no-outer-rings path in `points::poisson_disc` had no test exercising
    // them. Neither degenerate input should panic; both should come back
    // as a well-formed (possibly empty) mesh.

    #[test]
    fn an_empty_ring_slice_produces_an_empty_mesh_without_panicking() {
        let m = triangulate(&[], &params(25.0), (100, 100)).unwrap();
        assert!(m.positions.is_empty());
        assert!(m.triangles.is_empty());
        assert_eq!(m.uvs.len(), m.positions.len());
    }

    #[test]
    fn a_ring_with_fewer_than_three_points_is_skipped_without_panicking() {
        let degenerate = Ring {
            points: vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0)],
            is_hole: false,
        };
        let m = triangulate(&[degenerate], &params(25.0), (100, 100)).unwrap();
        assert_eq!(m.triangles.len() % 3, 0);
        assert_eq!(m.uvs.len(), m.positions.len());
        for i in &m.triangles {
            assert!((*i as usize) < m.positions.len());
        }
    }
}
