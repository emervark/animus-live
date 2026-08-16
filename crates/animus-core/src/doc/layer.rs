//! Layers: the paint-order list a `Project` composites.
//!
//! `Project::layers` (a `Vec<LayerId>`) holds the paint order; this module
//! holds the per-layer data those IDs point at.

use crate::ids::{LayerId, PuppetId};
use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    /// Authoritative world Z. This is how 2D layers interleave with 3D
    /// glTF models in the same scene — see spec §7.4.
    pub depth: f32,
    pub transform: Transform2Or3,
    pub contents: Vec<PuppetId>,
}

impl Layer {
    /// A visible, fully opaque, normally-blended layer at the origin with
    /// no contents yet.
    pub fn new(id: LayerId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
            depth: 0.0,
            transform: Transform2Or3::default(),
            contents: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    Normal,
    Add,
    Multiply,
    Screen,
}

/// A layer's placement. Most layers are flat 2D puppets moving in the
/// image plane; a layer hosting a glTF `ModelPuppet` needs a full 3D pose.
/// Kept as one enum (rather than always-3D) so the common 2D case doesn't
/// carry an unused Z/roll/pitch around.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transform2Or3 {
    Flat {
        translation: Vec2,
        rotation: f32,
        scale: Vec2,
    },
    Spatial {
        translation: Vec3,
        rotation: Quat,
        scale: Vec3,
    },
}

impl Default for Transform2Or3 {
    fn default() -> Self {
        Self::Flat {
            translation: Vec2::ZERO,
            rotation: 0.0,
            scale: Vec2::ONE,
        }
    }
}
