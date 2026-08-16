//! Determinism and regression guards for the solver.
//!
//! Bit-exact cross-platform float equality is not portable, so we assert
//! two things instead: (1) the same input twice gives bit-identical output
//! on THIS machine, which catches iteration-order and HashMap-ordering
//! bugs; (2) a committed set of positions is reproduced within a tight
//! tolerance, which catches accidental changes to the physics.

use animus_core::doc::*;
use animus_core::ids::{BoneId, JointId};
use animus_core::solver::{CompiledRig, SolverState, step};
use glam::Vec2;

/// A 12-joint chain with a pinned root, deterministically constructed.
fn chain_rig(n: u32) -> (SkeletonData, SolverConfig) {
    let mut skel = SkeletonData::default();
    for i in 0..n {
        skel.joints.insert(
            JointId((i + 1) as u64),
            Joint {
                id: JointId((i + 1) as u64),
                name: format!("j{i}"),
                rest: Vec2::new(i as f32 * 40.0, 0.0),
                rest_angle: 0.0,
                inv_mass: if i == 0 { 0.0 } else { 1.0 },
                pinned: i == 0,
            },
        );
    }
    for i in 0..n - 1 {
        skel.bones.insert(
            BoneId((i + 1) as u64),
            Bone {
                id: BoneId((i + 1) as u64),
                name: format!("b{i}"),
                a: JointId((i + 1) as u64),
                b: JointId((i + 2) as u64),
                rest_length: None,
                stiffness: 0.8,
                damping: 0.0,
                length_mul: 1.0,
                attach_radius: 25.0,
            },
        );
    }
    let cfg = SolverConfig {
        gravity: Vec2::new(0.0, 980.0),
        global_damping: 0.98,
        iterations: 8,
        ..Default::default()
    };
    (skel, cfg)
}

fn run(ticks: usize) -> Vec<Vec2> {
    let (skel, cfg) = chain_rig(12);
    let rig = CompiledRig::build(&skel, &cfg);
    let mut st = SolverState::rest(&rig);
    for t in 0..ticks {
        // A reproducible driving signal, no RNG.
        let phase = t as f32 * 0.05;
        st.set_target(0, Vec2::new(phase.sin() * 30.0, phase.cos() * 15.0));
        step(&rig, &mut st, 1.0 / 120.0);
    }
    st.positions().to_vec()
}

#[test]
fn the_solver_is_deterministic() {
    let a = run(600);
    let b = run(600);
    assert_eq!(a, b, "identical input must give bit-identical output");
}

#[test]
fn the_solver_stays_finite_and_bounded_over_a_long_run() {
    let p = run(20_000);
    assert!(
        p.iter().all(|v| v.is_finite()),
        "no NaN or Inf after 20k ticks"
    );
    // A 12-joint chain of 40px bones cannot legitimately reach 10_000px.
    assert!(
        p.iter().all(|v| v.length() < 10_000.0),
        "no explosion: {p:?}"
    );
}

#[test]
fn a_violent_yank_does_not_destabilise_the_rig() {
    let (skel, cfg) = chain_rig(12);
    let rig = CompiledRig::build(&skel, &cfg);
    let mut st = SolverState::rest(&rig);
    // Simulate a performer flinging a handle across the stage in one frame.
    st.set_target(0, Vec2::new(100_000.0, -100_000.0));
    step(&rig, &mut st, 1.0 / 120.0);
    st.clear_target(0);
    for _ in 0..2000 {
        step(&rig, &mut st, 1.0 / 120.0);
    }
    assert!(st.positions().iter().all(|p| p.is_finite()));
}

#[test]
fn golden_positions_are_unchanged() {
    // Regenerate deliberately, never casually: any diff here means the
    // physics changed and every existing show will move differently.
    //
    // Tolerance note. Bit-identical results hold on ONE machine (that is
    // what `the_solver_is_deterministic` asserts), but not across
    // platforms: Linux and Windows associate f32 additions and fuse
    // multiply-adds slightly differently, and 600 ticks amplify that to
    // around a thousandth of a pixel (first observed: joint 9 off by
    // 1.01e-3 px on Linux CI against a Windows-generated fixture). A real
    // physics change moves joints by whole pixels, so a 0.05 px tolerance
    // still catches everything this test exists to catch while absorbing
    // cross-platform float drift. Do not tighten it below ~1e-2 without
    // regenerating the fixture on every CI platform.
    const TOLERANCE_PX: f32 = 0.05;

    let got = run(600);
    let want = include_str!("fixtures/solver_golden_600.json");
    let want: Vec<[f32; 2]> = serde_json::from_str(want).unwrap();
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        assert!(
            (g.x - w[0]).abs() < TOLERANCE_PX && (g.y - w[1]).abs() < TOLERANCE_PX,
            "joint {i} moved: got {g:?}, want {w:?}"
        );
    }
}
