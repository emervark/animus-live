//! Image bytes to a rigged-ready mesh, in one call.
//!
//! Every step already existed; what did not exist was a single place where
//! they are composed and where **every failure becomes a sentence a person
//! can act on**. Silhouette extraction and triangulation each have their own
//! error type describing what went wrong internally
//! (`NoOpaqueRegion`, `ConstraintFailed`), which is exactly right for a
//! library and useless in a dialog. This module is the translation layer.
//!
//! It is Bevy-free and filesystem-free on purpose: the whole import can be
//! tested with an in-memory PNG and no GPU, and the editor's job shrinks to
//! choosing a file and showing a message.

use image::RgbaImage;

use super::decode::ImportError;
use super::matte::{MatteReport, resolve_alpha};
use crate::doc::{AutoMeshParams, MatteParams, MeshData};
use crate::silhouette;
use crate::triangulate;

/// What an import produced, including the parts worth telling the artist
/// about before they start rigging.
#[derive(Debug, Clone)]
pub struct ImportedMesh {
    pub mesh: MeshData,
    pub matte: MatteReport,
    /// Pixel dimensions of the source image, needed for UVs and for the
    /// puppet's pivot.
    pub size: (u32, u32),
}

impl ImportedMesh {
    pub fn vertex_count(&self) -> usize {
        self.mesh.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.mesh.triangles.len() / 3
    }
}

/// Resolve alpha, trace the silhouette, triangulate.
///
/// Takes the image by value because `resolve_alpha` may rewrite the alpha
/// channel: a future matte mode computes it rather than reading it, and
/// handing that back to the caller would invite two copies of the truth.
pub fn mesh_from_image(
    mut img: RgbaImage,
    matte_params: &MatteParams,
    mesh_params: &AutoMeshParams,
) -> Result<ImportedMesh, ImportError> {
    let size = (img.width(), img.height());
    let matte = resolve_alpha(&mut img, matte_params, mesh_params.alpha_threshold)?;

    let rings = silhouette::extract(&img, mesh_params).map_err(|e| match e {
        // The library says "no pixels at or above the alpha threshold"; the
        // artist needs to know which knob that is.
        silhouette::SilhouetteError::NoOpaqueRegion => ImportError::NothingAboveThreshold {
            threshold: mesh_params.alpha_threshold,
        },
    })?;

    let mesh = triangulate::triangulate(&rings, mesh_params, size).map_err(|e| {
        ImportError::CouldNotTriangulate {
            detail: e.to_string(),
        }
    })?;

    if mesh.triangles.is_empty() {
        return Err(ImportError::EmptyMesh);
    }

    Ok(ImportedMesh { mesh, matte, size })
}

/// Sensible starting parameters for a first import.
///
/// Deliberately not `AutoMeshParams::default()`: these are tuned for "a
/// person just dropped a drawing in and wants to see something", which
/// means a coarse enough mesh to stay responsive while the sliders are
/// moved, not the finest one the algorithm can produce.
pub fn starting_params() -> AutoMeshParams {
    AutoMeshParams {
        alpha_threshold: 16,
        close_radius: 2,
        rdp_epsilon_px: 2.0,
        min_region_area_px: 64.0,
        interior_spacing_px: 32.0,
        mode: crate::doc::AutoMeshMode::Silhouette,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{AutoMeshMode, MatteMode};
    use image::Rgba;

    fn disc(w: u32, h: u32, r: f32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            if d <= r {
                *px = Rgba([180, 60, 40, 255]);
            }
        }
        img
    }

    fn matte() -> MatteParams {
        MatteParams {
            mode: MatteMode::UseImageAlpha,
        }
    }

    #[test]
    fn a_cutout_becomes_a_mesh_that_covers_it() {
        let img = disc(256, 256, 90.0);
        let out = mesh_from_image(img, &matte(), &starting_params()).expect("imports");

        assert!(
            out.triangle_count() > 20,
            "a 90px disc should produce a real mesh, got {} triangles",
            out.triangle_count()
        );
        assert_eq!(out.size, (256, 256));

        // Every vertex must sit inside the image, or the UVs are wrong.
        for p in &out.mesh.positions {
            assert!(
                p.x >= -1.0 && p.x <= 257.0 && p.y >= -1.0 && p.y <= 257.0,
                "vertex outside the image: {p:?}"
            );
        }
        // And UVs must be normalized to it.
        for uv in &out.mesh.uvs {
            assert!(
                (-0.01..=1.01).contains(&uv.x) && (-0.01..=1.01).contains(&uv.y),
                "uv out of range: {uv:?}"
            );
        }
    }

    #[test]
    fn the_mesh_records_the_parameters_that_produced_it() {
        // Provenance is what makes "re-run auto-mesh" reproducible rather
        // than a fresh guess.
        let img = disc(128, 128, 40.0);
        let params = starting_params();
        let out = mesh_from_image(img, &matte(), &params).unwrap();
        match &out.mesh.source {
            crate::doc::MeshSource::Auto(recorded) => {
                assert_eq!(recorded.alpha_threshold, params.alpha_threshold);
                assert_eq!(recorded.interior_spacing_px, params.interior_spacing_px);
            }
            other => panic!("expected auto provenance, got {other:?}"),
        }
    }

    #[test]
    fn an_opaque_image_is_refused_before_any_geometry_runs() {
        let img = RgbaImage::from_pixel(64, 64, Rgba([10, 20, 30, 255]));
        let err = mesh_from_image(img, &matte(), &starting_params()).unwrap_err();
        assert_eq!(err, ImportError::NoAlphaChannel);
    }

    #[test]
    fn a_transparent_image_is_refused_separately() {
        let img = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 0]));
        let err = mesh_from_image(img, &matte(), &starting_params()).unwrap_err();
        assert_eq!(err, ImportError::FullyTransparent);
    }

    #[test]
    fn a_threshold_above_everything_names_the_threshold() {
        // The library's own error is "no opaque region", which sends the
        // artist looking at their artwork instead of at the slider they just
        // moved.
        let img = disc(64, 64, 20.0);
        let mut params = starting_params();
        params.alpha_threshold = 254;
        // The disc is alpha 255, so raise past it to trip the failure.
        let mut fainter = disc(64, 64, 20.0);
        for px in fainter.pixels_mut() {
            if px.0[3] == 255 {
                px.0[3] = 200;
            }
        }
        let _ = img;

        let err = mesh_from_image(fainter, &matte(), &params).unwrap_err();
        assert_eq!(err, ImportError::NothingAboveThreshold { threshold: 254 });
        assert!(err.to_string().contains("254"));
    }

    #[test]
    fn every_mesh_mode_produces_something_usable() {
        // The fallback ladder exists so a difficult silhouette never leaves
        // the artist with nothing; this asserts each rung actually stands.
        for mode in [
            AutoMeshMode::Silhouette,
            AutoMeshMode::ConvexHull,
            AutoMeshMode::BoundingBox,
            AutoMeshMode::Grid,
        ] {
            let mut params = starting_params();
            params.mode = mode;
            let out = mesh_from_image(disc(128, 128, 45.0), &matte(), &params)
                .unwrap_or_else(|e| panic!("{mode:?} failed: {e}"));
            assert!(out.triangle_count() > 0, "{mode:?} produced an empty mesh");
        }
    }

    #[test]
    fn a_finer_spacing_produces_more_vertices() {
        // The slider has to do something, and in the direction the label
        // implies.
        let coarse = {
            let mut p = starting_params();
            p.interior_spacing_px = 48.0;
            mesh_from_image(disc(256, 256, 100.0), &matte(), &p).unwrap()
        };
        let fine = {
            let mut p = starting_params();
            p.interior_spacing_px = 16.0;
            mesh_from_image(disc(256, 256, 100.0), &matte(), &p).unwrap()
        };
        assert!(
            fine.vertex_count() > coarse.vertex_count(),
            "finer spacing gave {} vertices, coarser gave {}",
            fine.vertex_count(),
            coarse.vertex_count()
        );
    }
}
