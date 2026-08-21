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
}

impl Source {
    /// What the operator sees in the list.
    pub fn label(&self) -> String {
        match self {
            Source::MidiCc { channel, cc } => format!("midi {}·cc {cc}", channel + 1),
            Source::Osc { address } => format!("osc {address}"),
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
}

/// What a binding drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    JointX(PuppetId, JointId),
    JointY(PuppetId, JointId),
}

impl Target {
    pub fn joint(self) -> (PuppetId, JointId) {
        match self {
            Target::JointX(p, j) | Target::JointY(p, j) => (p, j),
        }
    }

    pub fn axis(self) -> &'static str {
        match self {
            Target::JointX(..) => "position.x",
            Target::JointY(..) => "position.y",
        }
    }
}

/// A channel wired to a target, with the range it moves through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Binding {
    pub src: ChannelId,
    pub dst: Target,
    /// Where the joint sits when the channel reads 0, as an offset from rest
    /// in image pixels.
    pub low: f32,
    /// And at 1.
    pub high: f32,
    pub on: bool,
}

impl Binding {
    /// The offset this binding asks for, at a given channel value.
    ///
    /// Clamped, because a source is not a trusted input: an OSC sender that
    /// overshoots should reach the end of the range and stop, not throw the
    /// limb off the stage.
    pub fn map(&self, value: f32) -> f32 {
        let t = value.clamp(0.0, 1.0);
        self.low + (self.high - self.low) * t
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

    fn bind_if_learning(&mut self, src: ChannelId) {
        let Some(dst) = self.learn.take() else { return };
        // Re-arming over an existing binding replaces it rather than stacking
        // a second one on the same target, which would leave two sources
        // fighting with no way to see why.
        self.bindings.retain(|b| b.dst != dst);
        self.bindings.push(Binding {
            src,
            dst,
            low: -DEFAULT_RANGE_PX,
            high: DEFAULT_RANGE_PX,
            on: true,
        });
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
}
