//! The step sequencer: a drum machine for poses.
//!
//! Musical time is divided into a grid of equal steps. A step holds a **pose**
//! — where each joint should be — and the playhead walks the grid at a tempo.
//! On every step edge the pose is written into [`JointTargets`], and the
//! springs carry the puppet from wherever it is to wherever the step says.
//!
//! **The in-betweens are physics, not keyframes.** That is the whole design.
//! An animator would author the frames between two poses; here the solver
//! does it, and it does it differently every time depending on where the
//! puppet already was, how fast it was moving, and whether a hand is holding
//! part of it. Two identical bars never play identically, which is what makes
//! this an instrument rather than a player.
//!
//! Three consequences fall out of it rather than being features:
//!
//! - **A step is a target, not a command.** A puppet that cannot reach the
//!   pose in one step simply arrives late, and the lateness reads as weight.
//! - **The hand wins.** A joint being held is skipped, so grabbing a limb
//!   mid-bar takes it over and letting go hands it back.
//! - **Recording is the same act as playing.** With the transport running and
//!   record armed, whatever pose the puppet is in when the playhead crosses a
//!   step is written into that step — exactly how a drum machine records.
//!
//! Steps are session state, not document state: the v1 file format has no
//! place for them yet, so a pattern lives until the app closes. That is a
//! deliberate limit and not a hidden one — the panel says so.

use bevy::prelude::*;
use glam::Vec2;

use animus_core::ids::{JointId, PuppetId};

use crate::solve::{HeldJoint, JointTargets};

/// Where every driven joint should be at one step.
pub type Pose = Vec<(PuppetId, JointId, Vec2)>;

/// The step counts the grid offers.
///
/// A bar, two, four. An arbitrary length is a worse instrument, not a more
/// flexible one — and every one of these divides by four, so a pattern still
/// reads as a bar when the grid changes under it.
pub const STEP_COUNTS: [usize; 3] = [4, 8, 16];

/// The grid, the transport, and what is being recorded into it.
#[derive(Resource, Debug)]
pub struct Sequencer {
    /// One slot per step. `None` is a rest, and rests are as much of a
    /// pattern as hits: a puppet left alone for two steps is a puppet the
    /// springs are still settling.
    pub steps: Vec<Option<Pose>>,
    pub bpm: f32,
    pub running: bool,
    /// Position in steps, fractional. Wraps at `steps.len()`.
    pub position: f32,
    /// The step being edited, and the one a live recording writes into first.
    pub selected: usize,
    /// Record is armed by hand. **Entering PERFORM never arms it**: an editor
    /// that starts recording because you changed screens is an editor you
    /// cannot trust with a show.
    pub armed: bool,
    /// Where the glide starts: the pose the puppet was actually in when the
    /// playhead crossed into the current step.
    ///
    /// Taken from the puppet rather than from the previous step, because the
    /// operator may have grabbed a limb: the glide has to start from where
    /// the puppet *is*, not from where the pattern last told it to be.
    glide_from: Pose,
    /// What the last fired step wrote, so it can be taken back.
    ///
    /// A target is a standing instruction — the solver pins that joint until
    /// something removes it — so a step that fires without clearing the
    /// previous one leaves joints pinned by a pose two bars old.
    driven_last: Vec<(PuppetId, JointId)>,
}

impl Default for Sequencer {
    fn default() -> Self {
        Self {
            steps: vec![None; 8],
            bpm: 120.0,
            running: false,
            position: 0.0,
            selected: 0,
            armed: false,
            glide_from: Pose::new(),
            driven_last: Vec::new(),
        }
    }
}

impl Sequencer {
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The step the playhead is on.
    pub fn current(&self) -> usize {
        (self.position as usize).min(self.steps.len().saturating_sub(1))
    }

    /// How far through the current step the playhead is, 0..1.
    pub fn step_fraction(&self) -> f32 {
        self.position.fract().clamp(0.0, 1.0)
    }

    /// Seconds per step. One step is one beat.
    pub fn step_seconds(&self) -> f32 {
        60.0 / self.bpm.max(1.0)
    }

    pub fn pose(&self, step: usize) -> Option<&Pose> {
        self.steps.get(step).and_then(|s| s.as_ref())
    }

    /// How many steps hold a pose.
    pub fn filled(&self) -> usize {
        self.steps.iter().filter(|s| s.is_some()).count()
    }

    pub fn select(&mut self, step: usize) {
        if step < self.steps.len() {
            self.selected = step;
        }
    }

    pub fn set_pose(&mut self, step: usize, pose: Pose) {
        if let Some(slot) = self.steps.get_mut(step) {
            *slot = Some(pose);
        }
    }

    /// Write one joint into a step, keeping everything else the step held.
    ///
    /// This is what live recording uses. Replacing the whole pose would
    /// discard the parts of the pattern the operator authored earlier and is
    /// not currently touching — an overdub that erases the take.
    pub fn overdub(&mut self, step: usize, puppet: PuppetId, joint: JointId, at: Vec2) {
        let Some(slot) = self.steps.get_mut(step) else {
            return;
        };
        let pose = slot.get_or_insert_with(Pose::new);
        match pose
            .iter_mut()
            .find(|(p, j, _)| *p == puppet && *j == joint)
        {
            Some((_, _, existing)) => *existing = at,
            None => pose.push((puppet, joint, at)),
        }
    }

    pub fn clear_step(&mut self, step: usize) {
        if let Some(slot) = self.steps.get_mut(step) {
            *slot = None;
        }
    }

    pub fn clear_all(&mut self) {
        for slot in &mut self.steps {
            *slot = None;
        }
    }

    /// The last step holding a pose, if any.
    fn last_filled(&self) -> Option<usize> {
        self.steps.iter().rposition(|s| s.is_some())
    }

    /// Grow or shrink the grid.
    ///
    /// **Shrinking never destroys a pose.** If a step beyond the requested
    /// length holds one, the grid stops there instead: the alternative is a
    /// size button that silently deletes work.
    pub fn set_len(&mut self, n: usize) {
        let floor = self.last_filled().map(|i| i + 1).unwrap_or(1);
        let n = n.max(1).max(floor);
        self.steps.resize(n, None);
        if self.position >= n as f32 {
            self.position = 0.0;
        }
        self.selected = self.selected.min(n - 1);
    }

    /// Whether `set_len(n)` would be refused, and by which step — so the panel
    /// can say so rather than appearing to ignore the click.
    pub fn len_blocked_by(&self, n: usize) -> Option<usize> {
        let floor = self.last_filled().map(|i| i + 1)?;
        (n < floor).then_some(floor)
    }
}

/// Where the sequencer writes: before the hand, so the hand wins.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SequencerSet {
    /// Steps → targets.
    Play,
}

pub struct SequencerPlugin;

impl Plugin for SequencerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sequencer>()
            .add_systems(Update, run_sequencer.in_set(SequencerSet::Play));
    }
}

/// Walk the grid, record into it, and glide toward what each step says.
///
/// **The target moves; it does not jump.** A driven target snaps its joint
/// exactly (that is what makes dragging feel direct), so writing the next
/// step's pose in one frame would teleport every posed joint — a cut, not an
/// animation. Instead the written target slides from where the puppet was at
/// the step edge to where the step says, across the step's own duration. The
/// springs then lag behind that sliding target, and the lag is the weight.
pub fn run_sequencer(
    time: Res<Time>,
    mut seq: ResMut<Sequencer>,
    mut targets: ResMut<JointTargets>,
    held: Res<HeldJoint>,
    solvers: Query<(
        &crate::components::PuppetRoot,
        &crate::components::CompiledRigRef,
        &crate::components::PuppetSolver,
    )>,
) {
    if !seq.running || seq.steps.is_empty() {
        return;
    }

    let dt = time.delta_secs();
    let len = seq.len();
    let before = seq.current();
    seq.position = (seq.position + dt / seq.step_seconds()) % len as f32;
    let after = seq.current();

    // `!=` rather than `>`: the pattern wraps, and the wrap is a step edge
    // like any other — the one that starts the bar.
    if after != before {
        // Record first, then glide. Capturing the pose the operator is
        // holding and then gliding from it is a visual no-op, which is what
        // makes live recording feel like it is not disturbing the show.
        let now = capture_from(solvers.iter().map(|(r, rig, s)| (r.0, rig, s)));

        // **Arm records what the hand is playing, not the whole body.**
        //
        // Writing the full snapshot into each step looked right and was
        // wrong: every joint the operator was *not* touching got pinned to
        // wherever it happened to be, so the next step recorded that same
        // frozen body, and the one after that. Across a bar the only thing
        // that ever differed was the limb still in the hand — which is
        // exactly the "only the last change gets saved" the operator saw.
        //
        // Overdub instead, the way a drum machine does: merge the held
        // joint into whatever the step already held, and leave the rest of
        // the pattern alone. Full-body poses are what EDIT is for.
        if seq.armed
            && let Some((puppet, joint)) = held.0
            && let Some((_, _, at)) = now
                .iter()
                .find(|(p, j, _)| *p == puppet && *j == joint)
                .copied()
        {
            seq.overdub(after, puppet, joint, at);
            seq.selected = after;
        }
        seq.glide_from = now;

        // Take back the previous step's writes, but never the held joint:
        // that target belongs to the hand.
        for (puppet, joint) in std::mem::take(&mut seq.driven_last) {
            if held.0 != Some((puppet, joint)) {
                targets.clear_joint(puppet, joint);
            }
        }
    }

    let Some(to) = seq.pose(after).cloned() else {
        // A rest. Nothing is written, so the springs carry on from wherever
        // the last step left the puppet — which is the pattern too.
        return;
    };

    // Ease in and out rather than a constant slide: a linear target arrives
    // and stops dead, and the puppet's own overshoot is the only thing that
    // hides it. Smoothstep puts the acceleration where a limb would have it.
    let now = glide(&seq.glide_from, &to, seq.step_fraction());
    seq.driven_last.clear();
    for (puppet, joint, at) in now {
        if held.0 == Some((puppet, joint)) {
            continue;
        }
        targets.set(puppet, joint, at);
        seq.driven_last.push((puppet, joint));
    }
}

/// Hermite ease, 0..1. Zero slope at both ends.
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Where the written target sits `t` of the way from one pose to another.
///
/// A joint the destination names but the start does not is held at its
/// destination from the first frame: there is nowhere to glide it from, and
/// guessing rest would yank it across the stage.
pub fn glide(from: &Pose, to: &Pose, t: f32) -> Pose {
    let e = smoothstep(t);
    to.iter()
        .map(|(puppet, joint, at)| {
            let start = from
                .iter()
                .find(|(p, j, _)| p == puppet && j == joint)
                .map(|(_, _, v)| *v)
                .unwrap_or(*at);
            (*puppet, *joint, start + (*at - start) * e)
        })
        .collect()
}

/// The puppet's pose right now, in image pixels, as the solver holds it.
///
/// Takes an iterator rather than a `Query` so both the sequencer's own system
/// and the editor's posing path can call it: they hold different queries over
/// the same components, and duplicating the walk would be a second place for
/// the dense order to be read wrongly.
pub fn capture_from<'a>(
    items: impl Iterator<
        Item = (
            PuppetId,
            &'a crate::components::CompiledRigRef,
            &'a crate::components::PuppetSolver,
        ),
    >,
) -> Pose {
    let mut pose = Pose::new();
    for (id, rig, solver) in items {
        let positions = solver.0.positions();
        // Through the rig's dense order, never by the numeric value of a
        // `JointId`: those two agree until a joint is deleted, and then every
        // pose in the pattern would refer to its neighbour.
        for (dense, at) in positions.iter().enumerate() {
            if let Some(joint) = rig.0.joint_id(dense) {
                pose.push((id, joint, *at));
            }
        }
    }
    pose
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: PuppetId = PuppetId(1);
    const J: JointId = JointId(2);
    const K: JointId = JointId(9);

    fn pose_at(x: f32) -> Pose {
        vec![(P, J, Vec2::new(x, 0.0)), (P, K, Vec2::new(0.0, x))]
    }

    fn grid(n: usize, filled: &[(usize, f32)]) -> Sequencer {
        let mut s = Sequencer::default();
        s.set_len(n);
        for (i, x) in filled {
            s.set_pose(*i, pose_at(*x));
        }
        s
    }

    /// Advance the transport by hand, without a `Time`, and report the step
    /// edges crossed.
    fn advance(seq: &mut Sequencer, dt: f32) -> Option<usize> {
        let len = seq.len();
        let before = seq.current();
        seq.position = (seq.position + dt / seq.step_seconds()) % len as f32;
        let after = seq.current();
        (after != before).then_some(after)
    }

    #[test]
    fn a_step_is_one_beat_at_the_tempo() {
        let mut s = grid(8, &[]);
        s.bpm = 120.0;
        assert!((s.step_seconds() - 0.5).abs() < 1e-6);
        s.bpm = 60.0;
        assert!((s.step_seconds() - 1.0).abs() < 1e-6);
    }

    /// The grid divides time into equal parts, and the playhead visits every
    /// one of them in order.
    #[test]
    fn the_playhead_visits_every_step_in_order_and_wraps() {
        let mut s = grid(4, &[]);
        s.bpm = 120.0; // half a second per step
        s.running = true;

        // Six seconds at half a second per step is twelve edges, so the
        // first eight are there whatever the float arithmetic does at the
        // boundary. Sizing the loop to land exactly on the last edge is how
        // this test failed the first time.
        let mut visited = Vec::new();
        for _ in 0..600 {
            if let Some(step) = advance(&mut s, 0.01) {
                visited.push(step);
            }
        }
        assert!(
            visited.len() >= 8,
            "only {} edges: {visited:?}",
            visited.len()
        );
        assert_eq!(&visited[..8], &[1, 2, 3, 0, 1, 2, 3, 0], "got {visited:?}");
    }

    /// Shrinking must not eat a pose.
    #[test]
    fn shrinking_stops_at_the_last_posed_step() {
        let mut s = grid(16, &[(0, 1.0), (12, 2.0)]);
        s.set_len(8);
        assert_eq!(s.len(), 13, "step 12 holds a pose, so the grid stops there");
        assert!(s.pose(12).is_some());
        assert_eq!(s.len_blocked_by(8), Some(13));

        s.clear_step(12);
        s.set_len(8);
        assert_eq!(s.len(), 8);
        assert!(s.pose(0).is_some(), "and the pose that fitted survived");
    }

    /// The selected step has to stay inside the grid, or the panel would
    /// point at a step that no longer exists.
    #[test]
    fn shrinking_pulls_the_selection_back_inside() {
        let mut s = grid(16, &[]);
        s.select(15);
        s.set_len(4);
        assert_eq!(s.selected, 3);
    }

    #[test]
    fn a_rest_is_not_a_missing_pose() {
        let s = grid(4, &[(0, 1.0), (2, 2.0)]);
        assert_eq!(s.filled(), 2);
        assert!(
            s.pose(1).is_none(),
            "step 1 is a rest, and that is a choice"
        );
    }

    // ── the glide ──────────────────────────────────────────────────────

    /// **The bug this closes: steps cut.**
    ///
    /// A driven target snaps its joint exactly, so writing the destination in
    /// one frame teleports every posed joint. The written target has to move
    /// across the step instead — mid-step it must be *between* the two poses,
    /// not at either end.
    #[test]
    fn a_step_glides_rather_than_cutting() {
        let from = vec![(P, J, Vec2::new(0.0, 0.0))];
        let to = vec![(P, J, Vec2::new(100.0, 0.0))];

        let start = glide(&from, &to, 0.0)[0].2.x;
        let middle = glide(&from, &to, 0.5)[0].2.x;
        let end = glide(&from, &to, 1.0)[0].2.x;

        assert!(start.abs() < 1e-4, "the step opens where the puppet was");
        assert!(
            middle > 5.0 && middle < 95.0,
            "halfway must be between the poses, got {middle}"
        );
        assert!((end - 100.0).abs() < 1e-4, "and closes on the pose");
    }

    /// Ease at both ends, so the target does not arrive and stop dead.
    #[test]
    fn the_glide_starts_and_ends_slowly() {
        let d = |a: f32, b: f32| (smoothstep(b) - smoothstep(a)).abs();
        let edge = d(0.0, 0.1).max(d(0.9, 1.0));
        let centre = d(0.45, 0.55);
        assert!(
            centre > edge * 2.0,
            "the middle should move faster than the ends: {centre} vs {edge}"
        );
    }

    /// A joint that appears only in the destination has nowhere to glide
    /// from, so it is held there rather than yanked across the stage from a
    /// guess.
    #[test]
    fn a_joint_the_start_does_not_name_is_held_at_its_destination() {
        let from = vec![(P, J, Vec2::ZERO)];
        let to = vec![(P, J, Vec2::new(10.0, 0.0)), (P, K, Vec2::new(50.0, 50.0))];
        let mid = glide(&from, &to, 0.5);
        let k = mid.iter().find(|(_, j, _)| *j == K).unwrap().2;
        assert_eq!(k, Vec2::new(50.0, 50.0));
    }
}
