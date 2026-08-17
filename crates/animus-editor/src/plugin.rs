//! Wiring the editor into the app.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass};

use crate::state::{EditorState, save_layout};
use crate::{dock, theme};

/// The editor's systems, in `EguiPrimaryContextPass`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorSet {
    /// Draw the dock and panels.
    Ui,
    /// Turn tool output into `DocCommand`s. Task 10 onward.
    Commands,
}

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }
        app.init_resource::<EditorState>()
            .add_systems(Startup, setup)
            .configure_sets(
                EguiPrimaryContextPass,
                (EditorSet::Ui, EditorSet::Commands).chain(),
            )
            .add_systems(EguiPrimaryContextPass, ui_system.in_set(EditorSet::Ui))
            .add_systems(Last, persist_layout_on_exit);
    }
}

/// Spawns the window camera **before** anything spawns an offscreen one.
///
/// `bevy_egui` attaches its auto-created primary context to the *first*
/// camera spawned. If that turns out to be the viewport's offscreen camera,
/// egui renders into a `Bgra8UnormSrgb` render target with a pipeline built
/// for `Rgba8UnormSrgb` and wgpu rejects it. M0-2 found this by crashing;
/// the ordering here is the fix, and it is the reason this system exists at
/// all rather than the camera being spawned wherever it is needed.
fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn ui_system(
    mut contexts: EguiContexts,
    mut state: ResMut<EditorState>,
    doc: Res<animus_runtime::DocumentRes>,
    mut installed: Local<bool>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if !*installed {
        theme::install(ctx);
        theme::install_fonts(ctx);
        *installed = true;
    }
    dock::draw(ctx, &mut state, &doc);
    Ok(())
}

/// Layout is a preference, so it is written when the app closes rather than
/// on every rearrangement — a file write per splitter drag would be absurd.
fn persist_layout_on_exit(
    mut exits: MessageReader<AppExit>,
    state: Res<EditorState>,
    mut saved: Local<bool>,
) {
    if *saved {
        return;
    }
    if exits.read().next().is_some() {
        save_layout(&state.dock);
        *saved = true;
    }
}
