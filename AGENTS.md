# AGENTS.md: guide for AI agents and contributors

MathCraft is a native, pure-Rust (egui) symbolic-computation notebook: a Python-flavoured language, exact algebra and
calculus, and a reactive notebook modelled on marimo. It is meant to join the ArtCraft "Crafting Apps" suite, so it
follows their conventions. Their rules are copied under `reference/` (read-only snapshots, 2026-11-07): start with
`reference/craft-anything/AGENTS.md` and the house-rules block in `reference/figcraft/AGENTS.md`. **Where this file is
silent, those win. Where they disagree with this file, tell the user.**

The plan and its decision log are in `refs/PLAN.md`; what works today and how it was checked is in `refs/STATUS.md`.
Research notes are in `refs/research/`.

## Workspace map (layers enforced by convention for now; an `xtask layers` check is planned, see PLAN R0)

```text
crates/
  core       L0  Expr (immutable, canonical), exact numbers (dashu), elementary functions, numeric eval, printer, budget
  syntax     L0  Python parser (ruff) lowered to our own AST; friendly "not supported yet" errors
  algebra    L1  expand, diff, integrate, solve, factor, simplify, series, limit, linear algebra
  eval       L1  interpreter (Python subset), values, builtin function library (DOCS is the single list of names)
  notebook   L2  # %% file format, static analysis, dependency graph, rules, run planning, runner, kernel thread
apps/
  mathcraft       desktop app (egui). The only crate allowed to use egui/eframe/rfd
  mathcraft-cli   eval / run / check / repl
```

A crate may depend only on lower layers. core, syntax and algebra must stay WASM-safe: no I/O, no threads.

## Golden rules (from ArtCraft, adopted)

1. **Never crash.** Hostile input is the main input. No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`/`unreachable!`
   or indexing on input-derived values in non-test code; every crate root carries the `deny` attribute. Checked
   arithmetic, bounded recursion, capped allocation. Every crash fix gets a regression test.
2. **Everything long-running honours the budget** (`mathcraft_core::budget::tick`): loops in the interpreter and in
   algebra. Nothing may hang the UI; computation runs on the kernel thread.
3. **Safe Rust only** (`unsafe_code = "forbid"`). Prefer pure-Rust crates.
4. **Headless first.** Logic lives in the crates below the UI; the UI is a thin client that sends actions and renders state.
5. **Licences:** permissive only (see `deny.toml`, `refs/PLAN.md`). Run `cargo deny check` after changing dependencies.
   Never link GPL/LGPL/AGPL code. Symbolica is source-available and must not be a core dependency.
6. **Clean room:** behaviour comes from public documentation and our own design. Do not copy code, assets or strings from
   other products, and never reverse engineer Mathematica or any other incumbent. `reference/` is for reading rules, not code.
7. **Original work, honest status.** Report what ran and what did not. Update `refs/STATUS.md` in the same change.
8. **Deterministic results.** Canonical ordering and output must not depend on hash order or platform.
9. **Versioned files.** The notebook format has a version header; migrate, don't break.
10. **No secrets or personal data** in the repo.

## Before you finish

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

All four must pass. New behaviour needs tests; a language or notebook-rule change also needs a case in
`crates/eval/src/tests.rs` or `crates/notebook/src/tests.rs`.

## Conventions

- Rust 2024, `rustfmt.toml` and `clippy.toml` as in ArtCraft (wide lines, compact items).
- Errors are plain English with a hint on how to fix them, and carry the line number.
- Names of language functions live only in `crates/eval/src/builtins.rs::DOCS` (help, suggestions and highlighting read it).
- Add decisions and findings to `refs/PLAN.md` (decision log) as you go.
