//! Reading a glTF's structure, before anything is spawned.
//!
//! Only the skeleton and the clip names are read here — never the geometry.
//! Bevy loads the meshes, materials and textures because it is the thing
//! that will draw them; what this needs is the shape of the node tree, and
//! it needs it at *import* time, in a crate with no engine in it, so that
//! the ids a document hands out can be minted and tested without a window.
//!
//! **Ids are minted once and then kept.** A binding refers to a `JointId`;
//! an id derived fresh each session from whatever order a scene happened to
//! spawn in would point at a different limb every time the file was opened.

use crate::doc::ModelNode;
use crate::ids::{IdAlloc, JointId};
use glam::{Mat4, Vec3};

#[derive(Debug, thiserror::Error)]
pub enum GltfError {
    #[error("this file is not readable as glTF: {0}")]
    NotGltf(String),
    #[error(
        "this model has no named nodes, so nothing in it can be driven. \\
         Export it with node names — most exporters call this 'preserve \\
         hierarchy' or 'export empties'."
    )]
    NoNamedNodes,
}

/// What an import needs to know about a model.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelOutline {
    /// The nodes worth showing, with ids already assigned.
    pub nodes: Vec<ModelNode>,
    /// Clip names, in the order the file lists them.
    pub animations: Vec<String>,
    /// How many scenes it holds. Only the first is used, and a file with
    /// more says so rather than silently dropping the rest.
    pub scenes: usize,
    /// The scene's extent in its own units, as `(min, max)`, or `None` for a
    /// file whose meshes declare no bounds.
    ///
    /// **Read so that an import can be seen.** A glTF is authored in metres
    /// and this stage is about ten units tall, so a human model dropped in at
    /// its own scale arrives eighteen percent of the height of the frame —
    /// small enough that the first thing an operator does is wonder whether
    /// the import failed. Knowing how big it is means it can be placed at a
    /// size worth looking at, once, at import.
    pub bounds: Option<(Vec3, Vec3)>,
}

impl ModelOutline {
    /// How tall the model is in its own units, or `None` if it said nothing.
    pub fn height(&self) -> Option<f32> {
        self.bounds.map(|(min, max)| (max.y - min.y).abs())
    }
}

/// Read a `.glb` or `.gltf` and describe its skeleton.
///
/// `alloc` mints the ids, so they come from the document's own never-reused
/// sequence rather than from a counter that starts again at one per file —
/// two models in a show must not share joint ids.
pub fn outline(bytes: &[u8], alloc: &mut IdAlloc) -> Result<ModelOutline, GltfError> {
    let file = gltf::Gltf::from_slice(bytes).map_err(|e| GltfError::NotGltf(e.to_string()))?;

    let animations: Vec<String> = file
        .animations()
        .enumerate()
        .map(|(i, a)| {
            a.name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("clip {i}"))
        })
        .collect();

    // Walk the first scene depth-first, so the rig tree reads down a limb
    // rather than across every node at the same distance from the root.
    let Some(scene) = file.scenes().next() else {
        return Err(GltfError::NoNamedNodes);
    };

    let mut nodes = Vec::new();
    // Two boxes, because a file can hold both kinds of geometry and they are
    // not measured the same way. See `bounds` below.
    let mut solid: Option<(Vec3, Vec3)> = None;
    let mut rig: Option<(Vec3, Vec3)> = None;
    let mut skinned = false;
    // Collected before reversing: glTF's scene iterator is forward-only,
    // and the stack has to be pushed back to front for a depth-first walk
    // to come out in the file's own order.
    let roots: Vec<gltf::Node> = scene.nodes().collect();
    let mut stack: Vec<(gltf::Node, Option<JointId>, Mat4)> = roots
        .into_iter()
        .rev()
        .map(|n| (n, None, Mat4::IDENTITY))
        .collect();

    while let Some((node, parent, above)) = stack.pop() {
        let here = above * Mat4::from_cols_array_2d(&node.transform().matrix());
        // Where this node sits. Cheap, and for a rigged model it is the only
        // honest measurement available here — see `rig` below.
        //
        // A node that carries a *skinned* mesh is left out, because its
        // transform is the one thing in the file the specification says to
        // ignore. Exporters do park such a node far from everything else, and
        // a measurement that believed it would stretch the model's box across
        // empty space.
        let skinned_here = node.skin().is_some() && node.mesh().is_some();
        if !skinned_here {
            expand(&mut rig, here.transform_point3(Vec3::ZERO));
        }

        // A static mesh's own declared corner box, placed by the transform it
        // hangs under. glTF requires POSITION accessors to carry min and max,
        // so this needs no geometry read — which matters, because this
        // function runs in a crate that deliberately cannot decode buffers.
        //
        // **A skinned mesh contributes nothing here**, and this is the part
        // that is easy to get wrong. Its vertices are not in node space at
        // all: they are in whatever space its `inverseBindMatrices` map from,
        // which is routinely a different unit entirely — this is why a rig
        // measured in metres can carry a mesh whose accessors read in
        // hundreds. Reading those matrices means decoding a buffer, which is
        // exactly what this crate refuses to do, so a skinned mesh is
        // measured by its skeleton instead of by its vertices.
        if let Some(mesh) = node.mesh() {
            if skinned_here {
                skinned = true;
            } else {
                for primitive in mesh.primitives() {
                    let bb = primitive.bounding_box();
                    for corner in corners(Vec3::from(bb.min), Vec3::from(bb.max)) {
                        expand(&mut solid, here.transform_point3(corner));
                    }
                }
            }
        }
        // Unnamed nodes are skipped rather than given a made-up name: the
        // projection finds a spawned entity *by* its name, so an invented
        // one would be an id that can never resolve to anything. Their
        // children still come along, parented to the nearest named
        // ancestor, because a gap in the middle of a limb should not
        // detach the hand.
        let mine = match node.name() {
            Some(name) if !name.trim().is_empty() => {
                let id = JointId(alloc.next());
                nodes.push(ModelNode {
                    id,
                    name: name.to_string(),
                    parent,
                });
                Some(id)
            }
            _ => parent,
        };
        for child in node.children().collect::<Vec<_>>().into_iter().rev() {
            stack.push((child, mine, here));
        }
    }

    if nodes.is_empty() {
        return Err(GltfError::NoNamedNodes);
    }

    // A rigged model is sized by its bones, an unrigged one by its geometry,
    // and one that is both by the two together. Bones sit *inside* the body
    // they carry, so a skeleton reads a little smaller than the figure drawn
    // over it — which is why whatever fits a model to a stage should leave
    // headroom rather than aim at the edge of the frame.
    let bounds = match (skinned, solid, rig) {
        (true, Some(s), Some(r)) => Some((s.0.min(r.0), s.1.max(r.1))),
        (true, s, None) => s,
        (true, None, r) => r,
        (false, Some(s), _) => Some(s),
        (false, None, r) => r,
    };

    Ok(ModelOutline {
        nodes,
        animations,
        scenes: file.scenes().len(),
        bounds,
    })
}

fn expand(box_: &mut Option<(Vec3, Vec3)>, p: Vec3) {
    *box_ = Some(match *box_ {
        None => (p, p),
        Some((min, max)) => (min.min(p), max.max(p)),
    });
}

/// The eight corners of a box. All eight, because a rotated node turns a box
/// into something whose extent two opposite corners no longer describe.
fn corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, max.y, max.z),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal glTF with a named hierarchy, written by hand so the test
    /// depends on the format rather than on an exporter.
    fn tiny_gltf(nodes_json: &str, extra: &str) -> Vec<u8> {
        format!(
            r#"{{
              "asset": {{ "version": "2.0" }},
              "scene": 0,
              "scenes": [ {{ "nodes": [0] }} ],
              "nodes": {nodes_json}
              {extra}
            }}"#
        )
        .into_bytes()
    }

    /// One mesh with a declared corner box, optionally skinned. The accessor
    /// carries `min`/`max` and no buffer view, which is all `bounding_box`
    /// reads — no bytes are needed for a bound.
    fn boxed_mesh(min: [f32; 3], max: [f32; 3], skinned: bool) -> String {
        let skin = if skinned {
            r#", "skins": [ { "joints": [1] } ]"#
        } else {
            ""
        };
        format!(
            r#",
            "meshes": [ {{ "primitives": [ {{ "attributes": {{ "POSITION": 0 }} }} ] }} ],
            "buffers": [ {{
              "byteLength": 24,
              "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
            }} ],
            "bufferViews": [ {{ "buffer": 0, "byteOffset": 0, "byteLength": 24 }} ],
            "accessors": [ {{
              "bufferView": 0, "componentType": 5126, "count": 2, "type": "VEC3",
              "min": [{}, {}, {}], "max": [{}, {}, {}]
            }} ]{skin}"#,
            min[0], min[1], min[2], max[0], max[1], max[2]
        )
    }

    /// **A static mesh is measured where its node puts it.** The box has to
    /// travel through the transform chain, or a model assembled out of parts
    /// placed by their parents measures as though every part sat at the
    /// origin.
    #[test]
    fn a_static_mesh_is_measured_through_its_parents_transform() {
        let bytes = tiny_gltf(
            r#"[
              { "name": "root", "translation": [10.0, 0.0, 0.0], "children": [1] },
              { "name": "part", "mesh": 0 }
            ]"#,
            &boxed_mesh([-1.0, -2.0, -1.0], [1.0, 2.0, 1.0], false),
        );
        let mut alloc = IdAlloc::from_next(1);
        let outline = outline(&bytes, &mut alloc).expect("reads");
        let (min, max) = outline.bounds.expect("has bounds");
        assert!((min.x - 9.0).abs() < 1e-4, "moved with its parent: {min:?}");
        assert!((max.x - 11.0).abs() < 1e-4, "{max:?}");
        assert!((outline.height().unwrap() - 4.0).abs() < 1e-4);
    }

    /// **A skinned mesh's vertices are not in node space**, and the glTF
    /// specification says its node transform must be ignored. Worse than
    /// wrong: its accessors are routinely in a different unit from the rig
    /// they belong to, so measuring them at all would size a model in
    /// centimetres against a stage in metres. The skeleton is measured
    /// instead — and the mesh node here is parked five hundred units away on
    /// purpose, because that is exactly the placement an exporter writes when
    /// it knows nothing will read it.
    #[test]
    fn a_skinned_mesh_is_measured_by_its_skeleton_not_its_vertices() {
        let bytes = tiny_gltf(
            r#"[
              { "name": "root", "children": [1, 2] },
              { "name": "bone", "translation": [0.0, 1.8, 0.0] },
              { "name": "body", "translation": [0.0, 500.0, 0.0], "mesh": 0, "skin": 0 }
            ]"#,
            &boxed_mesh([-50.0, 0.0, -50.0], [50.0, 180.0, 50.0], true),
        );
        let mut alloc = IdAlloc::from_next(1);
        let outline = outline(&bytes, &mut alloc).expect("reads");
        let (min, max) = outline.bounds.expect("has bounds");
        assert!(
            max.y < 2.0 && min.y >= 0.0,
            "the rig's 1.8, not the mesh's 180 or its node's 500: {min:?}..{max:?}"
        );
    }

    /// A file that declares nothing measurable says so, rather than
    /// returning a box of zero size that something downstream divides by.
    #[test]
    fn a_model_with_no_geometry_still_has_the_extent_of_its_nodes() {
        let bytes = tiny_gltf(
            r#"[
              { "name": "a", "children": [1] },
              { "name": "b", "translation": [0.0, 3.0, 0.0] }
            ]"#,
            "",
        );
        let mut alloc = IdAlloc::from_next(1);
        let outline = outline(&bytes, &mut alloc).expect("reads");
        assert!((outline.height().unwrap() - 3.0).abs() < 1e-4);
    }

    #[test]
    fn a_named_hierarchy_becomes_nodes_with_parents() {
        let bytes = tiny_gltf(
            r#"[
              { "name": "hips",  "children": [1] },
              { "name": "spine", "children": [2] },
              { "name": "head" }
            ]"#,
            "",
        );
        let mut alloc = IdAlloc::from_next(100);
        let outline = outline(&bytes, &mut alloc).expect("reads");

        let names: Vec<&str> = outline.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["hips", "spine", "head"], "depth first");
        assert_eq!(outline.nodes[0].parent, None);
        assert_eq!(outline.nodes[1].parent, Some(outline.nodes[0].id));
        assert_eq!(outline.nodes[2].parent, Some(outline.nodes[1].id));
    }

    /// **A gap must not detach the limb.** An unnamed node cannot be found
    /// in the spawned scene, so it gets no id — but its children parent to
    /// the nearest named ancestor rather than becoming roots.
    #[test]
    fn an_unnamed_node_is_skipped_and_its_children_reattach() {
        let bytes = tiny_gltf(
            r#"[
              { "name": "hips", "children": [1] },
              { "children": [2] },
              { "name": "hand" }
            ]"#,
            "",
        );
        let mut alloc = IdAlloc::from_next(1);
        let outline = outline(&bytes, &mut alloc).expect("reads");

        assert_eq!(outline.nodes.len(), 2, "the unnamed one gets no id");
        assert_eq!(outline.nodes[1].name, "hand");
        assert_eq!(
            outline.nodes[1].parent,
            Some(outline.nodes[0].id),
            "the hand hangs from the hips, not from nothing"
        );
    }

    /// Ids come from the document's own sequence, so two models in one show
    /// cannot collide.
    #[test]
    fn ids_come_from_the_documents_allocator() {
        let bytes = tiny_gltf(r#"[ { "name": "root" } ]"#, "");
        let mut alloc = IdAlloc::from_next(500);
        let first = outline(&bytes, &mut alloc).expect("reads");
        let second = outline(&bytes, &mut alloc).expect("reads");
        assert_ne!(
            first.nodes[0].id, second.nodes[0].id,
            "a second import must not reuse the first's ids"
        );
        assert!(first.nodes[0].id.0 >= 500);
    }

    #[test]
    fn clip_names_are_read_and_unnamed_clips_still_get_one() {
        let bytes = tiny_gltf(
            r#"[ { "name": "root" } ]"#,
            r#", "animations": [
                 { "name": "wave", "channels": [], "samplers": [] },
                 { "channels": [], "samplers": [] }
               ]"#,
        );
        let mut alloc = IdAlloc::from_next(1);
        let outline = outline(&bytes, &mut alloc).expect("reads");
        assert_eq!(outline.animations, vec!["wave", "clip 1"]);
    }

    /// A model with nothing named cannot be driven, and saying so beats
    /// importing something inert and leaving the operator to work out why
    /// the rig tree is empty.
    #[test]
    fn a_model_with_no_named_nodes_is_refused_with_a_reason() {
        let bytes = tiny_gltf(r#"[ { } ]"#, "");
        let mut alloc = IdAlloc::from_next(1);
        let err = outline(&bytes, &mut alloc).expect_err("must refuse");
        assert!(matches!(err, GltfError::NoNamedNodes));
        assert!(err.to_string().contains("node names"));
    }

    #[test]
    fn rubbish_is_refused_rather_than_panicking() {
        let mut alloc = IdAlloc::from_next(1);
        assert!(outline(b"not a model at all", &mut alloc).is_err());
    }
}
