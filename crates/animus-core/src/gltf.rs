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
    // Collected before reversing: glTF's scene iterator is forward-only,
    // and the stack has to be pushed back to front for a depth-first walk
    // to come out in the file's own order.
    let roots: Vec<gltf::Node> = scene.nodes().collect();
    let mut stack: Vec<(gltf::Node, Option<JointId>)> =
        roots.into_iter().rev().map(|n| (n, None)).collect();

    while let Some((node, parent)) = stack.pop() {
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
            stack.push((child, mine));
        }
    }

    if nodes.is_empty() {
        return Err(GltfError::NoNamedNodes);
    }

    Ok(ModelOutline {
        nodes,
        animations,
        scenes: file.scenes().len(),
    })
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
