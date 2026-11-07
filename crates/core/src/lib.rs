//! MathCraft core (layer L0): numbers, canonical expressions, elementary functions, numeric evaluation and printing.
//! No I/O, no threads; safe to compile to WASM.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod budget;
pub mod error;
pub mod expr;
pub mod functions;
pub mod number;
pub mod numeric;
mod print;

pub use print::{Product, SumTerm, product_of, split_product, sum_terms};

pub use error::{MathError, MathResult};
pub use expr::{Expr, Kind, SYM_I};
pub use number::Num;
