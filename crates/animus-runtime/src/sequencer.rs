//! The step sequencer: a drum machine for a puppet.
//!
//! Musical time is divided into a grid of equal steps. Each **track** owns a
//! joint and a direction, and each cell in that track's row is a **hit**: a
//! shove given to that joint and everything hanging off it when the playhead
//! crosses the cell.
//!
//! ## Why a hit and not a pose
//!
//! A pose says where a limb should *be*; a hit says what happens *to* it. In
//! a mass-spring puppet the second is the honest one. A hit is delivered as
//! velocity — `pos` moves, `prev` does not, and in Verlet that difference is
//! the velocity — so the puppet leaves rest at speed and the bones and
//! `rest_pull` take the energy back out at whatever rate that puppet is tuned
//! for. Nothing in here decays anything: **the decay is the physics**, which
//! is why two hits of the same strength on two differently-tuned puppets look
//! like two different characters rather than the same animation twice.
//!
//! Three consequences fall out of it rather than being features:
//!
//! - **Two identical bars never play identically.** Where the limb already
//!   was, and how fast it was already moving, are part of the result.
//! - **The hand wins.** A joint being held is skipped, so grabbing a limb
//!   mid-bar takes it over and letting go hands it back.
//! - **Tracks are limbs, not channels.** One row per joint chain is what
//!   makes a pattern readable: the arm's rhythm is one line you can see.
//!
//! ## The chain, and why it falls off
//!
//! A hit travels down the tree from its joint, scaled by `FALLOFF^depth`.
//! Hitting a shoulder should carry the arm — a shoulder that moved while its
//! own hand stayed put is not a shoulder, it is a dislocation — but the hand
//! should arrive later and softer, which is exactly what a decaying impulse
//! down the chain produces once the springs are involved.
//!
//! Patterns are session state, not document state: the v1 file format has no
//! place for them yet, so a pattern lives until the app closes. That is a
//! deliberate limit and not a hidden one — the panel says so.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use glam::Vec2;

use animus_core::ids::{JointId, PuppetId};

use crate::project::DocumentRes;
use crate::solve::HeldJoint;

/// How far a hit reaches down the chain: each step away keeps this much.
///
/// From the comp. Low enough that a hand is a suggestion rather than a copy
/// of the shoulder, high enough that the limb moves as one thing.
pub const FALLOFF: f32 = 0.78;

/// A ghost hit's strength, as a fraction of a full one.
pub const GHOST: f32 = 0.45;
pub const FULL: f32 = 1.0;

/// The step counts the grid offers.
///
/// A bar, two, four. Every one divides by four, so a pattern still reads as
/// bars when the grid length changes under it.
pub const STEP_COUNTS: [usize; 3] = [8, 16, 32];

/// How a step relates to the beat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quantize {
    Quarter,
    #[default]
    Eighth,
    Sixteenth,
}

impl Quantize {
    pub const ALL: [Quantize; 3] = [Quantize::Quarter, Quantize::Eighth, Quantize::Sixteenth];

    pub fn label(self) -> &'static str {
        match self {
            Quantize::Quarter => "1/4",
            Quantize::Eighth => "1/8",
            Quantize::Sixteenth => "1/16",
        }
    }

    /// Steps per beat.
    pub fn division(self) -> f32 {
        match self {
            Quantize::Quarter => 1.0,
            Quantize::Eighth => 2.0,
            Quantize::Sixteenth => 4.0,
        }
    }
}

/// One row of the grid: a limb, a direction, and when it gets hit.
#[derive(Debug, Clone)]
pub struct Track {
    pub name: String,
    pub puppet: PuppetId,
    /// The joint the hit lands on. Everything below it comes along.
    pub joint: JointId,
    /// Which way the shove goes, in image pixels per hit.
    pub dir: Vec2,
    /// The row's colour in the grid. Tracks are told apart by colour before
    /// they are told apart by name, because the grid is read at a glance.
    pub ink: [u8; 3],
    pub mute: bool,
    pub solo: bool,
    /// `0.0` empty, [`GHOST`] quiet, [`FULL`] hard.
    pub steps: Vec<f32>,
}

impl Track {
    pub fn new(
        name: impl Into<String>,
        puppet: PuppetId,
        joint: JointId,
        dir: Vec2,
        ink: [u8; 3],
    ) -> Self {
        Self {
            name: name.into(),
            puppet,
            joint,
            dir,
            ink,
            mute: false,
            solo: false,
            steps: vec![0.0; *STEP_COUNTS.last().unwrap()],
        }
    }

    /// How many cells in the visible length hold a hit.
    pub fn hits(&self, len: usize) -> usize {
        self.steps.iter().take(len).filter(|v| **v > 0.0).count()
    }
}

/// The colours new tracks take, in order. Signal colours, because a track
/// *is* a live thing, and reused round-robin once they run out.
pub const TRACK_INKS: [[u8; 3]; 6] = [
    [0x57, 0xC8, 0x78],
    [0x45, 0xC8, 0xE8],
    [0xE3, 0xA9, 0x4F],
    [0xB9, 0x8B, 0xE8],
    [0xF2, 0x60, 0x6A],
    [0x8F, 0x8F, 0xFF],
];

/// The grid, the transport, and what is being recorded into it.
#[derive(Resource, Debug)]
pub struct Sequencer {
    pub tracks: Vec<Track>,
    /// Visible steps: one of [`STEP_COUNTS`].
    pub len: usize,
    pub quantize: Quantize,
    pub bpm: f32,
    pub running: bool,
    /// Position in steps, fractional. Wraps at `len`.
    pub position: f32,
    /// The track TAP writes into, and the one the footer's actions act on.
    pub selected: usize,
    /// Record is armed by hand. **Entering PERFORM never arms it**: an editor
    /// that starts recording because you changed screens is an editor you
    /// cannot trust with a show.
    pub armed: bool,
    /// A TAP held down this frame, to be written under the playhead.
    pub tapping: bool,
    /// Set on the frame a step edge is crossed, for the panel's flash.
    pub fired: Vec<f32>,
}

impl Default for Sequencer {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            len: 16,
            quantize: Quantize::default(),
            bpm: 120.0,
            running: false,
            position: 0.0,
            selected: 0,
            armed: false,
            tapping: false,
            fired: Vec::new(),
        }
    }
}

impl Sequencer {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// The step the playhead is on.
    pub fn current(&self) -> usize {
        (self.position as usize).min(self.len.saturating_sub(1))
    }

    /// Seconds per step, from the tempo and the grid division.
    pub fn step_seconds(&self) -> f32 {
        (60.0 / self.bpm.max(1.0)) / self.quantize.division()
    }

    /// Whether this track sounds right now.
    ///
    /// **Solo wins over mute, globally.** With any track soloed the others
    /// are silent whatever their own mute says — which is what makes solo
    /// usable mid-show: one click isolates a limb and one click restores the
    /// pattern exactly as it was.
    pub fn audible(&self, track: &Track) -> bool {
        if self.tracks.iter().any(|t| t.solo) {
            track.solo
        } else {
            !track.mute
        }
    }

    /// Cycle a cell: empty → full → ghost → empty.
    pub fn cycle(&mut self, track: usize, step: usize) {
        if let Some(t) = self.tracks.get_mut(track)
            && let Some(v) = t.steps.get_mut(step)
        {
            *v = if *v == 0.0 {
                FULL
            } else if *v == FULL {
                GHOST
            } else {
                0.0
            };
        }
    }

    pub fn clear_cell(&mut self, track: usize, step: usize) {
        if let Some(t) = self.tracks.get_mut(track)
            && let Some(v) = t.steps.get_mut(step)
        {
            *v = 0.0;
        }
    }

    pub fn clear_track(&mut self, track: usize) {
        if let Some(t) = self.tracks.get_mut(track) {
            t.steps.iter_mut().for_each(|v| *v = 0.0);
        }
    }

    pub fn clear_all(&mut self) {
        for t in &mut self.tracks {
            t.steps.iter_mut().for_each(|v| *v = 0.0);
        }
    }

    /// Change the visible length.
    ///
    /// **Shrinking never destroys a hit.** The steps beyond the new length
    /// keep their values and come back when the grid grows again: a length
    /// button that quietly deleted the second half of a pattern would be a
    /// button nobody dares press twice.
    pub fn set_len(&mut self, n: usize) {
        self.len = n.max(1);
        let need = self.len;
        for t in &mut self.tracks {
            if t.steps.len() < need {
                t.steps.resize(need, 0.0);
            }
        }
        if self.position >= self.len as f32 {
            self.position = 0.0;
        }
    }

    pub fn select(&mut self, track: usize) {
        if track < self.tracks.len() {
            self.selected = track;
        }
    }

    pub fn add_track(
        &mut self,
        name: impl Into<String>,
        puppet: PuppetId,
        joint: JointId,
        dir: Vec2,
    ) {
        let ink = TRACK_INKS[self.tracks.len() % TRACK_INKS.len()];
        let mut track = Track::new(name, puppet, joint, dir, ink);
        track.steps.resize(self.len.max(track.steps.len()), 0.0);
        self.tracks.push(track);
        self.selected = self.tracks.len() - 1;
    }

    pub fn remove_track(&mut self, track: usize) {
        if track < self.tracks.len() {
            self.tracks.remove(track);
            self.selected = self.selected.min(self.tracks.len().saturating_sub(1));
        }
    }
}

/// Where the sequencer writes: before the hand, so the hand wins.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SequencerSet {
    Play,
}

pub struct SequencerPlugin;

impl Plugin for SequencerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sequencer>()
            .add_systems(Update, run_sequencer.in_set(SequencerSet::Play));
    }
}

/// Walk the grid and deliver whatever each step says.
pub fn run_sequencer(
    time: Res<Time>,
    mut seq: ResMut<Sequencer>,
    doc: Res<DocumentRes>,
    held: Res<HeldJoint>,
    mut solvers: Query<(
        &crate::components::PuppetRoot,
        &crate::components::CompiledRigRef,
        &mut crate::components::PuppetSolver,
    )>,
) {
    // The flash lasts one frame and is cleared whether or not the transport
    // is running, so stopping mid-bar does not leave a row lit.
    let track_count = seq.tracks.len();
    seq.fired.clear();
    seq.fired.resize(track_count, 0.0);

    // TAP is independent of the transport: holding it plays the limb whether
    // or not a pattern is running, which is how an operator finds the sound
    // before committing it to a step.
    let tapping = std::mem::take(&mut seq.tapping);
    if tapping {
        let step = seq.current();
        let selected = seq.selected;
        if let Some(track) = seq.tracks.get(selected) {
            let (puppet, joint, dir) = (track.puppet, track.joint, track.dir);
            strike(&doc, &held, &mut solvers, puppet, joint, dir, FULL);
            if let Some(f) = seq.fired.get_mut(selected) {
                *f = FULL;
            }
        }
        if seq.armed
            && seq.running
            && let Some(t) = seq.tracks.get_mut(selected)
        {
            // Quantised to the step under the playhead rather than written
            // at the exact instant: this is a grid, and a hit half a step
            // late is a hit in the wrong place.
            if let Some(v) = t.steps.get_mut(step) {
                *v = FULL;
            }
        }
    }

    if !seq.running || seq.tracks.is_empty() {
        return;
    }

    let dt = time.delta_secs();
    let len = seq.len;
    let before = seq.current();
    seq.position = (seq.position + dt / seq.step_seconds()) % len as f32;
    let after = seq.current();

    // `!=` rather than `>`: the pattern wraps, and the wrap is a step edge
    // like any other — the one that starts the bar.
    if after == before {
        return;
    }

    for i in 0..seq.tracks.len() {
        let Some(track) = seq.tracks.get(i) else {
            continue;
        };
        let velocity = track.steps.get(after).copied().unwrap_or(0.0);
        if velocity <= 0.0 || !seq.audible(track) {
            continue;
        }
        let (puppet, joint, dir) = (track.puppet, track.joint, track.dir);
        strike(&doc, &held, &mut solvers, puppet, joint, dir, velocity);
        if let Some(f) = seq.fired.get_mut(i) {
            *f = velocity;
        }
    }
}

/// Deliver one hit: the joint and everything below it, softening with depth.
#[allow(clippy::too_many_arguments)]
fn strike(
    doc: &DocumentRes,
    held: &HeldJoint,
    solvers: &mut Query<(
        &crate::components::PuppetRoot,
        &crate::components::CompiledRigRef,
        &mut crate::components::PuppetSolver,
    )>,
    puppet: PuppetId,
    joint: JointId,
    dir: Vec2,
    velocity: f32,
) {
    let Some(animus_core::doc::PuppetKind::Mesh(mesh)) =
        doc.0.puppets.get(&puppet).map(|p| &p.kind)
    else {
        return;
    };
    // Derived from the bone graph, the same way forward kinematics derives
    // it, so what a hit carries and what a rotation carries are the same set
    // of joints. Two answers to that question would be one too many.
    let tree = animus_core::skeleton::rig_tree(&mesh.skeleton);
    let chain: Vec<(JointId, usize)> = std::iter::once((joint, 0))
        .chain(depths(&tree, joint))
        .collect();

    for (root, rig, mut solver) in solvers.iter_mut() {
        // The query is over every puppet on the stage; only one of them is
        // the track's. Matching on the root rather than on "does this rig
        // happen to know that joint id" matters once two puppets share id
        // numbering, which they do — ids are per-document, not per-puppet.
        if root.0 != puppet {
            continue;
        }
        for (id, depth) in &chain {
            if held.0 == Some((puppet, *id)) {
                // The hand outranks the pattern.
                continue;
            }
            let Some(dense) = rig.0.joint_index(*id) else {
                continue;
            };
            solver
                .0
                .kick(dense, dir * velocity * FALLOFF.powi(*depth as i32));
        }
    }
}

/// Every joint below `from`, paired with how far below it is.
fn depths(tree: &animus_core::skeleton::RigTree, from: JointId) -> Vec<(JointId, usize)> {
    let mut out = Vec::new();
    let mut depth: HashMap<JointId, usize> = HashMap::new();
    depth.insert(from, 0);
    for id in tree.descendants(from) {
        let d = tree
            .parent(id)
            .and_then(|p| depth.get(&p).copied())
            .unwrap_or(0)
            + 1;
        depth.insert(id, d);
        out.push((id, d));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str) -> Track {
        Track::new(
            name,
            PuppetId(1),
            JointId(1),
            Vec2::new(1.0, 0.0),
            TRACK_INKS[0],
        )
    }

    /// **Solo wins over mute, globally.** With one track soloed the others
    /// go quiet whatever their own mute says, so one click isolates a limb
    /// and one click restores the pattern exactly as it was.
    #[test]
    fn solo_silences_every_track_that_is_not_soloed() {
        let mut seq = Sequencer::default();
        seq.tracks.push(track("a"));
        seq.tracks.push(track("b"));
        seq.tracks.push(track("c"));
        seq.tracks[2].mute = true;

        assert!(seq.audible(&seq.tracks[0]), "nothing soloed: mute decides");
        assert!(!seq.audible(&seq.tracks[2]));

        seq.tracks[1].solo = true;
        assert!(!seq.audible(&seq.tracks[0]), "not soloed, so silent");
        assert!(seq.audible(&seq.tracks[1]));
        assert!(
            !seq.audible(&seq.tracks[2]),
            "a muted track stays silent when another is soloed"
        );
    }

    #[test]
    fn a_cell_cycles_empty_full_ghost_empty() {
        let mut seq = Sequencer::default();
        seq.tracks.push(track("a"));
        assert_eq!(seq.tracks[0].steps[3], 0.0);
        seq.cycle(0, 3);
        assert_eq!(seq.tracks[0].steps[3], FULL);
        seq.cycle(0, 3);
        assert_eq!(seq.tracks[0].steps[3], GHOST);
        seq.cycle(0, 3);
        assert_eq!(seq.tracks[0].steps[3], 0.0);
    }

    /// **Shrinking the grid never destroys a hit.** A length button that
    /// quietly deleted the second half of a pattern is a button nobody
    /// dares press twice.
    #[test]
    fn shrinking_the_grid_keeps_the_steps_beyond_it() {
        let mut seq = Sequencer::default();
        seq.tracks.push(track("a"));
        seq.set_len(32);
        seq.cycle(0, 30);
        assert_eq!(seq.tracks[0].steps[30], FULL);

        seq.set_len(8);
        assert_eq!(
            seq.tracks[0].steps[30], FULL,
            "the hit must survive the round trip"
        );
        seq.set_len(32);
        assert_eq!(seq.tracks[0].steps[30], FULL);
    }

    /// A hit reaches down the chain and softens as it goes. Monotonic, so
    /// no joint below ever gets a harder shove than the one it hangs from.
    #[test]
    fn the_hit_softens_with_every_step_down_the_chain() {
        let mut last = f32::INFINITY;
        for depth in 0..6 {
            let f = FALLOFF.powi(depth);
            assert!(f < last, "depth {depth} did not soften");
            assert!(f > 0.0, "and never reverses");
            last = f;
        }
    }

    #[test]
    fn quantize_divides_the_beat() {
        let mut seq = Sequencer {
            bpm: 120.0,
            quantize: Quantize::Quarter,
            ..Sequencer::default()
        };
        let quarter = seq.step_seconds();
        seq.quantize = Quantize::Sixteenth;
        assert!(
            (quarter / seq.step_seconds() - 4.0).abs() < 1e-4,
            "a sixteenth is a quarter of a quarter"
        );
    }

    #[test]
    fn tracks_take_different_colours_until_the_palette_runs_out() {
        let mut seq = Sequencer::default();
        for i in 0..TRACK_INKS.len() {
            seq.add_track(format!("t{i}"), PuppetId(1), JointId(1), Vec2::X);
        }
        let inks: Vec<_> = seq.tracks.iter().map(|t| t.ink).collect();
        let mut unique = inks.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), TRACK_INKS.len(), "no two rows share a colour");
    }
}
