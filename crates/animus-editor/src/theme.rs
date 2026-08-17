//! Showmesh's design system, expressed as egui.
//!
//! Animus Live and Showmesh are for the same operator, in the same booth, on
//! the same night, and often on the same screen — so this is the *same*
//! system, lifted from `Showmesh/DESIGN.md` rather than reinterpreted. Spec
//! §10.6.
//!
//! ## The three rules that make it a system rather than a palette
//!
//! **The Signal Rule.** Saturated colour is state, never decoration. Green is
//! ready or armed, coral is live or destructive, cyan is data and I/O, amber
//! is caution, violet is grouping. If a pixel is saturated it is telling the
//! operator something true about the show right now.
//!
//! **Structure from veils, not borders.** Depth comes from three near-black
//! surfaces and white overlays at 3–7% alpha. `SEAM` is the only line the UI
//! draws.
//!
//! **The Readable Floor.** Anything the operator *reads* sits at [`DIM`] or
//! brighter. [`FAINT`] and [`GHOST`] are decoration — separators, carets,
//! dormant marks — never text carrying information.
//!
//! ## Where Animus extends it
//!
//! This tool draws things Showmesh has no equivalent for: mesh wireframe,
//! bones, joints, attachment radii. Those are **drawing, not state**, so they
//! take the ink ramp ([`gizmo`]), which leaves the signal colours free to
//! keep meaning exactly what they mean in Showmesh. Amber bones would look
//! good and would break the one rule the system rests on.

use egui::{Color32, CornerRadius, Stroke, Visuals};

// ── surfaces: three tonal depths ───────────────────────────────────────

pub const APP_BG: Color32 = Color32::from_rgb(0x0A, 0x0C, 0x0F);
pub const WORKSPACE: Color32 = Color32::from_rgb(0x14, 0x17, 0x1D);
pub const SIDE_PANEL: Color32 = Color32::from_rgb(0x0D, 0x0F, 0x13);
pub const MENU_BG: Color32 = Color32::from_rgb(0x17, 0x1B, 0x22);
/// The cool raised card a *live* object becomes.
pub const PLAYING_CARD: Color32 = Color32::from_rgb(0x1B, 0x21, 0x2C);
pub const STATUS_BG: Color32 = Color32::from_rgb(0x0C, 0x0E, 0x12);
/// Video and viewport backgrounds are true black, always.
pub const SCREEN_BLACK: Color32 = Color32::BLACK;

// ── white veils: the structure ─────────────────────────────────────────

/// White veils are written as premultiplied literals rather than through
/// `Color32::from_white_alpha`, which is not `const`. The values are
/// identical — `from_white_alpha(a)` is defined as `[a, a, a, a]` — so this
/// is a spelling change, not an approximation.
pub const WELL: Color32 = Color32::from_rgba_premultiplied(0x09, 0x09, 0x09, 0x09);
pub const WELL_HOVER: Color32 = Color32::from_rgba_premultiplied(0x0D, 0x0D, 0x0D, 0x0D);
/// The only line this UI draws.
pub const SEAM: Color32 = Color32::from_rgba_premultiplied(0x0F, 0x0F, 0x0F, 0x0F);
pub const ROW_SEP: Color32 = Color32::from_rgba_premultiplied(0x08, 0x08, 0x08, 0x08);
pub const TRACK_BG: Color32 = Color32::from_rgba_premultiplied(0x12, 0x12, 0x12, 0x12);

// ── the ink ramp ───────────────────────────────────────────────────────

pub const BRIGHT: Color32 = Color32::from_rgb(0xE9, 0xEC, 0xF0);
pub const INK: Color32 = Color32::from_rgb(0xDE, 0xE2, 0xE8);
pub const MID: Color32 = Color32::from_rgb(0xC6, 0xCB, 0xD3);
pub const SOFT: Color32 = Color32::from_rgb(0x9A, 0xA1, 0xAB);
pub const SUB: Color32 = Color32::from_rgb(0x8B, 0x93, 0xA0);
/// The readable floor. Nothing carrying information goes below this.
pub const DIM: Color32 = Color32::from_rgb(0x7A, 0x82, 0x8E);
pub const FAINT: Color32 = Color32::from_rgb(0x4E, 0x55, 0x5F);
pub const GHOST: Color32 = Color32::from_rgb(0x3F, 0x45, 0x4E);

// ── signals: state, never decoration ───────────────────────────────────

pub const GO_GREEN: Color32 = Color32::from_rgb(0x57, 0xC8, 0x78);
pub const GO_BUTTON: Color32 = Color32::from_rgb(0x2E, 0x9D, 0x57);
pub const LIVE_CORAL: Color32 = Color32::from_rgb(0xF2, 0x60, 0x6A);
pub const DATA_CYAN: Color32 = Color32::from_rgb(0x45, 0xC8, 0xE8);
pub const CAUTION_AMBER: Color32 = Color32::from_rgb(0xE3, 0xA9, 0x4F);
pub const GROUP_VIOLET: Color32 = Color32::from_rgb(0x8F, 0x8F, 0xFF);

// ── radii, by component rather than one global rounding ────────────────

pub const R_TRACK: u8 = 2;
pub const R_BADGE: u8 = 4;
pub const R_INPUT: u8 = 5;
pub const R_CHIP: u8 = 6;
pub const R_BUTTON: u8 = 7;
pub const R_MENU: u8 = 8;
pub const R_PANEL: u8 = 10;

// ── spacing scale ──────────────────────────────────────────────────────

pub const S_HAIR: f32 = 3.0;
pub const S_XS: f32 = 5.0;
pub const S_SM: f32 = 8.0;
pub const S_MD: f32 = 12.0;
pub const S_LG: f32 = 16.0;
pub const S_XL: f32 = 24.0;

/// Colours for the things only this tool draws.
///
/// Deliberately all from the ink ramp: a wireframe is not a state, and using
/// a signal colour for it would make every signal colour mean less.
pub mod gizmo {
    use super::*;

    /// Mesh wireframe. Decoration, so the bottom of the ramp.
    pub const WIREFRAME: Color32 = GHOST;
    /// Bones at rest.
    pub const BONE: Color32 = MID;
    /// Joints at rest.
    pub const JOINT: Color32 = BRIGHT;
    /// A pinned joint is drawn as a square rather than recoloured — form
    /// carries the state so colour does not have to.
    pub const PINNED: Color32 = DIM;
    /// Attachment radius fill, over the artwork.
    pub const RADIUS_FILL: Color32 = Color32::from_rgba_premultiplied(0x14, 0x14, 0x14, 0x14);
    /// Selection is a white veil, exactly as a selected row is.
    pub const SELECTION: Color32 = Color32::from_rgba_premultiplied(0x24, 0x24, 0x24, 0x24);

    /// A joint bound to a live channel. Cyan because it *is* I/O — the same
    /// meaning the colour carries everywhere else.
    pub const BOUND: Color32 = DATA_CYAN;
    /// A joint being driven right now: by a hand, a binding or a clip.
    pub const DRIVEN: Color32 = LIVE_CORAL;
}

/// Install the visuals on an egui context.
pub fn install(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();

    visuals.panel_fill = SIDE_PANEL;
    visuals.window_fill = MENU_BG;
    visuals.extreme_bg_color = SCREEN_BLACK;
    visuals.faint_bg_color = WELL;
    visuals.code_bg_color = WELL;
    visuals.window_stroke = Stroke::new(1.0_f32, SEAM);
    visuals.window_corner_radius = CornerRadius::same(R_MENU);
    visuals.menu_corner_radius = CornerRadius::same(R_MENU);
    visuals.selection.bg_fill = Color32::from_rgba_premultiplied(0x0D, 0x0D, 0x0D, 0x0D);
    visuals.selection.stroke = Stroke::new(1.0_f32, BRIGHT);
    visuals.hyperlink_color = DATA_CYAN;
    visuals.error_fg_color = LIVE_CORAL;
    visuals.warn_fg_color = CAUTION_AMBER;

    // Quiet until touched: transparent at rest, one veil step on hover,
    // never a border unless something is active.
    let w = &mut visuals.widgets;
    w.noninteractive.bg_fill = Color32::TRANSPARENT;
    w.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, ROW_SEP);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, SOFT);
    w.noninteractive.corner_radius = CornerRadius::same(R_INPUT);

    w.inactive.bg_fill = WELL;
    w.inactive.weak_bg_fill = WELL;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0_f32, INK);
    w.inactive.corner_radius = CornerRadius::same(R_INPUT);

    w.hovered.bg_fill = WELL_HOVER;
    w.hovered.weak_bg_fill = WELL_HOVER;
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.0_f32, BRIGHT);
    w.hovered.corner_radius = CornerRadius::same(R_INPUT);

    w.active.bg_fill = Color32::from_rgba_premultiplied(0x18, 0x18, 0x18, 0x18);
    w.active.weak_bg_fill = Color32::from_rgba_premultiplied(0x18, 0x18, 0x18, 0x18);
    w.active.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.0_f32, BRIGHT);
    w.active.corner_radius = CornerRadius::same(R_INPUT);

    w.open.bg_fill = MENU_BG;
    w.open.weak_bg_fill = MENU_BG;
    w.open.bg_stroke = Stroke::new(1.0_f32, SEAM);
    w.open.fg_stroke = Stroke::new(1.0_f32, INK);
    w.open.corner_radius = CornerRadius::same(R_MENU);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(S_SM, S_SM);
    style.spacing.button_padding = egui::vec2(S_SM, S_XS);
    style.spacing.menu_margin = egui::Margin::same(6);
    style.spacing.indent = S_MD;
    style.spacing.slider_width = 140.0;
    style.spacing.interact_size.y = 22.0;
    style.visuals.striped = false;
    ctx.set_global_style(style);
}

/// Fonts.
///
/// **Not yet shipped, deliberately, and this is the note that stops it being
/// forgotten.** The system specifies Inter for anything a human wrote and
/// JetBrains Mono for anything the engine produced — the Mono-Means-Machine
/// rule — and until the files are vendored, egui's built-in faces stand in.
/// Both intended faces are SIL OFL, which is compatible with this project's
/// `MIT OR Apache-2.0` code licence provided the font licence ships beside
/// them, so the remaining work is dropping two `.ttf` files into
/// `crates/animus-editor/assets/fonts/` and calling this from `install`.
///
/// It is left undone rather than half-done because a silent fallback to a
/// different metric is worse than a known one: the layout is being tuned
/// against whatever this function installs.
pub fn install_fonts(_ctx: &egui::Context) {
    // Intentionally empty. See the doc comment above.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Readable Floor, as arithmetic rather than as good intentions.
    ///
    /// Contrast against `WORKSPACE`, which is the surface most text sits on.
    fn contrast(fg: Color32, bg: Color32) -> f32 {
        fn channel(c: u8) -> f32 {
            let c = c as f32 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        fn luminance(c: Color32) -> f32 {
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        }
        let (a, b) = (luminance(fg), luminance(bg));
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn every_ink_above_the_floor_is_actually_readable() {
        for (name, c) in [
            ("BRIGHT", BRIGHT),
            ("INK", INK),
            ("MID", MID),
            ("SOFT", SOFT),
            ("SUB", SUB),
            ("DIM", DIM),
        ] {
            let ratio = contrast(c, WORKSPACE);
            assert!(
                ratio >= 4.5,
                "{name} is {ratio:.2}:1 on the workspace surface, below the 4.5:1 floor"
            );
        }
    }

    #[test]
    fn faint_and_ghost_are_below_the_floor_which_is_why_they_are_decoration_only() {
        // Not a defect — this asserts they cannot be mistaken for readable
        // ink, so a future edit that promotes one to a label gets caught.
        assert!(contrast(FAINT, WORKSPACE) < 4.5);
        assert!(contrast(GHOST, WORKSPACE) < 4.5);
    }

    #[test]
    fn gizmo_colours_never_borrow_a_signal_colour() {
        // The Signal Rule, enforced. A wireframe is not a state.
        for c in [gizmo::WIREFRAME, gizmo::BONE, gizmo::JOINT, gizmo::PINNED] {
            assert!(
                ![GO_GREEN, GO_BUTTON, LIVE_CORAL, CAUTION_AMBER, GROUP_VIOLET].contains(&c),
                "{c:?} is a signal colour being used as decoration"
            );
        }
        // The two exceptions are deliberate and mean what they say.
        assert_eq!(gizmo::BOUND, DATA_CYAN, "a bound joint is I/O");
        assert_eq!(gizmo::DRIVEN, LIVE_CORAL, "a driven joint is live");
    }
}
