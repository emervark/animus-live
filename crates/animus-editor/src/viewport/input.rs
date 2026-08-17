//! The egui half of the viewport: the widget, its gates, and pointer
//! positions in image pixels.
//!
//! Kept separate from the camera half on purpose. M0-2 found a viewport
//! where the coordinate maths was correct and the *gating* was wrong, so
//! clicking, zooming and panning all silently did nothing — a test of the
//! maths alone would have passed the whole time. This half is testable with
//! a bare `egui::Context` and synthesised pointer input, no window and no
//! GPU, which is what makes the gates testable at all.

use egui::{PointerButton, Sense};
use glam::Vec2;

/// What the operator did to the viewport this frame, in **image pixels**
/// (physical, top-left origin) rather than egui points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportInput {
    pub hovered: bool,
    /// Where a click landed, if one did.
    pub clicked_at: Option<Vec2>,
    /// Where the pointer is, if it is over the viewport.
    pub hover_at: Option<Vec2>,
    /// Left-drag delta, for dragging joints.
    pub drag_left: Option<Vec2>,
    /// Middle-drag delta, for panning.
    pub drag_middle: Option<Vec2>,
    /// Scroll for zooming. Positive is zoom in.
    pub scroll: f32,
    /// The rect the image occupies, in egui points.
    pub rect: egui::Rect,
    /// Physical pixels per egui point, i.e. the OS display scale.
    pub pixels_per_point: f32,
}

impl Default for ViewportInput {
    fn default() -> Self {
        Self {
            hovered: false,
            clicked_at: None,
            hover_at: None,
            drag_left: None,
            drag_middle: None,
            scroll: 0.0,
            // `egui::Rect` has no `Default`, and NOTHING is the honest
            // starting value: a viewport that has not been laid out occupies
            // nowhere, and `desired_target_size` clamps it to 1x1 rather
            // than pretending otherwise.
            rect: egui::Rect::NOTHING,
            pixels_per_point: 1.0,
        }
    }
}

impl ViewportInput {
    /// The image-pixel size the render target should have to match the
    /// panel exactly. Rounded to whole physical pixels and never zero —
    /// a zero-sized render target is a wgpu validation error.
    pub fn desired_target_size(&self) -> glam::UVec2 {
        glam::UVec2::new(
            ((self.rect.width() * self.pixels_per_point).round() as u32).max(1),
            ((self.rect.height() * self.pixels_per_point).round() as u32).max(1),
        )
    }
}

/// egui point → image pixel.
fn to_image_pixel(pos: egui::Pos2, rect: egui::Rect, ppp: f32) -> Vec2 {
    Vec2::new((pos.x - rect.min.x) * ppp, (pos.y - rect.min.y) * ppp)
}

/// Draw the viewport image and read what happened to it.
///
/// Three things here are load-bearing and each was a bug in M0-2:
///
/// 1. **`.sense(Sense::click_and_drag())`.** `egui::Image` is built with
///    `Sense::hover()`, so without this `response.clicked()` is never true
///    and `dragged_by` never fires. Clicking did nothing at all.
/// 2. **No `ctx.wants_pointer_input()` guard.** It expands to
///    `is_using_pointer() || (is_pointer_over_egui() && !any_down())`, and
///    this viewport *is* an egui widget, so the pointer is over an egui area
///    whenever it is over the viewport. Hovering to scroll and clicking
///    (reported on release, with no button down) both evaluated to "egui
///    wants this" and were dropped. That guard is for applications whose 3D
///    world is drawn *outside* egui.
/// 3. **Drags go through `dragged_by`**, not a raw `pointer.middle_down()`
///    check: egui only reports a drag on a widget that senses drags, so the
///    raw check moved for a frame or two and then stopped.
pub fn viewport_widget(
    ui: &mut egui::Ui,
    texture: Option<egui::TextureId>,
    size: egui::Vec2,
) -> ViewportInput {
    let response = match texture {
        Some(id) => ui.add(
            egui::Image::new(egui::load::SizedTexture::new(id, size))
                .sense(Sense::click_and_drag()),
        ),
        // Still allocate and still sense, so the gates are exercised the
        // same way before a render target exists.
        None => ui.allocate_response(size, Sense::click_and_drag()),
    };

    let rect = response.rect;
    let ppp = ui.ctx().pixels_per_point();
    let hovered = response.hovered();

    let hover_at = response
        .hover_pos()
        .map(|p| to_image_pixel(p, rect, ppp))
        .filter(|_| hovered);

    let clicked_at = response
        .clicked()
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|p| to_image_pixel(p, rect, ppp));

    // Drag deltas are in points; the rest of this module works in image
    // pixels, so convert once, here.
    let drag = |button| {
        response
            .dragged_by(button)
            .then(|| {
                let d = response.drag_delta();
                Vec2::new(d.x * ppp, d.y * ppp)
            })
            .filter(|d| *d != Vec2::ZERO)
    };

    let scroll = if hovered {
        ui.input(|i| i.smooth_scroll_delta.y)
    } else {
        0.0
    };

    ViewportInput {
        hovered,
        clicked_at,
        hover_at,
        drag_left: drag(PointerButton::Primary),
        drag_middle: drag(PointerButton::Middle),
        scroll,
        rect,
        pixels_per_point: ppp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run one egui frame with synthesised input and return what the
    /// viewport widget reported.
    ///
    /// This is the harness M0-2 did not have. A bare `egui::Context` runs
    /// headlessly, so the gates can be driven by real pointer events rather
    /// than by a person clicking and reporting back.
    fn run_frame(
        ctx: &egui::Context,
        pixels_per_point: f32,
        events: Vec<egui::Event>,
        pointer: Option<egui::Pos2>,
    ) -> ViewportInput {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        if let Some(p) = pointer {
            raw.events.insert(0, egui::Event::PointerMoved(p));
        }
        ctx.set_pixels_per_point(pixels_per_point);

        let mut out = ViewportInput::default();
        let _ = ctx.run_ui(raw, |ctx| {
            show_central(ctx, &mut out);
        });
        out
    }

    /// `CentralPanel::show` is deprecated with no top-level replacement; see
    /// `dock::draw` for the same note.
    #[expect(deprecated)]
    fn show_central(ctx: &egui::Context, out: &mut ViewportInput) {
        egui::CentralPanel::default().show(ctx, |ui| {
            *out = viewport_widget(ui, None, egui::vec2(400.0, 300.0));
        });
    }

    fn press_release(at: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]
    }

    #[test]
    fn a_click_is_reported_at_all() {
        // The M0-2 regression, as a test: `egui::Image` senses hover only, so
        // without an explicit sense this is `None` forever and no amount of
        // correct maths downstream matters.
        let ctx = egui::Context::default();
        run_frame(&ctx, 1.0, vec![], Some(egui::pos2(100.0, 100.0)));
        let out = run_frame(&ctx, 1.0, press_release(egui::pos2(100.0, 100.0)), None);
        assert!(
            out.clicked_at.is_some(),
            "the viewport must report clicks; it reported {out:?}"
        );
    }

    #[test]
    fn a_click_lands_on_the_pixel_it_was_made_at() {
        let ctx = egui::Context::default();
        // First frame establishes the widget's rect; egui needs a frame of
        // memory before hit-testing is meaningful.
        run_frame(&ctx, 1.0, vec![], Some(egui::pos2(10.0, 10.0)));

        let target = egui::pos2(123.0, 87.0);
        let out = run_frame(&ctx, 1.0, press_release(target), None);

        let clicked = out.clicked_at.expect("a click");
        let expected = Vec2::new(target.x - out.rect.min.x, target.y - out.rect.min.y);
        assert!(
            (clicked - expected).length() < 1.0,
            "click landed at {clicked:?}, expected {expected:?} (within 1 px)"
        );
    }

    #[test]
    fn a_click_lands_within_one_pixel_at_every_display_scale() {
        // The check M0-2 left to a human on one machine. 1.25 is what the
        // laptop the spike ran on actually uses, and 1.5 is the one that was
        // never tested at all.
        for ppp in [1.0_f32, 1.25, 1.5, 2.0] {
            let ctx = egui::Context::default();
            run_frame(&ctx, ppp, vec![], Some(egui::pos2(10.0, 10.0)));

            let target = egui::pos2(200.0, 150.0);
            let out = run_frame(&ctx, ppp, press_release(target), None);
            let clicked = out.clicked_at.expect("a click");

            // In image pixels, so the expected value scales with the display.
            let expected = Vec2::new(
                (target.x - out.rect.min.x) * ppp,
                (target.y - out.rect.min.y) * ppp,
            );
            assert!(
                (clicked - expected).length() < 1.0,
                "at {ppp}x: click landed at {clicked:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn hovering_reports_a_position_and_leaving_does_not() {
        let ctx = egui::Context::default();
        run_frame(&ctx, 1.0, vec![], Some(egui::pos2(50.0, 50.0)));
        let inside = run_frame(&ctx, 1.0, vec![], Some(egui::pos2(50.0, 50.0)));
        assert!(inside.hovered);
        assert!(inside.hover_at.is_some());

        // Hover is derived from the *previous* frame's widget rects, so it
        // lags by one frame: `PointerGone` does not clear it until egui has
        // laid out again. Worth knowing before writing a test that flickers.
        run_frame(&ctx, 1.0, vec![egui::Event::PointerGone], None);
        let outside = run_frame(&ctx, 1.0, vec![], None);
        assert!(!outside.hovered);
        assert!(outside.hover_at.is_none());
    }

    #[test]
    fn scroll_is_only_read_while_hovering() {
        // Otherwise scrolling a side panel would zoom the viewport.
        let ctx = egui::Context::default();
        run_frame(&ctx, 1.0, vec![], Some(egui::pos2(50.0, 50.0)));

        let hovering = run_frame(
            &ctx,
            1.0,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, 3.0),
                phase: egui::TouchPhase::Move,
                modifiers: Default::default(),
            }],
            Some(egui::pos2(50.0, 50.0)),
        );
        assert!(
            hovering.scroll.abs() > 0.0,
            "scroll over the viewport counts"
        );

        // One frame for the hover state to catch up (see the hover test).
        run_frame(&ctx, 1.0, vec![egui::Event::PointerGone], None);
        let gone = run_frame(
            &ctx,
            1.0,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, 3.0),
                phase: egui::TouchPhase::Move,
                modifiers: Default::default(),
            }],
            None,
        );
        assert_eq!(gone.scroll, 0.0, "scroll elsewhere must not reach it");
    }

    #[test]
    fn the_target_size_is_whole_physical_pixels_and_never_zero() {
        let out = ViewportInput {
            rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.4, 300.6)),
            pixels_per_point: 1.25,
            ..Default::default()
        };
        assert_eq!(out.desired_target_size(), glam::UVec2::new(501, 376));

        let collapsed = ViewportInput {
            rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(0.0, 0.0)),
            pixels_per_point: 1.0,
            ..Default::default()
        };
        assert_eq!(
            collapsed.desired_target_size(),
            glam::UVec2::new(1, 1),
            "a collapsed panel must not ask for a zero-sized render target"
        );
    }
}
