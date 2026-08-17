//! Document model, geometry and physics for Animus Live.
//!
//! This crate has **no engine dependency**. It compiles and tests on any
//! platform with no GPU. See the design spec, section 3.1.
#![forbid(unsafe_code)]

pub mod doc;
pub mod ids;
pub mod image_in;
pub mod mesh;
pub mod migrate;
pub mod remap;
pub mod silhouette;
pub mod skeleton;
pub mod solver;
pub mod triangulate;
