//! Regenerates the PNG fixtures used by `animus_core::silhouette` tests.
//!
//! These images are committed to the repo (see
//! `crates/animus-core/tests/fixtures/images/`) so tests don't depend on
//! this being run, but the generator lives here so the fixtures are
//! reproducible rather than mystery binaries. Re-run explicitly with:
//!
//! ```text
//! cargo test -p animus-core --test generate_fixtures -- --ignored
//! ```
use image::{Rgba, RgbaImage};

fn fixtures_dir() -> String {
    format!("{}/tests/fixtures/images", env!("CARGO_MANIFEST_DIR"))
}

fn save(img: &RgbaImage, name: &str) {
    let path = format!("{}/{name}", fixtures_dir());
    img.save(&path)
        .unwrap_or_else(|e| panic!("failed to save {path}: {e}"));
}

/// A tiny deterministic PRNG (xorshift-ish LCG) so the speckle in
/// `antialiased_edge.png` is reproducible without pulling in a `rand` dep.
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

#[test]
#[ignore = "regenerates fixture PNGs; run explicitly, not on every `cargo test`"]
fn generate_fixtures() {
    std::fs::create_dir_all(fixtures_dir()).unwrap();

    let (cx, cy, r) = (100.0f32, 100.0f32, 70.0f32);

    // blob.png: 200x200, filled circle radius 70 at centre, hard alpha edge.
    let mut blob = RgbaImage::new(200, 200);
    for y in 0..200 {
        for x in 0..200 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let inside = (dx * dx + dy * dy).sqrt() <= r;
            blob.put_pixel(
                x,
                y,
                if inside {
                    Rgba([255, 255, 255, 255])
                } else {
                    Rgba([0, 0, 0, 0])
                },
            );
        }
    }
    save(&blob, "blob.png");

    // blob_with_hole.png: same circle, with a radius-25 transparent hole at centre.
    let mut with_hole = blob.clone();
    let hr = 25.0f32;
    for y in 0..200 {
        for x in 0..200 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            if (dx * dx + dy * dy).sqrt() <= hr {
                with_hole.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }
    }
    save(&with_hole, "blob_with_hole.png");

    // two_islands.png: two disjoint 40x40 squares, 60px apart.
    let mut islands = RgbaImage::new(200, 200);
    for y in 20..60 {
        for x in 20..60 {
            islands.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
    }
    for y in 20..60 {
        for x in 120..160 {
            islands.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
    }
    save(&islands, "two_islands.png");

    // fully_opaque.png: 64x64, alpha 255 everywhere.
    let mut opaque = RgbaImage::new(64, 64);
    for p in opaque.pixels_mut() {
        *p = Rgba([255, 255, 255, 255]);
    }
    save(&opaque, "fully_opaque.png");

    // fully_transparent.png: 64x64, alpha 0 everywhere (RgbaImage::new default).
    let transparent = RgbaImage::new(64, 64);
    save(&transparent, "fully_transparent.png");

    // one_pixel.png: 1x1 opaque.
    let mut one_pixel = RgbaImage::new(1, 1);
    one_pixel.put_pixel(0, 0, Rgba([255, 255, 255, 255]));
    save(&one_pixel, "one_pixel.png");

    // antialiased_edge.png: the blob with a genuine 3px alpha gradient at
    // its edge, plus scattered alpha-1..=8 speckle well outside it.
    let mut aa = RgbaImage::new(200, 200);
    for y in 0..200 {
        for x in 0..200 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            let alpha: u8 = if d <= r - 3.0 {
                255
            } else if d <= r {
                // Linear ramp: 255 at (r - 3), 0 at r.
                let t = (r - d) / 3.0;
                (t * 255.0).round().clamp(0.0, 255.0) as u8
            } else {
                0
            };
            aa.put_pixel(x, y, Rgba([255, 255, 255, alpha]));
        }
    }
    let mut rng = Lcg(12345);
    let mut placed = 0;
    while placed < 60 {
        let rx = rng.next() % 200;
        let ry = rng.next() % 200;
        let dx = rx as f32 + 0.5 - cx;
        let dy = ry as f32 + 0.5 - cy;
        let d = (dx * dx + dy * dy).sqrt();
        // Keep speckle well clear of the blob and its AA fringe so it stays
        // isolated: a single pixel dilated then eroded by the same radius
        // vanishes, which is exactly what `the_closing_pass_removes_antialiasing_speckle`
        // relies on.
        if d > r + 15.0 {
            let a = 1 + (rng.next() % 8) as u8; // 1..=8
            aa.put_pixel(rx, ry, Rgba([255, 255, 255, a]));
            placed += 1;
        }
    }
    save(&aa, "antialiased_edge.png");

    // nested_island.png: three levels of nesting — a filled circle, a
    // concentric transparent hole inside it, and a smaller filled circle
    // (an "island") inside that hole. Exercises hole classification beyond
    // a single level: the innermost circle must come back as an outer
    // ring, not a hole, even though it's contained in both the outer ring
    // and the hole ring.
    let mut nested = RgbaImage::new(200, 200);
    let (hole_r, island_r) = (35.0f32, 12.0f32);
    for y in 0..200 {
        for x in 0..200 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            let inside = if d <= island_r {
                true // innermost island: filled again
            } else if d <= hole_r {
                false // hole: transparent
            } else {
                d <= r // outer body: filled
            };
            nested.put_pixel(
                x,
                y,
                if inside {
                    Rgba([255, 255, 255, 255])
                } else {
                    Rgba([0, 0, 0, 0])
                },
            );
        }
    }
    save(&nested, "nested_island.png");

    // crescent.png: a filled circle minus an off-centre filled circle of
    // similar radius, leaving a concave "C"/crescent shape. Every other
    // fixture in this file is a circle, a square, or concentric circles —
    // all convex-at-every-vertex once traced. Character silhouettes are
    // routinely concave (the design spec's own example is a tooth visible
    // inside an open mouth), and nothing here exercised that until now.
    let mut crescent = RgbaImage::new(200, 200);
    let bite_cx = cx + 35.0; // offset far enough to leave a real crescent, not a ring
    for y in 0..200 {
        for x in 0..200 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let in_outer = (dx * dx + dy * dy).sqrt() <= r;
            let bdx = x as f32 + 0.5 - bite_cx;
            let bdy = y as f32 + 0.5 - cy;
            let in_bite = (bdx * bdx + bdy * bdy).sqrt() <= r;
            let inside = in_outer && !in_bite;
            crescent.put_pixel(
                x,
                y,
                if inside {
                    Rgba([255, 255, 255, 255])
                } else {
                    Rgba([0, 0, 0, 0])
                },
            );
        }
    }
    save(&crescent, "crescent.png");
}
