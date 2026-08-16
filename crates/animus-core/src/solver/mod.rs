//! The physics solver: a graph of springs between point masses, not a
//! bone hierarchy. Joints are 2D particles; bones are distance
//! constraints relaxed with a fixed number of Gauss-Seidel passes per
//! step. Incomplete convergence at low iteration counts and stiffness
//! below 1.0 is deliberate — it is the organic, slightly-lagging feel the
//! original Animata was known for. See spec, physics section.

mod compiled;
mod guard;
mod state;
mod step;

pub use compiled::CompiledRig;
pub use state::SolverState;
pub use step::{StepOutcome, step};
