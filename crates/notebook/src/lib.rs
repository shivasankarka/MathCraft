//! MathCraft notebooks (layer L2).
//!
//! A notebook is a list of cells in a plain `# %%` script file. The notebook crate analyses each cell statically,
//! builds the dependency graph, enforces the reproducibility rules (one definition per name, no cycles), decides what
//! to run when something changes, and executes cells on a kernel thread. It has no UI code.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod analysis;
pub mod format;
mod kernel;
mod notebook;
mod runner;

pub use kernel::Kernel;
pub use notebook::{Cell, CellId, CellKind, CellOutput, CellState, Diagnostic, Job, Layout, Notebook, Problem, RunMode, RunRequest};
pub use runner::{CellResult, Event, JobResult, Runner, execute};

#[cfg(test)]
mod tests;
