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
    /// An arrow curving back on itself: undo.
    Undo,
    /// The same, mirrored: redo.
    Redo,
    /// A turn anticlockwise, by a fixed step.
    RotateCcw,
    /// A turn clockwise, by a fixed step.
    RotateCw,
    /// A shackle over a body: the layer is locked.
    Lock,
    /// The same with the shackle open.
    Unlock,
    /// Six dots: somewhere to take hold of a row.
    Grip,
    /// A cross: add one.
    Plus,
    /// The application's mark: a joint with a target on it.
    Mark,
    /// A rightward arrow: this drives that.
    ArrowRight,
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
        Icon::Undo | Icon::Redo => {
            // An arrow that leaves, loops under and comes back: the shape
            // reads as "the way you came" rather than as a plain rotation,
            // which is what separates it from `Reset` beside it.
            let flip = if icon == Icon::Undo { 1.0 } else { -1.0 };
            let x = |t: f32| at(0.5 + flip * (t - 0.5), 0.0).x;
            let mut pts = Vec::with_capacity(11);
            for i in 0..=10 {
                let t = i as f32 / 10.0;
                let a = std::f32::consts::PI * (1.0 - t);
                pts.push(egui::pos2(
                    x(0.5 + 0.34 * a.cos()),
                    at(0.0, 0.62 - 0.30 * a.sin()).y,
                ));
            }
            painter.add(egui::Shape::line(pts, stroke));
            painter.add(line(vec![
                egui::pos2(x(0.30), at(0.0, 0.34).y),
                egui::pos2(x(0.16), at(0.0, 0.62).y),
                egui::pos2(x(0.44), at(0.0, 0.66).y),
            ]));
        }
        Icon::RotateCcw | Icon::RotateCw => {
            // `Reset`'s ring, mirrored for the clockwise one. Two buttons
            // that differ only in direction have to differ *visibly* in
            // direction, or the pair is a coin toss.
            let flip = if icon == Icon::RotateCw { -1.0 } else { 1.0 };
            let mut pts = Vec::with_capacity(15);
            for i in 0..=14 {
                let a =
                    std::f32::consts::PI * 0.35 + (std::f32::consts::PI * 1.55) * i as f32 / 14.0;
                pts.push(at(0.5 + flip * 0.36 * a.cos(), 0.5 + 0.36 * a.sin()));
            }
            painter.add(egui::Shape::line(pts, stroke));
            painter.add(line(vec![
                at(0.5 - flip * 0.34, 0.16),
                at(0.5 - flip * 0.32, 0.44),
                at(0.5 - flip * 0.04, 0.36),
            ]));
        }
        Icon::Lock | Icon::Unlock => {
            let body = egui::Rect::from_min_max(at(0.18, 0.46), at(0.82, 0.95));
            painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Inside);
            // The shackle: centred when locked, lifted and offset when not,
            // so the two states differ in silhouette and not only in detail.
            let (cx, top) = if icon == Icon::Lock {
                (0.5, 0.20)
            } else {
                (0.68, 0.10)
            };
            let mut pts = Vec::with_capacity(9);
            for i in 0..=8 {
                let a = std::f32::consts::PI * (1.0 - i as f32 / 8.0);
                pts.push(at(cx + 0.22 * a.cos(), 0.46 - (0.46 - top) * a.sin()));
            }
            painter.add(egui::Shape::line(pts, stroke));
        }
        Icon::Grip => {
            for row in 0..3 {
                for col in 0..2 {
                    painter.circle_filled(
                        at(0.34 + col as f32 * 0.32, 0.22 + row as f32 * 0.28),
                        rect.width() * 0.07,
                        ink,
                    );
                }
            }
        }
        Icon::Plus => {
            painter.add(line(vec![at(0.5, 0.14), at(0.5, 0.86)]));
            painter.add(line(vec![at(0.14, 0.5), at(0.86, 0.5)]));
        }
        Icon::ArrowRight => {
            painter.add(line(vec![at(0.08, 0.5), at(0.86, 0.5)]));
            painter.add(line(vec![at(0.62, 0.26), at(0.9, 0.5), at(0.62, 0.74)]));
        }
        Icon::Mark => {
            // A joint with a target on it: what this tool does, in one glyph.
            // The ring is the go colour and the ticks are structure, which is
            // the Signal Rule applied to the logo itself.
            painter.circle_stroke(
                at(0.5, 0.5),
                rect.width() * 0.40,
                egui::Stroke::new(1.4_f32, theme::GO_GREEN),
            );
            painter.circle_filled(at(0.5, 0.5), rect.width() * 0.13, theme::GO_GREEN);
            let tick = egui::Stroke::new(1.1_f32, theme::FAINT);
            for (a, b) in [
                ((0.5, 0.02), (0.5, 0.24)),
                ((0.5, 0.76), (0.5, 0.98)),
                ((0.02, 0.5), (0.24, 0.5)),
                ((0.76, 0.5), (0.98, 0.5)),
            ] {
                painter.add(egui::Shape::line(vec![at(a.0, a.1), at(b.0, b.1)], tick));
            }
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
