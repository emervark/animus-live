//! The one place bytes become an image.

use image::RgbaImage;
use thiserror::Error;

/// Formats this build can read.
///
/// The `image` crate is feature-narrowed on purpose (spec §3.1) and
/// `imageproc` is `default-features = false` to keep it that way, so the
/// AVIF and OpenEXR codec stacks stay out of a build that only opens PNGs.
/// Growing this enum is the first half of adding a format; the second half
/// is a feature in `Cargo.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
}

impl ImageFormat {
    /// The formats a file dialog should offer.
    pub const SUPPORTED_EXTENSIONS: &'static [&'static str] = &["png"];

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ImportError {
    /// Named rather than shrugged at: "unsupported format" without saying
    /// *which* leaves the operator guessing at their own file.
    #[error(
        "{ext} files are not supported yet. Save the image as a PNG with a \
         transparent background and import that."
    )]
    UnsupportedFormat { ext: String },

    #[error("the file has no extension, so there is no way to tell what it is")]
    NoExtension,

    #[error("this file is not a valid image, or it is damaged: {detail}")]
    Undecodable { detail: String },

    /// The message the whole PNG-only decision rests on. It has to say what
    /// to do, because for most people this is the first thing that will go
    /// wrong.
    #[error(
        "this image has no transparency, so there is nothing to cut out. \
         Remove the background first and save it as a PNG."
    )]
    NoAlphaChannel,

    #[error("this image is fully transparent, so there is nothing to import")]
    FullyTransparent,

    #[error("this image is {w}x{h}, larger than the {max}px limit")]
    TooLarge { w: u32, h: u32, max: u32 },

    /// Names the knob rather than the symptom. The underlying error says
    /// "no opaque region", which sends the artist to look at their artwork
    /// instead of at the slider they just moved.
    #[error(
        "nothing in this image is more opaque than the alpha threshold ({threshold}).          Lower the threshold, or check the image really has a subject in it."
    )]
    NothingAboveThreshold { threshold: u8 },

    #[error("the outline could not be turned into a mesh: {detail}")]
    CouldNotTriangulate { detail: String },

    #[error("the outline produced no triangles, so there is nothing to rig")]
    EmptyMesh,
}

/// Hard ceiling on either dimension.
///
/// Below every GPU's max texture size this project could plausibly meet, and
/// a friendlier failure than a driver-level allocation error.
pub const MAX_DIMENSION: u32 = 8192;

/// Decode `bytes`, using `name` only to pick the format.
///
/// The name is not opened, read, or trusted for anything else — the caller
/// may have the bytes from a drag-and-drop with no path at all.
pub fn decode(bytes: &[u8], name: &str) -> Result<RgbaImage, ImportError> {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .ok_or(ImportError::NoExtension)?;

    let format =
        ImageFormat::from_extension(ext).ok_or_else(|| ImportError::UnsupportedFormat {
            ext: ext.to_ascii_lowercase(),
        })?;

    let img = match format {
        ImageFormat::Png => image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
            .map_err(|e| ImportError::Undecodable {
                detail: e.to_string(),
            })?,
    };

    let (w, h) = (img.width(), img.height());
    if w > MAX_DIMENSION || h > MAX_DIMENSION {
        return Err(ImportError::TooLarge {
            w,
            h,
            max: MAX_DIMENSION,
        });
    }

    Ok(img.to_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32, alpha: u8) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, alpha]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn a_png_decodes() {
        let img = decode(&png_bytes(4, 3, 255), "hero.png").unwrap();
        assert_eq!((img.width(), img.height()), (4, 3));
    }

    #[test]
    fn an_unsupported_extension_names_itself() {
        // The operator's actual test files are JPEGs, so this message is the
        // one most people will meet first.
        let err = decode(&png_bytes(2, 2, 255), "dancer.jpg").unwrap_err();
        assert_eq!(
            err,
            ImportError::UnsupportedFormat {
                ext: "jpg".to_string()
            }
        );
        assert!(err.to_string().contains("jpg"));
    }

    #[test]
    fn the_extension_check_is_case_insensitive() {
        assert!(decode(&png_bytes(2, 2, 255), "HERO.PNG").is_ok());
    }

    #[test]
    fn a_file_with_no_extension_is_a_clear_error() {
        assert_eq!(
            decode(&png_bytes(2, 2, 255), "hero").unwrap_err(),
            ImportError::NoExtension
        );
    }

    #[test]
    fn garbage_bytes_are_undecodable_not_a_panic() {
        let err = decode(b"not a png at all", "hero.png").unwrap_err();
        assert!(matches!(err, ImportError::Undecodable { .. }));
    }

    #[test]
    fn an_oversized_image_is_refused_before_the_driver_sees_it() {
        // Build the header only: a real 9000px image would make this test
        // slow for no extra coverage.
        let img = RgbaImage::new(MAX_DIMENSION + 1, 4);
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        let err = decode(&out.into_inner(), "huge.png").unwrap_err();
        assert!(matches!(err, ImportError::TooLarge { .. }));
    }
}
