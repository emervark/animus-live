//! M0-4 spike: second window (editor + projector output), `RenderLayers`
//! isolation, and vsync coupling, per
//! `docs/superpowers/plans/2026-08-16-foundation-m0-core.md` Task 5.
//!
//! CLI flags:
//!   --editor-vsync on|off     PresentMode for the editor (primary) window
//!   --output-vsync on|off     PresentMode for the output (second) window
//!   --auto-close <frames>     close after N frames and print per-window
//!                             frame-time diagnostics (this spike keeps its
//!                             own timers per window since Bevy's built-in
//!                             FrameTimeDiagnosticsPlugin is global, not
//!                             per-window)
//!
//! Behaviour:
//!   - Enumerates monitors at startup and prints them.
//!   - If a second monitor is present, the output window opens
//!     BorderlessFullscreen on it. If only one monitor is present, the
//!     output window opens windowed-borderless at a fixed offset position
//!     instead, and this is logged — re-run with a projector attached to
//!     exercise the real path.
//!   - Content (a rotating cube) is on RenderLayers 0, visible in both
//!     windows. A wireframe gizmo box is on RenderLayers 1, visible only
//!     in the editor window.
//!   - Esc despawns the output window (and its camera) while it has focus.

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::color::palettes::basic::{LIME, YELLOW};
use bevy::prelude::*;
use bevy::window::{Monitor, PresentMode, PrimaryMonitor, WindowMode, WindowRef};

#[derive(Resource, Debug, Clone)]
struct SpikeArgs {
    editor_vsync: PresentMode,
    output_vsync: PresentMode,
    auto_close_frames: Option<u32>,
    /// Force the output window onto the monitor at this enumeration index,
    /// overriding the not-the-primary-monitor rule. The escape hatch for a
    /// venue where the projector reports as primary, or where the automatic
    /// choice is wrong for any other reason.
    output_monitor: Option<usize>,
}

fn parse_present_mode(s: &str) -> PresentMode {
    match s {
        "off" => PresentMode::AutoNoVsync,
        _ => PresentMode::AutoVsync,
    }
}

impl SpikeArgs {
    fn parse() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let mut editor_vsync = PresentMode::AutoVsync;
        let mut output_vsync = PresentMode::AutoVsync;
        let mut auto_close_frames = None;
        let mut output_monitor = None;
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--editor-vsync" => {
                    i += 1;
                    if let Some(v) = args.get(i) {
                        editor_vsync = parse_present_mode(v);
                    }
                }
                "--output-vsync" => {
                    i += 1;
                    if let Some(v) = args.get(i) {
                        output_vsync = parse_present_mode(v);
                    }
                }
                "--auto-close" => {
                    i += 1;
                    auto_close_frames = args.get(i).and_then(|s| s.parse().ok());
                }
                "--output-monitor" => {
                    i += 1;
                    output_monitor = args.get(i).and_then(|s| s.parse().ok());
                }
                other => eprintln!("[m0-4] ignoring unrecognized arg: {other}"),
            }
            i += 1;
        }
        Self {
            editor_vsync,
            output_vsync,
            auto_close_frames,
            output_monitor,
        }
    }
}

#[derive(Component)]
struct ContentCube;

#[derive(Component)]
struct OutputWindowMarker;
#[derive(Component)]
struct OutputCameraMarker;

/// Tracks frame count and last-frame wall time separately per window entity
/// so the vsync-coupling table can be filled in even though
/// `FrameTimeDiagnosticsPlugin` is app-global, not per-window.
#[derive(Component, Default)]
struct WindowFrameStats {
    frames: u32,
    last_instant: Option<std::time::Instant>,
    min_dt_ms: f64,
    max_dt_ms: f64,
    sum_dt_ms: f64,
}

#[derive(Resource, Default)]
struct FrameCounter(u32);

fn main() {
    let spike_args = SpikeArgs::parse();
    println!("[m0-4] args: {spike_args:?}");

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "m0-4 EDITOR".into(),
                resolution: (960u32, 600u32).into(),
                present_mode: spike_args.editor_vsync,
                ..default()
            }),
            ..default()
        }))
        .insert_resource(spike_args)
        .insert_resource(FrameCounter::default())
        .add_systems(Startup, (list_monitors, setup_scene_assets))
        .add_systems(Startup, spawn_output_window.after(setup_scene_assets))
        .add_systems(Update, rotate_cube)
        .add_systems(Update, draw_gizmo_box)
        .add_systems(Update, close_output_on_esc)
        .add_systems(Update, track_window_frame_times)
        .add_systems(Update, auto_close_and_report)
        .run();
}

fn list_monitors(monitors: Query<(Entity, &Monitor)>) {
    println!("[m0-4] enumerating monitors:");
    let mut count = 0;
    for (entity, m) in &monitors {
        count += 1;
        let name = m.name.clone().unwrap_or_else(|| "<no name>".into());
        let hz = m
            .refresh_rate_millihertz
            .map(|x| format!("{:.1}Hz", x as f32 / 1000.0))
            .unwrap_or_else(|| "<unknown>".into());
        println!(
            "[m0-4]   monitor {entity:?}: name={name:?} pos=({}, {}) size={}x{}px refresh={hz} scale={:.2}",
            m.physical_position.x,
            m.physical_position.y,
            m.physical_width,
            m.physical_height,
            m.scale_factor,
        );
    }
    println!("[m0-4] total monitors: {count}");
}

fn setup_scene_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.5, 1.5, 1.5))),
        MeshMaterial3d(materials.add(Color::from(YELLOW))),
        Transform::default(),
        ContentCube,
        RenderLayers::layer(0),
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 5000.0,
            ..default()
        },
        Transform::from_xyz(3.0, 5.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Editor (primary window) camera: sees layer 0 (content) and layer 1
    // (gizmo overlay).
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            ..default()
        },
        Transform::from_xyz(4.0, 3.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::from_layers(&[0, 1]),
        WindowFrameStats::default(),
    ));
}

fn spawn_output_window(
    mut commands: Commands,
    monitors: Query<(Entity, &Monitor, Option<&PrimaryMonitor>)>,
    mut gizmo_store: ResMut<GizmoConfigStore>,
    args: Res<SpikeArgs>,
) {
    let (gizmo_config, _) = gizmo_store.config_mut::<DefaultGizmoConfigGroup>();
    gizmo_config.render_layers = RenderLayers::layer(1);

    let monitor_count = monitors.iter().count();

    // Choosing "the second monitor enumerated" is wrong, and wrong in the
    // worst way: it works on the machine you wrote it on and puts the output
    // on the operator's screen somewhere else. Enumeration order is not
    // specified and does not put the primary first -- on the machine this was
    // caught on, `\.\DISPLAY2` (the TV) enumerated first and DISPLAY1 (the
    // laptop panel) second, so `nth(1)` selected the laptop and covered the
    // editor with a fullscreen output window that cannot be dragged away.
    //
    // Select by the property that is actually meant: the monitor that is not
    // the primary one. `--output-monitor <index>` overrides it, because at a
    // venue the display that reports as primary is not always the one the
    // projector is on.
    let chosen = if let Some(index) = args.output_monitor {
        monitors.iter().nth(index).map(|(e, m, _)| (e, m))
    } else {
        monitors
            .iter()
            .find(|(_, _, primary)| primary.is_none())
            .map(|(e, m, _)| (e, m))
    };

    let (window, note) = if let Some((monitor_entity, monitor)) = chosen {
        let chosen_name = monitor.name.clone().unwrap_or_else(|| "<unnamed>".into());
        let chosen_desc = format!(
            "{chosen_name} {}x{}px @ {:.1}Hz scale={:.2}",
            monitor.physical_width,
            monitor.physical_height,
            monitor.refresh_rate_millihertz.unwrap_or(0) as f32 / 1000.0,
            monitor.scale_factor,
        );
        (
            Window {
                title: "m0-4 OUTPUT".into(),
                mode: WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Entity(
                    monitor_entity,
                )),
                decorations: false,
                present_mode: args.output_vsync,
                ..default()
            },
            format!("opening BorderlessFullscreen on {chosen_desc}"),
        )
    } else {
        (
            Window {
                title: "m0-4 OUTPUT (fallback: single-monitor windowed-borderless)".into(),
                mode: WindowMode::Windowed,
                decorations: false,
                position: bevy::window::WindowPosition::At(IVec2::new(80, 80)),
                resolution: (960u32, 540u32).into(),
                present_mode: args.output_vsync,
                ..default()
            },
            "only one monitor present: opening windowed-borderless at (80,80) 960x540 instead. \
             Re-run with a projector/second display attached to exercise the real \
             BorderlessFullscreen(MonitorSelection::Entity) path."
                .to_string(),
        )
    };
    println!("[m0-4] monitors detected: {monitor_count}. {note}");

    let output_window = commands
        .spawn((
            window,
            OutputWindowMarker,
            bevy::window::CursorOptions {
                visible: false,
                ..default()
            },
        ))
        .id();

    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 10,
            ..default()
        },
        Transform::from_xyz(4.0, 3.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderTarget::Window(WindowRef::Entity(output_window)),
        // Output camera sees ONLY layer 0 (content), never the layer-1 gizmo.
        RenderLayers::layer(0),
        OutputCameraMarker,
        WindowFrameStats::default(),
    ));
}

fn rotate_cube(time: Res<Time>, mut query: Query<&mut Transform, With<ContentCube>>) {
    for mut t in &mut query {
        t.rotate_y(time.delta_secs());
    }
}

/// The layer-1-only wireframe gizmo box: visible in the editor, absent
/// from the output. Drawn every frame; `GizmoConfig::render_layers` (set
/// to layer 1 in `spawn_output_window`) is what confines it.
fn draw_gizmo_box(mut gizmos: Gizmos) {
    gizmos.cube(Transform::from_scale(Vec3::splat(2.0)), LIME);
}

fn close_output_on_esc(
    mut commands: Commands,
    windows: Query<(Entity, &Window), With<OutputWindowMarker>>,
    output_cameras: Query<Entity, With<OutputCameraMarker>>,
    input: Res<ButtonInput<KeyCode>>,
) {
    if !input.just_pressed(KeyCode::Escape) {
        return;
    }
    for (window_entity, window) in &windows {
        if !window.focused {
            continue;
        }
        println!("[m0-4] Esc pressed with output window focused; despawning it.");
        commands.entity(window_entity).despawn();
        for cam in &output_cameras {
            commands.entity(cam).despawn();
        }
    }
}

fn track_window_frame_times(time: Res<Time>, mut query: Query<&mut WindowFrameStats>) {
    let now = std::time::Instant::now();
    for mut stats in &mut query {
        if let Some(last) = stats.last_instant {
            let dt_ms = (now - last).as_secs_f64() * 1000.0;
            stats.frames += 1;
            stats.sum_dt_ms += dt_ms;
            if stats.min_dt_ms == 0.0 || dt_ms < stats.min_dt_ms {
                stats.min_dt_ms = dt_ms;
            }
            if dt_ms > stats.max_dt_ms {
                stats.max_dt_ms = dt_ms;
            }
        }
        stats.last_instant = Some(now);
    }
    let _ = time;
}

fn auto_close_and_report(
    mut frame_counter: ResMut<FrameCounter>,
    args: Res<SpikeArgs>,
    stats: Query<&WindowFrameStats>,
    mut exit: MessageWriter<AppExit>,
) {
    frame_counter.0 += 1;
    let Some(target) = args.auto_close_frames else {
        return;
    };
    if frame_counter.0 == target {
        for s in &stats {
            let avg = if s.frames > 0 {
                s.sum_dt_ms / s.frames as f64
            } else {
                0.0
            };
            let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };
            println!(
                "[m0-4] window camera stats: frames={} avg_dt_ms={avg:.3} min_dt_ms={:.3} max_dt_ms={:.3} approx_fps={fps:.1}",
                s.frames, s.min_dt_ms, s.max_dt_ms,
            );
        }
        println!("[m0-4] auto-close at frame {target}, exiting");
        exit.write(AppExit::Success);
    }
}
