//! On-disk project format for Animus Live.
//!
//! A project is a directory (conventionally named `Show.animus/`)
//! containing a hand-written-pretty-printed `project.json` and an
//! `assets/` tree of content-addressed files. The format is documented
//! independently of this crate in `spec/animus-project-format-v1.md` and
//! published under CC0-1.0, so any tool can implement a reader or writer
//! without needing this code or a license negotiation.
//!
//! This crate provides:
//! - [`to_json`]: canonical, key-order-stable, non-finite-float-rejecting
//!   JSON serialization of a `Project`.
//! - [`save`] / [`load`]: atomic save and schema-gated load of a project
//!   directory.
//! - [`AssetStore`]: content-addressed storage for the asset files a
//!   project references.
#![forbid(unsafe_code)]

mod assets;
mod error;
mod json;
mod load;
mod save;

pub use assets::AssetStore;
pub use error::ProjectError;
pub use json::to_json;
pub use load::load;
pub use save::save;
