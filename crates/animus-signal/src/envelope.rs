//! Envelopes: the shape a value takes on its way in.
//!
//! **An envelope is a transfer function, not an animation.** It does not
//! produce a value; it reshapes whatever arrives — a fader, an OSC message,
//! an LFO, the transport's own phase. That is the whole idea, borrowed
//! deliberately from Resolume, and it is what stops this becoming two
//! parallel systems where one is "keyframe animation" and the other is "live
//! input". There is one value, it comes from somewhere, and a curve shapes
//! it.
//!
//! ## The x axis is phase, not seconds
//!
//! A keyframe sits at a fraction of the way through, never at a time. The
//! reason is not tidiness: the same envelope has to work on a control turned
//! by hand, on an LFO at 0.2 Hz, and on a bar of the pattern. Seconds mean
//! nothing to the first, and change under the second and third the moment
//! anyone touches the rate. Phase stays put.
//!
//! ## Curves apply *behind* their keyframe
//!
//! A keyframe's curve describes the segment arriving at it, not leaving it —
//! again as Resolume does. It reads oddly for about a minute and then stops
//! mattering, and the alternative (the curve leaving a keyframe) leaves the
//! last keyframe holding a curve that shapes nothing.

use serde::{Deserialize, Serialize};

/// How the line between two keyframes is shaped.
///
/// The families are the standard easing set. Quadratic, Sine, Circular and
/// Exponential are the same idea at four strengths; Elastic, Back and Bounce
/// are the animation principles that are hard to hand-key and cheap to
/// compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Curve {
    #[default]
    Linear,
    /// Hold the previous value until this keyframe is reached. A step, for
    /// parameters that jump rather than slide.
    Hold,
    Quadratic(Ease),
    Sine(Ease),
    Circular(Ease),
    Exponential(Ease),
    /// Overshoots, then settles. Anticipation and follow-through.
    Back(Ease),
    /// Overshoots and wobbles home.
    Elastic(Ease),
    /// Lands and bounces.
    Bounce(Ease),
}

/// Which end of the segment the curve acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Ease {
    /// Slow to start.
    In,
    /// Slow to finish.
    Out,
    #[default]
    InOut,
}

impl Curve {
    /// Every curve the editor offers, in the order it lists them.
    pub fn all() -> Vec<Curve> {
        let mut out = vec![Curve::Linear, Curve::Hold];
        for family in [
            Curve::Quadratic as fn(Ease) -> Curve,
            Curve::Sine,
            Curve::Circular,
            Curve::Exponential,
            Curve::Back,
            Curve::Elastic,
            Curve::Bounce,
        ] {
            for ease in [Ease::In, Ease::Out, Ease::InOut] {
                out.push(family(ease));
            }
        }
        out
    }

    pub fn label(self) -> String {
        let (name, ease) = match self {
            Curve::Linear => return "Linear".into(),
            Curve::Hold => return "Hold".into(),
            Curve::Quadratic(e) => ("Quadratic", e),
            Curve::Sine(e) => ("Sine", e),
            Curve::Circular(e) => ("Circular", e),
            Curve::Exponential(e) => ("Exponential", e),
            Curve::Back(e) => ("Back", e),
            Curve::Elastic(e) => ("Elastic", e),
            Curve::Bounce(e) => ("Bounce", e),
        };
        format!(
            "{name} {}",
            match ease {
                Ease::In => "In",
                Ease::Out => "Out",
                Ease::InOut => "In/Out",
            }
        )
    }

    /// Shape `t`, which is 0..1 along the segment.
    ///
    /// **Every curve passes through (0,0) and (1,1)**, so the keyframes
    /// themselves are always hit exactly whatever curve joins them. Back and
    /// Elastic leave the 0..1 band in between on purpose — that overshoot is
    /// the point of them — and the caller is expected not to clamp it, or
    /// they become expensive ways of writing Quadratic.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Curve::Linear => t,
            // Reaching the keyframe is what makes the step happen, so the
            // jump is at 1 rather than anywhere earlier.
            Curve::Hold => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Curve::Quadratic(e) => ease(e, t, |t| t * t),
            Curve::Sine(e) => ease(e, t, |t| 1.0 - (t * std::f32::consts::FRAC_PI_2).cos()),
            Curve::Circular(e) => ease(e, t, |t| 1.0 - (1.0 - t * t).max(0.0).sqrt()),
            Curve::Exponential(e) => ease(e, t, |t| {
                if t <= 0.0 {
                    0.0
                } else {
                    (2.0_f32).powf(10.0 * (t - 1.0))
                }
            }),
            Curve::Back(e) => ease(e, t, |t| {
                const S: f32 = 1.70158;
                t * t * ((S + 1.0) * t - S)
            }),
            Curve::Elastic(e) => ease(e, t, elastic_in),
            Curve::Bounce(e) => ease(e, t, |t| 1.0 - bounce_out(1.0 - t)),
        }
    }
}

/// Turn an "in" curve into in, out or in/out.
///
/// One definition each, mirrored, rather than three hand-written variants:
/// three hand-written variants is three chances for one of them to stop
/// meeting its own endpoints, and a curve that misses (1,1) moves a keyframe
/// the operator placed.
fn ease(kind: Ease, t: f32, f: impl Fn(f32) -> f32) -> f32 {
    match kind {
        Ease::In => f(t),
        Ease::Out => 1.0 - f(1.0 - t),
        Ease::InOut => {
            if t < 0.5 {
                f(t * 2.0) * 0.5
            } else {
                1.0 - f((1.0 - t) * 2.0) * 0.5
            }
        }
    }
}

fn elastic_in(t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let p = 0.3;
    -(2.0_f32).powf(10.0 * (t - 1.0))
        * ((t - 1.0 - p / 4.0) * (2.0 * std::f32::consts::PI) / p).sin()
}

fn bounce_out(t: f32) -> f32 {
    const N: f32 = 7.5625;
    const D: f32 = 2.75;
    if t < 1.0 / D {
        N * t * t
    } else if t < 2.0 / D {
        let t = t - 1.5 / D;
        N * t * t + 0.75
    } else if t < 2.5 / D {
        let t = t - 2.25 / D;
        N * t * t + 0.9375
    } else {
        let t = t - 2.625 / D;
        N * t * t + 0.984375
    }
}

/// One point on an envelope.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    /// Where it sits along the input, 0..1. Never a time — see the module
    /// docs.
    pub phase: f32,
    /// What comes out there, 0..1.
    pub value: f32,
    /// How the segment *arriving* at this keyframe is shaped.
    pub curve: Curve,
}

/// A curve from input to output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// Sorted by phase. [`Envelope::set`] and [`Envelope::add`] keep it that
    /// way; nothing else may push directly.
    keys: Vec<Keyframe>,
}

impl Default for Envelope {
    /// The identity: in equals out.
    ///
    /// A new envelope must change nothing. Resolume does the same and says
    /// why it looks broken — "you don't see any result, as parameters are
    /// linear themselves" — which is exactly right: the operator adds the
    /// shape, the tool does not guess one for them.
    fn default() -> Self {
        Self {
            keys: vec![
                Keyframe {
                    phase: 0.0,
                    value: 0.0,
                    curve: Curve::Linear,
                },
                Keyframe {
                    phase: 1.0,
                    value: 1.0,
                    curve: Curve::Linear,
                },
            ],
        }
    }
}

impl Envelope {
    pub fn keys(&self) -> &[Keyframe] {
        &self.keys
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Is this envelope the identity — in equals out, no shape at all?
    pub fn is_identity(&self) -> bool {
        self.keys.len() == 2
            && self.keys.iter().all(|k| k.curve == Curve::Linear)
            && self.keys[0].phase == 0.0
            && self.keys[0].value == 0.0
            && self.keys[1].phase == 1.0
            && self.keys[1].value == 1.0
    }

    /// Add a keyframe and return its index.
    pub fn add(&mut self, key: Keyframe) -> usize {
        self.keys.push(key);
        self.sort_and_find(key.phase)
    }

    /// Move a keyframe, keeping the list sorted. Returns its new index.
    ///
    /// **The ends do not move along the phase axis.** An envelope that did
    /// not start at 0 and end at 1 would be undefined outside its own span,
    /// and the operator would meet that as a parameter that stops responding
    /// near one end for no visible reason.
    pub fn set(&mut self, index: usize, phase: f32, value: f32) -> usize {
        let last = self.keys.len().saturating_sub(1);
        let Some(key) = self.keys.get_mut(index) else {
            return index;
        };
        key.value = value.clamp(0.0, 1.0);
        if index != 0 && index != last {
            key.phase = phase.clamp(0.0, 1.0);
        }
        let phase = self.keys[index].phase;
        self.sort_and_find(phase)
    }

    pub fn set_curve(&mut self, index: usize, curve: Curve) {
        if let Some(key) = self.keys.get_mut(index) {
            key.curve = curve;
        }
    }

    /// Remove a keyframe. The two ends cannot go.
    pub fn remove(&mut self, index: usize) {
        if index == 0 || index + 1 >= self.keys.len() {
            return;
        }
        self.keys.remove(index);
    }

    fn sort_and_find(&mut self, phase: f32) -> usize {
        self.keys.sort_by(|a, b| {
            a.phase
                .partial_cmp(&b.phase)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.keys.iter().position(|k| k.phase == phase).unwrap_or(0)
    }

    /// The output for an input of `phase`.
    pub fn sample(&self, phase: f32) -> f32 {
        let phase = phase.clamp(0.0, 1.0);
        match self.keys.len() {
            0 => phase,
            1 => self.keys[0].value,
            _ => {
                // Before the first and after the last: hold the end value.
                // The ends are pinned at 0 and 1 so this only bites on a
                // hand-built envelope, and holding is the least surprising
                // of the wrong answers available.
                if phase <= self.keys[0].phase {
                    return self.keys[0].value;
                }
                let last = self.keys.len() - 1;
                if phase >= self.keys[last].phase {
                    return self.keys[last].value;
                }

                let i = self
                    .keys
                    .windows(2)
                    .position(|w| phase >= w[0].phase && phase <= w[1].phase)
                    .unwrap_or(0);
                let (a, b) = (self.keys[i], self.keys[i + 1]);
                let span = b.phase - a.phase;
                let t = if span.abs() < 1e-6 {
                    1.0
                } else {
                    (phase - a.phase) / span
                };
                // The curve belongs to the keyframe being arrived at.
                a.value + (b.value - a.value) * b.curve.apply(t)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every curve must pass through both ends.** A keyframe the operator
    /// placed is a promise about where the value will be; a curve that
    /// missed (1,1) would quietly move it.
    #[test]
    fn every_curve_hits_zero_and_one_exactly() {
        for curve in Curve::all() {
            let at0 = curve.apply(0.0);
            let at1 = curve.apply(1.0);
            assert!(at0.abs() < 1e-4, "{} starts at {at0}, not 0", curve.label());
            assert!(
                (at1 - 1.0).abs() < 1e-4,
                "{} ends at {at1}, not 1",
                curve.label()
            );
        }
    }

    /// Hold is a step: nothing until the keyframe is reached.
    #[test]
    fn hold_does_not_move_until_it_arrives() {
        assert_eq!(Curve::Hold.apply(0.0), 0.0);
        assert_eq!(Curve::Hold.apply(0.99), 0.0);
        assert_eq!(Curve::Hold.apply(1.0), 1.0);
    }

    /// **Back and Elastic leave the band on purpose.** Clamping them would
    /// make them expensive ways of writing Quadratic, and the overshoot is
    /// the entire reason an animator reaches for them.
    #[test]
    fn back_and_elastic_overshoot() {
        let back = (0..=100)
            .map(|i| Curve::Back(Ease::Out).apply(i as f32 / 100.0))
            .fold(f32::MIN, f32::max);
        assert!(back > 1.0, "Back Out never overshot: peaked at {back}");

        let elastic = (0..=100)
            .map(|i| Curve::Elastic(Ease::Out).apply(i as f32 / 100.0))
            .fold(f32::MIN, f32::max);
        assert!(elastic > 1.0, "Elastic Out never overshot");
    }

    /// The four "basic easing at various strengths" families never reverse.
    #[test]
    fn the_plain_families_are_monotonic() {
        for curve in [
            Curve::Quadratic(Ease::InOut),
            Curve::Sine(Ease::InOut),
            Curve::Circular(Ease::InOut),
            Curve::Exponential(Ease::InOut),
        ] {
            let mut last = f32::MIN;
            for i in 0..=200 {
                let v = curve.apply(i as f32 / 200.0);
                assert!(
                    v >= last - 1e-4,
                    "{} went backwards at {i}: {v} after {last}",
                    curve.label()
                );
                last = v;
            }
        }
    }

    /// **A new envelope must change nothing.** The operator adds the shape;
    /// the tool does not guess one for them.
    #[test]
    fn a_fresh_envelope_is_the_identity() {
        let e = Envelope::default();
        assert!(e.is_identity());
        for i in 0..=20 {
            let t = i as f32 / 20.0;
            assert!((e.sample(t) - t).abs() < 1e-5, "at {t}");
        }
    }

    #[test]
    fn a_keyframe_bends_the_curve_through_itself() {
        let mut e = Envelope::default();
        e.add(Keyframe {
            phase: 0.5,
            value: 0.9,
            curve: Curve::Linear,
        });
        assert!((e.sample(0.5) - 0.9).abs() < 1e-5, "passes through the key");
        assert!(e.sample(0.25) > 0.25, "and lifts the segment before it");
    }

    /// **The ends stay at 0 and 1 on the phase axis.** An envelope that did
    /// not span the whole input would be undefined near one end, and the
    /// operator would meet that as a control that stops responding.
    #[test]
    fn the_end_keyframes_cannot_be_dragged_along_the_phase_axis() {
        let mut e = Envelope::default();
        e.set(0, 0.4, 0.2);
        e.set(1, 0.6, 0.8);
        assert_eq!(e.keys()[0].phase, 0.0);
        assert_eq!(e.keys()[1].phase, 1.0);
        assert_eq!(e.keys()[0].value, 0.2, "but their value is free");
        assert_eq!(e.keys()[1].value, 0.8);
    }

    #[test]
    fn the_end_keyframes_cannot_be_removed() {
        let mut e = Envelope::default();
        e.remove(0);
        e.remove(1);
        assert_eq!(e.len(), 2);
    }

    #[test]
    fn a_middle_keyframe_can_be_removed() {
        let mut e = Envelope::default();
        e.add(Keyframe {
            phase: 0.5,
            value: 0.5,
            curve: Curve::Linear,
        });
        assert_eq!(e.len(), 3);
        e.remove(1);
        assert_eq!(e.len(), 2);
        assert!(e.is_identity());
    }

    #[test]
    fn keyframes_stay_sorted_however_they_are_added() {
        let mut e = Envelope::default();
        for phase in [0.7, 0.2, 0.5] {
            e.add(Keyframe {
                phase,
                value: 0.5,
                curve: Curve::Linear,
            });
        }
        let phases: Vec<f32> = e.keys().iter().map(|k| k.phase).collect();
        let mut sorted = phases.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(phases, sorted);
    }

    /// A curve shapes the segment *arriving* at its keyframe. Put a Hold on
    /// the last one and the value should stay put until the very end.
    #[test]
    fn a_curve_belongs_to_the_segment_that_arrives_at_it() {
        let mut e = Envelope::default();
        e.set_curve(1, Curve::Hold);
        assert_eq!(e.sample(0.5), 0.0, "held all the way to the keyframe");
        assert_eq!(e.sample(1.0), 1.0);
    }
}
