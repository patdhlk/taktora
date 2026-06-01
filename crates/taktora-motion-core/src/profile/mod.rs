//! Single-axis setpoint profiles (uncoupled motion generators).

mod scurve;
mod trapezoid;
mod velocity;

pub use scurve::SCurveState;
pub use trapezoid::TrapState;
pub use velocity::VelocityMove;
