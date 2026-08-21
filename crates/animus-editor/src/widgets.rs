//! Controls drawn to the comp, rather than egui's defaults restyled.
//!
//! The design system ships tokens and no components — its own `styles.css`
//! says so — so the components live in the comp as inline styles, and porting
//! them means reading those styles and painting them. Restyling egui's
//! `Slider` gets close and stays wrong in the ways that are most visible: a
//! rail twice the specified height, a hollow handle where the comp has a
//! filled one, and the value crammed inside the track instead of standing in
//! its own field.
//!
//! Every number here is from `Animus Live - Perform.dc.html`.

use bevy_egui::egui;
use std::ops::RangeInclusive;

use crate::icons;
use crate::theme;

/// Label, value field, and a track under it.
///
/// ```text
///  Spring strength      modified  ↺  [ 0.72 ]
///  ▬▬▬▬▬▬▬▬▬▬▬▬▬●───────────────────────────
/// ```
///
/// The value sits in its own field rather than inside the track because the
/// operator reads it and edits it at different moments: reading happens while
/// dragging, editing happens when they already know the number they want.
pub struct SliderRow<'a> {
    label: &'a str,
    value: &'a mut f32,
    range: RangeInclusive<f32>,
    suffix: &'a str,
    decimals: usize,
    default: Option<f32>,
}

impl<'a> SliderRow<'a> {
    pub fn new(label: &'a str, value: &'a mut f32, range: RangeInclusive<f32>) -> Self {
        Self {
            label,
            value,
            range,
            suffix: "",
            decimals: 3,
            default: None,
        }
    }

    pub fn suffix(mut self, suffix: &'a str) -> Self {
        self.suffix = suffix;
        self
    }

    pub fn decimals(mut self, n: usize) -> Self {
        self.decimals = n;
        self
    }

    /// Show a "modified" tag and a reset button when the value has been moved
    /// away from this default.
    ///
    /// Worth the two extra controls: a solver parameter that has been nudged
    /// is invisible otherwise, and "why does this puppet behave differently
    /// from the other one" is a question with no answer on screen.
    pub fn default_value(mut self, v: f32) -> Self {
        self.default = Some(v);
        self
    }

    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        // Comp: label row, 6px gap, 3px track. The track's hit area is taller
        // than the track so it can be grabbed — a 3px target is a target for
        // nobody.
        const TRACK: f32 = 3.0;
        const HANDLE: f32 = 12.0;
        const GAP: f32 = 6.0;

        let Self {
            label,
            value,
            range,
            suffix,
            decimals,
            default,
        } = self;

        let start = *value;
        let width = ui.available_width();
        let mut changed = false;

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = GAP;

            // ── label row ──
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(label)
                        .size(theme::FS_SM)
                        .color(theme::MID),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    value_field(ui, *value, decimals, suffix);

                    if let Some(d) = default
                        && (*value - d).abs() > f32::EPSILON.max(d.abs() * 1e-4)
                    {
                        if icons::button(ui, icons::Icon::Reset, false, Some(theme::SUB))
                            .on_hover_text(format!("Reset to {d:.decimals$}"))
                            .clicked()
                        {
                            *value = d;
                            changed = true;
                        }
                        ui.label(
                            egui::RichText::new("modified")
                                .monospace()
                                .size(theme::FS_TINY)
                                .color(theme::CAUTION_AMBER),
                        );
                    }
                });
            });

            // ── track ──
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(width, HANDLE), egui::Sense::click_and_drag());

            let lo = *range.start();
            let hi = *range.end();
            let span = (hi - lo).max(f32::EPSILON);

            if response.is_pointer_button_down_on()
                && let Some(p) = response.interact_pointer_pos()
            {
                // Inset by the handle radius so the ends are reachable: with
                // the raw rect, the value can never quite get to either end
                // because the handle centre stops at the edge.
                let usable = rect.shrink2(egui::vec2(HANDLE * 0.5, 0.0));
                let t = ((p.x - usable.min.x) / usable.width().max(1.0)).clamp(0.0, 1.0);
                let next = lo + t * span;
                if next != *value {
                    *value = next;
                    changed = true;
                }
            }

            if ui.is_rect_visible(rect) {
                let t = ((*value - lo) / span).clamp(0.0, 1.0);
                let usable = rect.shrink2(egui::vec2(HANDLE * 0.5, 0.0));
                let cx = usable.min.x + usable.width() * t;
                let cy = rect.center().y;
                let p = ui.painter();

                let rail = egui::Rect::from_center_size(
                    egui::pos2(rect.center().x, cy),
                    egui::vec2(rect.width(), TRACK),
                );
                p.rect_filled(rail, 2.0, theme::TRACK_BG);

                // **The fill starts where the value's neutral is.** On a
                // range that spans zero — a rotation dial, a sideways force
                // — filling from the far left makes 0 look like "half way
                // up" instead of "none", and the operator reads a centred
                // handle as a value they have already set.
                let origin_x = if lo < 0.0 && hi > 0.0 {
                    usable.min.x + usable.width() * ((0.0 - lo) / span)
                } else {
                    rail.min.x
                };
                let mut filled = rail;
                filled.min.x = origin_x.min(cx);
                filled.max.x = origin_x.max(cx);
                if filled.max.x > filled.min.x {
                    p.rect_filled(filled, 2.0, theme::GO_GREEN);
                }

                // Filled, not a ring. The comp's handle is a solid disc, and
                // a ring at 12px reads as an empty slot rather than a grip.
                p.circle_filled(
                    egui::pos2(cx, cy),
                    HANDLE * 0.5,
                    if response.hovered() {
                        theme::BRIGHT
                    } else {
                        theme::MID
                    },
                );
            }
        });

        let mut response = ui.interact(
            ui.min_rect(),
            ui.id().with(("slider_row", label)),
            egui::Sense::hover(),
        );
        if changed || *value != start {
            response.mark_changed();
        }
        response
    }
}

/// The boxed monospace readout from the comp: mono, inked, on the raised
/// surface, hairline border, 4px radius.
fn value_field(ui: &mut egui::Ui, value: f32, decimals: usize, suffix: &str) {
    // The suffix carries its own spacing: a unit reads as "1.00 kg" and a
    // degree sign reads as "12°", and no rule about spaces gets both right.
    let text = format!("{value:.decimals$}{suffix}");
    let font = egui::FontId::monospace(theme::FS_LABEL);
    let galley = ui.painter().layout_no_wrap(text, font, theme::INK);
    let size = galley.size() + egui::vec2(theme::S_MD, theme::S_XS);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, theme::R_BADGE, theme::MENU_BG);
        p.rect_stroke(
            rect,
            theme::R_BADGE,
            egui::Stroke::new(1.0_f32, theme::SEAM),
            egui::StrokeKind::Inside,
        );
        p.galley(
            rect.min + egui::vec2(theme::S_2XS, theme::S_HAIR),
            galley,
            theme::INK,
        );
    }
}

// ── the inspector's own furniture ──────────────────────────────────────
//
// The comp gives the inspector a vocabulary the rest of the editor does
// not use: a breadcrumb, read-only value fields in boxes, a pill switch,
// hairline dividers between sections. Each is a handful of lines and each
// is drawn from the comp's numbers rather than approximated with an egui
// default that is close.

/// `Main Character / Rig / chest`, with the last segment lit.
///
/// It answers "what am I looking at" without the operator having to trace
/// the selection back through three panels — the inspector is often the
/// only part of the screen they are watching.
pub fn breadcrumb(ui: &mut egui::Ui, trail: &[&str]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = theme::S_HAIR + 1.0;
        let last = trail.len().saturating_sub(1);
        for (i, part) in trail.iter().enumerate() {
            if i > 0 {
                ui.label(
                    egui::RichText::new("/")
                        .monospace()
                        .size(theme::FS_LABEL)
                        .color(theme::GHOST),
                );
            }
            ui.label(
                egui::RichText::new(*part)
                    .monospace()
                    .size(theme::FS_LABEL)
                    .color(if i == last { theme::BRIGHT } else { theme::DIM }),
            );
        }
    });
}

/// The uppercase section heading: `JOINT`, `BEHAVIOUR`, `LIVE`.
pub fn section_label(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .size(theme::FS_LABEL)
            .color(theme::FAINT)
            .strong(),
    );
}

/// A hairline between sections, full width.
pub fn divider(ui: &mut egui::Ui) {
    ui.add_space(theme::S_XS);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::ROW_SEP);
    ui.add_space(theme::S_XS);
}

/// The width the inspector's field labels share, from the comp.
const LABEL_W: f32 = 64.0;

/// Paint one boxed read-only value, optionally with a mono prefix like `X`.
fn value_box(ui: &mut egui::Ui, width: f32, prefix: Option<&str>, value: &str) {
    let height = theme::FS_SM + theme::S_SM + 1.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, theme::R_CONTROL as f32, theme::MENU_BG);
    p.rect_stroke(
        rect,
        theme::R_CONTROL as f32,
        egui::Stroke::new(1.0_f32, theme::SEAM),
        egui::StrokeKind::Inside,
    );

    let mut x = rect.min.x + theme::S_2XS + 1.0;
    if let Some(prefix) = prefix {
        let galley = p.layout_no_wrap(
            prefix.to_string(),
            egui::FontId::monospace(theme::FS_TINY),
            theme::FAINT,
        );
        p.galley(
            egui::pos2(x, rect.center().y - galley.size().y * 0.5),
            galley.clone(),
            theme::FAINT,
        );
        x += galley.size().x + theme::S_XS + 1.0;
    }
    let ink = if prefix.is_some() {
        theme::MID
    } else {
        theme::INK
    };
    let galley = p.layout_no_wrap(
        value.to_string(),
        egui::FontId::monospace(theme::FS_SM),
        ink,
    );
    p.galley(
        egui::pos2(x, rect.center().y - galley.size().y * 0.5),
        galley,
        ink,
    );
}

/// `Name   [ chest ]` — a label and one read-only field.
pub fn field_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            egui::vec2(LABEL_W, theme::FS_CONTROL),
            egui::Label::new(
                egui::RichText::new(label)
                    .size(theme::FS_SM + 0.5)
                    .color(theme::SUB),
            )
            .selectable(false),
        );
        value_box(ui, ui.available_width(), None, value);
    });
}

/// `Position  [ X 10.75 ] [ Y 6.20 ]` — two fields sharing the row.
pub fn vec_row(ui: &mut egui::Ui, label: &str, x: f32, y: f32) {
    ui.horizontal(|ui| {
        ui.add_sized(
            egui::vec2(LABEL_W, theme::FS_CONTROL),
            egui::Label::new(
                egui::RichText::new(label)
                    .size(theme::FS_SM + 0.5)
                    .color(theme::SUB),
            )
            .selectable(false),
        );
        let each = (ui.available_width() - theme::S_XS - 1.0) * 0.5;
        value_box(ui, each, Some("X"), &format!("{x:.2}"));
        value_box(ui, each, Some("Y"), &format!("{y:.2}"));
    });
}

/// A pill switch: `Pin  (●——)  Free — follows physics`.
///
/// A switch rather than egui's checkbox because the comp draws one, and
/// because pinning is a state the puppet is *in* rather than an option
/// ticked — the track carries the colour, so it reads at a glance from
/// across a room, which is where an operator running a show is standing.
pub fn toggle_row(ui: &mut egui::Ui, label: &str, on: &mut bool, caption: &str) -> egui::Response {
    let response = ui
        .horizontal(|ui| {
            ui.add_sized(
                egui::vec2(LABEL_W, theme::FS_CONTROL),
                egui::Label::new(
                    egui::RichText::new(label)
                        .size(theme::FS_SM + 0.5)
                        .color(theme::SUB),
                )
                .selectable(false),
            );

            let (rect, mut response) =
                ui.allocate_exact_size(egui::vec2(32.0, 18.0), egui::Sense::click());
            if response.clicked() {
                *on = !*on;
                response.mark_changed();
            }

            let p = ui.painter();
            p.rect_filled(
                rect,
                9.0,
                if *on {
                    theme::GO_BUTTON
                } else {
                    theme::TRACK_BG
                },
            );
            let knob = 14.0;
            let cx = if *on {
                rect.max.x - 2.0 - knob * 0.5
            } else {
                rect.min.x + 2.0 + knob * 0.5
            };
            p.circle_filled(
                egui::pos2(cx, rect.center().y),
                knob * 0.5,
                if *on { theme::BRIGHT } else { theme::SOFT },
            );

            ui.add_space(theme::S_SM);
            ui.label(
                egui::RichText::new(caption)
                    .size(theme::FS_SM)
                    .color(theme::DIM),
            );
            response
        })
        .inner;
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, label));
    response
}

/// The LIVE panel's read-out: a label and a coloured value on a soft well.
pub fn readout_row(ui: &mut egui::Ui, label: &str, value: &str, ink: egui::Color32) {
    let height = theme::FS_CONTROL + theme::S_MD + 1.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let p = ui.painter();
    p.rect_filled(rect, theme::R_CHIP as f32, theme::WELL);

    let left = p.layout_no_wrap(
        label.to_string(),
        egui::FontId::proportional(theme::FS_SM + 0.5),
        theme::MID,
    );
    p.galley(
        egui::pos2(
            rect.min.x + theme::S_SM + 1.0,
            rect.center().y - left.size().y * 0.5,
        ),
        left,
        theme::MID,
    );

    let right = p.layout_no_wrap(
        value.to_string(),
        egui::FontId::monospace(theme::FS_SM - 0.5),
        ink,
    );
    p.galley(
        egui::pos2(
            rect.max.x - theme::S_SM - 1.0 - right.size().x,
            rect.center().y - right.size().y * 0.5,
        ),
        right,
        ink,
    );
}

/// A full-width secondary button, the comp's outlined kind.
pub fn wide_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .size(theme::FS_CONTROL)
                .color(theme::INK),
        )
        .fill(theme::WELL)
        .stroke(egui::Stroke::new(1.0_f32, theme::SEAM))
        .corner_radius(theme::R_CHIP + 1)
        .min_size(egui::vec2(
            ui.available_width(),
            theme::FS_CONTROL + theme::S_MD + 6.0,
        )),
    )
}

/// The small explanatory paragraph under a control.
pub fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme::FS_SM)
            .color(theme::DIM),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The end of the range has to be reachable.
    ///
    /// Without the handle-radius inset, the pointer can never drive the value
    /// to either end: the handle centre stops half a handle short, and a
    /// damping slider that will not reach 1.0 is a slider that cannot express
    /// "no damping at all".
    #[test]
    fn both_ends_of_the_range_are_reachable() {
        const HANDLE: f32 = 12.0;
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, HANDLE));
        let usable = rect.shrink2(egui::vec2(HANDLE * 0.5, 0.0));

        let t_at = |x: f32| ((x - usable.min.x) / usable.width().max(1.0)).clamp(0.0, 1.0);
        assert_eq!(t_at(rect.min.x), 0.0, "the far left reaches the minimum");
        assert_eq!(
            t_at(rect.max.x),
            1.0,
            "and the far right reaches the maximum"
        );
        assert!(
            (t_at(rect.center().x) - 0.5).abs() < 1e-6,
            "centre is centre"
        );
    }
}
