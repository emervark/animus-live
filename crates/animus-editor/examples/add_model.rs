//! Add a glTF model to an existing `.animus` project.
//!
//! The same reason `demo_project` exists: the only import path is
//! drag-and-drop, so nothing can put a model into a project without a hand
//! on the mouse — and the load path, the projection and the rig tree all
//! need one that can.
//!
//! ```text
//! cargo run -p animus-editor --example add_model -- <model.gltf> <project.animus>
//! ```

use std::path::PathBuf;

use animus_core::doc::{UndoStack, apply_command};
use animus_editor::import::build_model_import;
use animus_project::AssetStore;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let (Some(model), Some(project_dir)) = (args.next(), args.next()) else {
        eprintln!("usage: add_model <model.gltf> <project.animus>");
        std::process::exit(2);
    };
    let model = PathBuf::from(model);
    let project_dir = PathBuf::from(project_dir);

    let mut project = animus_project::load(&project_dir)?;
    let mut store = AssetStore::new(&project_dir);

    let (command, outline) = build_model_import(&model, &mut project, &mut store)?;
    let name = command.puppet.name.clone();

    let mut undo = UndoStack::new();
    apply_command(&mut project, &mut undo, Box::new(command))?;
    animus_project::save(&project, &project_dir)?;

    println!(
        "added {name}: {} node(s), {} clip(s), {} scene(s)",
        outline.nodes.len(),
        outline.animations.len(),
        outline.scenes
    );
    for node in outline.nodes.iter().take(12) {
        println!("  {:?}  {}", node.id, node.name);
    }
    Ok(())
}
