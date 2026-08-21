//! Hit-testing the artwork itself, not the rig on top of it.
//!
//! Joints are small and deliberate; the picture is large and is what an
//! operator points at when they mean "that character". So a click that misses
//! every joint falls through to here, and here it asks the only question that
//! matters: is this point on the puppet?
//!
//! Answered against the mesh triangles rather than a bounding box. A humanoid
//! in a T-pose fills about a third of its own box, and a box hit-test means
//! grabbing empty air between the legs picks the character up.

use animus_core::doc::{Project, PuppetKind};
use animus_core::ids::{LayerId, PuppetId};
use animus_runtime::world_to_img;
use glam::Vec2;

/// Where a puppet's artwork has been moved to, in world units.
pub fn layer_offset(project: &Project, layer: LayerId) -> Vec2 {
    use animus_core::doc::Transform2Or3;
    match project.layer_data.get(&layer).map(|l| &l.transform) {
        Some(Transform2Or3::Flat { translation, .. }) => *translation,
        Some(Transform2Or3::Spatial { translation, .. }) => translation.truncate(),
        None => Vec2::ZERO,
    }
}

/// Is this layer on the stage?
pub fn layer_visible(project: &Project, layer: LayerId) -> bool {
    project.layer_data.get(&layer).is_some_and(|l| l.visible)
}

/// Is this puppet on the stage?
///
/// One predicate, used by everything that draws or clicks. Hiding a layer has
/// to hide **the whole puppet** — artwork, mesh wireframe, bones and joints —
/// because those are all the same object seen four ways. A hidden character
/// whose rig still floats over the stage is worse than not hiding it: the
/// operator now has a skeleton with no body and no way to tell what it is.
pub fn puppet_visible(project: &Project, puppet: PuppetId) -> bool {
    layer_of(project, puppet).is_none_or(|l| layer_visible(project, l))
}

/// The layer a puppet stands on.
pub fn layer_of(project: &Project, puppet: PuppetId) -> Option<LayerId> {
    project
        .layer_data
        .iter()
        .find(|(_, l)| l.contents.contains(&puppet))
        .map(|(id, _)| *id)
}

/// A layer's flat placement, as the document holds it.
pub fn layer_placement(project: &Project, layer: LayerId) -> animus_core::doc::LayerPlacement {
    use animus_core::doc::{LayerPlacement, Transform2Or3};
    match project.layer_data.get(&layer).map(|l| &l.transform) {
        Some(Transform2Or3::Flat {
            translation, scale, ..
        }) => LayerPlacement {
            translation: *translation,
            scale: *scale,
        },
        Some(Transform2Or3::Spatial {
            translation, scale, ..
        }) => LayerPlacement {
            translation: translation.truncate(),
            scale: scale.truncate(),
        },
        None => LayerPlacement {
            translation: Vec2::ZERO,
            scale: Vec2::ONE,
        },
    }
}

/// Image pixels → stage (world) units, through the layer.
///
/// **The one conversion.** Image space is the puppet's own; the layer's
/// placement is what stands between it and the stage. Every caller that maps
/// between the two must go through here, because the failure mode of a second
/// copy is not a crash — it is a second puppet on screen, drawn correctly by
/// one path and incorrectly by the other.
///
/// That is exactly what happened: the renderer applied the layer's scale and
/// the gizmos applied only its translation, so scaling a puppet left the
/// wireframe, bones and joints at the old size beside the new artwork.
pub fn img_to_stage(project: &Project, puppet: PuppetId, ppu: f32, img: Vec2) -> Option<Vec2> {
    use animus_runtime::img_to_world;
    let pivot = puppet_pivot_of(project, puppet)?;
    let local = img_to_world(img, pivot, ppu).truncate();
    let pl = placement_of(project, puppet);
    Some(pl.translation + local * pl.scale)
}

/// Stage (world) units → image pixels, through the layer. The inverse of
/// [`img_to_stage`], and it has to stay that way.
pub fn stage_to_img(project: &Project, puppet: PuppetId, ppu: f32, world: Vec2) -> Option<Vec2> {
    let pivot = puppet_pivot_of(project, puppet)?;
    let pl = placement_of(project, puppet);
    // A zero scale would be a division by zero on the path that turns a click
    // into a joint. `MIN_LAYER_SCALE` keeps the document away from it; this
    // keeps the arithmetic safe even if a file arrives that did not.
    let sx = if pl.scale.x.abs() > 1e-6 {
        pl.scale.x
    } else {
        1.0
    };
    let sy = if pl.scale.y.abs() > 1e-6 {
        pl.scale.y
    } else {
        1.0
    };
    let local = Vec2::new(
        (world.x - pl.translation.x) / sx,
        (world.y - pl.translation.y) / sy,
    );
    Some(world_to_img(local.extend(0.0), pivot, ppu))
}

fn puppet_pivot_of(project: &Project, puppet: PuppetId) -> Option<Vec2> {
    match project.puppets.get(&puppet).map(|p| &p.kind) {
        Some(PuppetKind::Mesh(mp)) => Some(animus_runtime::puppet_pivot(mp)),
        _ => None,
    }
}

fn placement_of(project: &Project, puppet: PuppetId) -> animus_core::doc::LayerPlacement {
    layer_of(project, puppet)
        .map(|l| layer_placement(project, l))
        .unwrap_or(animus_core::doc::LayerPlacement {
            translation: Vec2::ZERO,
            scale: Vec2::ONE,
        })
}

/// A puppet's artwork bounds in **its own** space, before the layer moves or
/// resizes it. World units, y up.
pub fn puppet_bounds_local(project: &Project, puppet: PuppetId, ppu: f32) -> Option<(Vec2, Vec2)> {
    use animus_runtime::img_to_world;
    let p = project.puppets.get(&puppet)?;
    let PuppetKind::Mesh(mp) = &p.kind else {
        return None;
    };
    let pivot = animus_runtime::puppet_pivot(mp);
    let mut lo = Vec2::splat(f32::INFINITY);
    let mut hi = Vec2::splat(f32::NEG_INFINITY);
    for v in &mp.mesh.positions {
        let w = img_to_world(*v, pivot, ppu).truncate();
        lo = lo.min(w);
        hi = hi.max(w);
    }
    (lo.x.is_finite() && hi.x > lo.x && hi.y > lo.y).then_some((lo, hi))
}

/// The selection box on the stage: the artwork bounds with the layer applied.
pub fn selection_box(project: &Project, puppet: PuppetId, ppu: f32) -> Option<(Vec2, Vec2)> {
    let (lo, hi) = puppet_bounds_local(project, puppet, ppu)?;
    let layer = layer_of(project, puppet)?;
    let pl = layer_placement(project, layer);
    let a = pl.translation + lo * pl.scale;
    let b = pl.translation + hi * pl.scale;
    Some((a.min(b), a.max(b)))
}

/// Which corner of a selection box, if any.
///
/// Corners only. Edge handles would give eight targets in the space four can
/// hold at the sizes a puppet is usually drawn, and a handle you cannot hit is
/// worse than one that is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    BottomLeft,
    BottomRight,
    TopRight,
    TopLeft,
}

pub const CORNERS: [Corner; 4] = [
    Corner::BottomLeft,
    Corner::BottomRight,
    Corner::TopRight,
    Corner::TopLeft,
];

impl Corner {
    /// This corner of a box.
    pub fn of(self, lo: Vec2, hi: Vec2) -> Vec2 {
        match self {
            Corner::BottomLeft => Vec2::new(lo.x, lo.y),
            Corner::BottomRight => Vec2::new(hi.x, lo.y),
            Corner::TopRight => Vec2::new(hi.x, hi.y),
            Corner::TopLeft => Vec2::new(lo.x, hi.y),
        }
    }

    /// The corner diagonally across, which stays put while this one is
    /// dragged — the anchor.
    pub fn opposite(self) -> Corner {
        match self {
            Corner::BottomLeft => Corner::TopRight,
            Corner::BottomRight => Corner::TopLeft,
            Corner::TopRight => Corner::BottomLeft,
            Corner::TopLeft => Corner::BottomRight,
        }
    }
}

/// The handle under a world point, within `radius` world units.
pub fn corner_at(lo: Vec2, hi: Vec2, p: Vec2, radius: f32) -> Option<Corner> {
    CORNERS
        .into_iter()
        .map(|c| (c, c.of(lo, hi).distance(p)))
        .filter(|(_, d)| *d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(c, _)| c)
}

/// The puppet under a world-space point, front-most first.
///
/// Front-most because that is what the operator can see. `Project::layers` is
/// back-to-front, so this walks it in reverse: the puppet whose artwork is on
/// top is the one the click means.
pub fn puppet_at(project: &Project, ppu: f32, world: Vec2) -> Option<(PuppetId, LayerId)> {
    for layer_id in project.layers.iter().rev() {
        let Some(layer) = project.layer_data.get(layer_id) else {
            continue;
        };
        // A hidden layer is not clickable either. It is not on the stage, so
        // it is not under the cursor.
        if !layer.visible {
            continue;
        }
        for puppet_id in layer.contents.iter().rev() {
            let Some(puppet) = project.puppets.get(puppet_id) else {
                continue;
            };
            let PuppetKind::Mesh(mp) = &puppet.kind else {
                continue;
            };
            let Some(img) = stage_to_img(project, *puppet_id, ppu, world) else {
                continue;
            };
            if point_in_mesh(&mp.mesh.positions, &mp.mesh.triangles, img) {
                return Some((*puppet_id, *layer_id));
            }
        }
    }
    None
}

/// Is `p` inside any triangle of the mesh?
pub fn point_in_mesh(positions: &[Vec2], triangles: &[u32], p: Vec2) -> bool {
    triangles.chunks_exact(3).any(|t| {
        let (Some(a), Some(b), Some(c)) = (
            positions.get(t[0] as usize),
            positions.get(t[1] as usize),
            positions.get(t[2] as usize),
        ) else {
            return false;
        };
        point_in_triangle(p, *a, *b, *c)
    })
}

/// Barycentric sign test, winding-agnostic.
///
/// Winding-agnostic on purpose: the triangulator's output order is not
/// something this should depend on, and a hit test that only worked for
/// counter-clockwise triangles would pass every unit test and then fail on
/// half of a real mesh.
fn point_in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let d1 = cross(p - a, b - a);
    let d2 = cross(p - b, c - b);
    let d3 = cross(p - c, a - c);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

fn cross(u: Vec2, v: Vec2) -> f32 {
    u.x * v.y - u.y * v.x
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square made of two triangles, 0..10.
    fn square() -> (Vec<Vec2>, Vec<u32>) {
        (
            vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(10.0, 10.0),
                Vec2::new(0.0, 10.0),
            ],
            vec![0, 1, 2, 0, 2, 3],
        )
    }

    #[test]
    fn a_point_inside_the_artwork_hits_it() {
        let (pos, tri) = square();
        assert!(point_in_mesh(&pos, &tri, Vec2::new(5.0, 5.0)));
        assert!(point_in_mesh(&pos, &tri, Vec2::new(0.5, 9.5)));
    }

    #[test]
    fn a_point_outside_does_not() {
        let (pos, tri) = square();
        assert!(!point_in_mesh(&pos, &tri, Vec2::new(-1.0, 5.0)));
        assert!(!point_in_mesh(&pos, &tri, Vec2::new(11.0, 5.0)));
        assert!(!point_in_mesh(&pos, &tri, Vec2::new(5.0, 20.0)));
    }

    /// The gap a bounding box would wrongly claim.
    ///
    /// Two legs with air between them: the box says hit, the mesh says miss,
    /// and the mesh is right — grabbing the gap between a character's legs
    /// must not pick the character up.
    #[test]
    fn the_gap_between_two_limbs_is_not_a_hit() {
        let positions = vec![
            // left leg
            Vec2::new(0.0, 0.0),
            Vec2::new(3.0, 0.0),
            Vec2::new(3.0, 10.0),
            Vec2::new(0.0, 10.0),
            // right leg
            Vec2::new(7.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(7.0, 10.0),
        ];
        let triangles = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
        assert!(point_in_mesh(&positions, &triangles, Vec2::new(1.5, 5.0)));
        assert!(point_in_mesh(&positions, &triangles, Vec2::new(8.5, 5.0)));
        assert!(
            !point_in_mesh(&positions, &triangles, Vec2::new(5.0, 5.0)),
            "the air between the legs is inside the bounding box and outside the puppet"
        );
    }

    /// Winding must not decide the answer.
    #[test]
    fn a_clockwise_triangle_is_hit_the_same_as_a_counter_clockwise_one() {
        let ccw = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)];
        let cw = [Vec2::ZERO, Vec2::new(0.0, 10.0), Vec2::new(10.0, 0.0)];
        let inside = Vec2::new(2.0, 2.0);
        assert!(point_in_triangle(inside, ccw[0], ccw[1], ccw[2]));
        assert!(point_in_triangle(inside, cw[0], cw[1], cw[2]));
    }

    #[test]
    fn a_ragged_triangle_list_is_ignored_rather_than_panicking() {
        let (pos, mut tri) = square();
        tri.push(99);
        assert!(point_in_mesh(&pos, &tri, Vec2::new(5.0, 5.0)));
        // An index past the end must not be read.
        assert!(!point_in_mesh(&pos, &[0, 1, 99], Vec2::new(5.0, 5.0)));
    }

    /// A puppet with no layer at all is still drawn.
    ///
    /// Orphaned is not the same as hidden, and treating it as hidden would
    /// make a data problem invisible instead of visible.
    #[test]
    fn a_puppet_on_no_layer_is_visible() {
        let project = Project::new("empty");
        assert!(puppet_visible(&project, PuppetId(1)));
    }

    // ── the selection box ──────────────────────────────────────────────

    #[test]
    fn every_corner_is_its_own_and_opposites_are_diagonal() {
        let (lo, hi) = (Vec2::new(-2.0, -1.0), Vec2::new(4.0, 3.0));
        let seen: Vec<Vec2> = CORNERS.into_iter().map(|c| c.of(lo, hi)).collect();
        for (i, a) in seen.iter().enumerate() {
            for b in seen.iter().skip(i + 1) {
                assert_ne!(a, b, "two corners landed on the same point");
            }
        }
        for c in CORNERS {
            assert_eq!(c.opposite().opposite(), c);
            let here = c.of(lo, hi);
            let there = c.opposite().of(lo, hi);
            assert_ne!(here.x, there.x, "the anchor must differ on both axes");
            assert_ne!(here.y, there.y);
        }
    }

    #[test]
    fn the_nearest_corner_wins_and_a_far_click_hits_none() {
        let (lo, hi) = (Vec2::ZERO, Vec2::new(10.0, 10.0));
        assert_eq!(
            corner_at(lo, hi, Vec2::new(0.4, 0.4), 1.0),
            Some(Corner::BottomLeft)
        );
        assert_eq!(
            corner_at(lo, hi, Vec2::new(9.7, 9.7), 1.0),
            Some(Corner::TopRight)
        );
        assert_eq!(corner_at(lo, hi, Vec2::new(5.0, 5.0), 1.0), None);
    }

    // ── the one conversion ─────────────────────────────────────────────

    use animus_core::doc::{
        Layer, MeshData, MeshPuppet, Puppet, PuppetKind, SkeletonData, Transform2Or3,
    };
    use animus_core::ids::{AssetId, LayerId as LId, PuppetId as PId};

    /// A one-triangle puppet on a layer with the given placement.
    fn staged(translation: Vec2, scale: Vec2) -> (Project, PId) {
        let mut p = Project::new("t");
        let pid = PId(7);
        let lid = LId(3);
        let mesh = MeshData {
            positions: vec![Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(0.0, 100.0)],
            uvs: vec![Vec2::ZERO; 3],
            triangles: vec![0, 1, 2],
            source: Default::default(),
        };
        p.puppets.insert(
            pid,
            Puppet {
                id: pid,
                name: "t".into(),
                kind: PuppetKind::Mesh(MeshPuppet {
                    texture: AssetId(1),
                    matte: Default::default(),
                    mesh,
                    skeleton: SkeletonData::default(),
                    attachments: Default::default(),
                    material: Default::default(),
                    solver_override: None,
                }),
            },
        );
        let mut layer = Layer::new(lid, "l");
        layer.contents.push(pid);
        layer.transform = Transform2Or3::Flat {
            translation,
            rotation: 0.0,
            scale,
        };
        p.layer_data.insert(lid, layer);
        p.layers.push(lid);
        (p, pid)
    }

    /// **The bug this closes.** The renderer applied the layer's scale and
    /// the gizmos applied only its translation, so scaling a puppet drew the
    /// rig at one size and the artwork at another — two puppets on screen.
    ///
    /// A round trip through both directions is the property that keeps them
    /// honest: if either one forgets the scale, this stops closing.
    #[test]
    fn image_to_stage_and_back_is_the_identity_under_scale_and_offset() {
        for (t, sc) in [
            (Vec2::ZERO, Vec2::ONE),
            (Vec2::new(3.5, -2.0), Vec2::ONE),
            (Vec2::ZERO, Vec2::splat(0.4)),
            (Vec2::new(-7.0, 4.25), Vec2::new(2.5, 0.75)),
        ] {
            let (project, pid) = staged(t, sc);
            for img in [Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(37.5, -12.25)] {
                let world = img_to_stage(&project, pid, 100.0, img).expect("forward");
                let back = stage_to_img(&project, pid, 100.0, world).expect("inverse");
                assert!(
                    img.distance(back) < 1e-3,
                    "t={t:?} scale={sc:?}: {img:?} -> {world:?} -> {back:?}"
                );
            }
        }
    }

    /// Scaling has to actually move things, or the round trip above would
    /// pass on a pair of functions that both ignore it.
    #[test]
    fn scale_changes_where_a_point_lands() {
        let (unscaled, pid) = staged(Vec2::ZERO, Vec2::ONE);
        let (half, _) = staged(Vec2::ZERO, Vec2::splat(0.5));
        let img = Vec2::new(100.0, 0.0);
        let a = img_to_stage(&unscaled, pid, 100.0, img).unwrap();
        let b = img_to_stage(&half, pid, 100.0, img).unwrap();
        assert!(
            (a * 0.5).distance(b) < 1e-4,
            "half scale should halve the offset from the layer origin: {a:?} vs {b:?}"
        );
    }

    /// A degenerate scale must not divide by zero on the path that turns a
    /// click into a joint.
    #[test]
    fn a_zero_scale_does_not_produce_a_nan() {
        let (project, pid) = staged(Vec2::ZERO, Vec2::ZERO);
        let img = stage_to_img(&project, pid, 100.0, Vec2::new(5.0, 5.0)).unwrap();
        assert!(img.is_finite(), "got {img:?}");
    }

    /// Hit-testing must follow the scale too, or a shrunk puppet stays
    /// clickable at its old size and unclickable at its new one.
    #[test]
    fn a_scaled_puppet_is_hit_where_it_is_drawn() {
        let (project, pid) = staged(Vec2::ZERO, Vec2::splat(0.5));
        // A point inside the triangle in image space.
        let inside_img = Vec2::new(20.0, 20.0);
        let world = img_to_stage(&project, pid, 100.0, inside_img).unwrap();
        assert_eq!(
            puppet_at(&project, 100.0, world).map(|(p, _)| p),
            Some(pid),
            "the click that lands on the drawn artwork must find it"
        );
    }
}
