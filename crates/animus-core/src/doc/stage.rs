//! The output canvas: size and background the stage composites onto.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StageConfig {
    /// Canvas size in pixels, `[width, height]`.
    pub canvas: [u32; 2],
    /// RGBA, 0..1. Opaque black by default.
    pub background: [f32; 4],
}

impl Default for StageConfig {
    fn default() -> Self {
        Self {
            canvas: [1920, 1080],
            background: [0.0, 0.0, 0.0, 1.0],
        }
    }
}
