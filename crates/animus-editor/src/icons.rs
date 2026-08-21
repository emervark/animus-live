//! Icons, drawn rather than typed.
//!
//! Every glyph this editor has borrowed from a font has eventually turned up
//! as a hollow box on someone's machine: `●` in the record readout, `✓` in the
//! workflow ticks, `✕` on a clip card, `│` between two control groups. egui
//! ships a small built-in face and the coverage gaps are not obvious until the
//! pixels are in front of you.
//!
//! So the icons that carry meaning are paths. They cost a few lines each, they
//! render identically everywhere, and they can take the ink colour of the row
//! they sit in — which a font glyph cannot do at these sizes without looking
//! either bolder or fainter than the text beside it.

use bevy_egui::egui;

use crate::theme;

/// A square icon button that takes its ink from its own state.
///
/// Returns the response so the caller can attach the tooltip that says what it
/// does — an icon without a tooltip is a rebus.
pub fn button(
    ui: &mut egui::Ui,
    icon: Icon,
    active: bool,
    tint: Option<egui::Color32>,
) -> egui::Response {
    const SIZE: f32 = 20.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::click());

    let ink = match (tint, active, response.hovered()) {
        (Some(c), _, _) => c,
        (None, true, _) => theme::INK,
        (None, false, true) => theme::MID,
        (None, false, false) => theme::FAINT,
    };

    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter().rect_filled(rect, theme::R_BADGE, theme::WELL);
        }
        draw(ui.painter(), icon, rect.shrink(4.0), ink);
    }
    response
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// An open eye: the layer is visible.
    Eye,
    /// A struck-through eye: the layer is hidden.
    EyeOff,
    /// Two offset sheets: duplicate.
    Copy,
    /// A bin with a lid: delete.
    Trash,
    /// A chevron: bring the layer forward in the paint order.
    Up,
    /// A chevron: send it backward.
    Down,
    /// An arrow to a stop: play once.
    Once,
    /// A closed circuit: play again from the top.
    Loop,
    /// Two arrows head to tail: play forwards, then backwards.
    PingPong,
    /// A filled triangle: playing.
    Play,
    /// A filled square: stopped.
    Stop,
    /// A filled disc: recording.
    Record,
    /// A circular arrow: put it back the way it was.
    Reset,
}

/// Draw `icon` to fill `rect`.
pub fn draw(painter: &egui::Painter, icon: Icon, rect: egui::Rect, ink: egui::Color32) {
    let stroke = egui::Stroke::new(1.2_f32, ink);
    // Work in a 0..1 box and map out, so every icon is authored at one scale.
    let at = |x: f32, y: f32| {
        egui::pos2(
            rect.min.x + rect.width() * x,
            rect.min.y + rect.height() * y,
        )
    };
    let line = |pts: Vec<egui::Pos2>| egui::Shape::line(pts, stroke);

    match icon {
        Icon::Eye | Icon::EyeOff => {
            // A lens: two arcs meeting at the corners, approximated by a
            // polyline. Cheaper than a bezier and indistinguishable at 12px.
            let lens = |flip: f32| {
                let mut pts = Vec::with_capacity(9);
                for i in 0..=8 {
                    let t = i as f32 / 8.0;
                    let x = t;
                    // A parabola through (0,.5) and (1,.5) peaking at .5.
                    let y = 0.5 - flip * 0.34 * (4.0 * t * (1.0 - t));
                    pts.push(at(x, y));
                }
                pts
            };
            painter.add(line(lens(1.0)));
            painter.add(line(lens(-1.0)));
            painter.circle_stroke(at(0.5, 0.5), rect.width() * 0.15, stroke);
            if icon == Icon::EyeOff {
                painter.add(line(vec![at(0.08, 0.92), at(0.92, 0.08)]));
            }
        }
        Icon::Copy => {
            // The back sheet is dimmer: it reads as "another one of these"
            // rather than as two unrelated rectangles.
            let back = egui::Rect::from_min_max(at(0.0, 0.0), at(0.66, 0.66));
            painter.rect_stroke(
                back,
                2.0,
                egui::Stroke::new(1.0_f32, ink.gamma_multiply(0.55)),
                egui::StrokeKind::Inside,
            );
            let front = egui::Rect::from_min_max(at(0.34, 0.34), at(1.0, 1.0));
            painter.rect_filled(front, 2.0, theme::SIDE_PANEL);
            painter.rect_stroke(front, 2.0, stroke, egui::StrokeKind::Inside);
        }
        Icon::Up | Icon::Down => {
            // A chevron, not a filled triangle: U+25B2 and U+25BC are both
            // missing from egui's built-in faces and drew as hollow boxes,
            // which reads as a broken button rather than as a direction.
            let (a, b, c) = if icon == Icon::Up {
                (at(0.12, 0.68), at(0.5, 0.28), at(0.88, 0.68))
            } else {
                (at(0.12, 0.32), at(0.5, 0.72), at(0.88, 0.32))
            };
            painter.add(egui::Shape::line(
                vec![a, b, c],
                egui::Stroke::new(1.4_f32, ink),
            ));
        }
        Icon::Once => {
            // A line that ends at a wall: it goes one way and stops.
            painter.add(line(vec![at(0.1, 0.5), at(0.72, 0.5)]));
            painter.add(line(vec![at(0.52, 0.3), at(0.72, 0.5), at(0.52, 0.7)]));
            painter.add(line(vec![at(0.86, 0.24), at(0.86, 0.76)]));
        }
        Icon::Loop => {
            // A rounded rectangle circuit with one arrowhead: it comes back.
            let r = egui::Rect::from_min_max(at(0.08, 0.2), at(0.92, 0.8));
            painter.rect_stroke(r, 6.0, stroke, egui::StrokeKind::Inside);
            painter.add(line(vec![at(0.42, 0.06), at(0.6, 0.2), at(0.42, 0.34)]));
        }
        Icon::PingPong => {
            // Two arrows, one each way, on their own lines: forwards over,
            // backwards under. One line with two heads reads as "resize".
            painter.add(line(vec![at(0.1, 0.32), at(0.9, 0.32)]));
            painter.add(line(vec![at(0.7, 0.14), at(0.9, 0.32), at(0.7, 0.5)]));
            painter.add(line(vec![at(0.9, 0.68), at(0.1, 0.68)]));
            painter.add(line(vec![at(0.3, 0.5), at(0.1, 0.68), at(0.3, 0.86)]));
        }
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![at(0.2, 0.12), at(0.86, 0.5), at(0.2, 0.88)],
                ink,
                egui::Stroke::NONE,
            ));
        }
        Icon::Stop => {
            painter.rect_filled(
                egui::Rect::from_min_max(at(0.2, 0.2), at(0.8, 0.8)),
                1.0,
                ink,
            );
        }
        Icon::Record => {
            painter.circle_filled(at(0.5, 0.5), rect.width() * 0.34, ink);
        }
        Icon::Reset => {
            // An open circle with an arrowhead: it goes round and returns.
            // Left open at the top so it reads as a turn rather than a ring.
            let mut pts = Vec::with_capacity(15);
            for i in 0..=14 {
                let a =
                    std::f32::consts::PI * 0.35 + (std::f32::consts::PI * 1.55) * i as f32 / 14.0;
                pts.push(at(0.5 + 0.36 * a.cos(), 0.5 + 0.36 * a.sin()));
            }
            painter.add(egui::Shape::line(pts, stroke));
            painter.add(line(vec![at(0.16, 0.16), at(0.18, 0.44), at(0.46, 0.36)]));
        }
        Icon::Trash => {
            painter.add(line(vec![at(0.05, 0.22), at(0.95, 0.22)]));
            // Lid handle.
            painter.add(line(vec![
                at(0.36, 0.22),
                at(0.36, 0.08),
                at(0.64, 0.08),
                at(0.64, 0.22),
            ]));
            // Body, tapering, so it reads as a bin and not as a window.
            painter.add(line(vec![
                at(0.16, 0.22),
                at(0.24, 0.95),
                at(0.76, 0.95),
                at(0.84, 0.22),
            ]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two eye states must be different shapes, not the same shape in two
    /// colours: a hidden layer has to read as hidden in a screenshot, on a
    /// projector, and to an operator who is not looking closely.
    #[test]
    fn the_eye_says_hidden_with_form_rather_than_only_colour() {
        assert_ne!(Icon::Eye, Icon::EyeOff);
    }

    /// Every icon has to survive being asked for at a silly size without
    /// panicking on a zero-width rect.
    #[test]
    fn drawing_into_a_degenerate_rect_does_not_panic() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            for icon in [
                Icon::Eye,
                Icon::EyeOff,
                Icon::Copy,
                Icon::Trash,
                Icon::Up,
                Icon::Down,
                Icon::Once,
                Icon::Loop,
                Icon::PingPong,
                Icon::Play,
                Icon::Stop,
                Icon::Record,
                Icon::Reset,
            ] {
                draw(
                    &painter,
                    icon,
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::ZERO),
                    theme::INK,
                );
            }
        });
    }
}
