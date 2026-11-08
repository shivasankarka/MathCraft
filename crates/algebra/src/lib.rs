//! MathCraft algebra (layer L1): expansion, substitution, calculus, polynomials, simplification and linear algebra.
//! Every routine takes immutable expressions, returns `MathResult`, honours the thread's computation budget and
//! refuses pathologically deep or large inputs instead of crashing.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod assume;
mod calculus;
mod complex;
mod expand;
mod limits;
pub mod linalg;
mod poly;
mod rational;
mod simplify;
mod solve;
mod sparse;
mod sums;
mod system;
mod util;

pub use assume::{Assumptions, Facts, Sign, admissible, is_integer, is_nonzero, refine, sign_of, simplify_with};
pub use calculus::{diff, diff_n, integrate, integrate_definite, series};
pub use complex::{abs as complex_abs, atan2, conjugate, parts as complex_parts};
pub use expand::expand;

/// Greatest common divisor of two polynomials with integer coefficients (`None` if they are not such polynomials).
pub fn poly_gcd(a: &mathcraft_core::Expr, b: &mathcraft_core::Expr) -> mathcraft_core::MathResult<Option<mathcraft_core::Expr>> {
    sparse::gcd(a, b)
}

/// Least common multiple of two polynomials with integer coefficients.
pub fn poly_lcm(a: &mathcraft_core::Expr, b: &mathcraft_core::Expr) -> mathcraft_core::MathResult<Option<mathcraft_core::Expr>> {
    sparse::lcm(a, b)
}

pub use limits::{Side, limit, limit_side};
pub use poly::{factor, from_poly, linear_in, solve_polynomial, to_poly};
pub use rational::{apart, cancel};
pub use simplify::simplify;
pub use solve::{nroots, solve};
pub use sums::summation;
pub use system::solve_system;
pub use util::{has_singularity, is_zero_expr, subs};
