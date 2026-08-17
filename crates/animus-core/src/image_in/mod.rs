//! Getting an image into the document, and the hinges that make adding
//! formats later a small change.
//!
//! **M1 reads PNG with an alpha channel and nothing else.** Background
//! removal happens in whatever tool the operator already uses. That is a
//! deliberate scope decision (spec §13 planning notes, plan
//! `2026-08-17-m1-2d-vertical-slice.md`), taken because the milestone has to
//! prove that image → puppet → projector works end to end, and background
//! removal is a whole feature with its own failure modes.
//!
//! The requirement attached to that decision is the reason this module
//! exists as a module rather than as three lines at a call site. Adding a
//! format later must be a small change, so:
//!
//! 1. **One decode table.** Every byte that becomes an image passes through
//!    [`decode`]. Adding AVIF or JPEG is a feature flag and one match arm,
//!    not a hunt for scattered `image::open` calls.
//! 2. **The alpha source is a document field from day one.** [`MatteParams`]
//!    is serialized in the v1 format with a single `mode` value, so a second
//!    mode later is a new *value* in an existing field — additive, and no
//!    migration.
//! 3. **The rejection message is the seam.** An opaque image is refused with
//!    a sentence that names the fix, and that is exactly where a future
//!    matte step gets offered instead.

mod decode;
mod matte;

pub use decode::{ImageFormat, ImportError, decode};
pub use matte::{MatteReport, is_effectively_opaque, resolve_alpha};
