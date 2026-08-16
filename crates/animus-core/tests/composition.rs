//! Integration tests that compose real module output into the next
//! module, rather than hand-building inputs.
//!
//! Every other test in this crate hand-builds its input for the module
//! under test. That's appropriate for unit tests, but it means the
//! contract *between* modules — e.g. that `triangulate`'s hole-winding
//! convention agrees with what `silhouette::extract` actually produces —
//! was asserted only inside each module's own tests, never by actually
//! running one module's output into the other.

use animus_core::doc::{AutoMeshMode, AutoMeshParams};
use animus_core::mesh;
use animus_core::silhouette;
use animus_core::triangulate;

fn params() -> AutoMeshParams {
    AutoMeshParams {
        alpha_threshold: 8,
        close_radius: 2,
        rdp_epsilon_px: 2.0,
        min_region_area_px: 64.0,
        interior_spacing_px: 15.0,
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

/// Even-odd ray-casting point-in-polygon, for asserting against a ring's
/// *actual* boundary (which RDP simplification bends slightly inward from
/// the ideal circle) rather than a fixed-radius approximation of it.
fn point_in_polygon(p: glam::Vec2, poly: &[glam::Vec2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (pi, pj) = (poly[i], poly[j]);
        if (pi.y > p.y) != (pj.y > p.y) {
            let x_at_p_y = (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y) + pi.x;
            if p.x < x_at_p_y {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// `extract` -> `triangulate` -> `mesh::validate`, for a shape with a real
/// hole. This is the winding-convention contract described in
/// `silhouette/mod.rs`'s module doc, exercised end to end instead of only
/// inside `silhouette`'s own hand-built tests.
#[test]
fn extract_then_triangulate_a_blob_with_a_hole_produces_a_valid_mesh() {
    let img = load("blob_with_hole.png");
    let p = params();
    let rings = silhouette::extract(&img, &p).unwrap();
    assert_eq!(rings.len(), 2, "one outer ring, one hole");

    let (w, h) = img.dimensions();
    let mesh = triangulate::triangulate(&rings, &p, (w, h)).unwrap();

    let defects = mesh::validate(&mesh);
    assert!(defects.is_empty(), "mesh has defects: {defects:?}");
    assert!(!mesh.triangles.is_empty());

    // No triangle centroid should land inside the ring `extract` itself
    // classified as the hole.
    let hole = rings.iter().find(|r| r.is_hole).unwrap();
    for t in mesh.triangles.chunks_exact(3) {
        let c = (mesh.positions[t[0] as usize]
            + mesh.positions[t[1] as usize]
            + mesh.positions[t[2] as usize])
            / 3.0;
        assert!(
            !point_in_polygon(c, &hole.points),
            "triangle centroid {c:?} is inside the hole"
        );
    }
}

/// `extract` -> `triangulate` -> `mesh::validate` for a concave shape (a
/// crescent). Every other composed shape in this crate is convex at the
/// point the fan-triangulation trick would matter; this is the only test
/// that runs a genuinely concave silhouette all the way through the real
/// pipeline.
#[test]
fn extract_then_triangulate_a_concave_crescent_produces_a_valid_mesh() {
    let img = load("crescent.png");
    let p = params();
    let rings = silhouette::extract(&img, &p).unwrap();
    assert_eq!(rings.len(), 1, "a crescent is a single outer ring, no hole");
    assert!(!rings[0].is_hole);

    let (w, h) = img.dimensions();
    let mesh = triangulate::triangulate(&rings, &p, (w, h)).unwrap();

    let defects = mesh::validate(&mesh);
    assert!(defects.is_empty(), "mesh has defects: {defects:?}");
    assert!(!mesh.triangles.is_empty());

    // No triangle centroid should land outside the ring `extract` itself
    // traced for the crescent (in particular, inside the concave notch
    // where the "bite" circle was subtracted).
    for t in mesh.triangles.chunks_exact(3) {
        let c = (mesh.positions[t[0] as usize]
            + mesh.positions[t[1] as usize]
            + mesh.positions[t[2] as usize])
            / 3.0;
        assert!(
            point_in_polygon(c, &rings[0].points),
            "triangle centroid {c:?} escaped the crescent's own boundary"
        );
    }
}
