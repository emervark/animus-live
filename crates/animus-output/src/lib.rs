//! The output window: the projector's half of the application.
//!
//! Everything the audience sees goes through here, and nothing the editor
//! draws for itself does: the output camera carries `RenderLayers::layer(0)`
//! only, while gizmos and helpers live on layer 1. M0-4 verified the
//! isolation by eye — the wireframe cube was visible in the editor and
//! absent from the TV.
//!
//! ## Vsync is the operator's switch
//!
//! Measured in M0-4 and folded into spec §11.3: **the application's frame
//! rate is set by the slowest vsync-enabled window.** An output window
//! synced to a 30Hz display clamps the whole app — editor included — to
//! ~29fps, and the editor's own present mode changes nothing. Turning the
//! output's vsync off restores 138–185fps at the cost of slight tearing on
//! the projector. Neither answer is right for every venue, operators know
//! this trade, so it is a toggle, not a policy.

#![forbid(unsafe_code)]

pub mod monitor;

use animus_core::doc::StageConfig;
use animus_runtime::{DocumentRes, RenderScale};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::prelude::*;
use bevy::window::{Monitor, PresentMode, PrimaryMonitor, Window, WindowMode, WindowRef};

pub use monitor::{MonitorDesc, choose_output_monitor};

/// The operator's output settings.
#[derive(Resource, Debug, Clone)]
pub struct OutputConfig {
    /// Open the output window at startup.
    pub enabled: bool,
    /// Explicit monitor index (enumeration order, shown in the log). At a
    /// venue the display that reports as primary is not always the console.
    pub monitor_override: Option<usize>,
    /// Vsync on the output window. See the module doc; default on, because
    /// a clean projector image is the safer default and the cost is editor
    /// frame rate, not show correctness.
    pub vsync: bool,
    /// Fullscreen on the chosen display.
    ///
    /// `None` means "decide from the hardware", which is the startup
    /// behaviour: fullscreen when there is a second display to go fullscreen
    /// *on*, a draggable window when there is only the console. Once the
    /// operator says either way, that answer sticks — a venue where the
    /// projector appears halfway through setup should not silently change
    /// what they chose.
    pub fullscreen: Option<bool>,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            monitor_override: None,
            vsync: true,
            fullscreen: None,
        }
    }
}

/// The live output window, if one is open.
#[derive(Resource, Debug, Default)]
pub struct OutputState {
    pub window: Option<Entity>,
    pub camera: Option<Entity>,
    /// What was chosen and why, in a full sentence. Belongs in a status bar
    /// or a tooltip, where there is room to read it.
    pub description: String,
    /// Whether the window went fullscreen when it opened. The panel needs a
    /// starting answer for a config that has not been told one yet.
    pub is_fullscreen: bool,
    /// The same fact in two or three words, for a chip.
    ///
    /// Separate from [`Self::description`] because a chip is not a sentence:
    /// the windowed-fallback explanation is 90 characters of good advice, and
    /// putting it in the title bar pushed everything else off the row.
    pub short: String,
}

/// Marks the output window entity.
#[derive(Component)]
pub struct OutputWindow;

/// Marks the output camera.
#[derive(Component)]
pub struct OutputCamera;

pub struct OutputPlugin;

impl Plugin for OutputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OutputConfig>()
            .init_resource::<OutputState>()
            .add_systems(PostStartup, open_output_window)
            .add_systems(
                Update,
                (
                    apply_vsync_changes,
                    apply_fullscreen_changes,
                    follow_stage,
                    close_on_escape,
                ),
            );
    }
}

fn describe(monitors: &Query<(Entity, &Monitor, Option<&PrimaryMonitor>)>) -> Vec<MonitorDesc> {
    monitors
        .iter()
        .map(|(entity, m, primary)| MonitorDesc {
            entity,
            name: m.name.clone().unwrap_or_else(|| "<unnamed>".into()),
            physical_size: UVec2::new(m.physical_width, m.physical_height),
            refresh_hz: m.refresh_rate_millihertz.unwrap_or(0) as f32 / 1000.0,
            scale: m.scale_factor,
            is_primary: primary.is_some(),
        })
        .collect()
}

/// The projector's framing: the whole stage canvas, letterboxed.
///
/// `ScalingMode::AutoMin` keeps every part of the canvas on screen whatever
/// the projector's aspect ratio turns out to be at the venue — the one thing
/// that must not depend on which display got plugged in. It also replaces
/// the default `scale: 1.0`, which under `WindowSize` means one world unit
/// per pixel: with `ppu` at 100 that rendered a 2160px puppet as a 21px
/// speck in the middle of a black projector.
fn stage_projection(stage: &StageConfig, ppu: f32) -> OrthographicProjection {
    let ppu = if ppu > 0.0 { ppu } else { 100.0 };
    OrthographicProjection {
        scaling_mode: ScalingMode::AutoMin {
            min_width: stage.canvas[0].max(1) as f32 / ppu,
            min_height: stage.canvas[1].max(1) as f32 / ppu,
        },
        ..OrthographicProjection::default_3d()
    }
}

fn stage_clear(stage: &StageConfig) -> Color {
    let [r, g, b, a] = stage.background;
    Color::srgba(r, g, b, a)
}

/// Open the output window on the chosen display.
///
/// `PostStartup`, so every `Startup` camera exists first. That ordering is
/// not what keeps `bevy_egui`'s primary context off this window's camera,
/// though it once read as if it were: the editor names its own camera the
/// primary context outright, because `bevy_egui`'s automatic choice went to
/// whichever camera its query yielded first and that turned out to be this
/// one — the editor drew onto the projector.
fn open_output_window(
    mut commands: Commands,
    config: Res<OutputConfig>,
    mut state: ResMut<OutputState>,
    doc: Res<DocumentRes>,
    scale: Res<RenderScale>,
    monitors: Query<(Entity, &Monitor, Option<&PrimaryMonitor>)>,
) {
    if !config.enabled {
        state.description = "output disabled".into();
        state.short = "OUTPUT · OFF".into();
        return;
    }

    let described = describe(&monitors);
    for m in &described {
        info!("monitor: {}", m.describe());
    }

    let present_mode = if config.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };

    // Hidden on the projector, kept on the stand-in. The audience must never
    // see a cursor parked on the show; but with one display the output is a
    // window on the operator's own screen, and hiding the cursor there means
    // it vanishes whenever it crosses that window — including on the way to
    // the editor behind it.
    let (window, description, short, cursor_visible) =
        match choose_output_monitor(&described, config.monitor_override) {
            Some(chosen) => (
                Window {
                    title: "Animus Live — output".into(),
                    mode: WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Entity(
                        chosen.entity,
                    )),
                    decorations: false,
                    present_mode,
                    ..default()
                },
                format!("output on {}", chosen.describe()),
                format!("OUTPUT · {}", chosen.short_name().to_uppercase()),
                false,
            ),
            None => (
                // One monitor: a windowed stand-in rather than burying the
                // editor. Deliberately decorated so it can be dragged to a
                // display that arrives later.
                Window {
                    title: "Animus Live — output (windowed: one display)".into(),
                    mode: WindowMode::Windowed,
                    resolution: (960u32, 540u32).into(),
                    position: bevy::window::WindowPosition::At(IVec2::new(120, 120)),
                    present_mode,
                    ..default()
                },
                "one display attached: output is windowed. Connect the projector and restart, \
                 or drag this window to it"
                    .to_string(),
                "OUTPUT · WINDOWED".to_string(),
                true,
            ),
        };

    info!("{description}");
    state.is_fullscreen = !matches!(window.mode, WindowMode::Windowed);
    state.description = description;
    state.short = short;

    let window_entity = commands
        .spawn((
            window,
            OutputWindow,
            bevy::window::CursorOptions {
                visible: cursor_visible,
                ..default()
            },
        ))
        .id();

    let camera_entity = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: 10,
                clear_color: ClearColorConfig::Custom(stage_clear(&doc.0.stage)),
                ..default()
            },
            RenderTarget::Window(WindowRef::Entity(window_entity)),
            // Layer 0 only: the show, and none of the editor's drawing.
            RenderLayers::layer(0),
            Projection::Orthographic(stage_projection(&doc.0.stage, scale.ppu)),
            Transform::from_xyz(0.0, 0.0, 100.0).looking_at(Vec3::ZERO, Vec3::Y),
            OutputCamera,
        ))
        .id();

    state.window = Some(window_entity);
    state.camera = Some(camera_entity);
}

/// Apply a vsync toggle to the live window.
///
/// Changing `PresentMode` on an open window is how the operator trades
/// editor frame rate against projector tearing mid-session, without
/// restarting the show.
fn apply_vsync_changes(
    config: Res<OutputConfig>,
    state: Res<OutputState>,
    mut windows: Query<&mut Window, With<OutputWindow>>,
) {
    if !config.is_changed() {
        return;
    }
    let Some(entity) = state.window else { return };
    let Ok(mut window) = windows.get_mut(entity) else {
        return;
    };
    let wanted = if config.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };
    if window.present_mode != wanted {
        window.present_mode = wanted;
        info!("output vsync {}", if config.vsync { "on" } else { "off" });
    }
}

/// Put the output window fullscreen, or take it back out.
///
/// Applied live rather than only at startup, because the moment an operator
/// most wants this is after the projector has appeared — which at a venue is
/// usually after the app is already open.
///
/// Fullscreen goes to the display the window is currently on
/// (`MonitorSelection::Current`), not to a remembered index: the operator has
/// just dragged it where they want it, and that gesture *is* the choice.
fn apply_fullscreen_changes(
    config: Res<OutputConfig>,
    state: Res<OutputState>,
    mut windows: Query<&mut Window, With<OutputWindow>>,
) {
    if !config.is_changed() {
        return;
    }
    let Some(wanted) = config.fullscreen else {
        return;
    };
    let Some(entity) = state.window else { return };
    let Ok(mut window) = windows.get_mut(entity) else {
        return;
    };

    let is_full = matches!(
        window.mode,
        WindowMode::BorderlessFullscreen(_) | WindowMode::Fullscreen(_, _)
    );
    if is_full == wanted {
        return;
    }
    window.mode = if wanted {
        WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };
    // Decorations come back with the window: a bare rectangle with no title
    // bar cannot be dragged to another display, which is the whole reason to
    // leave fullscreen in the first place.
    window.decorations = !wanted;
    info!("output {}", if wanted { "fullscreen" } else { "windowed" });
}

/// Keep the projector framed on the stage the document describes.
///
/// The canvas and background are document data, so they can change while the
/// show is open. Re-deriving both from `DocumentRes` costs a comparison per
/// frame and removes the class of bug where the projector keeps yesterday's
/// framing because nothing thought to tell it.
fn follow_stage(
    doc: Res<DocumentRes>,
    scale: Res<RenderScale>,
    state: Res<OutputState>,
    mut cameras: Query<(&mut Camera, &mut Projection), With<OutputCamera>>,
) {
    if !doc.is_changed() && !scale.is_changed() {
        return;
    }
    let Some(entity) = state.camera else { return };
    let Ok((mut camera, mut projection)) = cameras.get_mut(entity) else {
        return;
    };

    // Assigned outright rather than compared first: neither `ScalingMode` nor
    // `ClearColorConfig` is `PartialEq`, and the change detection above is
    // already the gate — this runs on the frames the document moved, not on
    // every frame of the show.
    camera.clear_color = ClearColorConfig::Custom(stage_clear(&doc.0.stage));
    *projection = Projection::Orthographic(stage_projection(&doc.0.stage, scale.ppu));
}

/// Esc on the focused output window closes it and its camera, cleanly.
///
/// Verified in M0-4: the window is undecorated and fullscreen, so a failed
/// despawn strands a black rectangle on the projector with no way to close
/// it by mouse. The camera must go in the same frame or it renders to a
/// dead target.
fn close_on_escape(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<OutputState>,
    windows: Query<(Entity, &Window), With<OutputWindow>>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    for (entity, window) in &windows {
        if !window.focused {
            continue;
        }
        commands.entity(entity).despawn();
        if let Some(camera) = state.camera.take() {
            commands.entity(camera).despawn();
        }
        state.window = None;
        state.description = "output closed (Esc)".into();
        state.short = "OUTPUT · CLOSED".into();
        info!("output window closed by Esc");
    }
}
