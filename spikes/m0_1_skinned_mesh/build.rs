//! Generates `assets/top_marker.png`: a texture with the word "TOP" drawn
//! legibly along the top edge, so the Y-flip / UV-orientation question
//! (spec §7.1) can be answered by looking at the rendered quad.
//!
//! Regenerated on every build; not committed as a binary asset so the
//! spike stays fully reproducible from source.

use image::{Rgba, RgbaImage};
use std::path::Path;

const W: u32 = 128;
const H: u32 = 128;

// 5x7 blocky glyphs, top row first.
const GLYPH_T: [&str; 7] = [
    "11111", "00100", "00100", "00100", "00100", "00100", "00100",
];
const GLYPH_O: [&str; 7] = [
    "01110", "10001", "10001", "10001", "10001", "10001", "01110",
];
const GLYPH_P: [&str; 7] = [
    "11110", "10001", "10001", "11110", "10000", "10000", "10000",
];

fn draw_glyph(
    img: &mut RgbaImage,
    glyph: &[&str; 7],
    ox: u32,
    oy: u32,
    scale: u32,
    color: Rgba<u8>,
) {
    for (row, line) in glyph.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            if ch == '1' {
                for dy in 0..scale {
                    for dx in 0..scale {
                        let x = ox + col as u32 * scale + dx;
                        let y = oy + row as u32 * scale + dy;
                        if x < img.width() && y < img.height() {
                            img.put_pixel(x, y, color);
                        }
                    }
                }
            }
        }
    }
}

fn main() {
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    std::fs::create_dir_all(&out_dir).unwrap();
    let out_path = out_dir.join("top_marker.png");

    let mut img = RgbaImage::from_pixel(W, H, Rgba([40, 40, 200, 255])); // blue body

    // A distinct red strip along the very top edge (image row 0..12), so
    // orientation is readable even without reading the letters.
    for y in 0..12 {
        for x in 0..W {
            img.put_pixel(x, y, Rgba([220, 30, 30, 255]));
        }
    }

    // A distinct green strip along the very bottom edge, for contrast.
    for y in (H - 12)..H {
        for x in 0..W {
            img.put_pixel(x, y, Rgba([30, 200, 60, 255]));
        }
    }

    // "TOP" drawn in white, starting just below the red strip, near the
    // top edge of the image (small-image-Y = 0 = top-of-texture).
    let scale = 4;
    let glyph_w = 5 * scale;
    let gap = 2 * scale;
    let total_w = glyph_w * 3 + gap * 2;
    let ox = (W - total_w) / 2;
    let oy = 16;
    let white = Rgba([255, 255, 255, 255]);
    draw_glyph(&mut img, &GLYPH_T, ox, oy, scale, white);
    draw_glyph(&mut img, &GLYPH_O, ox + glyph_w + gap, oy, scale, white);
    draw_glyph(
        &mut img,
        &GLYPH_P,
        ox + 2 * (glyph_w + gap),
        oy,
        scale,
        white,
    );

    img.save(&out_path).expect("failed to write top_marker.png");

    // Also place a copy next to the built executable. Bevy resolves its
    // asset root from `CARGO_MANIFEST_DIR` when the binary is launched by
    // Cargo, but falls back to the executable's own directory otherwise --
    // so without this copy, `target/release/m0-1-skinned-mesh.exe` started
    // directly renders an untextured quad and the TOP-orientation check
    // (the whole point of this spike) silently cannot be performed.
    //
    // OUT_DIR is `<target>/<profile>/build/<pkg>-<hash>/out`; three levels
    // up is the profile directory holding the executable.
    let out_dir_env = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    if let Some(profile_dir) = Path::new(&out_dir_env).ancestors().nth(3) {
        let exe_assets = profile_dir.join("assets");
        if std::fs::create_dir_all(&exe_assets).is_ok() {
            let _ = std::fs::copy(&out_path, exe_assets.join("top_marker.png"));
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
}
