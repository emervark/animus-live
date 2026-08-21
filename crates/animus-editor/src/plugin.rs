//! Wiring the editor into the app.

use bevy::prelude::*;
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext,
};

use crate::files;
use crate::fit;
use crate::gizmos;
use crate::import::{self, ImportStatus, ProjectRoot};
use crate::interact::{
    self, ActiveDrag, EguiWantsKeyboard, FrameDockOutput, FrameViewportInput, PendingBone,
};
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
        // Left on, `bevy_egui` picks the primary context's camera itself: the
        // first entity its `Added<Camera>` query yields, which is archetype
        // order, not spawn order. Spawning the window camera first is
        // therefore not the guarantee it reads as, and when the output
        // camera won the race the whole editor drew onto the projector
        // window — with the cursor hidden, because that window hides it.
        app.world_mut()
            .resource_mut::<EguiGlobalSettings>()
            .auto_create_primary_context = false;
        app.init_resource::<EditorState>()
            .init_resource::<ImportStatus>()
            .init_resource::<ActiveDrag>()
            .init_resource::<FrameViewportInput>()
            .init_resource::<FrameDockOutput>()
            .init_resource::<EguiWantsKeyboard>()
            .init_resource::<PendingBone>()
            .init_resource::<ProjectRoot>()
            .init_gizmo_group::<gizmos::WireGizmos>()
            .add_systems(Startup, import::load_existing_textures)
            .add_systems(Update, import::handle_dropped_files)
            .add_systems(Update, interact::apply_interactions)
            .add_systems(Update, interact::apply_dock_output)
            .add_systems(Update, interact::keyboard_shortcuts)
            .add_systems(Update, crate::mode::settle_on_mode_change)
            .add_systems(Update, crate::mode::track_selection_live)
            .add_systems(Update, crate::mode::hold_rest_in_rig)
            .init_resource::<crate::rotate::LiveRotations>()
            .init_resource::<crate::rotate::RotationDriven>()
            .init_resource::<crate::rotate::RotationDrag>()
            // After the drag, so a hand that has grabbed a joint has already
            // claimed it and the rotation steps around it.
            .add_systems(
                Update,
                crate::rotate::apply_live_rotations.after(interact::apply_interactions),
            )
            .init_resource::<interact::LayerDrag>()
            .init_resource::<files::FileRequest>()
            .init_resource::<files::ProjectSaved>()
            .init_resource::<files::FileStatus>()
            .add_systems(
                Update,
                (files::file_shortcuts, files::apply_file_request).chain(),
            )
            .init_resource::<fit::WantsFit>()
            .init_resource::<fit::AutoFitDone>()
            .add_systems(
                Update,
                (fit::request_fit_on_load, fit::fit_shortcut, fit::apply_fit).chain(),
            )
            .add_systems(
                PostUpdate,
                (
                    gizmos::draw_stage,
                    gizmos::draw_rigs,
                    gizmos::draw_selection_box,
                    gizmos::draw_rotation_gizmo,
                )
                    .chain(),
            )
            // Ordered, not incidental: the window camera must exist before
            // the viewport's offscreen one. See `setup`.
            .add_systems(Startup, (setup, viewport::setup, gizmos::setup).chain())
            .configure_sets(
                EguiPrimaryContextPass,
                (EditorSet::Ui, EditorSet::Commands).chain(),
            )
            .add_systems(EguiPrimaryContextPass, ui_system.in_set(EditorSet::Ui))
            .add_systems(Last, persist_layout_on_exit);
    }
}

/// Spawns the editor's window camera and names it the primary egui context.
///
/// The context belongs here and nowhere else. On the viewport's offscreen
/// camera, egui renders into a `Bgra8UnormSrgb` target with a pipeline built
/// for `Rgba8UnormSrgb` and wgpu rejects it (M0-2 found this by crashing);
/// on the output camera the editor draws onto the projector. Both used to
/// depend on spawn order, which `bevy_egui` does not actually promise, so
/// the attachment is stated outright — see `auto_create_primary_context` in
/// the plugin above.
fn setup(mut commands: Commands) {
    commands.spawn((Camera2d, PrimaryEguiContext));
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
    mut frame_input: ResMut<FrameViewportInput>,
    mut dock_out: ResMut<FrameDockOutput>,
    mut egui_focus: ResMut<EguiWantsKeyboard>,
    output_state: Option<Res<animus_output::OutputState>>,
    output_config: Option<Res<animus_output::OutputConfig>>,
    seq: Res<animus_runtime::Sequencer>,
    file_status: Res<files::FileStatus>,
    mut file_request: ResMut<files::FileRequest>,
    mut installed: Local<bool>,
) -> Result {
    let texture = contexts.image_id(&target.image);
    let ctx = contexts.ctx_mut()?;
    if !*installed {
        theme::install(ctx);
        theme::install_fonts(ctx);
        *installed = true;
    }

    let output_info = output_state
        .as_ref()
        .zip(output_config.as_ref())
        .map(|(st, cfg)| dock::OutputInfo {
            vsync: cfg.vsync,
            fullscreen: cfg.fullscreen.unwrap_or(st.is_fullscreen),
            description: st.description.clone(),
            short: st.short.clone(),
        });
    egui_focus.0 = ctx.egui_wants_keyboard_input();
    let mut out = dock::draw(
        ctx,
        &mut state,
        &doc,
        texture,
        Some(&target),
        &status,
        output_info,
        &seq,
        file_status.message.as_deref(),
    );
    if let Some(action) = out.file_request.take() {
        file_request.0 = Some(action);
    }
    frame_input.0 = out.viewport_input.take();
    dock_out.0 = Some(out);
    let input = frame_input.0;

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
        save_layout(&state.panels);
        *saved = true;
    }
}
