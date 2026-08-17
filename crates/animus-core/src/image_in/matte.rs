//! Where the silhouette's alpha comes from.
//!
//! One mode today. The field exists in the document and in the v1 file
//! format anyway, so a second mode later is a new value in an existing
//! field: additive, and no migration.

use image::RgbaImage;

use super::decode::ImportError;
use crate::doc::{MatteMode, MatteParams};

/// What resolving the alpha produced, for the UI to state plainly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatteReport {
    /// Fraction of pixels that ended up opaque enough to be inside the
    /// silhouette. A puppet that is 2% of its image is usually a mistake
    /// worth mentioning before triangulating it.
    pub covered_fraction: f32,
    /// Whether the subject runs off the edge of the image. Not an error —
    /// plenty of legitimate art bleeds — but it changes what the silhouette
    /// will look like, so the UI should say so once.
    pub touched_border: bool,
}

/// True when nothing in the image is meaningfully transparent.
///
/// Used to decide whether an import can proceed at all under
/// [`MatteMode::UseImageAlpha`], and later to decide whether to offer a
/// matte step.
pub fn is_effectively_opaque(img: &RgbaImage) -> bool {
    const THRESHOLD: u8 = 250;
    img.pixels().all(|p| p.0[3] >= THRESHOLD)
}

/// Resolve the image's alpha channel according to `params`.
///
/// Today this only validates the alpha already present. It takes `&mut` and
/// returns a report because that is the shape a real matte mode needs, and
/// changing the signature later would touch every call site.
pub fn resolve_alpha(
    img: &mut RgbaImage,
    params: &MatteParams,
    alpha_threshold: u8,
) -> Result<MatteReport, ImportError> {
    match params.mode {
        MatteMode::UseImageAlpha => {
            if is_effectively_opaque(img) {
                return Err(ImportError::NoAlphaChannel);
            }
        }
    }

    let (w, h) = (img.width(), img.height());
    let mut covered = 0usize;
    let mut touched_border = false;

    for (x, y, px) in img.enumerate_pixels() {
        if px.0[3] >= alpha_threshold {
            covered += 1;
            if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
                touched_border = true;
            }
        }
    }

    if covered == 0 {
        return Err(ImportError::FullyTransparent);
    }

    Ok(MatteReport {
        covered_fraction: covered as f32 / (w as f32 * h as f32),
        touched_border,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn disc(w: u32, h: u32, r: f32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            if d <= r {
                *px = Rgba([200, 40, 40, 255]);
            }
        }
        img
    }

    fn params() -> MatteParams {
        MatteParams {
            mode: MatteMode::UseImageAlpha,
        }
    }

    #[test]
    fn a_cutout_png_resolves_and_reports_its_coverage() {
        let mut img = disc(64, 64, 20.0);
        let report = resolve_alpha(&mut img, &params(), 8).unwrap();
        // pi*20^2 / 64^2 is about 0.307
        assert!(
            (report.covered_fraction - 0.307).abs() < 0.02,
            "coverage was {}",
            report.covered_fraction
        );
        assert!(!report.touched_border);
    }

    #[test]
    fn a_subject_running_off_the_edge_is_reported_but_allowed() {
        // Radius 20 in a 32x32 frame: the disc reaches the middle of each
        // edge while the corners stay transparent. A disc that covered the
        // *whole* frame would be refused instead, and rightly — an image
        // with no transparent pixel anywhere has nothing to cut out.
        let mut img = disc(32, 32, 20.0);
        let report = resolve_alpha(&mut img, &params(), 8).unwrap();
        assert!(report.touched_border, "the subject reaches the frame edge");
        assert!(
            report.covered_fraction > 0.7 && report.covered_fraction < 1.0,
            "coverage was {}",
            report.covered_fraction
        );
    }

    #[test]
    fn a_fully_opaque_image_is_refused_with_the_message_that_says_what_to_do() {
        let mut img = RgbaImage::from_pixel(16, 16, Rgba([10, 20, 30, 255]));
        let err = resolve_alpha(&mut img, &params(), 8).unwrap_err();
        assert_eq!(err, ImportError::NoAlphaChannel);
        assert!(err.to_string().contains("Remove the background"));
    }

    #[test]
    fn a_fully_transparent_image_is_refused_separately() {
        let mut img = RgbaImage::from_pixel(16, 16, Rgba([0, 0, 0, 0]));
        assert_eq!(
            resolve_alpha(&mut img, &params(), 8).unwrap_err(),
            ImportError::FullyTransparent
        );
    }

    #[test]
    fn nearly_opaque_still_counts_as_opaque() {
        // A JPEG round-tripped through a PNG export keeps alpha 255
        // everywhere; a stray 251 should not sneak past the check.
        let mut img = RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 252]));
        assert!(is_effectively_opaque(&img));
        assert_eq!(
            resolve_alpha(&mut img, &params(), 8).unwrap_err(),
            ImportError::NoAlphaChannel
        );
    }
}
