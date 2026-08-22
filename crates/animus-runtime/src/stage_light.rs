//! Light for the stage.
//!
//! **Only models need this, and nothing else notices.** A cutout puppet is
//! drawn `unlit` — its artwork already contains whatever lighting the person
//! who painted it intended, and a shaded sprite is a sprite with the drawing
//! ruined. A glTF is the opposite: it arrives as geometry and materials with
//! no light of its own, so a show with no lights renders an imported model as
//! a black silhouette, which reads as a broken import rather than as a dark
//! room.
//!
//! So the rig here exists for the models and passes straight through the 2D
//! work. It is deliberately plain: a key from the front and above, a weaker
//! fill from the opposite side to keep the shadowed half readable, and enough
//! ambient that nothing goes fully black. Shadows are off — there is no floor
//! to catch them, and a self-shadowing figure on a black background costs a
//! shadow map to look slightly worse.
//!
//! It is a *default*, not a lighting design. When per-layer lighting arrives
//! this becomes the fallback for a show that has not asked for anything else.
//!
//! **Three directionals rather than two plus ambient**, because ambient light
//! in this engine version belongs to a camera and the cameras belong to the
//! editor and the projector. A third light aimed from behind and below does
//! the job ambient would have done — it keeps the shadowed side off pure
//! black — and it does it without this module needing to know that a camera
//! exists.

use bevy::prelude::*;

/// Marks the lights this module owns, so a future lighting panel can find
/// them rather than guess which lights in the world are the defaults.
#[derive(Component, Debug)]
pub struct StageLight;

/// Roughly a bright interior. High enough that a mid-grey material reads
/// clearly against the stage's black, low enough not to blow out a pale one.
const KEY_LUX: f32 = 6000.0;
/// The fill is a fraction of the key rather than its own round number: the
/// ratio is what makes the shading read, so it should survive a change to
/// the key.
const FILL_RATIO: f32 = 0.35;

/// A weak wash from behind and below, standing in for ambient.
const WASH_RATIO: f32 = 0.18;

/// Put the default lights on the stage.
///
/// `looking_at` rather than a hand-built quaternion: a direction is what a
/// light *means*, and the quaternion is only an implementation of it.
pub fn spawn_stage_lights(mut commands: Commands) {
    for (illuminance, from) in [
        // Key: front, above, and to the left — the ordinary place to put one,
        // and the one that makes a figure facing the audience read as facing
        // the audience.
        (KEY_LUX, Vec3::new(-4.0, 6.0, 8.0)),
        // Fill: opposite side, level, so the shadowed half keeps its shape.
        (KEY_LUX * FILL_RATIO, Vec3::new(6.0, 1.0, 4.0)),
        // Wash: from behind and below, so nothing ends up pure black.
        (KEY_LUX * WASH_RATIO, Vec3::new(0.0, -3.0, -6.0)),
    ] {
        commands.spawn((
            StageLight,
            DirectionalLight {
                illuminance,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(from).looking_at(Vec3::ZERO, Vec3::Y),
        ));
    }
}
