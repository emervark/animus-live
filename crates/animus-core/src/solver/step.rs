//! The fixed-tick solver step: Verlet integration, driven targets, then
//! Gauss-Seidel distance-constraint relaxation.

use crate::solver::guard::all_finite;
use crate::solver::{CompiledRig, SolverState};

/// Outcome of a single [`step`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    Ok,
    /// The state went non-finite (NaN/Inf) and was reset to rest. This
    /// puppet only — the rest of the show is unaffected.
    ResetDueToNonFinite,
}

/// Advance `st` by one fixed tick of `dt` seconds under `rig`.
pub fn step(rig: &CompiledRig, st: &mut SolverState, dt: f32) -> StepOutcome {
    let n = rig.rest.len();

    // 1. Verlet integrate.
    for i in 0..n {
        st.prev_tick[i] = st.pos[i];
        if rig.pinned[i] || rig.inv_mass[i] == 0.0 {
            st.prev[i] = st.pos[i];
            continue;
        }
        let vel = (st.pos[i] - st.prev[i]) * rig.damping;
        st.prev[i] = st.pos[i];
        st.pos[i] += vel + rig.gravity * dt * dt;
    }

    // 2. Apply driven targets (from the mouse or the signal bus).
    for i in 0..n {
        if let Some(t) = st.target[i] {
            st.pos[i] = t;
            st.prev[i] = t;
        }
    }

    // 3. Pull toward rest. The zero point, and the reason a released limb
    //    comes home rather than staying where the hand left it.
    //
    //    Applied to the position and *not* to `prev`, so the difference
    //    between them — Verlet's velocity — grows as the joint returns. The
    //    limb therefore accelerates home and overshoots slightly before
    //    damping settles it, which is what makes the return read as a swing
    //    rather than a slide. A joint being driven is skipped: the hand and
    //    the clip outrank rest while they hold it.
    if rig.rest_pull > 0.0 {
        for i in 0..n {
            if rig.pinned[i] || rig.inv_mass[i] == 0.0 || st.target[i].is_some() {
                continue;
            }
            let home = (rig.rest[i] - st.pos[i]) * rig.rest_pull;
            st.pos[i] += home;
        }
    }

    // 4. Gauss-Seidel relaxation. Bone order is stable by construction.
    //    Incomplete convergence at these iteration counts is the organic
    //    feel — do NOT raise iterations to "fix" softness.
    for _ in 0..rig.iterations {
        for b in 0..rig.bone_a.len() {
            let (ia, ib) = (rig.bone_a[b] as usize, rig.bone_b[b] as usize);
            let d = st.pos[ib] - st.pos[ia];
            let len = d.length();
            if len < 1e-6 {
                continue; // coincident joints: no direction
            }
            let target = rig.rest_length[b] * rig.length_mul[b];
            let err = (len - target) / len;
            let wa = if rig.pinned[ia] || st.target[ia].is_some() {
                0.0
            } else {
                rig.inv_mass[ia]
            };
            let wb = if rig.pinned[ib] || st.target[ib].is_some() {
                0.0
            } else {
                rig.inv_mass[ib]
            };
            let wsum = wa + wb;
            if wsum <= 0.0 {
                continue;
            }
            let corr = d * err * rig.stiffness[b];
            st.pos[ia] += corr * (wa / wsum);
            st.pos[ib] -= corr * (wb / wsum);
        }
    }

    // 5. Guard. A single non-finite value resets THIS puppet only.
    if !all_finite(&st.pos) {
        st.reset_to_rest(rig);
        return StepOutcome::ResetDueToNonFinite;
    }
    StepOutcome::Ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::*;
    use crate::ids::{BoneId, JointId};
    use crate::solver::{CompiledRig, SolverState};
    use approx::assert_relative_eq;
    use glam::Vec2;

    /// Two joints 100px apart, joint 0 pinned. Bone rest length 100.
    fn two_joint_rig(stiffness: f32) -> (SkeletonData, SolverConfig) {
        let mut skel = SkeletonData::default();
        skel.joints.insert(
            JointId(1),
            Joint {
                id: JointId(1),
                name: "root".into(),
                rest: Vec2::new(0.0, 0.0),
                rest_angle: 0.0,
                inv_mass: 0.0,
                pinned: true,
            },
        );
        skel.joints.insert(
            JointId(2),
            Joint {
                id: JointId(2),
                name: "tip".into(),
                rest: Vec2::new(100.0, 0.0),
                rest_angle: 0.0,
                inv_mass: 1.0,
                pinned: false,
            },
        );
        skel.bones.insert(
            BoneId(1),
            Bone {
                id: BoneId(1),
                name: "bone".into(),
                a: JointId(1),
                b: JointId(2),
                rest_length: None,
                stiffness,
                damping: 0.0,
                length_mul: 1.0,
                attach_radius: 20.0,
            },
        );
        let cfg = SolverConfig {
            gravity: Vec2::ZERO,
            global_damping: 1.0,
            // The rig these tests measure is a bone and nothing else. The
            // pull toward rest is a second force on the same joints, and a
            // test of stiffness that also contains it measures neither.
            // `returning_to_rest_is_what_a_release_looks_like` covers it.
            rest_pull: 0.0,
            ..Default::default()
        };
        (skel, cfg)
    }

    /// Letting go brings the limb home. This is the whole model.
    ///
    /// Rest is the zero point: edit mode authors it, live mode departs from
    /// it, and a release returns to it. Before the pull existed, a joint
    /// stayed wherever the hand dropped it — the puppet held every pose it
    /// was ever put in, no gesture ever "played back", and a recording of
    /// pulls had nothing to return from.
    #[test]
    fn returning_to_rest_is_what_a_release_looks_like() {
        let (skel, mut cfg) = two_joint_rig(0.9);
        cfg.rest_pull = SolverConfig::default().rest_pull;
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        let rest_tip = st.positions()[1];

        // A hand pulls the tip aside and holds it there.
        let held = rest_tip + Vec2::new(0.0, 60.0);
        st.set_target(1, held);
        for _ in 0..60 {
            step(&rig, &mut st, 1.0 / 120.0);
        }
        assert_relative_eq!(st.positions()[1].y, held.y, epsilon = 1e-3);

        // The hand lets go.
        st.clear_all_targets();
        for _ in 0..240 {
            step(&rig, &mut st, 1.0 / 120.0);
        }

        let settled = st.positions()[1];
        assert!(
            (settled - rest_tip).length() < 2.0,
            "a released joint must come home, ended {settled:?} against rest {rest_tip:?}"
        );
    }

    /// And the old behaviour stays reachable, because a rag doll that keeps
    /// its shape is a legitimate puppet.
    #[test]
    fn a_zero_pull_leaves_the_pose_where_it_was_dropped() {
        let (skel, cfg) = two_joint_rig(0.9); // rest_pull: 0.0
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        let rest_tip = st.positions()[1];

        st.set_target(1, rest_tip + Vec2::new(0.0, 60.0));
        for _ in 0..60 {
            step(&rig, &mut st, 1.0 / 120.0);
        }
        st.clear_all_targets();
        for _ in 0..240 {
            step(&rig, &mut st, 1.0 / 120.0);
        }

        assert!(
            (st.positions()[1] - rest_tip).length() > 20.0,
            "with no pull toward rest the limb stays where it was left"
        );
    }

    #[test]
    fn rest_state_is_stationary() {
        let (skel, cfg) = two_joint_rig(1.0);
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        let before = st.positions().to_vec();
        for _ in 0..100 {
            step(&rig, &mut st, 1.0 / 120.0);
        }
        for (a, b) in before.iter().zip(st.positions()) {
            assert_relative_eq!(a.x, b.x, epsilon = 1e-4);
            assert_relative_eq!(a.y, b.y, epsilon = 1e-4);
        }
    }

    #[test]
    fn rest_length_is_derived_from_rest_positions_when_none() {
        let (skel, cfg) = two_joint_rig(1.0);
        let rig = CompiledRig::build(&skel, &cfg);
        assert_relative_eq!(rig.rest_length(0), 100.0, epsilon = 1e-4);
    }

    #[test]
    fn a_pinned_joint_never_moves() {
        let (skel, cfg) = two_joint_rig(1.0);
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        // Yank the free joint far away.
        st.set_target(1, Vec2::new(5000.0, 5000.0));
        for _ in 0..50 {
            step(&rig, &mut st, 1.0 / 120.0);
        }
        assert_relative_eq!(st.positions()[0].x, 0.0, epsilon = 1e-4);
        assert_relative_eq!(st.positions()[0].y, 0.0, epsilon = 1e-4);
    }

    #[test]
    fn a_stretched_bone_relaxes_back_toward_its_rest_length() {
        let (skel, cfg) = two_joint_rig(1.0);
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        st.displace(1, Vec2::new(200.0, 0.0)); // now 300 apart
        let d0 = (st.positions()[1] - st.positions()[0]).length();
        for _ in 0..200 {
            step(&rig, &mut st, 1.0 / 120.0);
        }
        let d1 = (st.positions()[1] - st.positions()[0]).length();
        assert!(d1 < d0, "bone should contract: {d0} -> {d1}");
        assert_relative_eq!(d1, 100.0, epsilon = 2.0);
    }

    #[test]
    fn low_stiffness_relaxes_more_slowly_than_high_stiffness() {
        // The looseness IS the organic feel. Guard it against a future
        // "optimization" that makes the solver rigid.
        let mut lengths = vec![];
        for stiffness in [0.2f32, 1.0f32] {
            let (skel, cfg) = two_joint_rig(stiffness);
            let rig = CompiledRig::build(&skel, &cfg);
            let mut st = SolverState::rest(&rig);
            st.displace(1, Vec2::new(200.0, 0.0));
            for _ in 0..10 {
                step(&rig, &mut st, 1.0 / 120.0);
            }
            lengths.push((st.positions()[1] - st.positions()[0]).length());
        }
        assert!(
            lengths[0] > lengths[1],
            "soft bone must still be longer after 10 steps: {lengths:?}"
        );
    }

    #[test]
    fn length_mul_changes_the_target_length() {
        let (mut skel, cfg) = two_joint_rig(1.0);
        skel.bones.get_mut(&BoneId(1)).unwrap().length_mul = 1.5;
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        for _ in 0..400 {
            step(&rig, &mut st, 1.0 / 120.0);
        }
        let d = (st.positions()[1] - st.positions()[0]).length();
        assert_relative_eq!(d, 150.0, epsilon = 2.0);
    }

    #[test]
    fn a_non_finite_state_resets_the_puppet_instead_of_propagating() {
        let (skel, cfg) = two_joint_rig(1.0);
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        st.poison_for_test(1, Vec2::new(f32::NAN, 0.0));
        let outcome = step(&rig, &mut st, 1.0 / 120.0);
        assert_eq!(outcome, StepOutcome::ResetDueToNonFinite);
        assert!(st.positions().iter().all(|p| p.is_finite()));
        assert_relative_eq!(st.positions()[1].x, 100.0, epsilon = 1e-4);
    }

    #[test]
    fn a_zero_length_bone_does_not_produce_nan() {
        let (mut skel, cfg) = two_joint_rig(1.0);
        skel.joints.get_mut(&JointId(2)).unwrap().rest = Vec2::ZERO; // coincident
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        for _ in 0..50 {
            step(&rig, &mut st, 1.0 / 120.0);
        }
        assert!(st.positions().iter().all(|p| p.is_finite()));
    }
}
