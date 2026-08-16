//! M0-1 spike: procedural skinned mesh in Bevy 0.19.1.
//!
//! Answers, per `docs/superpowers/plans/2026-08-16-foundation-m0-core.md`
//! Task 2:
//!   - Do flat (non-hierarchical) joint sets skin correctly?
//!   - Which of position/UV flips in Y (spec 7.1)?
//!   - Is `with_generated_skinned_mesh_bounds` + `DynamicSkinnedMeshBounds`
//!     actually required to avoid wrong frustum culling?
//!   - What happens at the 256/257 joint ceiling?
//!   - What is the per-frame cost of 50 puppets x 40 joints?
//!
//! CLI flags (all optional):
//!   --auto-close <frames>   close after N frames and print diagnostics
//!   --stress                spawn 50 puppets instead of 1
//!   --joints <N>            joints per puppet (default 40); used for the
//!                           256/257 ceiling test
//!   --no-bounds-fix         omit with_generated_skinned_mesh_bounds() and
//!                           DynamicSkinnedMeshBounds, to test culling
//!   --no-animate            disable the joint-driving system, to isolate
//!                           render cost from CPU animation cost
//!   --edge-sweep            pitch the camera so the mesh crosses the top and
//!                           bottom frustum edges. Required for the
//!                           --no-bounds-fix comparison to mean anything: the
//!                           plain orbit keeps the mesh centred, where nothing
//!                           is ever culled regardless of its bounds.
//!   --amplitude <N>         wave amplitude in world units (default 0.3). The
//!                           mesh is 4x4 units, so 0.3 pushes it only ~7%
//!                           past its rest-pose AABB.
//!
//! The culling question is answered numerically, not by eye: the CULLING line
//! printed at --auto-close reports the share of frames in which the mesh was
//! discarded by frustum culling (via ViewVisibility).

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::DynamicSkinnedMeshBounds;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::light::GlobalAmbientLight;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

const GRID: u32 = 30; // 30x30 vertices = 900 vertices, 29*29*2 = 1682 triangles
const PPU: f32 = 32.0; // pixels per world unit
const TEX_W: f32 = 128.0;
const TEX_H: f32 = 128.0;

#[derive(Resource, Debug, Clone)]
struct SpikeArgs {
    auto_close_frames: Option<u32>,
    stress: bool,
    joints: u32,
    bounds_fix: bool,
    animate: bool,
    /// Sweep the camera so the mesh crosses the *edge* of the frustum
    /// instead of sitting permanently in its centre. Without this, the
    /// `--no-bounds-fix` comparison cannot fail: frustum culling only
    /// discards what leaves the frustum, and a centred object never does,
    /// however wrong its bounding box is.
    edge_sweep: bool,
    /// Wave amplitude in world units. The mesh is 4x4 units, so the default
    /// 0.3 pushes the deformed surface only ~7% outside its rest-pose AABB
    /// -- too little to see at the frustum edge. Raise it to make the
    /// stale-bounds error a wide, unmissable band.
    amplitude: f32,
}

impl SpikeArgs {
    fn parse() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let mut auto_close_frames = None;
        let mut stress = false;
        let mut joints = 40u32;
        let mut bounds_fix = true;
        let mut animate = true;
        let mut edge_sweep = false;
        let mut amplitude = 0.3f32;
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--auto-close" => {
                    i += 1;
                    auto_close_frames = args.get(i).and_then(|s| s.parse().ok());
                }
                "--stress" => stress = true,
                "--joints" => {
                    i += 1;
                    if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                        joints = v;
                    }
                }
                "--no-bounds-fix" => bounds_fix = false,
                "--no-animate" => animate = false,
                "--edge-sweep" => edge_sweep = true,
                "--amplitude" => {
                    i += 1;
                    if let Some(v) = args.get(i).and_then(|s| s.parse().ok()) {
                        amplitude = v;
                    }
                }
                other => eprintln!("[m0-1] ignoring unrecognized arg: {other}"),
            }
            i += 1;
        }
        Self {
            auto_close_frames,
            stress,
            joints,
            bounds_fix,
            animate,
            edge_sweep,
            amplitude,
        }
    }
}

#[derive(Component)]
struct AnimatedJoint {
    index: u32,
    rest_x: f32,
    /// Per-puppet offset into the sine wave. Without this every puppet in
    /// `--stress` moves in perfect lockstep, which both looks wrong and
    /// invalidates the stress test: 50 identical poses are a far friendlier
    /// load than 50 different ones.
    phase: f32,
    /// Per-puppet temporal frequency. Phase alone is not enough to make the
    /// puppets *look* independent: shifting the phase slides the identical
    /// waveform sideways along the puppet, so 50 phase-shifted puppets read
    /// to the eye as 50 copies of one motion. Differing speeds make them
    /// drift apart continuously, which is what the checklist item "all
    /// animate independently" is actually asking a human to confirm.
    speed: f32,
}

#[derive(Component)]
struct PuppetMarker;

#[derive(Resource, Default)]
struct FrameCounter(u32);

fn main() {
    let spike_args = SpikeArgs::parse();
    println!("[m0-1] args: {spike_args:?}");

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "m0-1-skinned-mesh".into(),
            resolution: (1280u32, 800u32).into(),
            // Uncapped so the auto-close diagnostics reflect actual CPU/GPU
            // cost rather than being clamped to the monitor's refresh rate.
            present_mode: bevy::window::PresentMode::AutoNoVsync,
            ..default()
        }),
        ..default()
    }))
    .add_plugins(FrameTimeDiagnosticsPlugin::default())
    .insert_resource(spike_args)
    .insert_resource(FrameCounter::default())
    .insert_resource(GlobalAmbientLight {
        brightness: 1500.0,
        ..default()
    })
    .add_systems(Startup, setup)
    .add_systems(Update, orbit_camera)
    .add_systems(Update, joint_animation.run_if(should_animate))
    .insert_resource(CullStats::default())
    .add_systems(Update, track_culling)
    .add_systems(Update, auto_close_and_report);

    app.run();
}

fn should_animate(args: Res<SpikeArgs>) -> bool {
    args.animate
}

fn build_puppet_mesh(joint_count: u32) -> Mesh {
    let mut positions = Vec::with_capacity((GRID * GRID) as usize);
    let mut uvs = Vec::with_capacity((GRID * GRID) as usize);
    let mut joint_indices = Vec::with_capacity((GRID * GRID) as usize);
    let mut joint_weights = Vec::with_capacity((GRID * GRID) as usize);
    let mut normals = Vec::with_capacity((GRID * GRID) as usize);

    for row in 0..GRID {
        for col in 0..GRID {
            // Pixel space: origin top-left, Y down.
            let x_px = col as f32 / (GRID - 1) as f32 * TEX_W;
            let y_px = row as f32 / (GRID - 1) as f32 * TEX_H;

            // World space: origin center, Y up. Position flips Y; UV does not.
            positions.push([x_px / PPU, -y_px / PPU, 0.0]);
            uvs.push([x_px / TEX_W, y_px / TEX_H]);
            normals.push([0.0, 0.0, 1.0]);

            // Bind each vertex 100% to the joint nearest its column. This is
            // not a realistic weight paint, just enough to make the grid
            // visibly wave per-joint and to exercise `ATTRIBUTE_JOINT_INDEX`
            // / `ATTRIBUTE_JOINT_WEIGHT` end to end.
            let frac = col as f32 / (GRID - 1) as f32;
            let joint_index = (frac * (joint_count - 1) as f32).round() as u16;
            joint_indices.push([joint_index, 0u16, 0u16, 0u16]);
            joint_weights.push([1.0f32, 0.0, 0.0, 0.0]);
        }
    }

    let mut indices = Vec::with_capacity(((GRID - 1) * (GRID - 1) * 6) as usize);
    for row in 0..(GRID - 1) {
        for col in 0..(GRID - 1) {
            let i0 = row * GRID + col;
            let i1 = row * GRID + col + 1;
            let i2 = (row + 1) * GRID + col;
            let i3 = (row + 1) * GRID + col + 1;
            indices.extend_from_slice(&[i0, i1, i3, i0, i3, i2]);
        }
    }

    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(joint_indices),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, joint_weights)
    .with_inserted_indices(Indices::U32(indices));

    println!(
        "[m0-1] built mesh: {} vertices, {} triangles, {joint_count} joints",
        GRID * GRID,
        (GRID - 1) * (GRID - 1) * 2
    );

    mesh
}

fn spawn_puppet(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    inverse_bindposes_assets: &mut Assets<SkinnedMeshInverseBindposes>,
    asset_server: &AssetServer,
    origin: Vec3,
    joint_count: u32,
    bounds_fix: bool,
    phase: f32,
) {
    let mesh = build_puppet_mesh(joint_count);
    let mesh = if bounds_fix {
        match mesh.clone().with_generated_skinned_mesh_bounds() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[m0-1] with_generated_skinned_mesh_bounds failed: {e:?}");
                mesh
            }
        }
    } else {
        mesh
    };
    let mesh_handle = meshes.add(mesh);

    let root = commands
        .spawn((
            PuppetMarker,
            Transform::from_translation(origin),
            Visibility::default(),
        ))
        .id();

    let mut joint_entities = Vec::with_capacity(joint_count as usize);
    let mut bindposes = Vec::with_capacity(joint_count as usize);
    for j in 0..joint_count {
        // Rest position spans the same X range as the mesh grid, at the
        // mesh's local top edge (world Y = 0 in mesh-local space).
        let rest_x = j as f32 / (joint_count - 1).max(1) as f32 * (TEX_W / PPU);
        let rest = Vec3::new(rest_x, 0.0, 0.0);
        bindposes.push(Mat4::from_translation(-rest));
        let joint = commands
            .spawn((
                AnimatedJoint {
                    index: j,
                    rest_x,
                    phase,
                    // Deterministic per-puppet speed in 1.6..2.4, derived
                    // from the phase seed so no extra argument is needed.
                    speed: 1.6 + (phase % 1.0) * 0.8,
                },
                Transform::from_translation(rest),
                ChildOf(root),
            ))
            .id();
        joint_entities.push(joint);
    }

    let inverse_bindposes = inverse_bindposes_assets.add(bindposes);

    let mut mesh_entity = commands.spawn((
        Mesh3d(mesh_handle),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(asset_server.load("top_marker.png")),
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        })),
        SkinnedMesh {
            inverse_bindposes,
            joints: joint_entities,
        },
        ChildOf(root),
        Transform::default(),
    ));
    if bounds_fix {
        mesh_entity.insert(DynamicSkinnedMeshBounds);
    }

    // `phase` is carried on each AnimatedJoint above; `joint_animation` adds
    // it to the sine argument so the puppets do not animate in lockstep.
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut inverse_bindposes_assets: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    args: Res<SpikeArgs>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(2.0, 1.5, 12.0).looking_at(Vec3::new(2.0, -1.5, 0.0), Vec3::Y),
    ));

    if args.stress {
        println!(
            "[m0-1] stress mode: spawning 50 puppets x {} joints",
            args.joints
        );
        let cols = 10;
        let spacing = 5.5;
        for n in 0..50u32 {
            let cx = (n % cols) as f32 * spacing;
            let cy = -((n / cols) as f32) * spacing;
            spawn_puppet(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut inverse_bindposes_assets,
                &asset_server,
                Vec3::new(cx, cy, 0.0),
                args.joints,
                args.bounds_fix,
                n as f32 * 0.3,
            );
        }
    } else {
        spawn_puppet(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut inverse_bindposes_assets,
            &asset_server,
            Vec3::new(-2.0, 1.5, 0.0),
            args.joints,
            args.bounds_fix,
            // 0.5 rather than 0.0 so the derived per-puppet speed lands on
            // exactly 2.0 -- the rate every single-puppet run and screenshot
            // before this change was taken at.
            0.5,
        );
    }
}

fn joint_animation(
    time: Res<Time>,
    args: Res<SpikeArgs>,
    mut query: Query<(&mut Transform, &AnimatedJoint)>,
) {
    let t = time.elapsed_secs();
    for (mut transform, joint) in &mut query {
        let wave =
            (t * joint.speed + joint.index as f32 * 0.4 + joint.phase).sin() * args.amplitude;
        transform.translation = Vec3::new(joint.rest_x, wave, 0.0);
    }
}

/// A slow automatic orbit, so the "does the mesh vanish under culling"
/// check (Task 2 Step 5) doesn't require manual camera input.
fn orbit_camera(
    time: Res<Time>,
    args: Res<SpikeArgs>,
    mut query: Query<&mut Transform, With<Camera3d>>,
) {
    let t = time.elapsed_secs() * 0.25;

    // In stress mode the 50 puppets occupy a 10x5 grid roughly 50x22 world
    // units across. Orbiting it at the single-puppet radius of 14 leaves ~90%
    // of the puppets outside the frustum on any given frame (measured: 5405
    // of 6000 puppet-frames culled), so the "50 puppets" frame-time figure
    // was really the cost of drawing about five. Frame the whole grid instead.
    let (radius, focus) = if args.stress {
        (70.0, Vec3::new(24.75, -11.0, 0.0))
    } else {
        (14.0, Vec3::new(2.0, -1.5, 0.0))
    };

    for mut transform in &mut query {
        let x = focus.x + radius * t.cos();
        let z = radius * t.sin();
        *transform = Transform::from_xyz(x, focus.y + 3.0, z).looking_at(focus, Vec3::Y);

        // With `--edge-sweep`, pitch the camera so the mesh repeatedly
        // crosses the **top and bottom** frustum boundaries.
        //
        // The axis matters, and getting it wrong makes the test silently
        // useless. `joint_animation` displaces vertices along Y only, so the
        // deformed mesh exceeds its rest-pose AABB along Y and *nowhere
        // else*. Sweeping horizontally compares two boxes with identical X
        // extents, which cull at exactly the same moment whether or not the
        // bounds are updated -- a guaranteed null result. Only a vertical
        // sweep puts the stale edge of the AABB against the frustum.
        if args.edge_sweep {
            let pitch = (time.elapsed_secs() * 0.6).sin() * 0.7; // ~+-40 degrees
            transform.rotate_local_x(pitch);
        }
    }
}

/// Counts how often the skinned mesh was actually discarded by frustum
/// culling. The "is the bounds fix load-bearing?" question was written up as
/// eyes-only, but it is not: `ViewVisibility` records the culling decision
/// directly, so the two configurations can be compared as numbers instead of
/// as an impression of whether something blinked out slightly early.
#[derive(Resource, Default, Debug)]
struct CullStats {
    observed: u32,
    culled: u32,
}

/// One-shot check that the per-puppet phase offsets actually reached the
/// joints. Reports how many distinct phase values exist across all
/// `AnimatedJoint`s, and the resulting spread in vertical position.
fn report_phase_spread(query: &Query<(&AnimatedJoint, &Transform)>) {
    let mut phases: Vec<f32> = query.iter().map(|(j, _)| j.phase).collect();
    phases.sort_by(|a, b| a.partial_cmp(b).unwrap());
    phases.dedup();
    let ys: Vec<f32> = query.iter().map(|(_, t)| t.translation.y).collect();
    let (min_y, max_y) = ys
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &y| (lo.min(y), hi.max(y)));
    println!(
        "[m0-1] PHASE: distinct phase values={} range={:?}..{:?} joint-y spread now={:.3}",
        phases.len(),
        phases.first(),
        phases.last(),
        max_y - min_y
    );
}

fn track_culling(query: Query<&ViewVisibility, With<SkinnedMesh>>, mut stats: ResMut<CullStats>) {
    for visibility in &query {
        stats.observed += 1;
        if !visibility.get() {
            stats.culled += 1;
        }
    }
}

fn auto_close_and_report(
    mut frame_counter: ResMut<FrameCounter>,
    args: Res<SpikeArgs>,
    diagnostics: Res<DiagnosticsStore>,
    cull_stats: Res<CullStats>,
    joints: Query<(&AnimatedJoint, &Transform)>,
    mut exit: MessageWriter<AppExit>,
) {
    frame_counter.0 += 1;
    let Some(target) = args.auto_close_frames else {
        return;
    };
    if frame_counter.0 == target {
        if let Some(fps) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS) {
            println!(
                "[m0-1] FPS: avg={:.2} smoothed={:.2}",
                fps.average().unwrap_or(f64::NAN),
                fps.smoothed().unwrap_or(f64::NAN)
            );
        }
        if let Some(ft) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME) {
            println!(
                "[m0-1] FRAME_TIME_MS: avg={:.4} smoothed={:.4}",
                ft.average().unwrap_or(f64::NAN),
                ft.smoothed().unwrap_or(f64::NAN)
            );
        }
        let pct = if cull_stats.observed > 0 {
            100.0 * cull_stats.culled as f32 / cull_stats.observed as f32
        } else {
            f32::NAN
        };
        println!(
            "[m0-1] CULLING: bounds_fix={} edge_sweep={} amplitude={} \
             mesh-frames observed={} culled={} ({:.1}%)",
            args.bounds_fix,
            args.edge_sweep,
            args.amplitude,
            cull_stats.observed,
            cull_stats.culled,
            pct
        );
        report_phase_spread(&joints);
        println!("[m0-1] auto-close at frame {target}, exiting");
        exit.write(AppExit::Success);
    }
}
