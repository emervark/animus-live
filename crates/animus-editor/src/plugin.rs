//! Wiring the editor into the app.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass};

use crate::import::{self, ImportStatus, ProjectRoot};
use crate::state::{EditorState, save_layout};
use crate::viewport::{self, ViewportCamera, ViewportTarget};
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
            .init_resource::<ImportStatus>()
            .init_resource::<ProjectRoot>()
            .add_systems(Update, import::handle_dropped_files)
            // Ordered, not incidental: the window camera must exist before
            // the viewport's offscreen one. See `setup`.
            .add_systems(Startup, (setup, viewport::setup).chain())
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

#[allow(clippy::too_many_arguments)]
fn ui_system(
    mut contexts: EguiContexts,
    mut state: ResMut<EditorState>,
    doc: Res<animus_runtime::DocumentRes>,
    mut target: ResMut<ViewportTarget>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<
        (&Camera, &GlobalTransform, &mut Projection, &mut Transform),
        With<ViewportCamera>,
    >,
    status: Res<ImportStatus>,
    mut installed: Local<bool>,
) -> Result {
    let texture = contexts.image_id(&target.image);
    let ctx = contexts.ctx_mut()?;
    if !*installed {
        theme::install(ctx);
        theme::install_fonts(ctx);
        *installed = true;
    }

    let input = dock::draw(ctx, &mut state, &doc, texture, Some(&target), &status);

    if let Some(input) = input {
        // Resize first: the camera reads the target's size when it
        // unprojects, so a stale size is a wrong world position for one
        // frame — which is exactly the kind of one-frame error that reads as
        // "clicking is slightly off" and takes a day to find.
        let desired = input.desired_target_size();
        if desired != target.size
            && let Some(mut image) = images.get_mut(&target.image)
        {
            image.resize(bevy::render::render_resource::Extent3d {
                width: desired.x,
                height: desired.y,
                ..default()
            });
            target.size = desired;
        }

        if let Ok((camera, global, mut projection, mut transform)) = cameras.single_mut() {
            viewport::apply_viewport_input(
                &input,
                camera,
                global,
                &mut projection,
                &mut transform,
                &mut target,
            );
        }
    }
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
