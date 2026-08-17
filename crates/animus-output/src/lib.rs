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

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
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
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            monitor_override: None,
            vsync: true,
        }
    }
}

/// The live output window, if one is open.
#[derive(Resource, Debug, Default)]
pub struct OutputState {
    pub window: Option<Entity>,
    pub camera: Option<Entity>,
    /// What was chosen and why, for the status bar.
    pub description: String,
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
            .add_systems(Update, (apply_vsync_changes, close_on_escape));
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

/// Open the output window on the chosen display.
///
/// `PostStartup`, so every `Startup` camera (the editor's window camera,
/// the viewport camera) exists first and `bevy_egui`'s primary-context
/// attachment cannot land here.
fn open_output_window(
    mut commands: Commands,
    config: Res<OutputConfig>,
    mut state: ResMut<OutputState>,
    monitors: Query<(Entity, &Monitor, Option<&PrimaryMonitor>)>,
) {
    if !config.enabled {
        state.description = "output disabled".into();
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

    let (window, description) = match choose_output_monitor(&described, config.monitor_override) {
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
        ),
    };

    info!("{description}");
    state.description = description;

    let window_entity = commands
        .spawn((
            window,
            OutputWindow,
            bevy::window::CursorOptions {
                // The audience must never see a cursor parked on the show.
                visible: false,
                ..default()
            },
        ))
        .id();

    let camera_entity = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: 10,
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            RenderTarget::Window(WindowRef::Entity(window_entity)),
            // Layer 0 only: the show, and none of the editor's drawing.
            RenderLayers::layer(0),
            Projection::Orthographic(OrthographicProjection {
                scale: 1.0,
                ..OrthographicProjection::default_3d()
            }),
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
        info!("output window closed by Esc");
    }
}
