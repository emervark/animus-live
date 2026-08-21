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
/// Between [`MID`] and [`SOFT`]: the comp's `--sm-text-3`, used for the
/// second line of a two-line row where [`SOFT`] is already the first.
pub const MUTED: Color32 = Color32::from_rgb(0xAE, 0xB5, 0xBF);
/// Below [`DIM`] but still legible: hints under a control, ruler numbers
/// that are not on a downbeat.
pub const HINT: Color32 = Color32::from_rgb(0x6B, 0x72, 0x7D);
/// A control that is present and cannot be used. Distinct from [`FAINT`]:
/// faint is quiet, disabled is *off*, and reading one as the other is how
/// an operator ends up clicking at something that will never respond.
pub const DISABLED: Color32 = Color32::from_rgb(0x5E, 0x66, 0x72);

// ── signals: state, never decoration ───────────────────────────────────

pub const GO_GREEN: Color32 = Color32::from_rgb(0x57, 0xC8, 0x78);
pub const GO_BUTTON: Color32 = Color32::from_rgb(0x2E, 0x9D, 0x57);
pub const GO_VIVID: Color32 = Color32::from_rgb(0x32, 0xC9, 0x6B);
pub const LIVE_CORAL: Color32 = Color32::from_rgb(0xF2, 0x60, 0x6A);
pub const DATA_CYAN: Color32 = Color32::from_rgb(0x45, 0xC8, 0xE8);
pub const CAUTION_AMBER: Color32 = Color32::from_rgb(0xE3, 0xA9, 0x4F);
pub const GROUP_VIOLET: Color32 = Color32::from_rgb(0x8F, 0x8F, 0xFF);
/// The selection blue. Nothing else in the palette means "this one".
pub const SELECT: Color32 = Color32::from_rgb(0x4F, 0xA0, 0xDC);

// ── washes: a signal's own quiet surface ───────────────────────────────
//
// A wash is the signal colour at low alpha, used to tint the row or chip a
// state belongs to. It is what lets a muted track and a soloed track differ
// at a glance without either of them shouting: the ink says which signal,
// the wash says how much of the row it owns.

// Premultiplied, like the veils above and for the same reason:
// `from_rgba_unmultiplied` is not `const`. Each is the comp's colour at the
// comp's alpha, multiplied through and rounded.
pub const GO_WASH: Color32 = Color32::from_rgba_premultiplied(0x0C, 0x1C, 0x11, 0x24);
pub const STOP_WASH: Color32 = Color32::from_rgba_premultiplied(0x19, 0x0A, 0x0B, 0x1A);
pub const STOP_BORDER: Color32 = Color32::from_rgba_premultiplied(0x61, 0x26, 0x2A, 0x66);
/// The one opaque wash: a recording surface has to stay legible over
/// whatever it sits on, so it is a colour rather than a veil.
pub const STOP_SURFACE: Color32 = Color32::from_rgb(0x1A, 0x12, 0x15);
pub const WARN_WASH: Color32 = Color32::from_rgba_premultiplied(0x17, 0x11, 0x08, 0x1A);
pub const LIVE_WASH: Color32 = Color32::from_rgba_premultiplied(0x0A, 0x1C, 0x21, 0x24);
pub const SELECT_WASH: Color32 = Color32::from_rgba_premultiplied(0x0D, 0x1A, 0x23, 0x29);

// ── radii ──────────────────────────────────────────────────────────────
//
// Straight from `tokens/spacing.css`. **Five is the default control
// radius**, not seven: buttons drawn at seven read as a slightly different
// product from every other surface, which is exactly the "the buttons are
// all wrong" that eyeballing produces.

/// Chips, pills, tick marks.
pub const R_TRACK: u8 = 2;
/// Small badges.
pub const R_BADGE: u8 = 4;
/// **Buttons and inputs — the default.**
pub const R_CONTROL: u8 = 5;
pub const R_CHIP: u8 = 6;
/// Cards.
pub const R_CARD: u8 = 8;
/// Panels and other large surfaces.
pub const R_PANEL: u8 = 10;

// Old names, kept so the sweep can happen in one place rather than in every
// call site at once. Both now resolve to the token value.
pub const R_INPUT: u8 = R_CONTROL;
pub const R_BUTTON: u8 = R_CONTROL;
pub const R_MENU: u8 = R_CARD;

// ── spacing ────────────────────────────────────────────────────────────
//
// A ~2px grid with 8 as the base rhythm, from `tokens/spacing.css`. The
// previous 3 and 5 were invented and landed between grid steps, which is
// why dense rows never quite aligned with each other.

pub const S_HAIR: f32 = 2.0;
pub const S_XS: f32 = 4.0;
pub const S_2XS: f32 = 6.0;
/// The base rhythm: default gap and padding.
pub const S_SM: f32 = 8.0;
pub const S_MD: f32 = 12.0;
pub const S_LG: f32 = 16.0;
pub const S_XL: f32 = 18.0;

// ── type scale ─────────────────────────────────────────────────────────
//
// From `tokens/typography.css`. Deliberately small and dense. Every size in
// this editor should come from here; a number typed at a call site is a
// number that will not match the one three rows above it.

/// Smallest tags and unit suffixes.
pub const FS_MICRO: f32 = 8.0;
/// Uppercase micro-labels and chips.
pub const FS_TINY: f32 = 9.0;
/// Control labels and secondary meta.
pub const FS_LABEL: f32 = 10.0;
/// Dense controls.
pub const FS_SM: f32 = 11.0;
/// Dense control labels.
///
/// **Not on the token scale**, which jumps 11 → 13. The comp itself sets
/// `font-size:12px` on tool rows, clip names and stage tabs, so this names
/// the deviation rather than rounding it away: at 11 the rows read as
/// captions, at 13 a sidebar of them stops fitting.
pub const FS_CONTROL: f32 = 12.0;
/// Body default.
pub const FS_BASE: f32 = 13.0;
/// Section titles.
pub const FS_MD: f32 = 14.0;
/// Prominent values.
pub const FS_LG: f32 = 15.0;
/// Transport timecode and hero numerics.
pub const FS_DISPLAY: f32 = 22.0;

/// Tracking for the uppercase 9–10px labels, in egui's `extra_letter_spacing`.
pub const TRACKING_LABEL: f32 = 0.5;

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
    /// Joints at rest: red, and its own red.
    ///
    /// A joint is the one thing in the viewport a hand reaches for, and pale
    /// ink over pale artwork disappeared — so it is red, at the operator's
    /// request. Deliberately *not* [`LIVE_CORAL`]: the Signal Rule reserves
    /// coral for live, and a joint sitting at rest is not live. The duller
    /// red leaves the brighter coral of [`DRIVEN`] a step to climb when the
    /// joint actually starts moving.
    pub const JOINT: Color32 = Color32::from_rgb(0xC8, 0x45, 0x45);
    /// A pinned joint is drawn as a square rather than recoloured — form
    /// carries the state so colour does not have to.
    pub const PINNED: Color32 = DIM;
    /// Attachment radius fill, over the artwork.
    pub const RADIUS_FILL: Color32 = Color32::from_rgba_premultiplied(0x14, 0x14, 0x14, 0x14);
    /// Selection is a white veil, exactly as a selected row is.
    pub const SELECTION: Color32 = Color32::from_rgba_premultiplied(0x24, 0x24, 0x24, 0x24);
    /// The ring around the selected joint. Opaque, because a veil over
    /// artwork is not an answer to "did my click land?".
    pub const SELECTED_RING: Color32 = BRIGHT;

    /// The stage edge. Furniture, so the very bottom of the ink ramp: it has
    /// to be findable when looked for and invisible when not.
    pub const STAGE_FRAME: Color32 = Color32::from_rgb(0x3A, 0x40, 0x49);
    /// The title-safe inset, a touch brighter than the frame it sits inside
    /// so the two do not read as one thick dashed line.
    pub const SAFE_FRAME: Color32 = Color32::from_rgb(0x59, 0x61, 0x6C);

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

    // Type comes from the scale, once, so a widget that does not override it
    // is already right. Every hand-typed size at a call site was a number
    // that did not match the one three rows above it.
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(FS_MD, FontFamily::Proportional),
        ),
        (
            TextStyle::Body,
            FontId::new(FS_SM, FontFamily::Proportional),
        ),
        (
            TextStyle::Button,
            FontId::new(FS_SM, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(FS_LABEL, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(FS_LABEL, FontFamily::Monospace),
        ),
    ]
    .into();

    // Dense, on the 2px grid.
    style.spacing.item_spacing = egui::vec2(S_2XS, S_XS);
    style.spacing.button_padding = egui::vec2(S_SM, S_XS);
    style.spacing.menu_margin = egui::Margin::same(S_XS as i8);
    style.spacing.indent = S_MD;
    style.spacing.interact_size.y = 22.0;

    // Sliders: a hairline rail with a small handle, as the comp draws them.
    // egui's defaults are a thick rail and a wide grab handle, which is the
    // single loudest thing in a panel that is meant to be quiet.
    style.spacing.slider_width = 120.0;
    style.spacing.slider_rail_height = 4.0;
    style.visuals.slider_trailing_fill = true;
    style.visuals.handle_shape = egui::style::HandleShape::Circle;

    // Menus and popovers get the one shadow the system allows.
    style.visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(153),
    };
    style.visuals.window_shadow = style.visuals.popup_shadow;

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
