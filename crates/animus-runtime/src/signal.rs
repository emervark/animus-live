//! The signal bus, wired into the frame.
//!
//! [`animus_signal`] owns the sources and the mapping; this owns the systems
//! that connect them to a puppet. Both are small on purpose — the
//! interesting decisions live in the crate that can be tested without an
//! engine.
//!
//! ## Three pieces, because one of them cannot be shared
//!
//! A MIDI connection and an `mpsc::Receiver` are `Send` but not `Sync`, so
//! neither can be an ordinary Bevy resource. Rather than wrap the whole bus
//! in a mutex — which would put a lock on the path the panel reads every
//! frame — the parts are split by what they need:
//!
//! - [`SignalBusRes`] is the bus itself: plain data, an ordinary resource,
//!   read freely by the panels.
//! - [`Sources`] holds the connections and the receivers, and is non-send.
//!   Exactly one system touches it.
//! - [`SignalStatus`] is what the sources had to say about themselves,
//!   copied out once at startup so the title bar never reaches across.
//!
//! ## Order, and why it is the order
//!
//! Bindings write **before** the hand, and they skip whatever is held. That
//! is the rule the sequencer follows and the rule the whole instrument rests
//! on: **grabbing a limb takes it over.** A binding that could not be
//! overridden would mean a controller left at the wrong value in the middle
//! of a show and no way to fix it but to unplug something.

use bevy::prelude::*;
use glam::Vec2;

use animus_core::ids::{JointId, PuppetId};
use animus_signal::{Bus, Target, midi, osc};

use crate::components::{CompiledRigRef, PuppetRoot};
use crate::solve::{HeldJoint, JointTargets, LiveRotations};

/// Everything arriving and everything it is wired to. Plain data.
#[derive(Resource, Debug, Default)]
pub struct SignalBusRes {
    pub bus: Bus,
    /// Which joints a binding wrote last frame, so they can be released when
    /// the binding stops.
    driven: Vec<(PuppetId, JointId)>,
    /// And which it turned, which are released the same way.
    turned: Vec<(PuppetId, JointId)>,
}

/// The live connections. Non-send: a MIDI port belongs to the thread that
/// opened it.
pub struct Sources {
    midi_rx: std::sync::mpsc::Receiver<animus_signal::Update>,
    osc_rx: std::sync::mpsc::Receiver<animus_signal::Update>,
    _midi: midi::MidiIn,
    _osc: osc::OscIn,
}

/// What the sources managed to open, for the panels to report honestly.
#[derive(Resource, Debug, Clone, Default)]
pub struct SignalStatus {
    /// Names of the MIDI input ports that opened.
    pub midi_ports: Vec<String>,
    pub midi_trouble: Option<String>,
    /// The address the OSC socket is listening on, if it opened.
    pub osc_bound: Option<String>,
    pub osc_trouble: Option<String>,
}

impl SignalStatus {
    pub fn midi_live(&self) -> bool {
        !self.midi_ports.is_empty()
    }

    pub fn osc_live(&self) -> bool {
        self.osc_bound.is_some()
    }

    /// A short line for the title bar's chip.
    pub fn midi_summary(&self) -> String {
        match self.midi_ports.len() {
            0 => self
                .midi_trouble
                .clone()
                .unwrap_or_else(|| "no MIDI input port found".into()),
            1 => self.midi_ports[0].clone(),
            n => format!("{n} ports: {}", self.midi_ports.join(", ")),
        }
    }

    pub fn osc_summary(&self) -> String {
        match (&self.osc_bound, &self.osc_trouble) {
            (Some(at), _) => format!("listening on {at}"),
            (None, Some(why)) => why.clone(),
            (None, None) => "not listening".into(),
        }
    }
}

pub struct SignalPlugin;

impl Plugin for SignalPlugin {
    fn build(&self, app: &mut App) {
        // Opened once at startup rather than lazily: a controller plugged in
        // before the app started should work without anyone touching a
        // setting, and that only happens if the ports are opened eagerly.
        let (midi_rx, midi) = midi::open();
        let (osc_rx, osc) = osc::open(osc::DEFAULT_PORT);

        if let Some(trouble) = &midi.trouble {
            warn!("MIDI: {trouble}");
        }
        if let Some(trouble) = &osc.trouble {
            warn!("OSC: {trouble}");
        }
        info!(
            "signal bus: {} MIDI port(s), OSC {}",
            midi.ports.len(),
            osc.bound.clone().unwrap_or_else(|| "off".into())
        );

        app.init_resource::<SignalBusRes>()
            .insert_resource(SignalStatus {
                midi_ports: midi.ports.clone(),
                midi_trouble: midi.trouble.clone(),
                osc_bound: osc.bound.clone(),
                osc_trouble: osc.trouble.clone(),
            })
            .insert_non_send(Sources {
                midi_rx,
                osc_rx,
                _midi: midi,
                _osc: osc,
            })
            .add_systems(
                Update,
                (drain_sources, tick_generators, apply_bindings).chain(),
            );
    }
}

/// Take whatever the source threads produced since the last frame.
///
/// Non-blocking by construction. A source thread that has wedged costs this
/// nothing, which is the entire reason the threads exist.
pub fn drain_sources(mut signal: ResMut<SignalBusRes>, sources: NonSend<Sources>) {
    signal.bus.drain(&sources.midi_rx);
    signal.bus.drain(&sources.osc_rx);
}

/// Advance the generators: LFOs on the clock, the rest on the transport.
///
/// **A transport-locked generator only moves while the transport does.**
/// That is what "locked" means: stopping the pattern has to stop everything
/// riding on it, or an operator who pressed stop would still have limbs
/// drifting on stage.
pub fn tick_generators(
    time: Res<Time>,
    seq: Res<crate::sequencer::Sequencer>,
    mut signal: ResMut<SignalBusRes>,
) {
    let dt = time.delta_secs();
    let beats = if seq.running {
        dt * seq.bpm.max(1.0) / 60.0
    } else {
        0.0
    };
    signal.bus.tick(dt, beats);
}

/// Turn every live binding into a joint target.
pub fn apply_bindings(
    mut signal: ResMut<SignalBusRes>,
    held: Res<HeldJoint>,
    mut targets: ResMut<JointTargets>,
    mut rotations: ResMut<LiveRotations>,
    rigs: Query<(&PuppetRoot, &CompiledRigRef)>,
) {
    // Release first: a binding switched off, or a channel silenced, has to
    // let its joint go. A written target is a standing instruction, so
    // skipping this would leave the limb pinned wherever the controller
    // happened to be when it was turned off.
    for (puppet, joint) in std::mem::take(&mut signal.driven) {
        if held.0 != Some((puppet, joint)) {
            targets.clear_joint(puppet, joint);
        }
    }
    for (puppet, joint) in std::mem::take(&mut signal.turned) {
        rotations.set(puppet, joint, 0.0);
    }

    let offsets = signal.bus.offsets();
    if offsets.is_empty() {
        return;
    }

    // Both axes of one joint are one target, so they are gathered before
    // anything is written: two bindings on the same joint must combine
    // rather than take turns overwriting each other.
    let mut wanted: Vec<((PuppetId, JointId), Vec2)> = Vec::new();
    for (target, offset) in offsets {
        let key = target.joint();
        // Rotation is not a position, so it leaves here by a different door
        // — into `LiveRotations`, where the editor's forward-kinematics
        // system turns one angle into targets for a whole chain. Adding it
        // to an offset would have moved the joint sideways by the number of
        // degrees, which is the sort of thing that looks like a physics bug
        // for an afternoon.
        if let Target::JointRotation(puppet, joint) = target {
            if held.0 != Some((puppet, joint)) {
                rotations.set(puppet, joint, offset.to_radians());
                signal.turned.push((puppet, joint));
            }
            continue;
        }
        let delta = match target {
            Target::JointX(..) => Vec2::new(offset, 0.0),
            Target::JointY(..) => Vec2::new(0.0, offset),
            Target::JointRotation(..) => unreachable!("handled above"),
        };
        match wanted.iter_mut().find(|(k, _)| *k == key) {
            Some((_, sum)) => *sum += delta,
            None => wanted.push((key, delta)),
        }
    }

    for ((puppet, joint), offset) in wanted {
        // **The hand wins.** Same rule as the sequencer: grabbing a limb
        // takes it over, and letting go hands it back.
        if held.0 == Some((puppet, joint)) {
            continue;
        }
        let Some((_, rig)) = rigs.iter().find(|(root, _)| root.0 == puppet) else {
            continue;
        };
        let Some(dense) = rig.0.joint_index(joint) else {
            continue;
        };
        let Some(rest) = rig.0.joint_rest(dense as usize) else {
            continue;
        };
        // Offsets are measured from rest, not from wherever the limb
        // currently is: an absolute mapping means a fader at the same place
        // always means the same pose, which is the only way a cue repeats.
        targets.set(puppet, joint, rest + offset);
        signal.driven.push((puppet, joint));
    }
}
