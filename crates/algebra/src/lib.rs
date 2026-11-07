//! MathCraft algebra (layer L1): expansion, substitution, calculus, polynomials, simplification and linear algebra.
//! Every routine takes immutable expressions, returns `MathResult`, honours the thread's computation budget and
//! refuses pathologically deep or large inputs instead of crashing.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod calculus;
mod complex;
mod expand;
mod limits;
pub mod linalg;
mod poly;
mod rational;
mod simplify;
mod solve;
mod util;

pub use calculus::{diff, diff_n, integrate, integrate_definite, series};
pub use complex::{abs as complex_abs, conjugate, parts as complex_parts};
pub use expand::expand;
pub use limits::{Side, limit, limit_side};
pub use poly::{factor, from_poly, linear_in, solve_polynomial, to_poly};
pub use rational::{apart, cancel};
pub use simplify::simplify;
pub use solve::solve;
pub use util::{has_singularity, is_zero_expr, subs};
