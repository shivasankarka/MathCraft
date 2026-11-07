# MathCraft: living plan

Last updated 2026-11-07. Status: **working prototype v0.0.2 (typeset math, themes, new UI)** (see [STATUS.md](STATUS.md)). This file is the source of
truth for decisions; research is in `docs/research/`. Every change to a decision goes in the Decision log (bottom).

## Vision

A Mathematica-like symbolic computation environment in Rust, in the style of the ArtCraft "Crafting Apps": native
desktop app, headless engine, CLI and (planned) MCP server from one codebase. **Usability and performance are the top
priorities.** The intent is to merge MathCraft into the ArtCraft suite later, so we follow their rules (copied under
`reference/`, summarised in `AGENTS.md`).

One language, two ways to use it: a reactive **notebook** (done first) and **script files** (same engine, `mathcraft-cli run`).

## Decisions (agreed with the user)

| # | Decision | Date | Notes |
|---|---|---|---|
| D1 | Language: Rust | 2026-11-07 | |
| D2 | UI: egui/eframe 0.37, for consistency with ArtCraft | 2026-11-07 | Typeset math needs custom work (R4) |
| D3 | Language: Python-flavoured, as much real Python syntax as possible | 2026-11-07 | Deviations only where maths needs them; `1/3` is exact, undefined names are symbols |
| D4 | Dependencies: existing permissive crates first; write our own where the licence does not allow | 2026-11-07 | Policy below |
| D5 | Notebook model follows marimo: static-analysis dependency graph, one definition per name, no cycles | 2026-11-07 | `crates/notebook`; rules MC001 to MC004 |
| D6 | Notebook first, scripts follow | 2026-11-07 | Same engine; `mathcraft-cli run` works today |
| D7 | Targets as Craft Anything: desktop + CLI + MCP on one headless engine; low layers WASM-safe | 2026-11-07 | Desktop and CLI done; MCP not started |
| D8 | Symbolica is not a core dependency | 2026-11-07 | Source-available licence, see `research/crate-survey.md` |
| D9 | Free symbols: undefined names are symbols by default; `symbols('x y')` declares them explicitly; **Strict mode** (Settings) requires declarations | 2026-11-07 | User asked for both. A name defined in any cell is a reference, anything else is a free symbol |
| D10 | Notebook file = plain script with `# %%` cell markers (`# %% [markdown]` for text); version header; outputs not stored | 2026-11-07 | User choice |
| D11 | Front end: ruff's Python parser (lowered to our own AST) + our own tree-walking evaluator | 2026-11-07 | User said try the parser we found, switch to own if it fails. Spike succeeded; an embedded VM is not used because symbolic semantics are ours |
| D12 | Start with a focused Python subset, grow on demand | 2026-11-07 | Subset listed in STATUS |
| D13 | Keep a read-only copy of ArtCraft's rules in `reference/` and follow them | 2026-11-07 | For the later merge |
| D14 | Calling an undefined name is an error with a "did you mean" hint, unless the name was declared with `symbols()` (then it is an unknown function, `f(x)`) | 2026-11-07 | Typo protection; chosen by the implementation, tell us if you prefer Mathematica-style silent symbolic calls |
| D15 | Numbers: exact integers and rationals are the default; `/` of integers gives an exact fraction; floats only when a float is involved | 2026-11-07 | Differs from Python (`1/3`), on purpose |
| D16 | Containers: `list` stays the Python container and holds anything (functions, expressions, strings). A separate numeric `Vector`/`Matrix` type (exact numbers and symbolic expressions only) gets numpy semantics: elementwise `+ - *`, `@`, scalar broadcasting; explicit conversion (`vec(...)`, `.tolist()`) | 2026-11-07 | User proposal, accepted. Replaces list-of-lists matrices. Built (first version), see D20 |
| D17 | Typeset math is laid out by our own `typeset` crate directly from `Expr` (no LaTeX round trip), using STIX Two Math; painting is in the app | 2026-11-07 | `latex-rust` rejected: pins `egui =0.36.2`. Same sum/product ordering as the text printer (shared in `core`) |
| D18 | Themes are plain data (`apps/mathcraft/src/theme.rs`): Dark and Light built in, System/Light/Dark preference; custom themes later by loading the same struct from a file | 2026-11-07 | Fonts: Inter (UI), JetBrains Mono (code), STIX Two Math (math), all OFL, in `assets/fonts/` |
| D19 | Complex numbers are `a + b*I` expressions (no separate complex number type); symbols are assumed real | 2026-11-07 | Works with expand/diff/solve/typeset unchanged; limits listed in STATUS |
| D20 | Element types on vectors/matrices: `exact` (default), `i8..i64`, `f32`, `f64`. No implicit mixing or promotion; scalar literals adopt an array's type only if exact; overflow is an error; scalar `x: i32 = 1` annotations come later. Exact stays the default for plain numbers (`1`, `1/3`) because the symbolic engine needs it; a small-integer fast path will give machine speed invisibly (R7) | 2026-11-07 | User proposal (Rust-like dtypes), refined in discussion |

## Licence policy

From ArtCraft `docs/licences.md` and `deny.toml` (our `deny.toml` is derived from it; `cargo deny check` passes):
permissive licences only (MIT, Apache-2.0 (+LLVM-exception), BSD-2/3, ISC, Zlib, 0BSD, Unicode-3.0, BSL-1.0, OFL-1.1 for
fonts); MPL-2.0 only for named crates; never link GPL/LGPL/AGPL; programs we only *run* may have any licence. The stricter
ArtCraft *house* allowlist (`reference/figcraft/AGENTS.md`) is shorter (no Zlib, BSL-1.0, Unicode-3.0, 0BSD); the Craft
Anything repo itself uses the wider list, and so do we for now. See Q10.

## Architecture (implemented)

```text
crates/
  core       L0  Expr, exact numbers (dashu), functions, numeric eval, printer, computation budget
  syntax     L0  ruff parser -> our AST, friendly errors
  algebra    L1  expand, diff, integrate, solve, factor, simplify, series, limit, linear algebra
  eval       L1  interpreter and builtin library (DOCS = single list of function names)
  notebook   L2  file format, analysis, dependency graph, rules, planning, runner, kernel thread
apps/ mathcraft (egui) . mathcraft-cli (eval, run, check, repl)
```

Planned: `engine` (tool registry for UI/CLI/MCP, as in Craft Anything), `mcp`, `typeset`, `xtask` (layers, secrets, licences).

## Open questions

| # | Question | Leaning / state |
|---|---|---|
| Q4 | Store outputs in the file? | No (clean diffs). Add an optional sidecar or export later |
| Q8 | Project licence | MIT OR Apache-2.0, as ArtCraft |
| Q9 | How do we use Craft Anything to build further? Its documented flow is study-and-rebuild for existing targets; I have **not run it** and it needs a local coding-agent login | Verify on a throwaway project before relying on it. The `AGENTS.md` here is written so any agent can work from it |
| Q10 | `Unicode-DFS-2016` (the older text of the Unicode licence that ArtCraft allows as `Unicode-3.0`; ruff declares the old one and every ruff_unicode_names2 release uses it) is needed (via the Python parser's `ruff_unicode_names2`) but is not on the ArtCraft house allowlist | Ask ArtCraft maintainers, or replace the parser dependency |
| Q11 | ~~List/vector semantics~~ | **Decided: D16** (list + numeric Vector/Matrix) |
| Q12 | ~~Typeset math in egui~~ | **Done: D17**. Open: line-breaking of very long sums, inline `$...$` in text cells, copy as LaTeX |
| Q13 | `integrate` conventions: add `+ C`? `log|x|`? | Currently no constant, `log(x)` |
| Q14 | ~~Complex numbers~~ | **Done (D19)**: `I` as a constant symbol with `I**2 = -1` in the canonical form |
| Q15 | Slider-style interactive controls (`Manipulate`): shape of the API | Marimo-like `slider(...)` bound to a variable; needs UI-to-kernel plumbing |
| Q17 | App shell ideas from the user (2026-11-07): (1) command palette on Cmd+P (Obsidian-style: new file, open, run all, switch theme, jump to cell/function, settings); (2) professional macOS look via a design pass; (3) folder panel on the left, several `.py` notebooks per folder, tabs for open files | Accepted. Order: design pass 1 (visual layer only, by an Opus design agent) in parallel with algebra work; then tabs + multi-document model; then folder panel; then command palette (it lists commands from all of them); then design pass 2 over the new chrome |
| Q18 | Optional Vim mode, and two ways to work on one document: **Notebook view** (cells) and **Text view** (the whole `# %%` file in one editor, run cell under cursor / run file, results beside it) | Accepted by the user as high priority. Vim engine is a pure crate (`crates/vim`: modes, counts, motions, operators, registers, undo, visual, `:`/`/` command line; text + cursor in, text + cursor out), so it is testable without UI and reusable by CLI/web. Wiring: Settings toggle + status line showing the mode; in notebook view each cell editor is modal, Esc leaves insert, `:w`/`:q` map to save/close. Text view needs the multi-document model (tabs) first because the view is a per-document setting; both views share the same file and engine. Order: vim crate now (no UI conflicts), wire after tabs, Text view right after |
| Q16 | ~~Python interoperability~~ | **Decided 2026-11-07**: (a) `pycode(expr, args)` plus copy-as-Python/NumPy/Rust/LaTeX on outputs: wanted, built after `solve`/`simplify`; (b) `mathcraft` Python package (PyO3, numeric `lambdify`): wanted later, not immediate, with the engine/MCP work; (c) calling NumPy/SciPy from cells: skipped for good |

## Roadmap

IDs follow Craft Anything's style (`R<milestone>.<n>`).

- **R0 Foundations** (partly done): workspace, `deny.toml`, `AGENTS.md`, reference copy, docs. **Missing**: CI, `xtask layers`
  and `secrets`, `ATTRIBUTION.md`/`NOTICE`, `about.toml`, WASM-safe clock.
- **R1 Core engine** (done): numbers, canonical expressions, parser, interpreter, CLI.
- **R2 Algebra v1** (done for the basics): see STATUS. Next (agreed order): complex numbers, Vector/Matrix types (D16), partial
  fractions and more `solve`, `simplify`, `oo` and one-sided limits.
- **R3 Notebook model** (done): format, analysis, graph, rules, scheduler, kernel, `check`.
- **R4 UI** (v0.0.2): typeset math, themes, new notebook UI, GPU screenshot test: done. Next: autocompletion, undo, sliders, theme
  persistence + custom theme files, copy as LaTeX, inline math in text cells, interaction tests (egui_kittest).
- **R5 Numerics and plotting**: 2D plots done; ODEs, FFT, optimisation, 3D plots, numeric linear algebra (`faer`).
- **R6 MCP and tools**: `engine` tool registry (everything is a tool), MCP server, packaging, WASM viewer.
- **R7 Performance**: hash-consed expressions, resolved variable slots in the interpreter, parallel algebra, JIT for numeric hot paths.

## Research index

- `research/craft-anything-audit.md`: what ArtCraft/Craft Anything is and what to reuse
- `research/crate-survey.md`: crates with verified licences, spike results, verdicts
- `research/marimo-notebook-model.md`: marimo rules and how they map

## Decision log

- 2026-11-07: D1 to D8 agreed. Questions Q1 to Q9 raised.
- 2026-11-07: User answered Q1 to Q6 (explicit symbols as an option, `# %%` plain scripts, try the Python parser, subset
  first, keep ArtCraft rules locally, start building). D9 to D15 recorded. Prototype v0.0.1 built: 89 tests, clippy and
  `cargo deny` clean. Findings: ruff parser works but brings a `Unicode-DFS-2016` licence (Q10); lower layers compile for
  wasm32 but the timeout clock needs a WASM-safe replacement; `fib(25)` shows the interpreter is the performance gap.
- 2026-11-07: User asked for typeset math first, then a full UI/UX pass with dark and light themes (custom themes later), then complex
  numbers and the rest of the agreed order. Decided D16 (lists + numeric vector types), D17 (own typesetter), D18 (themes). Error-with-hint
  for undefined function calls confirmed (D14). Built `crates/typeset`, `theme/fonts/icons/markdown/math_view` in the app; 107 tests pass.
  First real visual check via `egui_kittest` GPU rendering found and fixed five defects (empty output divider, text cells counted as stale,
  coloured constants, cramped exponent fractions, dense plot grid).
- 2026-11-07: User found the UI still looked machine-made. Flattened it (D18 stays): no cards, gutter + hover controls, neutral palette, outlined Run all, quiet plots. Complex numbers next.
- 2026-11-07: Complex numbers built (D19): `I`, `3j`, sqrt of negatives, complex roots in `solve`, `re/im/conjugate/abs`; typeset shows upright i.
- 2026-11-07: Vector/Matrix with dtypes built (D20): own implementation, exact tier + float/int tiers, no implicit mixing. faer for large float matrices remains a spike.
- 2026-11-07: Editor QoL: autocompletion popup (`apps/mathcraft/src/complete.rs`), headless interaction tests with egui_kittest (found and fixed: Escape dropped focus while the list was open).
- 2026-11-07: Algebra depth, part 1: `oo`, limits at infinity and one-sided limits, `apart`, rational integration. Still open from the agreed list: more `solve`, stronger `simplify`.
- 2026-11-07: Welcome notebook rewritten as a feature tour (apps/mathcraft/src/welcome.py), including a list of what does not work yet. Python interop raised by the user (Q16).
- 2026-11-07: User decided Q16: build export (`pycode`, copy-as) after solve/simplify; Python package later; no NumPy/SciPy calls. Continuing with `solve` depth.
- 2026-11-07: `solve` depth: cubics, biquadratics, substitution, inversion of exp/log/trig, `nsolve`. General quartics and log rules still open.
- 2026-11-07: User proposed command palette (Cmd+P), pro-Mac design pass, folder panel + tabs (Q17). Accepted; order recorded in Q17. Design agent started for pass 1.
- 2026-11-07: `simplify` gains fraction cancelling (polynomial gcd), tan, abs(sqrt). Algebra depth list from the agreed order is done except general quartics and log rules.
- 2026-11-07: User asked for optional Vim support and a Notebook/Text view switch (Q18). Vim engine crate started.
- 2026-11-07: `crates/vim` built (pure engine, 9 tests). Wiring waits for tabs/document model.
- 2026-11-07: Design pass 1 reviewed (agent; gates green). Git initialised, file-by-file history. Tabs and the multi-document model built (`Doc`, tab strip). Next: folder panel, then Vim wiring + Text view, then palette.
- 2026-11-07: Folder sidebar built. Next: Vim wiring + Text view, then the Cmd+P palette, then design pass 2.
- 2026-11-07: Vim wired into editors (Settings > Vim keys) and Text view built (Q18 done apart from persistence of the setting, dot repeat and text objects). Next: Cmd+P command palette, then design pass 2, then `pycode`/copy-as.
