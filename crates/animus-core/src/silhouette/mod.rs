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

pub use fallback::{bounding_box_ring, convex_hull_ring};
pub use topology::signed_area;

use crate::doc::AutoMeshParams;
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

/// Extracts silhouette rings from an image's alpha channel.
///
/// Pipeline: alpha-threshold to a binary mask, close (dilate then erode) to
/// merge anti-aliasing speckle into the body, trace with marching squares,
/// simplify each ring with closed-ring RDP, then classify/normalize/clean
/// up topology.
///
/// An image with no pixel at or above `params.alpha_threshold` is a genuine
/// error ([`SilhouetteError::NoOpaqueRegion`]): there is nothing to
/// extract. An image that *does* have opaque pixels, but where every
/// resulting region falls below `params.min_region_area_px`, is not an
/// error — a too-small region is a normal, expected filtering outcome, not
/// a failure of the algorithm — so that case returns `Ok(vec![])`.
pub fn extract(img: &RgbaImage, params: &AutoMeshParams) -> Result<Vec<Ring>, SilhouetteError> {
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

    Ok(topology::build_rings(simplified, params.min_region_area_px))
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
