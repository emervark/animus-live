//! Live input: what is arriving from outside, and what it is wired to.
//!
//! Two halves, deliberately separated:
//!
//! - **The bus** ([`Bus`]) is pure. Channels carry a normalised value,
//!   bindings map a channel onto something in the show, and both are
//!   ordinary data with ordinary tests. Nothing here talks to hardware.
//! - **The sources** ([`midi`], [`osc`]) each own a thread and push
//!   [`Update`]s down a channel. **The UI thread never waits on a
//!   controller.** A MIDI port that stops responding, or a UDP socket with
//!   nothing on the other end, must cost the show nothing at all — and the
//!   only way to promise that is to keep the blocking read somewhere the
//!   frame loop never looks.
//!
//! ## Channels appear when they speak
//!
//! Nothing here has a list of what a controller *might* send. A channel is
//! created the first time a value arrives on it, named after where it came
//! from, which is how every DAW does it and for the same reason: an operator
//! finds the knob they want by turning it, not by reading a manual.
//!
//! ## Normalised on the way in
//!
//! Every source converts to `0.0..=1.0` before the bus sees it. A MIDI CC is
//! seven bits, an OSC float is whatever the sender felt like, and a binding
//! should not have to know which. The range that matters — how far the joint
//! actually moves — belongs to the binding, where the operator can see it.

use std::sync::mpsc::{Receiver, TryRecvError};

use animus_core::ids::{JointId, PuppetId};

pub use envelope::{Curve, Ease, Envelope, Keyframe};

pub mod envelope;
pub mod midi;
pub mod osc;

/// A channel's identity for the life of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelId(pub u32);

/// Where a value came from.
///
/// Compared structurally, so the same knob turned twice lands on the same
/// channel rather than filling the panel with duplicates.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Source {
    /// A control change: MIDI channel (0-based) and controller number.
    MidiCc { channel: u8, cc: u8 },
    /// An OSC address, exactly as it arrived.
    Osc { address: String },
    /// A free-running oscillator with its own rate, in Hz.
    Lfo { index: u32 },
    /// An oscillator locked to the transport, so a joint breathes in step
    /// with the pattern rather than beside it.
    BpmSync { index: u32 },
}

impl Source {
    /// What the operator sees in the list.
    pub fn label(&self) -> String {
        match self {
            Source::MidiCc { channel, cc } => format!("midi {}·cc {cc}", channel + 1),
            Source::Osc { address } => format!("osc {address}"),
            Source::Lfo { index } => format!("lfo {index}"),
            Source::BpmSync { index } => format!("sync {index}"),
        }
    }

    /// Does the app produce this value, rather than receive it?
    pub fn is_generated(&self) -> bool {
        matches!(self, Source::Lfo { .. } | Source::BpmSync { .. })
    }
}

/// The wave a generator traces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Shape {
    #[default]
    Sine,
    Triangle,
    /// Ramps up and snaps back.
    Saw,
    Square,
    /// One new value per cycle, held. Sample-and-hold, not white noise:
    /// a value that changed every frame would read as a broken joint
    /// rather than as a choice.
    Noise,
}

impl Shape {
    pub const ALL: [Shape; 5] = [
        Shape::Sine,
        Shape::Triangle,
        Shape::Saw,
        Shape::Square,
        Shape::Noise,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Shape::Sine => "sine",
            Shape::Triangle => "tri",
            Shape::Saw => "saw",
            Shape::Square => "square",
            Shape::Noise => "noise",
        }
    }

    /// The wave at `phase` (0..1), as a value in 0..1.
    pub fn at(self, phase: f32) -> f32 {
        let p = phase.rem_euclid(1.0);
        match self {
            Shape::Sine => 0.5 - 0.5 * (p * std::f32::consts::TAU).cos(),
            Shape::Triangle => 1.0 - (2.0 * p - 1.0).abs(),
            Shape::Saw => p,
            Shape::Square => {
                if p < 0.5 {
                    0.0
                } else {
                    1.0
                }
            }
            // Hashed from the cycle number, so the same cycle always gives
            // the same value: a pattern that changed every time it played
            // would be unrehearsable.
            Shape::Noise => {
                let cycle = phase.floor() as i64 as u64;
                let mut h = cycle.wrapping_mul(0x9E37_79B9_7F4A_7C15);
                h ^= h >> 29;
                h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
                h ^= h >> 32;
                (h & 0xFFFF) as f32 / 65535.0
            }
        }
    }
}

/// A channel the app drives itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Generator {
    pub shape: Shape,
    /// Free LFO: cycles per second. Transport-locked: beats per cycle.
    pub rate: f32,
    /// Where it has got to. Kept rather than derived from a clock, so a
    /// rate change bends the wave from here instead of jumping it.
    pub phase: f32,
}

impl Default for Generator {
    fn default() -> Self {
        Self {
            shape: Shape::Sine,
            // Slow enough to read as breathing rather than as a fault, and
            // for the transport-locked kind, one cycle per bar.
            rate: 0.25,
            phase: 0.0,
        }
    }
}

/// One live input, as the panel shows it.
#[derive(Debug, Clone)]
pub struct Channel {
    pub id: ChannelId,
    pub source: Source,
    /// Normalised 0..1, as last seen.
    pub value: f32,
    /// Whether this channel is allowed to drive anything.
    ///
    /// Off is not the same as absent: a controller that sends a stream of
    /// something unwanted should be silenceable without the operator having
    /// to unplug it mid-show.
    pub on: bool,
    /// Set on the channels the app drives itself. `None` means the value
    /// arrives from outside.
    pub generator: Option<Generator>,
}

/// What a binding drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    JointX(PuppetId, JointId),
    JointY(PuppetId, JointId),
    /// Turn the joint, carrying everything below it. The range is in
    /// degrees rather than pixels, which is why the binding's own range
    /// asks what unit it is in before showing a number.
    JointRotation(PuppetId, JointId),
}

impl Target {
    pub fn joint(self) -> (PuppetId, JointId) {
        match self {
            Target::JointX(p, j) | Target::JointY(p, j) | Target::JointRotation(p, j) => (p, j),
        }
    }

    pub fn axis(self) -> &'static str {
        match self {
            Target::JointX(..) => "position.x",
            Target::JointY(..) => "position.y",
            Target::JointRotation(..) => "rotation",
        }
    }

    /// What the binding's range is measured in.
    pub fn unit(self) -> &'static str {
        match self {
            Target::JointRotation(..) => "\u{00b0}",
            _ => " px",
        }
    }

    /// A sensible span for a freshly-learned binding on this target.
    pub fn default_range(self) -> f32 {
        match self {
            Target::JointRotation(..) => 45.0,
            _ => DEFAULT_RANGE_PX,
        }
    }
}

/// A channel wired to a target, with the range it moves through.
///
/// Not `Copy` any more: an envelope owns its keyframes. Bindings are edited
/// in place and read by reference, so nothing wanted a bitwise copy of one.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub src: ChannelId,
    pub dst: Target,
    /// Where the joint sits when the channel reads 0, as an offset from rest
    /// in image pixels.
    pub low: f32,
    /// And at 1.
    pub high: f32,
    pub on: bool,
    /// The shape the incoming value takes on its way through.
    ///
    /// **On the input, not on the output.** One curve reshapes whatever
    /// arrives — a fader, an OSC message, an LFO, the transport — which is
    /// what stops this becoming two parallel systems, one for "keyframe
    /// animation" and one for "live input".
    pub envelope: Envelope,
}

impl Binding {
    /// The offset this binding asks for, at a given channel value.
    ///
    /// Clamped, because a source is not a trusted input: an OSC sender that
    /// overshoots should reach the end of the range and stop, not throw the
    /// limb off the stage.
    pub fn map(&self, value: f32) -> f32 {
        let t = self.envelope.sample(value.clamp(0.0, 1.0));
        self.low + (self.high - self.low) * t
    }

    /// What arrives, before the envelope. Shown beside the output so the
    /// operator can see the curve doing its work.
    pub fn input(&self, value: f32) -> f32 {
        value.clamp(0.0, 1.0)
    }
}

/// One value arriving from a source.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub source: Source,
    /// Already normalised to 0..1 by whoever produced it.
    pub value: f32,
}

/// Everything arriving, and everything it is wired to.
#[derive(Debug, Default)]
pub struct Bus {
    pub channels: Vec<Channel>,
    pub bindings: Vec<Binding>,
    /// A target waiting for the next channel that moves.
    pub learn: Option<Target>,
    next_id: u32,
}

/// How far a channel must move while Learn is armed before it counts.
///
/// A controller at rest still sends jitter, and a fader nudged by a sleeve
/// should not claim a binding the operator meant for the knob they were
/// reaching for.
pub const LEARN_THRESHOLD: f32 = 0.08;

impl Bus {
    /// Record a value, creating the channel if this is the first time it has
    /// spoken. Returns the channel it landed on.
    pub fn feed(&mut self, update: Update) -> ChannelId {
        let (id, moved) = match self.channels.iter_mut().find(|c| c.source == update.source) {
            Some(existing) => {
                let moved = (existing.value - update.value).abs();
                existing.value = update.value;
                (existing.id, moved)
            }
            None => {
                self.next_id += 1;
                let id = ChannelId(self.next_id);
                self.channels.push(Channel {
                    id,
                    source: update.source,
                    value: update.value,
                    on: true,
                    generator: None,
                });
                // A channel appearing at all is movement enough: the operator
                // just touched something that had never spoken before.
                (id, f32::INFINITY)
            }
        };
        if moved >= LEARN_THRESHOLD {
            self.bind_if_learning(id);
        }
        id
    }

    /// Bind the armed target to a channel by name.
    ///
    /// **A generator never "arrives", so Learn can never catch one.** An LFO
    /// that claimed an armed Learn the moment it was armed would be worse
    /// than useless: it would steal every binding from the fader the
    /// operator was reaching for, because it is always moving. So arming
    /// offers two ways to finish — move a control, or pick a source — and a
    /// generator can only be picked.
    pub fn bind_armed_to(&mut self, src: ChannelId) -> bool {
        if self.learn.is_none() || self.channel(src).is_none() {
            return false;
        }
        self.bind_if_learning(src);
        true
    }

    fn bind_if_learning(&mut self, src: ChannelId) {
        let Some(dst) = self.learn.take() else { return };
        // Re-arming over an existing binding replaces it rather than stacking
        // a second one on the same target, which would leave two sources
        // fighting with no way to see why.
        self.bindings.retain(|b| b.dst != dst);
        let span = dst.default_range();
        self.bindings.push(Binding {
            src,
            dst,
            low: -span,
            high: span,
            on: true,
            envelope: Envelope::default(),
        });
    }

    /// Add a generator channel: an LFO, or one locked to the transport.
    ///
    /// **A generator is a channel like any other**, which is the whole
    /// reason it is worth doing this way: binding a joint to an LFO uses
    /// the same code as binding it to a fader, and an envelope shapes both
    /// identically. Nothing downstream needs to know the difference.
    pub fn add_generator(&mut self, locked: bool) -> ChannelId {
        let index = self
            .channels
            .iter()
            .filter(|c| {
                matches!(
                    (&c.source, locked),
                    (Source::Lfo { .. }, false) | (Source::BpmSync { .. }, true)
                )
            })
            .count() as u32
            + 1;
        self.next_id += 1;
        let id = ChannelId(self.next_id);
        self.channels.push(Channel {
            id,
            source: if locked {
                Source::BpmSync { index }
            } else {
                Source::Lfo { index }
            },
            value: 0.0,
            on: true,
            generator: Some(Generator {
                // One cycle per bar for the locked kind; a slow breath for
                // the free one.
                rate: if locked { 4.0 } else { 0.25 },
                ..Generator::default()
            }),
        });
        id
    }

    /// Advance every generator.
    ///
    /// `dt` is the frame in seconds; `beats` is how far the transport has
    /// moved since the last call, in beats. A transport-locked generator
    /// reads the second and ignores the first, which is what keeps it in
    /// step when the tempo changes mid-show.
    pub fn tick(&mut self, dt: f32, beats: f32) {
        for channel in &mut self.channels {
            let locked = matches!(channel.source, Source::BpmSync { .. });
            let Some(wave) = channel.generator.as_mut() else {
                continue;
            };
            let rate = wave.rate.max(1e-4);
            wave.phase += if locked { beats / rate } else { dt * rate };
            // Wrapped rather than left to grow: an f32 phase counted in
            // hours loses the precision that makes a slow LFO smooth.
            if wave.phase > 1e6 {
                wave.phase = wave.phase.fract();
            }
            channel.value = wave.shape.at(wave.phase);
        }
    }

    pub fn channel(&self, id: ChannelId) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == id)
    }

    pub fn binding_for(&self, dst: Target) -> Option<&Binding> {
        self.bindings.iter().find(|b| b.dst == dst)
    }

    /// Every offset the bindings currently ask for, by joint.
    ///
    /// Returned rather than applied: this crate knows nothing about a solver,
    /// and the caller is the one place allowed to write targets.
    pub fn offsets(&self) -> Vec<(Target, f32)> {
        self.bindings
            .iter()
            .filter(|b| b.on)
            .filter_map(|b| {
                let channel = self.channel(b.src)?;
                channel.on.then(|| (b.dst, b.map(channel.value)))
            })
            .collect()
    }

    /// Drain whatever the sources have produced since the last frame.
    ///
    /// Non-blocking by construction: a source thread that has wedged costs
    /// this call nothing, which is the whole reason the threads exist.
    pub fn drain(&mut self, rx: &Receiver<Update>) {
        loop {
            match rx.try_recv() {
                Ok(update) => {
                    self.feed(update);
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return,
            }
        }
    }
}

/// How far a freshly-learned binding moves the joint, either side of rest.
///
/// Generous enough to be unmistakable on a first try — a binding that does
/// something too small to see reads as a binding that failed — and small
/// enough not to throw a limb across the stage.
pub const DEFAULT_RANGE_PX: f32 = 60.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn cc(n: u8) -> Source {
        Source::MidiCc { channel: 0, cc: n }
    }

    /// **A channel appears when it speaks.** Nothing here has a list of what
    /// a controller might send, which is why an operator finds a knob by
    /// turning it rather than by reading a manual.
    #[test]
    fn a_channel_is_created_the_first_time_a_value_arrives() {
        let mut bus = Bus::default();
        assert!(bus.channels.is_empty());

        let id = bus.feed(Update {
            source: cc(12),
            value: 0.5,
        });
        assert_eq!(bus.channels.len(), 1);
        assert_eq!(bus.channel(id).unwrap().value, 0.5);

        bus.feed(Update {
            source: cc(12),
            value: 0.9,
        });
        assert_eq!(bus.channels.len(), 1, "the same knob is not two channels");
        assert_eq!(bus.channel(id).unwrap().value, 0.9);
    }

    #[test]
    fn different_sources_are_different_channels() {
        let mut bus = Bus::default();
        bus.feed(Update {
            source: cc(12),
            value: 0.1,
        });
        bus.feed(Update {
            source: Source::Osc {
                address: "/puppet/lean".into(),
            },
            value: 0.2,
        });
        assert_eq!(bus.channels.len(), 2);
    }

    /// **Learn takes the next thing that moves, not the next thing that
    /// twitches.** A controller at rest still sends jitter, and a fader
    /// brushed by a sleeve should not steal the binding.
    #[test]
    fn learn_ignores_a_channel_that_barely_moves() {
        let mut bus = Bus::default();
        let target = Target::JointX(PuppetId(1), JointId(2));
        bus.feed(Update {
            source: cc(12),
            value: 0.50,
        });
        bus.learn = Some(target);

        bus.feed(Update {
            source: cc(12),
            value: 0.51,
        });
        assert!(bus.bindings.is_empty(), "jitter must not bind");

        bus.feed(Update {
            source: cc(12),
            value: 0.90,
        });
        assert_eq!(bus.bindings.len(), 1, "a real move binds");
        assert_eq!(bus.bindings[0].dst, target);
    }

    /// Re-learning a target replaces its binding. Two sources on one joint
    /// would fight, and nothing on screen would say which was winning.
    #[test]
    fn learning_the_same_target_twice_leaves_one_binding() {
        let mut bus = Bus::default();
        let target = Target::JointY(PuppetId(1), JointId(2));

        bus.learn = Some(target);
        bus.feed(Update {
            source: cc(1),
            value: 1.0,
        });
        bus.learn = Some(target);
        bus.feed(Update {
            source: cc(2),
            value: 1.0,
        });

        assert_eq!(bus.bindings.len(), 1);
        let src = bus.bindings[0].src;
        assert_eq!(bus.channel(src).unwrap().source, cc(2), "the newer wins");
    }

    /// **A source is not a trusted input.** An OSC sender that overshoots
    /// should reach the end of the range and stop, not throw the limb off
    /// the stage.
    #[test]
    fn a_binding_clamps_whatever_arrives() {
        let b = Binding {
            src: ChannelId(1),
            dst: Target::JointX(PuppetId(1), JointId(1)),
            low: -10.0,
            high: 10.0,
            on: true,
            envelope: Envelope::default(),
        };
        assert_eq!(b.map(0.0), -10.0);
        assert_eq!(b.map(1.0), 10.0);
        assert_eq!(b.map(0.5), 0.0);
        assert_eq!(b.map(-5.0), -10.0, "under");
        assert_eq!(b.map(99.0), 10.0, "over");
    }

    /// Off is not the same as absent: a controller sending something
    /// unwanted must be silenceable without unplugging it mid-show.
    #[test]
    fn a_silenced_channel_drives_nothing() {
        let mut bus = Bus::default();
        let target = Target::JointX(PuppetId(1), JointId(2));
        bus.learn = Some(target);
        let id = bus.feed(Update {
            source: cc(12),
            value: 1.0,
        });

        assert_eq!(bus.offsets().len(), 1);
        bus.channels.iter_mut().find(|c| c.id == id).unwrap().on = false;
        assert!(bus.offsets().is_empty());
    }

    #[test]
    fn a_disabled_binding_drives_nothing_either() {
        let mut bus = Bus {
            learn: Some(Target::JointX(PuppetId(1), JointId(2))),
            ..Bus::default()
        };
        bus.feed(Update {
            source: cc(12),
            value: 1.0,
        });
        bus.bindings[0].on = false;
        assert!(bus.offsets().is_empty());
    }

    /// **A generator is a channel like any other.** That is the whole point
    /// of building it this way: binding a joint to an LFO uses the same code
    /// as binding it to a fader, and an envelope shapes both the same.
    #[test]
    fn a_generator_channel_is_bindable_like_any_other() {
        let mut bus = Bus::default();
        let id = bus.add_generator(false);
        bus.bindings.push(Binding {
            src: id,
            dst: Target::JointX(PuppetId(1), JointId(2)),
            low: -10.0,
            high: 10.0,
            on: true,
            envelope: Envelope::default(),
        });
        bus.tick(0.5, 0.0);
        assert_eq!(bus.offsets().len(), 1);
    }

    /// A free LFO runs on the clock; a locked one runs on the transport and
    /// ignores the clock, which is what keeps it in step when the tempo
    /// changes mid-show.
    #[test]
    fn a_locked_generator_follows_beats_and_a_free_one_follows_seconds() {
        let mut bus = Bus::default();
        let free = bus.add_generator(false);
        let locked = bus.add_generator(true);

        bus.tick(0.0, 2.0);
        assert_eq!(
            bus.channel(free).unwrap().value,
            0.0,
            "no time passed, so the free one has not moved"
        );
        assert!(
            bus.channel(locked).unwrap().value > 0.01,
            "beats moved the locked one"
        );

        bus.tick(2.0, 0.0);
        assert!(
            bus.channel(free).unwrap().value > 0.0,
            "seconds moved the free one"
        );
    }

    /// Sample-and-hold, not white noise: a value that changed every frame
    /// would read as a broken joint rather than as a choice, and the same
    /// cycle must give the same value or a pattern is unrehearsable.
    #[test]
    fn the_noise_shape_holds_one_value_per_cycle() {
        let a = Shape::Noise.at(3.1);
        let b = Shape::Noise.at(3.9);
        let c = Shape::Noise.at(4.1);
        assert_eq!(a, b, "the same cycle is the same value");
        assert_ne!(a, c, "a new cycle is a new value");
    }

    #[test]
    fn every_shape_stays_inside_zero_and_one() {
        for shape in Shape::ALL {
            for i in 0..200 {
                let v = shape.at(i as f32 / 37.0);
                assert!(
                    (0.0..=1.0).contains(&v),
                    "{} left the band: {v}",
                    shape.label()
                );
            }
        }
    }

    /// **The envelope is on the input.** It reshapes whatever arrives, which
    /// is what keeps live input and keyframed shape one system instead of
    /// two.
    #[test]
    fn the_envelope_reshapes_the_value_before_the_range_is_applied() {
        let mut envelope = Envelope::default();
        envelope.add(Keyframe {
            phase: 0.5,
            value: 0.9,
            curve: Curve::Linear,
        });
        let b = Binding {
            src: ChannelId(1),
            dst: Target::JointX(PuppetId(1), JointId(1)),
            low: 0.0,
            high: 100.0,
            on: true,
            envelope,
        };
        assert!(
            (b.map(0.5) - 90.0).abs() < 1e-3,
            "half in should give ninety out, not fifty"
        );
        assert_eq!(b.map(0.0), 0.0, "and the ends are still the ends");
        assert_eq!(b.map(1.0), 100.0);
    }

    /// Rotation is measured in degrees, so a binding on it must not inherit
    /// a range meant for pixels.
    #[test]
    fn a_rotation_binding_gets_a_range_in_degrees() {
        let rotation = Target::JointRotation(PuppetId(1), JointId(1));
        let position = Target::JointX(PuppetId(1), JointId(1));
        assert_ne!(rotation.default_range(), position.default_range());
        assert_ne!(rotation.unit(), position.unit());
    }
}
