//! Mesh integrity checks.
//!
//! `validate` never panics and never stops at the first problem — it
//! returns every defect it finds, so the UI can show a complete report
//! for a mesh that arrived from disk, from a migration, or from an edit.

use crate::doc::MeshData;

#[derive(Debug, Clone, PartialEq)]
pub enum MeshDefect {
    UvCountMismatch {
        positions: usize,
        uvs: usize,
    },
    RaggedTriangleList {
        len: usize,
    },
    TriangleIndexOutOfRange {
        triangle: usize,
        index: u32,
        vertex_count: usize,
    },
    DegenerateTriangle {
        triangle: usize,
        area: f32,
    },
    NonFinitePosition {
        vertex: usize,
    },
    NonFiniteUv {
        vertex: usize,
    },
}

/// Degeneracy threshold: a triangle whose `|perp_dot| / 2.0` (its signed
/// area) falls below this is treated as degenerate (collinear or
/// coincident points).
const DEGENERATE_AREA_THRESHOLD: f32 = 1e-3;

pub fn validate(m: &MeshData) -> Vec<MeshDefect> {
    let mut defects = Vec::new();

    if m.positions.len() != m.uvs.len() {
        defects.push(MeshDefect::UvCountMismatch {
            positions: m.positions.len(),
            uvs: m.uvs.len(),
        });
    }

    if !m.triangles.len().is_multiple_of(3) {
        defects.push(MeshDefect::RaggedTriangleList {
            len: m.triangles.len(),
        });
    }

    for (vertex, p) in m.positions.iter().enumerate() {
        if !p.is_finite() {
            defects.push(MeshDefect::NonFinitePosition { vertex });
        }
    }
    for (vertex, uv) in m.uvs.iter().enumerate() {
        if !uv.is_finite() {
            defects.push(MeshDefect::NonFiniteUv { vertex });
        }
    }

    for (triangle, tri) in m.triangles.chunks_exact(3).enumerate() {
        let vertex_count = m.positions.len();
        let mut out_of_range = false;
        for &index in tri {
            if index as usize >= vertex_count {
                defects.push(MeshDefect::TriangleIndexOutOfRange {
                    triangle,
                    index,
                    vertex_count,
                });
                out_of_range = true;
            }
        }
        // Skip the degeneracy/area check for a triangle that already has an
        // out-of-range index, to avoid a cascade of noise from one real
        // problem.
        if out_of_range {
            continue;
        }

        let a = m.positions[tri[0] as usize];
        let b = m.positions[tri[1] as usize];
        let c = m.positions[tri[2] as usize];
        let area = (b - a).perp_dot(c - a).abs() / 2.0;
        if area < DEGENERATE_AREA_THRESHOLD {
            defects.push(MeshDefect::DegenerateTriangle { triangle, area });
        }
    }

    defects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::*;
    use glam::Vec2;

    fn ok_mesh() -> MeshData {
        MeshData {
            positions: vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)],
            uvs: vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)],
            triangles: vec![0, 1, 2],
            source: MeshSource::Manual,
        }
    }

    #[test]
    fn a_well_formed_mesh_has_no_defects() {
        assert!(validate(&ok_mesh()).is_empty());
    }

    #[test]
    fn a_dangling_triangle_index_is_reported() {
        let mut m = ok_mesh();
        m.triangles = vec![0, 1, 99];
        assert!(
            validate(&m)
                .iter()
                .any(|d| matches!(d, MeshDefect::TriangleIndexOutOfRange { index: 99, .. }))
        );
    }

    #[test]
    fn uvs_that_are_not_parallel_to_positions_are_reported() {
        let mut m = ok_mesh();
        m.uvs.pop();
        assert!(
            validate(&m)
                .iter()
                .any(|d| matches!(d, MeshDefect::UvCountMismatch { .. }))
        );
    }

    #[test]
    fn a_triangle_list_that_is_not_a_multiple_of_three_is_reported() {
        let mut m = ok_mesh();
        m.triangles = vec![0, 1];
        assert!(
            validate(&m)
                .iter()
                .any(|d| matches!(d, MeshDefect::RaggedTriangleList { len: 2 }))
        );
    }

    #[test]
    fn a_degenerate_triangle_is_reported() {
        let mut m = ok_mesh();
        m.positions[2] = Vec2::new(20.0, 0.0); // all three collinear
        assert!(
            validate(&m)
                .iter()
                .any(|d| matches!(d, MeshDefect::DegenerateTriangle { .. }))
        );
    }

    #[test]
    fn a_non_finite_position_is_reported() {
        let mut m = ok_mesh();
        m.positions[0] = Vec2::new(f32::NAN, 0.0);
        assert!(
            validate(&m)
                .iter()
                .any(|d| matches!(d, MeshDefect::NonFinitePosition { vertex: 0 }))
        );
    }

    #[test]
    fn validate_reports_every_defect_rather_than_stopping_at_the_first() {
        let mut m = ok_mesh();
        m.triangles = vec![0, 1, 99, 0, 1, 98];
        assert_eq!(validate(&m).len(), 2);
    }
}
