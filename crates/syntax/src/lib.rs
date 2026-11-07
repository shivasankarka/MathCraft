//! MathCraft syntax (layer L0): parse the Python-flavoured language into [`ast`] nodes.
//!
//! Parsing is done by ruff's Python parser. Everything it produces is lowered into our own small tree here, so the
//! rest of MathCraft does not depend on its (unstable) types, and unsupported Python gets a friendly message.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod ast;
mod lower;

pub use lower::parse;

use thiserror::Error;

/// A problem found while reading source code: invalid syntax, or valid Python that MathCraft does not support (yet).
#[derive(Debug, Clone, PartialEq, Error)]
#[error("line {line}: {message}")]
pub struct SyntaxError {
    pub line: u32,
    pub message: String,
}
