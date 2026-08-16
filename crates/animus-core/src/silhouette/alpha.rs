//! Alpha thresholding and the morphological closing pass.

use image::{GrayImage, Luma, RgbaImage};
use imageproc::distance_transform::Norm;
use imageproc::morphology::{dilate_mut, erode_mut};

/// Builds a binary mask: 255 where `rgba.a >= threshold`, 0 otherwise.
pub fn alpha_mask(img: &RgbaImage, threshold: u8) -> GrayImage {
    GrayImage::from_fn(img.width(), img.height(), |x, y| {
        let a = img.get_pixel(x, y).0[3];
        Luma([if a >= threshold { 255 } else { 0 }])
    })
}

/// Morphological *closing*: dilate then erode, both by `radius`.
///
/// Order matters. Dilate-then-erode fills small gaps and merges
/// anti-aliasing speckle into the body — that's a closing. Erode-then-dilate
/// is an *opening* and does the opposite (it eats thin limbs off the
/// puppet); getting this backwards is a real bug, not a style choice.
///
/// A no-op when `radius == 0`, so callers can disable the pass entirely.
pub fn close_mask(mask: &GrayImage, radius: u32) -> GrayImage {
    if radius == 0 {
        return mask.clone();
    }
    let k = radius.min(u8::MAX as u32) as u8;
    let mut out = mask.clone();
    dilate_mut(&mut out, Norm::LInf, k);
    erode_mut(&mut out, Norm::LInf, k);
    out
}
