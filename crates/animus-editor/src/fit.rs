//! Framing the puppet in the viewport.
//!
//! A project that opens showing the middle of a 2160px torso is a project the
//! operator has to fight before they can start. Bevy has no opinion about
//! where a 2D camera should sit, so this supplies one: on load, and whenever
//! the operator asks, put everything in the frame.

use bevy::camera::primitives::Aabb;
use bevy::prelude::*;

use crate::viewport::{ViewportCamera, ViewportTarget, camera};

/// Set to ask for a fit on the next frame that can serve one.
///
/// A request rather than a direct call, because the fit needs the camera's
/// *measured* world-per-pixel, and that is only meaningful once the camera
/// has a viewport — which is not true on the frame a project finishes
/// loading. The flag survives until a frame can honour it.
#[derive(Resource, Debug, Default)]
pub struct WantsFit(pub bool);

/// True once a fit has actually been performed for the current document, so
/// the automatic one happens once rather than every frame a puppet exists.
#[derive(Resource, Debug, Default)]
pub struct AutoFitDone(pub bool);

/// Ask for a fit when a document first has something to look at.
pub fn request_fit_on_load(
    meshes: Query<(), With<Mesh3d>>,
    mut wants: ResMut<WantsFit>,
    mut done: ResMut<AutoFitDone>,
) {
    if done.0 {
        return;
    }
    if meshes.iter().next().is_some() {
        wants.0 = true;
        done.0 = true;
    }
}

/// F frames everything. Deliberately the same key every 3D tool uses.
pub fn fit_shortcut(
    keys: Res<ButtonInput<KeyCode>>,
    egui_focus: Res<crate::interact::EguiWantsKeyboard>,
    mut wants: ResMut<WantsFit>,
) {
    if !egui_focus.0 && keys.just_pressed(KeyCode::KeyF) {
        wants.0 = true;
    }
}

/// Honour a pending fit request, if this frame can.
pub fn apply_fit(
    mut wants: ResMut<WantsFit>,
    target: Res<ViewportTarget>,
    mut cameras: Query<
        (&Camera, &GlobalTransform, &mut Projection, &mut Transform),
        With<ViewportCamera>,
    >,
    content: Query<(&GlobalTransform, &Aabb), With<Mesh3d>>,
    mut last_size: Local<UVec2>,
) {
    // Fit against the frame the operator will actually have.
    //
    // On the first frames the dock has not yet given the panels their share,
    // so the viewport is briefly taller than it ends up — and a fit computed
    // there leaves the puppet overflowing the smaller frame that follows.
    // Waiting for one frame of a stable size costs nothing the eye can see
    // and removes the whole class of "it opened almost right".
    let stable = target.size == *last_size;
    *last_size = target.size;
    if !wants.0 || !stable {
        return;
    }
    let Ok((cam, global, mut projection, mut transform)) = cameras.single_mut() else {
        return;
    };

    let Some((min, max)) = world_bounds(&content) else {
        // Nothing to frame yet. Keep the request: the puppet's meshes may
        // simply not have been projected into the ECS on this frame.
        return;
    };

    let world_per_px = camera::world_per_pixel(cam, global);
    let viewport = Vec2::new(target.size.x as f32, target.size.y as f32);
    let Projection::Orthographic(ortho) = &mut *projection else {
        return;
    };
    let Some((scale, centre)) =
        camera::fit_to_bounds(ortho.scale, world_per_px, viewport, min, max)
    else {
        return;
    };

    ortho.scale = scale;
    transform.translation.x = centre.x;
    transform.translation.y = centre.y;
    wants.0 = false;
}

/// The union of every rendered mesh's bounding box, in world space.
///
/// Taken from the *rendered* geometry rather than recomputed from the
/// document: the whole point is to frame what the operator can see, and a
/// second derivation of "where the puppet is" would be a second thing that
/// can disagree with the first.
fn world_bounds(content: &Query<(&GlobalTransform, &Aabb), With<Mesh3d>>) -> Option<(Vec2, Vec2)> {
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    let mut any = false;

    for (global, aabb) in content {
        // Every corner, because the transform may rotate the box — taking
        // only the two extreme corners is right until the first rotated
        // layer, and then it is quietly wrong.
        for sx in [-1.0_f32, 1.0] {
            for sy in [-1.0_f32, 1.0] {
                let local = Vec3::from(aabb.center)
                    + Vec3::new(aabb.half_extents.x * sx, aabb.half_extents.y * sy, 0.0);
                let world = global.transform_point(local);
                min = min.min(world.truncate());
                max = max.max(world.truncate());
                any = true;
            }
        }
    }

    (any && min.x.is_finite() && max.x.is_finite()).then_some((min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A render target with no image behind it. Nothing in `apply_fit` reads
    /// the handle — it needs the size and nothing else.
    fn target() -> ViewportTarget {
        ViewportTarget {
            image: Handle::default(),
            size: UVec2::ZERO,
            world_per_pixel: 0.05,
            cursor_world: None,
            last_click_world: None,
        }
    }

    /// The request must survive a frame that cannot serve it.
    ///
    /// A project loads, the flag goes up, and the very next frame there is
    /// still no camera to move. Clearing the flag there would mean the fit
    /// silently never happens — which is the failure an operator reports as
    /// "it opened zoomed in again".
    #[test]
    fn a_fit_that_cannot_be_served_stays_requested() {
        let mut app = App::new();
        app.init_resource::<WantsFit>()
            .insert_resource(target())
            .add_systems(Update, apply_fit);
        app.world_mut().resource_mut::<WantsFit>().0 = true;

        // Two frames, so the size-stability guard is satisfied and the only
        // remaining reason to bail is the missing camera.
        app.update();
        app.update();

        assert!(
            app.world().resource::<WantsFit>().0,
            "apply_fit consumed a request it could not serve"
        );
    }

    /// The other half: a request must not be served against a frame size that
    /// is still settling, or the puppet is framed for a viewport it is about
    /// to stop having.
    #[test]
    fn a_resizing_viewport_defers_the_fit() {
        let mut app = App::new();
        app.init_resource::<WantsFit>()
            .insert_resource(target())
            .add_systems(Update, apply_fit);
        app.world_mut().resource_mut::<WantsFit>().0 = true;

        // A size that changes every frame never becomes stable, so the
        // request is still standing however many frames pass.
        for i in 1..4u32 {
            app.world_mut().resource_mut::<ViewportTarget>().size = UVec2::new(100 * i, 100);
            app.update();
            assert!(app.world().resource::<WantsFit>().0, "frame {i}");
        }
    }

    #[test]
    fn auto_fit_fires_once_and_then_stays_quiet() {
        let mut app = App::new();
        app.init_resource::<WantsFit>()
            .init_resource::<AutoFitDone>()
            .add_systems(Update, request_fit_on_load);

        // Nothing to frame: no request, and nothing marked done.
        app.update();
        assert!(!app.world().resource::<WantsFit>().0);
        assert!(!app.world().resource::<AutoFitDone>().0);

        app.world_mut().spawn(Mesh3d(Handle::default()));
        app.update();
        assert!(app.world().resource::<WantsFit>().0, "a puppet arrived");
        assert!(app.world().resource::<AutoFitDone>().0);

        // The operator zooms in, and the auto-fit must not yank it back.
        app.world_mut().resource_mut::<WantsFit>().0 = false;
        app.update();
        assert!(
            !app.world().resource::<WantsFit>().0,
            "auto-fit fired a second time and stole the operator's view"
        );
    }
}
