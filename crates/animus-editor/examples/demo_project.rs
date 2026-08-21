//! Build a demo `.animus` project from an image, rig included, so the app
//! can be started with a puppet already on stage.
//!
//! A development tool, and it exists because M1's only import path is
//! drag-and-drop: there is no file dialog and no import CLI, so a project
//! cannot otherwise be made without a hand on the mouse. Screenshots,
//! demos and any test of the *load* path need one that can.
//!
//! ```text
//! cargo run -p animus-editor --example demo_project -- <image.png> <out.animus>
//! ```
//!
//! The rig is deliberately crude — a spine down the middle and one bone out
//! to each side, placed from the mesh's own bounds rather than from any
//! understanding of what was drawn. It is enough to see the puppet skinned
//! and deforming; it is not a rigging tool.

use std::path::PathBuf;

use animus_core::doc::{
    Bone, Joint, MeshData, Project, SetSkeleton, SkeletonData, UndoStack, apply_command,
};
use animus_core::ids::{BoneId, JointId, PuppetId};
use animus_core::skeleton::auto_attach;
use animus_editor::import::build_import;
use animus_editor::rig::default_attach_radius;
use animus_project::AssetStore;
use glam::Vec2;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(image), Some(out)) = (args.next(), args.next()) else {
        eprintln!(
            "usage: cargo run -p animus-editor --example demo_project -- <image.png> <out.animus>"
        );
        std::process::exit(2);
    };
    let image = PathBuf::from(image);
    let out = PathBuf::from(out);

    let mut project = Project::new("Demo");
    let mut store = AssetStore::new(&out);
    let mut undo = UndoStack::default();

    let (import, imported) = match build_import(&image, &mut project, &mut store) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("demo_project: {e}");
            std::process::exit(1);
        }
    };
    let puppet_id = import.puppet.id;
    println!(
        "meshed {}: {} vertices, {} triangles, source {}x{}",
        image.display(),
        imported.vertex_count(),
        imported.triangle_count(),
        imported.size.0,
        imported.size.1,
    );

    apply_command(&mut project, &mut undo, Box::new(import)).expect("import applies");

    let mesh = mesh_of(&project, puppet_id);
    let skeleton = rig_from_bounds(&mesh, &mut project);
    let joints = skeleton.joints.len();
    let bones = skeleton.bones.len();
    let attachments = auto_attach(&mesh, &skeleton);
    let attached = attachments
        .entries
        .iter()
        .map(|a| a.vertex)
        .collect::<std::collections::HashSet<_>>()
        .len();

    apply_command(
        &mut project,
        &mut undo,
        Box::new(SetSkeleton::new(puppet_id, "Rig", skeleton, attachments)),
    )
    .expect("rig applies");

    println!(
        "rigged: {joints} joints, {bones} bones, {attached}/{} vertices attached",
        mesh.positions.len()
    );

    // The stage stays 1920x1080. A projector is 16:9, and a demo that quietly
    // reshapes the output to fit its own artwork teaches the wrong thing: the
    // stage is the frame the show is composed *against*, so the puppet is
    // scaled to fit it rather than the other way round.
    let (min, max) = bounds(&mesh);
    let stage = project.stage.canvas;
    let fit = (stage[1] as f32 * 0.82) / (max.y - min.y).max(1.0);
    if let Some(layer) = project
        .layers
        .first()
        .copied()
        .and_then(|id| project.layer_data.get_mut(&id))
    {
        layer.transform = animus_core::doc::Transform2Or3::Flat {
            translation: glam::Vec2::ZERO,
            rotation: 0.0,
            scale: glam::Vec2::splat(fit),
        };
    }
    println!(
        "stage canvas {}x{}px",
        project.stage.canvas[0], project.stage.canvas[1]
    );

    if let Err(e) = animus_project::save(&project, &out) {
        eprintln!("demo_project: could not save {}: {e}", out.display());
        std::process::exit(1);
    }
    println!("wrote {}", out.display());
}

fn mesh_of(project: &Project, puppet: PuppetId) -> MeshData {
    match &project.puppets[&puppet].kind {
        animus_core::doc::PuppetKind::Mesh(m) => m.mesh.clone(),
        _ => unreachable!("the import built a mesh puppet"),
    }
}

/// A spine plus two side bones, sized from the mesh's bounding box.
///
/// Image space: pixels, origin top-left, Y down. The topmost joint is
/// pinned, because a rig whose every joint is free falls out of frame the
/// moment the solver starts.
fn bounds(mesh: &MeshData) -> (Vec2, Vec2) {
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for p in &mesh.positions {
        min = min.min(*p);
        max = max.max(*p);
    }
    (min, max)
}

fn rig_from_bounds(mesh: &MeshData, project: &mut Project) -> SkeletonData {
    let (min, max) = bounds(mesh);
    let size = max - min;
    let centre_x = (min.x + max.x) * 0.5;

    // The widest band of the silhouette: shoulders on a figure, and on
    // anything else simply where the two side bones have most to hold.
    let bands = 24usize;
    let mut widest = (0.0f32, min.y + size.y * 0.3);
    for b in 0..bands {
        let lo = min.y + size.y * (b as f32 / bands as f32);
        let hi = min.y + size.y * ((b + 1) as f32 / bands as f32);
        let (mut bmin, mut bmax) = (f32::INFINITY, f32::NEG_INFINITY);
        for p in mesh.positions.iter().filter(|p| p.y >= lo && p.y < hi) {
            bmin = bmin.min(p.x);
            bmax = bmax.max(p.x);
        }
        if bmax - bmin > widest.0 {
            widest = (bmax - bmin, (lo + hi) * 0.5);
        }
    }

    let spine_n = 5;
    let mut skeleton = SkeletonData::default();
    let mut spine: Vec<JointId> = Vec::new();
    for i in 0..spine_n {
        let t = i as f32 / (spine_n - 1) as f32;
        let y = min.y + size.y * (0.06 + t * 0.88);
        let id = JointId(project.alloc_id());
        skeleton.joints.insert(
            id,
            Joint {
                id,
                name: format!("spine {}", i + 1),
                rest: Vec2::new(centre_x, y),
                rest_angle: 0.0,
                inv_mass: if i == 0 { 0.0 } else { 1.0 },
                pinned: i == 0,
            },
        );
        spine.push(id);
    }

    let side_y = widest.1;
    let reach = widest.0 * 0.5 * 0.85;
    let mut sides = Vec::new();
    for (name, x) in [("left", centre_x - reach), ("right", centre_x + reach)] {
        let id = JointId(project.alloc_id());
        skeleton.joints.insert(
            id,
            Joint {
                id,
                name: name.into(),
                rest: Vec2::new(x, side_y),
                rest_angle: 0.0,
                inv_mass: 1.0,
                pinned: false,
            },
        );
        sides.push(id);
    }

    // The spine joint the side bones hang off: whichever sits closest to the
    // widest band.
    let anchor = *spine
        .iter()
        .min_by(|a, b| {
            let da = (skeleton.joints[*a].rest.y - side_y).abs();
            let db = (skeleton.joints[*b].rest.y - side_y).abs();
            da.total_cmp(&db)
        })
        .expect("the spine has joints");

    // The reach comes from `rig::default_attach_radius`, which is the same
    // rule the Bone tool applies to a bone drawn by hand. A demo rigged by a
    // rule of its own would be a demo of something the application does not
    // do — and this generator's whole job is to produce what a person would
    // have produced with the mouse.
    let link =
        |skeleton: &mut SkeletonData, name: &str, a: JointId, b: JointId, project: &mut Project| {
            let (rest_a, rest_b) = (skeleton.joints[&a].rest, skeleton.joints[&b].rest);
            let id = BoneId(project.alloc_id());
            skeleton.bones.insert(
                id,
                Bone {
                    id,
                    name: name.into(),
                    a,
                    b,
                    rest_length: None,
                    stiffness: 0.8,
                    damping: 0.1,
                    length_mul: 1.0,
                    attach_radius: default_attach_radius(rest_a, rest_b),
                },
            );
        };

    for i in 0..spine_n - 1 {
        link(
            &mut skeleton,
            &format!("spine {}", i + 1),
            spine[i],
            spine[i + 1],
            project,
        );
    }
    link(&mut skeleton, "left", anchor, sides[0], project);
    link(&mut skeleton, "right", anchor, sides[1], project);

    skeleton
}
