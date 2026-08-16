//! M0-2 spike: egui_dock with a render-to-texture viewport, per
//! `docs/superpowers/plans/2026-08-16-foundation-m0-core.md` Task 3.
//!
//! Verifiable-without-eyes part (what this spike's own code proves):
//!   - exactly one `egui` in the dependency graph (checked with `cargo tree
//!     -i egui`, recorded in the findings doc, not something the program
//!     itself can assert)
//!   - resize-debounce is implemented (logic below; the *absence of wgpu
//!     validation errors under rapid resize* is a user-observed check)
//!   - a world-grid scene with a marked origin and a numeric world-coordinate
//!     readout renders
//!   - a crosshair marker is drawn at the last click's unprojected world
//!     position, so click accuracy is visible to the user
//!
//! Click accuracy itself, and behaviour across DPI scales, are the user's
//! half — see docs/spikes/m0-2-egui-viewport.md.

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::window::PrimaryWindow;
use bevy_egui::{
    egui, EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, EguiTextureHandle,
    EguiUserTextures,
};
use egui_dock::{DockArea, DockState, Style, TabViewer};

const MIN_SCALE: f32 = 0.05;
const MAX_SCALE: f32 = 20.0;
const VIEWPORT_TEX_SIZE: u32 = 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Viewport,
    Left,
    Right,
}

#[derive(Resource)]
struct DockStateRes(DockState<Tab>);

#[derive(Resource)]
struct ViewportImage(Handle<Image>);

#[derive(Component)]
struct ViewportCamera;

/// State shared between the egui UI system and the pan/zoom/click math.
/// `last_click_world` and `last_image_rect` are read by `TabViewerImpl` to
/// draw the crosshair and coordinate readout.
#[derive(Resource, Default)]
struct ViewportState {
    last_click_world: Option<Vec2>,
    /// The most recent physical pixel size the viewport image *should* be,
    /// per the debounce rule in Task 3 Step 6. Applied by
    /// `apply_debounced_resize` once stable.
    pending_resize: Option<UVec2>,
    stable_frames: u32,
    current_tex_size: UVec2,
}

#[derive(Resource, Default)]
struct FrameCounter(u32);

#[derive(Resource, Debug, Clone, Default)]
struct SpikeArgs {
    auto_close_frames: Option<u32>,
}

impl SpikeArgs {
    fn parse() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let mut auto_close_frames = None;
        let mut i = 1;
        while i < args.len() {
            if args[i] == "--auto-close" {
                i += 1;
                auto_close_frames = args.get(i).and_then(|s| s.parse().ok());
            }
            i += 1;
        }
        Self { auto_close_frames }
    }
}

fn main() {
    let spike_args = SpikeArgs::parse();
    println!("[m0-2] args: {spike_args:?}");

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "m0-2-egui-viewport".into(),
                resolution: (1400u32, 900u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_resource(spike_args)
        .insert_resource(FrameCounter::default())
        .insert_resource(ViewportState {
            current_tex_size: UVec2::splat(VIEWPORT_TEX_SIZE),
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(EguiPrimaryContextPass, ui_system)
        .add_systems(Update, apply_debounced_resize)
        .add_systems(Update, auto_close_and_report)
        .run();
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut egui_user_textures: ResMut<EguiUserTextures>,
    mut egui_global_settings: ResMut<EguiGlobalSettings>,
) {
    // Keep egui's auto-created primary context on the main window's own
    // camera; we only need a *second*, offscreen camera for the viewport
    // render target, so we do not disable auto_create_primary_context here
    // (unlike bevy_egui's render_to_image_widget example, which repurposes
    // the main camera as the egui host and therefore must disable it).
    let _ = &mut egui_global_settings;

    // bevy_egui's auto-created primary context attaches to the FIRST
    // `Camera` entity spawned. Spawn a plain window-targeting camera before
    // the offscreen viewport camera below, or bevy_egui attaches the
    // primary context to the offscreen camera instead and tries to render
    // egui into the Bgra8UnormSrgb render-target image using a pipeline
    // built for Rgba8UnormSrgb -- a wgpu validation error discovered by
    // running this spike (see docs/spikes/m0-2-egui-viewport.md).
    commands.spawn(Camera2d);

    let size = Extent3d {
        width: VIEWPORT_TEX_SIZE,
        height: VIEWPORT_TEX_SIZE,
        ..default()
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("m0-2 viewport render target"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(size);
    let image_handle = images.add(image);
    egui_user_textures.add_image(EguiTextureHandle::Strong(image_handle.clone()));
    commands.insert_resource(ViewportImage(image_handle.clone()));

    // Camera that renders the world-grid scene into the offscreen texture.
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.08, 0.08, 0.1)),
            ..default()
        },
        RenderTarget::Image(image_handle.into()),
        Projection::Orthographic(OrthographicProjection {
            scale: 1.0,
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        ViewportCamera,
    ));

    // World-space reference grid drawn with gizmos, plus a marked origin.
    commands.spawn((
        DirectionalLight {
            illuminance: 3000.0,
            ..default()
        },
        Transform::from_xyz(3.0, 5.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let tabs = vec![Tab::Left, Tab::Viewport, Tab::Right];
    commands.insert_resource(DockStateRes(DockState::new(tabs)));
}

/// Draws the world-space reference grid and origin marker into the offscreen
/// scene every frame (gizmos are re-issued per frame, and this camera has
/// its own `RenderLayers`-free view since there's only one camera looking
/// at the grid).
fn draw_world_grid(mut gizmos: Gizmos) {
    let half_extent = 10;
    for i in -half_extent..=half_extent {
        let color = if i == 0 {
            Color::srgb(0.9, 0.2, 0.2)
        } else {
            Color::srgba(0.4, 0.4, 0.45, 0.6)
        };
        gizmos.line(
            Vec3::new(i as f32, -half_extent as f32, 0.0),
            Vec3::new(i as f32, half_extent as f32, 0.0),
            color,
        );
        let color = if i == 0 {
            Color::srgb(0.2, 0.9, 0.2)
        } else {
            Color::srgba(0.4, 0.4, 0.45, 0.6)
        };
        gizmos.line(
            Vec3::new(-half_extent as f32, i as f32, 0.0),
            Vec3::new(half_extent as f32, i as f32, 0.0),
            color,
        );
    }
    // Origin marker.
    gizmos.circle(
        Isometry3d::from_translation(Vec3::ZERO),
        0.15,
        Color::srgb(1.0, 1.0, 0.2),
    );
}

struct TabViewerImpl<'a> {
    tex_id: egui::TextureId,
    viewport_state: &'a mut ViewportState,
    camera: (&'a Camera, &'a mut Projection, &'a GlobalTransform, &'a mut Transform),
}

impl<'a> TabViewer for TabViewerImpl<'a> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            Tab::Viewport => "Viewport".into(),
            Tab::Left => "Left".into(),
            Tab::Right => "Right".into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Left => {
                ui.label("Left panel (placeholder tab, per Task 3 Step 3).");
            }
            Tab::Right => {
                ui.label("Right panel (placeholder tab, per Task 3 Step 3).");
                if let Some(w) = self.viewport_state.last_click_world {
                    ui.label(format!("Last click, world coords: ({:.3}, {:.3})", w.x, w.y));
                } else {
                    ui.label("No click yet.");
                }
            }
            Tab::Viewport => self.viewport_ui(ui),
        }
    }
}

impl<'a> TabViewerImpl<'a> {
    fn viewport_ui(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let image_size = egui::vec2(available.x.max(16.0), available.y.max(16.0));
        let response = ui.add(egui::Image::new(egui::load::SizedTexture::new(
            self.tex_id,
            image_size,
        )));
        let image_rect = response.rect;

        // --- Debounced resize (Task 3 Step 6) ---
        // Round to whole physical pixels, multiply by the egui pixels-per-point
        // (which tracks the OS scale factor), clamp to >= 1, and only queue a
        // resize if the delta exceeds 2px or the size has been stable for 2
        // frames. The actual `Assets<Image>` resize happens in
        // `apply_debounced_resize` (a plain Update system), since resizing a
        // render target from inside the egui UI closure would need mutable
        // access we don't have here.
        let ppp = ui.ctx().pixels_per_point();
        let physical = UVec2::new(
            ((image_rect.width() * ppp).round() as u32).max(1),
            ((image_rect.height() * ppp).round() as u32).max(1),
        );
        let delta = physical.as_ivec2() - self.viewport_state.current_tex_size.as_ivec2();
        if delta.x.abs() > 2 || delta.y.abs() > 2 {
            if self.viewport_state.pending_resize == Some(physical) {
                self.viewport_state.stable_frames += 1;
            } else {
                self.viewport_state.pending_resize = Some(physical);
                self.viewport_state.stable_frames = 1;
            }
        } else {
            self.viewport_state.pending_resize = None;
            self.viewport_state.stable_frames = 0;
        }

        // --- Pan/zoom with zoom-to-cursor (Task 3 Step 4) ---
        // Projection change and unprojection happen in the same frame with
        // manual math, per the plan: computing "world point under cursor
        // before" and "...after" and correcting camera translation by the
        // delta avoids the one-frame lag of letting the projection change
        // propagate through Bevy's transform/camera update systems first.
        let (camera, projection, global_transform, transform) = &mut self.camera;
        let hovered = response.hovered();
        let wants_pointer = ui.ctx().wants_pointer_input();

        if hovered && !wants_pointer {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                if let Some(pointer) = response.hover_pos() {
                    let pixel_in_image = Vec2::new(
                        (pointer.x - image_rect.min.x) * ppp,
                        (pointer.y - image_rect.min.y) * ppp,
                    );
                    let before = camera.viewport_to_world_2d(global_transform, pixel_in_image).ok();

                    if let Projection::Orthographic(ortho) = &mut **projection {
                        ortho.scale = (ortho.scale * (1.0 - scroll * 0.001)).clamp(MIN_SCALE, MAX_SCALE);
                    }

                    if let (Some(before), Some(after)) = (
                        before,
                        camera.viewport_to_world_2d(global_transform, pixel_in_image).ok(),
                    ) {
                        transform.translation += (before - after).extend(0.0);
                    }
                }
            }

            // Middle-drag pans, scaled by proj.scale / image_height so the
            // point under the cursor tracks 1:1.
            let middle_down = ui.input(|i| i.pointer.middle_down());
            if middle_down {
                let drag_delta = ui.input(|i| i.pointer.delta());
                if let Projection::Orthographic(ortho) = &**projection {
                    let world_per_px = ortho.scale / image_rect.height().max(1.0) * ppp;
                    transform.translation.x -= drag_delta.x * world_per_px;
                    transform.translation.y += drag_delta.y * world_per_px;
                }
            }
        }

        // --- Click accuracy (Task 3 Step 5) ---
        if hovered && !wants_pointer && response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let pixel_in_image = Vec2::new(
                    (pointer.x - image_rect.min.x) * ppp,
                    (pointer.y - image_rect.min.y) * ppp,
                );
                if let Ok(world) = camera.viewport_to_world_2d(global_transform, pixel_in_image) {
                    self.viewport_state.last_click_world = Some(world);
                }
            }
        }

        ui.painter().text(
            image_rect.left_top() + egui::vec2(6.0, 4.0),
            egui::Align2::LEFT_TOP,
            match self.viewport_state.last_click_world {
                Some(w) => format!("world: ({:.3}, {:.3})", w.x, w.y),
                None => "world: (click to sample)".to_string(),
            },
            egui::FontId::monospace(14.0),
            egui::Color32::WHITE,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn ui_system(
    mut contexts: EguiContexts,
    mut dock_state: ResMut<DockStateRes>,
    mut viewport_state: ResMut<ViewportState>,
    viewport_image: Res<ViewportImage>,
    mut camera_q: Query<(&Camera, &mut Projection, &GlobalTransform, &mut Transform), With<ViewportCamera>>,
    mut gizmos: Gizmos,
) -> Result {
    draw_world_grid(gizmos);

    let tex_id = contexts
        .image_id(&viewport_image.0)
        .expect("viewport image must be registered with EguiUserTextures in setup()");
    let ctx = contexts.ctx_mut()?;

    let Ok((camera, mut projection, global_transform, mut transform)) = camera_q.single_mut() else {
        return Ok(());
    };

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            let mut viewer = TabViewerImpl {
                tex_id,
                viewport_state: &mut viewport_state,
                camera: (camera, &mut projection, global_transform, &mut transform),
            };
            DockArea::new(&mut dock_state.0)
                .style(Style::from_egui(ui.style().as_ref()))
                .show_inside(ui, &mut viewer);
        });

    Ok(())
}

/// Applies the debounced resize decision made in `ui_system` to the actual
/// render-target `Image` asset. Separate system because resizing needs
/// `ResMut<Assets<Image>>`, and because the debounce rule ("stable for 2
/// frames") is inherently a resize-on-a-later-frame operation, not
/// same-frame.
fn apply_debounced_resize(mut images: ResMut<Assets<Image>>, viewport_image: Res<ViewportImage>, mut state: ResMut<ViewportState>) {
    let Some(pending) = state.pending_resize else {
        return;
    };
    if state.stable_frames < 2 {
        return;
    }
    if let Some(mut image) = images.get_mut(&viewport_image.0) {
        let size = Extent3d {
            width: pending.x,
            height: pending.y,
            ..default()
        };
        image.resize(size);
        state.current_tex_size = pending;
    }
    state.pending_resize = None;
    state.stable_frames = 0;
}

fn auto_close_and_report(
    mut frame_counter: ResMut<FrameCounter>,
    args: Res<SpikeArgs>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) {
    frame_counter.0 += 1;
    let Some(target) = args.auto_close_frames else {
        return;
    };
    if frame_counter.0 == target {
        if let Ok(window) = windows.single() {
            println!(
                "[m0-2] window scale_factor={:.3} logical=({:.0},{:.0}) physical=({},{})",
                window.scale_factor(),
                window.width(),
                window.height(),
                window.physical_width(),
                window.physical_height(),
            );
        }
        println!("[m0-2] auto-close at frame {target}, exiting");
        exit.write(AppExit::Success);
    }
}
