//! The one place image space becomes world space.
//!
//! Image space is pixels, origin top-left, **Y down**. World space is Y up.
//! Spec §7.1.
//!
//! **Only positions flip. UVs do not.** Image-space `(u,v) = (x/w, y/h)`
//! with v increasing downward is already wgpu's convention, so naive UV
//! assignment is correct and "fixing" it produces an upside-down texture.
//! The M0-1 spike settled this by rendering a texture with a visible "TOP"
//! label and looking at it: TOP renders at the top, right side up, with no
//! flip anywhere on this path.

use glam::{Vec2, Vec3};

/// Image-space pixel → world-space position.
///
/// `pivot` is the image-space point that lands on the puppet's origin, and
/// `ppu` is pixels per world unit.
#[inline]
pub fn img_to_world(p: Vec2, pivot: Vec2, ppu: f32) -> Vec3 {
    Vec3::new((p.x - pivot.x) / ppu, -(p.y - pivot.y) / ppu, 0.0)
}

/// World-space position → image-space pixel. The exact inverse of
/// [`img_to_world`], used when a viewport click has to become a document
/// edit.
#[inline]
pub fn world_to_img(w: Vec3, pivot: Vec2, ppu: f32) -> Vec2 {
    Vec2::new(w.x * ppu + pivot.x, -w.y * ppu + pivot.y)
}

/// Image-space angle → world-space angle. Angles negate because Y flips.
#[inline]
pub fn img_to_world_angle(a: f32) -> f32 {
    -a
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn y_flips_and_x_does_not() {
        let pivot = Vec2::new(50.0, 50.0);
        // 10 px below the pivot in image space is 10 px *down*, so in world
        // space it must be below the origin: negative Y.
        let w = img_to_world(Vec2::new(60.0, 60.0), pivot, 10.0);
        assert_relative_eq!(w.x, 1.0);
        assert_relative_eq!(w.y, -1.0);
        assert_relative_eq!(w.z, 0.0);
    }

    #[test]
    fn the_pivot_lands_on_the_origin() {
        let pivot = Vec2::new(700.0, 1050.0);
        let w = img_to_world(pivot, pivot, 32.0);
        assert_relative_eq!(w.x, 0.0);
        assert_relative_eq!(w.y, 0.0);
    }

    #[test]
    fn world_to_img_is_the_inverse() {
        let pivot = Vec2::new(12.5, -3.25);
        let ppu = 37.0;
        for p in [
            Vec2::new(0.0, 0.0),
            Vec2::new(1400.0, 2100.0),
            Vec2::new(-8.0, 19.5),
        ] {
            let back = world_to_img(img_to_world(p, pivot, ppu), pivot, ppu);
            assert_relative_eq!(back.x, p.x, epsilon = 1e-4);
            assert_relative_eq!(back.y, p.y, epsilon = 1e-4);
        }
    }

    #[test]
    fn angles_negate() {
        assert_relative_eq!(img_to_world_angle(0.5), -0.5);
    }
}
