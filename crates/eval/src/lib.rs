//! MathCraft evaluator (layer L1): runs the Python-flavoured language.
//!
//! The interpreter is a tree walker over [`mathcraft_syntax::ast`]. Numbers and formulas are
//! [`mathcraft_core::Expr`] values, so `1/3` is the exact fraction 1/3 and an undefined name is a symbol.
//! All long-running work honours the thread's computation budget (see `mathcraft_core::budget`).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod array;
mod builtins;
mod fastla;
mod interp;
mod value;

pub use array::{Array, DType};
pub use builtins::DOCS;
pub use interp::{MAX_CALL_DEPTH, MAX_EXPR_DEPTH, Outcome, run, run_source};
pub use value::{EvalError, PlotSeries, PlotSpec, Value};

#[cfg(test)]
mod tests;
