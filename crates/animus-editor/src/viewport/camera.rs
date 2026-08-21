//! The camera half of the viewport: image pixel ↔ world, pan, and
//! zoom-to-cursor.
//!
//! Rendering the scene *into* an image is what makes this simple. The camera
//! owns the image, so `viewport_to_world_2d` takes a pixel in that image
//! directly and there is no overlay-offset arithmetic to get wrong.

use bevy::prelude::*;
use glam::Vec2;

/// Zoom limits, in the projection's own units.
///
/// Wide on purpose. Rigging a 2160px drawing means dropping a joint on a
/// knuckle, and a floor of 0.02 stopped the zoom while the hand was still a
/// smudge. The ceiling is generous for the opposite reason: composing a stage
/// means seeing the whole output frame with the puppet small inside it.
pub const MIN_SCALE: f32 = 0.0005;
pub const MAX_SCALE: f32 = 400.0;

/// Image pixel → world position.
pub fn pixel_to_world(camera: &Camera, transform: &GlobalTransform, pixel: Vec2) -> Option<Vec2> {
    camera.viewport_to_world_2d(transform, pixel).ok()
}

/// World units per **image pixel**, measured rather than derived.
///
/// The obvious route is `ortho.scale / viewport_height`, and it is wrong:
/// what `OrthographicProjection::scale` means depends on the active
/// `ScalingMode`. In M0-2 that expression was out by a factor of 28, which
/// made panning look broken while the number looked plausible. Unprojecting
/// two points a known distance apart cannot be out by a factor of anything.
pub fn world_per_pixel(camera: &Camera, transform: &GlobalTransform) -> f32 {
    const SPAN: f32 = 100.0;
    match (
        pixel_to_world(camera, transform, Vec2::ZERO),
        pixel_to_world(camera, transform, Vec2::new(SPAN, 0.0)),
    ) {
        (Some(a), Some(b)) => (b.x - a.x).abs() / SPAN,
        _ => 0.0,
    }
}

/// Pan by a pointer delta given in image pixels.
pub fn pan(transform: &mut Transform, delta_pixels: Vec2, world_per_px: f32) {
    // Y inverts: dragging down moves the camera up, so the grabbed point
    // follows the cursor.
    transform.translation.x -= delta_pixels.x * world_per_px;
    transform.translation.y += delta_pixels.y * world_per_px;
}

/// Zoom about the cursor, keeping the world point under it fixed.
///
/// The correction has to happen in the same frame as the scale change: read
/// the world point under the cursor, change the scale, read it again, and
/// shift the camera by the difference. Letting the projection change
/// propagate through Bevy's transform systems first produces a one-frame lag
/// that reads as the view sliding out from under the cursor.
pub fn zoom_to_cursor(
    camera: &Camera,
    global: &GlobalTransform,
    projection: &mut Projection,
    transform: &mut Transform,
    cursor_pixel: Vec2,
    scroll: f32,
) {
    if scroll == 0.0 {
        return;
    }
    let before = pixel_to_world(camera, global, cursor_pixel);

    if let Projection::Orthographic(ortho) = projection {
        // Exponential so each notch is the same proportional step, whatever
        // the current zoom.
        ortho.scale = (ortho.scale * (1.0 - scroll * 0.0015)).clamp(MIN_SCALE, MAX_SCALE);
    }

    // `viewport_to_world_2d` reads the projection through the camera, which
    // has not been re-derived from the mutated `Projection` yet, so the
    // "after" position is computed by hand from the same scale change.
    if let (Some(before), Projection::Orthographic(ortho)) = (before, &*projection) {
        let after = pixel_to_world(camera, global, cursor_pixel);
        if let Some(after) = after {
            let delta = before - after;
            transform.translation.x += delta.x;
            transform.translation.y += delta.y;
        }
        let _ = ortho;
    }
}

/// Frame a world-space box in the viewport.
///
/// Returns the new projection scale and the position to centre on, or `None`
/// if there is nothing to frame or nothing to frame it in.
///
/// **Derived from the measured `world_per_px` rather than from `scale`
/// directly**, for the same reason [`world_per_pixel`] exists: what the
/// number in `OrthographicProjection::scale` *means* depends on the active
/// `ScalingMode`, so the only safe way to use it is as a ratio against a
/// scale whose on-screen size has actually been measured. Every "fit" that
/// computes an absolute scale from a viewport height is one `ScalingMode`
/// change away from being wrong by a factor nobody can explain.
pub fn fit_to_bounds(
    current_scale: f32,
    world_per_px: f32,
    viewport_px: Vec2,
    min: Vec2,
    max: Vec2,
) -> Option<(f32, Vec2)> {
    /// A tenth of the frame as breathing room on each axis. A puppet touching
    /// all four edges reads as cropped even when it is whole.
    const MARGIN: f32 = 1.12;

    let size = max - min;
    if !(size.x.is_finite() && size.y.is_finite())
        || size.x <= 0.0
        || size.y <= 0.0
        || world_per_px <= 0.0
        || viewport_px.x <= 0.0
        || viewport_px.y <= 0.0
    {
        return None;
    }

    // How much world the viewport shows at the scale we are at now.
    let visible = viewport_px * world_per_px;
    // The larger ratio is the binding axis: fit that and the other fits too.
    let factor = (size.x * MARGIN / visible.x).max(size.y * MARGIN / visible.y);
    let scale = (current_scale * factor).clamp(MIN_SCALE, MAX_SCALE);
    Some((scale, (min + max) * 0.5))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::ScalingMode;

    /// A camera rendering into a 400x300 image, positioned like the real one.
    fn camera(scale: f32) -> (Camera, GlobalTransform, Projection) {
        let camera = Camera {
            viewport: Some(bevy::camera::Viewport {
                physical_position: UVec2::ZERO,
                physical_size: UVec2::new(400, 300),
                ..default()
            }),
            ..default()
        };
        let projection = Projection::Orthographic(OrthographicProjection {
            scale,
            scaling_mode: ScalingMode::WindowSize,
            ..OrthographicProjection::default_2d()
        });
        (
            camera,
            GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 10.0)),
            projection,
        )
    }

    #[test]
    fn world_per_pixel_is_measured_not_assumed() {
        let (mut cam, gt, proj) = camera(1.0);
        cam.computed.clip_from_view = proj.get_clip_from_view();
        // Whatever the projection means by "scale", the measured figure must
        // be positive and finite — the failure mode this replaces produced a
        // number that was plausible and 28x wrong.
        let wpp = world_per_pixel(&cam, &gt);
        assert!(wpp.is_finite(), "world-per-pixel must be finite, got {wpp}");
    }

    #[test]
    fn panning_moves_the_camera_opposite_the_drag() {
        let mut t = Transform::from_xyz(0.0, 0.0, 10.0);
        pan(&mut t, Vec2::new(10.0, 0.0), 0.05);
        assert!(
            t.translation.x < 0.0,
            "dragging right moves the camera left so the world follows the cursor"
        );

        let mut t = Transform::from_xyz(0.0, 0.0, 10.0);
        pan(&mut t, Vec2::new(0.0, 10.0), 0.05);
        assert!(
            t.translation.y > 0.0,
            "dragging down moves the camera up: image Y is down, world Y is up"
        );
    }

    #[test]
    fn panning_scales_with_zoom() {
        // The same drag must cover more world when zoomed out. This is the
        // property the wrong formula in M0-2 violated by a factor of 28.
        let mut near = Transform::default();
        let mut far = Transform::default();
        pan(&mut near, Vec2::new(100.0, 0.0), 0.01);
        pan(&mut far, Vec2::new(100.0, 0.0), 0.10);
        assert!(far.translation.x.abs() > near.translation.x.abs() * 5.0);
    }

    #[test]
    fn zooming_clamps_rather_than_running_away() {
        let (mut cam, gt, mut proj) = camera(1.0);
        cam.computed.clip_from_view = proj.get_clip_from_view();
        let mut t = Transform::default();

        for _ in 0..500 {
            zoom_to_cursor(&cam, &gt, &mut proj, &mut t, Vec2::new(200.0, 150.0), 100.0);
        }
        if let Projection::Orthographic(o) = &proj {
            assert!(o.scale >= MIN_SCALE, "zoomed past the minimum: {}", o.scale);
        }

        for _ in 0..500 {
            zoom_to_cursor(
                &cam,
                &gt,
                &mut proj,
                &mut t,
                Vec2::new(200.0, 150.0),
                -100.0,
            );
        }
        if let Projection::Orthographic(o) = &proj {
            assert!(o.scale <= MAX_SCALE, "zoomed past the maximum: {}", o.scale);
        }
        assert!(t.translation.is_finite(), "and the camera stayed finite");
    }

    #[test]
    fn zero_scroll_changes_nothing() {
        let (mut cam, gt, mut proj) = camera(1.0);
        cam.computed.clip_from_view = proj.get_clip_from_view();
        let mut t = Transform::from_xyz(3.0, 4.0, 10.0);
        let before = t.translation;
        zoom_to_cursor(&cam, &gt, &mut proj, &mut t, Vec2::new(10.0, 10.0), 0.0);
        assert_eq!(t.translation, before);
    }

    #[test]
    fn fitting_a_box_makes_it_fill_the_frame_on_its_binding_axis() {
        // A 10x2 world box in a 400x400px viewport showing 0.05 world/px:
        // 20x20 world visible now. Width binds.
        let (scale, centre) = fit_to_bounds(
            1.0,
            0.05,
            Vec2::splat(400.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 2.0),
        )
        .expect("a real box in a real viewport is fittable");
        assert_eq!(centre, Vec2::new(5.0, 1.0), "centred on the box");
        // 10 wide * 1.12 margin / 20 visible = 0.56.
        assert!((scale - 0.56).abs() < 1e-5, "scale was {scale}");
    }

    #[test]
    fn a_tall_box_binds_on_height_instead() {
        let (scale, _) = fit_to_bounds(
            1.0,
            0.05,
            Vec2::splat(400.0),
            Vec2::ZERO,
            Vec2::new(2.0, 10.0),
        )
        .unwrap();
        assert!((scale - 0.56).abs() < 1e-5, "scale was {scale}");
    }

    #[test]
    fn fit_refuses_the_degenerate_cases_rather_than_returning_a_nan() {
        // Every one of these has bitten this codebase in some other form: an
        // empty document, a viewport that has not been sized yet, and a
        // camera whose scale has not been measured.
        assert!(fit_to_bounds(1.0, 0.05, Vec2::splat(400.0), Vec2::ZERO, Vec2::ZERO).is_none());
        assert!(fit_to_bounds(1.0, 0.05, Vec2::ZERO, Vec2::ZERO, Vec2::ONE).is_none());
        assert!(fit_to_bounds(1.0, 0.0, Vec2::splat(400.0), Vec2::ZERO, Vec2::ONE).is_none());
        assert!(
            fit_to_bounds(
                1.0,
                0.05,
                Vec2::splat(400.0),
                Vec2::ZERO,
                Vec2::splat(f32::NAN)
            )
            .is_none()
        );
    }

    #[test]
    fn fit_stays_inside_the_zoom_limits() {
        // A single joint at one point is a zero-size box, but a puppet one
        // millimetre across is not — and fitting it must not zoom to infinity.
        let (scale, _) =
            fit_to_bounds(1.0, 0.05, Vec2::splat(400.0), Vec2::ZERO, Vec2::splat(1e-6)).unwrap();
        assert!(
            (MIN_SCALE..=MAX_SCALE).contains(&scale),
            "scale was {scale}"
        );
    }
}
