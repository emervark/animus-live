//! The viewport: a camera rendering into an image, drawn inside a dock tab.
//!
//! The two halves live in [`input`] (egui: the widget and its gates) and
//! [`camera`] (Bevy: pixels ↔ world, pan, zoom). They are separate because
//! each can be wrong on its own, and M0-2 proved that the one people test is
//! not the one that breaks.

pub mod camera;
pub mod input;

use bevy::camera::{RenderTarget, ScalingMode};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy_egui::{EguiTextureHandle, EguiUserTextures};
use glam::Vec2;

pub use input::{ViewportInput, viewport_widget};

/// Marks the camera that renders the editor viewport.
#[derive(Component, Debug, Clone, Copy)]
pub struct ViewportCamera;

/// The render target and everything read back from it.
#[derive(Resource, Debug, Clone)]
pub struct ViewportTarget {
    pub image: Handle<Image>,
    /// Current size of the image, in physical pixels.
    pub size: UVec2,
    /// World units per image pixel, measured from the live camera. Shown in
    /// the status strip, and the unit every offset in this editor is quoted
    /// in.
    pub world_per_pixel: f32,
    /// Where the cursor is in world space, if it is over the viewport.
    pub cursor_world: Option<Vec2>,
    /// The last click, in world space.
    pub last_click_world: Option<Vec2>,
}

pub const INITIAL_SIZE: UVec2 = UVec2::new(1024, 768);

fn new_target_image(size: UVec2) -> Image {
    let extent = Extent3d {
        width: size.x.max(1),
        height: size.y.max(1),
        ..default()
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("animus viewport"),
            size: extent,
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
    image.resize(extent);
    image
}

/// Create the render target and the camera that draws into it.
///
/// Runs after the editor's window camera, never before: `bevy_egui` attaches
/// its primary context to the first camera spawned, and attaching it to this
/// `Bgra8UnormSrgb` target fails wgpu validation (M0-2).
pub fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut user_textures: ResMut<EguiUserTextures>,
) {
    let handle = images.add(new_target_image(INITIAL_SIZE));
    user_textures.add_image(EguiTextureHandle::Strong(handle.clone()));

    commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.04, 0.05, 0.06)),
            ..default()
        },
        RenderTarget::Image(handle.clone().into()),
        Projection::Orthographic(OrthographicProjection {
            scale: 1.0,
            scaling_mode: ScalingMode::WindowSize,
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 0.0, 100.0).looking_at(Vec3::ZERO, Vec3::Y),
        ViewportCamera,
    ));

    commands.insert_resource(ViewportTarget {
        image: handle,
        size: INITIAL_SIZE,
        world_per_pixel: 0.0,
        cursor_world: None,
        last_click_world: None,
    });
}

/// Resize the render target to match the panel.
///
/// **Immediately, with no debounce.** The spec called for one; M0-2 measured
/// both ways and found zero wgpu validation errors and zero warnings either
/// way, while the debounce produced a visible rubber-band stretch because
/// the target deliberately lags the panel. Asked to compare, the user
/// preferred it gone.
pub fn apply_resize(
    mut images: ResMut<Assets<Image>>,
    mut target: ResMut<ViewportTarget>,
    desired: UVec2,
) {
    if desired == target.size || desired.x == 0 || desired.y == 0 {
        return;
    }
    if let Some(image) = images.get_mut(&target.image).as_mut() {
        image.resize(Extent3d {
            width: desired.x,
            height: desired.y,
            ..default()
        });
        target.size = desired;
    }
}

/// Everything the viewport learned this frame, handed to the camera.
///
/// Split out from the UI system so the order is explicit: zoom before pan,
/// because zoom moves the camera and a pan applied first would be measured
/// against the old scale.
pub fn apply_viewport_input(
    input: &ViewportInput,
    camera: &Camera,
    global: &GlobalTransform,
    projection: &mut Projection,
    transform: &mut Transform,
    target: &mut ViewportTarget,
) {
    let wpp = camera::world_per_pixel(camera, global);
    target.world_per_pixel = wpp;

    if let Some(pixel) = input.hover_at {
        if input.scroll != 0.0 {
            camera::zoom_to_cursor(camera, global, projection, transform, pixel, input.scroll);
        }
        target.cursor_world = camera::pixel_to_world(camera, global, pixel);
    } else {
        target.cursor_world = None;
    }

    if let Some(delta) = input.drag_middle {
        camera::pan(transform, delta, wpp);
    }

    if let Some(pixel) = input.clicked_at {
        target.last_click_world = camera::pixel_to_world(camera, global, pixel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_target_is_never_zero_sized() {
        let image = new_target_image(UVec2::ZERO);
        assert_eq!(image.texture_descriptor.size.width, 1);
        assert_eq!(image.texture_descriptor.size.height, 1);
    }

    #[test]
    fn the_target_carries_the_usages_a_render_target_and_egui_both_need() {
        let image = new_target_image(UVec2::new(64, 64));
        let usage = image.texture_descriptor.usage;
        assert!(
            usage.contains(TextureUsages::RENDER_ATTACHMENT),
            "the camera must be able to draw into it"
        );
        assert!(
            usage.contains(TextureUsages::TEXTURE_BINDING),
            "egui must be able to sample it"
        );
    }
}
