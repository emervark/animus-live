//! Silhouette extraction: turn an artist's alpha-channel PNG into a set of
//! closed image-space rings ready for triangulation.
//!
//! # Winding convention
//!
//! Image space is Y-down (row 0 is the top of the image). [`signed_area`]
//! is the plain shoelace formula with no axis flip. In this Y-down space,
//! that formula is **positive** for a ring that, drawn on screen, runs
//! counter-clockwise, and **negative** for one that runs clockwise. We take
//! that as the canonical convention for the whole crate: outer boundaries
//! are normalized to positive area (CCW), holes to negative area (CW).
//! The `triangulate` module must agree with this sign, not invert it.

mod alpha;
mod fallback;
mod marching;
mod rdp;
pub(crate) mod topology;

pub use fallback::{bounding_box_ring, convex_hull_ring, grid_ring};
pub use topology::signed_area;

use crate::doc::{AutoMeshMode, AutoMeshParams};
use glam::Vec2;
use image::RgbaImage;

/// A closed polygon in image space (Y down): an outer silhouette boundary,
/// or a hole cut out of one. See the module-level doc for the winding
/// convention (`is_hole == false` => CCW / positive area; `is_hole == true`
/// => CW / negative area).
#[derive(Debug, Clone)]
pub struct Ring {
    pub points: Vec<Vec2>,
    pub is_hole: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SilhouetteError {
    #[error("image contains no pixels at or above the alpha threshold")]
    NoOpaqueRegion,
}

/// A ring with real (non-degenerate) area — the bar a fallback ring must
/// clear before it counts as "usable" rather than a phantom fed by a
/// single opaque pixel or a handful of collinear ones.
fn is_usable(r: &Ring) -> bool {
    r.points.len() >= 3 && signed_area(&r.points).abs() > 0.0
}

/// Extracts silhouette rings from an image's alpha channel, dispatching on
/// `params.mode`:
///
/// - [`AutoMeshMode::Silhouette`]: alpha-threshold to a binary mask, close
///   (dilate then erode) to merge anti-aliasing speckle into the body,
///   trace with marching squares, simplify each ring with closed-ring RDP,
///   then classify/normalize/clean up topology. If every resulting region
///   is filtered out by `params.min_region_area_px` despite the image
///   having opaque pixels, falls back down the spec §6.2 ladder: convex
///   hull, then bounding box, rather than leaving the artist with nothing.
/// - [`AutoMeshMode::ConvexHull`]: [`convex_hull_ring`] directly, no
///   marching squares.
/// - [`AutoMeshMode::BoundingBox`]: [`bounding_box_ring`] directly.
/// - [`AutoMeshMode::Grid`]: [`grid_ring`] directly, tiled at
///   `params.interior_spacing_px`.
///
/// An image with no pixel at or above `params.alpha_threshold` is a genuine
/// error ([`SilhouetteError::NoOpaqueRegion`]) in every mode: there is
/// nothing to extract. In `Silhouette` mode, an image that *does* have
/// opaque pixels, but where every resulting region falls below
/// `params.min_region_area_px` AND the fallback ladder also finds nothing
/// usable, is not an error — a too-small region is a normal, expected
/// filtering outcome, not a failure of the algorithm — so that case
/// returns `Ok(vec![])`.
pub fn extract(img: &RgbaImage, params: &AutoMeshParams) -> Result<Vec<Ring>, SilhouetteError> {
    match params.mode {
        AutoMeshMode::ConvexHull => {
            let hull = convex_hull_ring(img, params.alpha_threshold);
            return if is_usable(&hull) {
                Ok(vec![hull])
            } else {
                Err(SilhouetteError::NoOpaqueRegion)
            };
        }
        AutoMeshMode::BoundingBox => {
            let bbox = bounding_box_ring(img, params.alpha_threshold);
            return if is_usable(&bbox) {
                Ok(vec![bbox])
            } else {
                Err(SilhouetteError::NoOpaqueRegion)
            };
        }
        AutoMeshMode::Grid => {
            let grid = grid_ring(img, params.alpha_threshold, params.interior_spacing_px);
            return if grid.is_empty() {
                Err(SilhouetteError::NoOpaqueRegion)
            } else {
                Ok(grid)
            };
        }
        AutoMeshMode::Silhouette => {}
    }

    let mask = alpha::alpha_mask(img, params.alpha_threshold);
    if !mask.pixels().any(|p| p.0[0] > 0) {
        return Err(SilhouetteError::NoOpaqueRegion);
    }

    let mask = alpha::close_mask(&mask, params.close_radius);

    let raw_rings = marching::trace_rings(&mask, 128);
    let simplified: Vec<Vec<Vec2>> = raw_rings
        .iter()
        .map(|r| rdp::simplify_closed_ring(r, params.rdp_epsilon_px))
        .collect();

    let rings = topology::build_rings(simplified, params.min_region_area_px);
    if !rings.is_empty() {
        return Ok(rings);
    }

    // Fallback ladder (spec §6.2 c, d): the image has opaque pixels (we
    // already checked above), but every candidate region fell below
    // `min_region_area_px`. Degrade to a convex hull, then a bounding box,
    // of the same opaque pixels rather than returning nothing.
    let hull = convex_hull_ring(img, params.alpha_threshold);
    if is_usable(&hull) {
        return Ok(vec![hull]);
    }
    let bbox = bounding_box_ring(img, params.alpha_threshold);
    if is_usable(&bbox) {
        return Ok(vec![bbox]);
    }
    Ok(rings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{AutoMeshMode, AutoMeshParams};

    fn params() -> AutoMeshParams {
        AutoMeshParams {
            alpha_threshold: 8,
            close_radius: 2,
            rdp_epsilon_px: 2.0,
            min_region_area_px: 64.0,
            interior_spacing_px: 40.0,
            mode: AutoMeshMode::Silhouette,
        }
    }

    fn load(name: &str) -> image::RgbaImage {
        let path = format!(
            "{}/tests/fixtures/images/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        image::open(path).unwrap().to_rgba8()
    }

    #[test]
    fn a_simple_blob_gives_one_outer_ring() {
        let rings = extract(&load("blob.png"), &params()).unwrap();
        assert_eq!(rings.len(), 1);
        assert!(!rings[0].is_hole);
        assert!(
            rings[0].points.len() >= 8,
            "a circle needs more than a few points"
        );
    }

    #[test]
    fn a_blob_with_a_hole_gives_an_outer_ring_and_a_hole() {
        let rings = extract(&load("blob_with_hole.png"), &params()).unwrap();
        assert_eq!(rings.len(), 2);
        assert_eq!(rings.iter().filter(|r| !r.is_hole).count(), 1);
        assert_eq!(rings.iter().filter(|r| r.is_hole).count(), 1);
    }

    #[test]
    fn two_islands_give_two_outer_rings() {
        let rings = extract(&load("two_islands.png"), &params()).unwrap();
        assert_eq!(rings.iter().filter(|r| !r.is_hole).count(), 2);
    }

    #[test]
    fn three_levels_of_nesting_classify_by_containment_depth_parity() {
        // Outer circle -> hole -> innermost island. The innermost island is
        // contained in *two* rings (the outer body and the hole), so a
        // single-level "contained in anything => hole" rule misclassifies
        // it as a hole. The correct rule is containment-depth parity: even
        // depth (0, the outer body; 2, the island) is outer, odd depth (1,
        // the hole) is a hole.
        let rings = extract(&load("nested_island.png"), &params()).unwrap();
        assert_eq!(
            rings.iter().filter(|r| !r.is_hole).count(),
            2,
            "outer body + innermost island"
        );
        assert_eq!(
            rings.iter().filter(|r| r.is_hole).count(),
            1,
            "the ring around the hole"
        );

        let innermost = rings
            .iter()
            .min_by(|a, b| {
                signed_area(&a.points)
                    .abs()
                    .partial_cmp(&signed_area(&b.points).abs())
                    .unwrap()
            })
            .unwrap();
        assert!(
            !innermost.is_hole,
            "the smallest ring is the island, which must be outer, not a hole"
        );
    }

    #[test]
    fn a_fully_opaque_image_gives_a_ring_around_the_whole_frame() {
        let rings = extract(&load("fully_opaque.png"), &params()).unwrap();
        assert_eq!(rings.len(), 1);
        let area = signed_area(&rings[0].points).abs();
        assert!(
            area > 64.0 * 64.0 * 0.9,
            "area {area} should be close to 4096"
        );
    }

    #[test]
    fn a_fully_transparent_image_is_an_error_not_a_panic() {
        let err = extract(&load("fully_transparent.png"), &params()).unwrap_err();
        assert!(matches!(err, SilhouetteError::NoOpaqueRegion));
    }

    #[test]
    fn a_one_pixel_image_does_not_panic() {
        // Below min_region_area_px, so it is treated as having no usable region.
        let r = extract(&load("one_pixel.png"), &params());
        assert!(r.is_err() || r.unwrap().is_empty());
    }

    #[test]
    fn the_closing_pass_removes_antialiasing_speckle() {
        let img = load("antialiased_edge.png");
        let with = extract(&img, &params()).unwrap();

        let mut p = params();
        p.close_radius = 0;
        p.min_region_area_px = 0.0;
        let without = extract(&img, &p).unwrap();

        assert!(
            with.len() < without.len(),
            "closing must merge speckle: {} rings with, {} without",
            with.len(),
            without.len()
        );
    }

    #[test]
    fn outer_rings_are_ccw_and_holes_are_cw() {
        let rings = extract(&load("blob_with_hole.png"), &params()).unwrap();
        for r in &rings {
            let a = signed_area(&r.points);
            if r.is_hole {
                assert!(a < 0.0, "hole must be CW, got area {a}");
            } else {
                assert!(a > 0.0, "outer must be CCW, got area {a}");
            }
        }
    }

    #[test]
    fn convex_hull_mode_dispatches_to_the_convex_hull_fallback() {
        let mut p = params();
        p.mode = AutoMeshMode::ConvexHull;
        let rings = extract(&load("blob.png"), &p).unwrap();
        assert_eq!(rings.len(), 1);
        assert!(!rings[0].is_hole);
        // A convex hull of a circle has real area, unlike the degenerate
        // (zero-point) case.
        assert!(signed_area(&rings[0].points).abs() > 1000.0);
    }

    #[test]
    fn bounding_box_mode_dispatches_to_the_bounding_box_fallback() {
        let mut p = params();
        p.mode = AutoMeshMode::BoundingBox;
        let rings = extract(&load("blob.png"), &p).unwrap();
        assert_eq!(rings.len(), 1);
        assert_eq!(rings[0].points.len(), 4, "a bounding box is a quad");
        assert!(!rings[0].is_hole);
    }

    #[test]
    fn grid_mode_dispatches_to_the_grid_fallback() {
        let mut p = params();
        p.mode = AutoMeshMode::Grid;
        let rings = extract(&load("blob.png"), &p).unwrap();
        assert!(
            rings.len() > 1,
            "a grid tiling of the bounding box should yield more than one cell, got {}",
            rings.len()
        );
        assert!(rings.iter().all(|r| !r.is_hole));
    }

    #[test]
    fn a_fully_transparent_image_errors_in_every_mode() {
        for mode in [
            AutoMeshMode::Silhouette,
            AutoMeshMode::ConvexHull,
            AutoMeshMode::BoundingBox,
            AutoMeshMode::Grid,
        ] {
            let mut p = params();
            p.mode = mode;
            let err = extract(&load("fully_transparent.png"), &p).unwrap_err();
            assert!(matches!(err, SilhouetteError::NoOpaqueRegion));
        }
    }

    #[test]
    fn silhouette_mode_falls_back_to_convex_hull_when_every_region_is_too_small() {
        // one_pixel.png has a single opaque pixel: marching squares +
        // min_region_area_px filtering finds nothing usable in Silhouette
        // mode, but there IS an opaque pixel, so the fallback ladder
        // (spec 6.2 c, d) should degrade to a real ring rather than
        // silently returning nothing.
        let mut p = params();
        p.min_region_area_px = 1_000_000.0; // guarantee the real pipeline finds nothing
        let rings = extract(&load("blob.png"), &p).unwrap();
        assert!(
            !rings.is_empty(),
            "an image with real opaque area must not fall all the way through to empty"
        );
    }

    #[test]
    fn rdp_simplification_reduces_point_count_without_losing_the_shape() {
        let mut coarse = params();
        coarse.rdp_epsilon_px = 8.0;
        let fine = params();

        let c = extract(&load("blob.png"), &coarse).unwrap();
        let f = extract(&load("blob.png"), &fine).unwrap();

        assert!(c[0].points.len() < f[0].points.len());
        let (ca, fa) = (
            signed_area(&c[0].points).abs(),
            signed_area(&f[0].points).abs(),
        );
        assert!(
            (ca - fa).abs() / fa < 0.10,
            "area changed by more than 10%: {ca} vs {fa}"
        );
    }
}
