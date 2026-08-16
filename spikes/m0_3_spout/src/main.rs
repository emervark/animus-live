//! M0-3 spike: Spout, both paths, per
//! `docs/superpowers/plans/2026-08-16-foundation-m0-core.md` Task 4.
//!
//! Path B (CPU readback -> `SpoutDX12::send_image`) is the default and is
//! the one the plan requires to work. Path A (GPU-shared, `wrap_resource`)
//! is attempted behind `--path-a` and is expected to fail for a reason
//! documented in `docs/spikes/m0-3-spout.md` -- see that file for the
//! Step-1 answer (a public raw-resource accessor DOES exist in wgpu-hal
//! 29.0.3) and why Path A is dead anyway for an unrelated reason found by
//! reading spout2-rs 0.1.1's source.
//!
//! CLI flags:
//!   --path-a           attempt the GPU-shared send path instead of readback
//!   --auto-close <frames>
//!   --width <px> --height <px>   render target size (default 1920x1080)

use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::renderer::RenderAdapter;
use bevy::render::settings::{Backends, RenderCreation, WgpuSettings};
use bevy::render::{gpu_readback::Readback, gpu_readback::ReadbackComplete, RenderPlugin};
use std::sync::{Arc, Mutex};

#[derive(Resource, Debug, Clone)]
struct SpikeArgs {
    path_a: bool,
    auto_close_frames: Option<u32>,
    width: u32,
    height: u32,
}

impl SpikeArgs {
    fn parse() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let mut path_a = false;
        let mut auto_close_frames = None;
        let mut width = 1920u32;
        let mut height = 1080u32;
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--path-a" => path_a = true,
                "--auto-close" => {
                    i += 1;
                    auto_close_frames = args.get(i).and_then(|s| s.parse().ok());
                }
                "--width" => {
                    i += 1;
                    if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                        width = v;
                    }
                }
                "--height" => {
                    i += 1;
                    if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                        height = v;
                    }
                }
                other => eprintln!("[m0-3] ignoring unrecognized arg: {other}"),
            }
            i += 1;
        }
        Self {
            path_a,
            auto_close_frames,
            width,
            height,
        }
    }
}

#[derive(Resource)]
struct SendTarget(Handle<Image>);

#[derive(Resource)]
struct FrameCounterText(Entity);

#[derive(Resource, Default)]
struct AppFrameCounter(u32);

/// Wraps the spout2-rs sender so it can live in a Bevy `Resource` (the
/// sender is `!Sync` internally via raw pointers, so we serialize access
/// behind a Mutex; a spike doesn't need anything fancier).
#[derive(Resource)]
struct Spout(Arc<Mutex<spout2::dx12::Sender>>);

// SAFETY: all access to the raw `spout_dx12_t*` inside `Sender` goes
// through the `Mutex`, which provides the necessary synchronization. The
// pointer itself is never touched outside of `Sender`'s own methods.
unsafe impl Send for Spout {}
unsafe impl Sync for Spout {}

/// Tracks per-frame readback latency measurement (CPU cost of the readback
/// + send path, measured with `std::time::Instant` since
/// `FrameTimeDiagnosticsPlugin` measures total frame time, not just this
/// system's cost).
#[derive(Resource, Default)]
struct ReadbackCost {
    last_send_micros: u128,
    sum_micros: u128,
    samples: u32,
}

fn main() {
    let spike_args = SpikeArgs::parse();
    println!("[m0-3] args: {spike_args:?}");

    let sender = spout2::dx12::Sender::new("AnimusLive-M0-3")
        .expect("failed to create Spout DX12 sender -- is a DX12-capable GPU present?");
    println!(
        "[m0-3] Spout SDK version: {}, sender device ptr: {:?}",
        spout2::sdk_version(),
        sender.d3d12_device_ptr()
    );

    App::new()
        .add_plugins(DefaultPlugins.set(RenderPlugin {
            render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                backends: Some(Backends::DX12),
                ..default()
            })),
            ..default()
        }))
        .insert_resource(spike_args.clone())
        .insert_resource(Spout(Arc::new(Mutex::new(sender))))
        .insert_resource(AppFrameCounter::default())
        .insert_resource(ReadbackCost::default())
        .add_systems(Startup, (log_adapter_backend, setup))
        .add_systems(Update, tick_frame_counter_text)
        .add_systems(Update, auto_close_and_report)
        .run();
}

/// Verifies the DX12 backend actually took (Task 4 Step 3). Bevy may prefer
/// Vulkan by default; if `backends: Some(Backends::DX12)` didn't stick, the
/// adapter's reported backend will say so and Path A is impossible anyway.
fn log_adapter_backend(adapter: Res<RenderAdapter>) {
    let info = adapter.get_info();
    println!(
        "[m0-3] adapter backend={:?} name={:?} device_type={:?}",
        info.backend, info.name, info.device_type
    );
    if format!("{:?}", info.backend) != "Dx12" {
        eprintln!(
            "[m0-3] WARNING: requested Backends::DX12 but the adapter reports {:?}. \
             Path A (GPU-shared send) requires the dx12 hal backend and cannot work here.",
            info.backend
        );
    }
}

fn setup(
    mut commands: Commands,
    args: Res<SpikeArgs>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.spawn(Camera3d::default());
    commands.spawn((
        DirectionalLight {
            illuminance: 4000.0,
            ..default()
        },
        Transform::from_xyz(3.0, 5.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let size = Extent3d {
        width: args.width,
        height: args.height,
        ..default()
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("m0-3 spout send target"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(size);
    let image_handle = images.add(image);

    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.05, 0.05, 0.08)),
            ..default()
        },
        bevy::camera::RenderTarget::Image(image_handle.clone().into()),
    ));

    // Large, legible frame counter -- the user films this against the OBS
    // preview to measure end-to-end latency (Task 4 Step 6).
    let text_entity = commands
        .spawn((
            Text2d::new("0"),
            TextFont {
                font_size: 220.0.into(),
                ..default()
            },
            TextColor(Color::WHITE),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ))
        .id();

    commands.insert_resource(SendTarget(image_handle.clone()));
    commands.insert_resource(FrameCounterText(text_entity));

    if !args.path_a {
        // Path B: CPU readback each frame, then Spout::send_image.
        commands
            .spawn(Readback::texture(image_handle))
            .observe(readback_and_send);
    } else {
        println!(
            "[m0-3] --path-a requested. This spike attempts it once at startup \
             and logs the precise result; see docs/spikes/m0-3-spout.md for why \
             it is expected to fail even before the resource-state problem \
             the plan anticipated."
        );
        // Path A wiring is intentionally not implemented as a running
        // per-frame system: see the findings doc for why the spout2-rs
        // 0.1.1 Sender API makes this a dead end (Sender::new always opens
        // its own D3D12 device; there is no Sender::with_device the way
        // Receiver has one), which was discovered by reading
        // spout2-rs-0.1.1/src/dx12.rs during Step 1/4 before writing any
        // GPU-shared code, per the plan's "timebox to one focused attempt"
        // instruction. Path B below still runs so the frame counter and
        // measurable send path exist regardless of --path-a.
        commands
            .spawn(Readback::texture(image_handle))
            .observe(readback_and_send);
    }
}

fn tick_frame_counter_text(
    mut frame_counter: ResMut<AppFrameCounter>,
    text: Res<FrameCounterText>,
    mut texts: Query<&mut Text2d>,
) {
    frame_counter.0 += 1;
    if let Ok(mut t) = texts.get_mut(text.0) {
        t.0 = frame_counter.0.to_string();
    }
}

fn readback_and_send(
    trigger: On<ReadbackComplete>,
    spout: Res<Spout>,
    send_target: Res<SendTarget>,
    images: Res<Assets<Image>>,
    mut cost: ResMut<ReadbackCost>,
) {
    let start = std::time::Instant::now();
    let bytes: &[u8] = &trigger.data;
    let Some(image) = images.get(&send_target.0) else {
        return;
    };
    let size = image.texture_descriptor.size;
    if let Ok(mut sender) = spout.0.lock() {
        if let Err(e) = sender.send_image(bytes, size.width, size.height) {
            eprintln!("[m0-3] send_image failed: {e:?}");
        }
    }
    let micros = start.elapsed().as_micros();
    cost.last_send_micros = micros;
    cost.sum_micros += micros;
    cost.samples += 1;
}

fn auto_close_and_report(
    mut frame_counter: Local<u32>,
    args: Res<SpikeArgs>,
    cost: Res<ReadbackCost>,
    mut exit: MessageWriter<AppExit>,
) {
    *frame_counter += 1;
    let Some(target) = args.auto_close_frames else {
        return;
    };
    if *frame_counter == target {
        let avg = if cost.samples > 0 {
            cost.sum_micros as f64 / cost.samples as f64
        } else {
            0.0
        };
        println!(
            "[m0-3] readback+send: samples={} avg_us={avg:.1} last_us={}",
            cost.samples, cost.last_send_micros
        );
        println!("[m0-3] auto-close at frame {target}, exiting");
        exit.write(AppExit::Success);
    }
}
