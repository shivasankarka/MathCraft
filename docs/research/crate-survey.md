# Crate survey (2026-11-07)

Licence field and version/date come from the crates.io API on 2026-11-07. **Not yet verified:** transitive
dependency licences, API fitness, performance, WASM support, MSRV. Each shortlisted crate needs a spike plus
`cargo deny check` before adoption. Allowed licences: see `PLAN.md`.

Verdict key: **USE** shortlist, **MAYBE** spike first, **NO** blocked by policy, **OWN** we write it.

## Symbolica (decision D8: not core)

- Rust crate `symbolica` 3.0.2 (updated 2026-10-30), crates.io licence field `non-standard`. Repo: github.com/symbolica-dev/symbolica.
- `License.md` is the "Symbolica Source-Available License 1.0" (summary from a fetch; I am not a lawyer, read the full text before any decision).
  - Linking, embedding and distributing an unmodified copy in another product is allowed, but **runtime rights do not transfer**: each end user must qualify for free use or hold a runtime licence.
  - Free: one core and one instance, non-commercial/academic. Hobbyist free for unaffiliated individuals. Standard EUR 3000/yr per developer machine; server/CI by quote; OEM/library by signed agreement.
  - No redistribution of modified versions; **no use as a reference or blueprint for a competing computer-algebra product** (MathCraft is one); no reverse engineering.
  - AI clause: source may not be used to train models or build retrieval indexes; AI coding assistants only if the provider retains nothing.
- Fails ArtCraft's permissive-only policy (`cargo deny`). It would also cap parallel performance for free users.
- **Numerica** 3.0.2 and **Graphica** 3.0.1 (MIT) were split out of Symbolica (per Symbolica's release post). **MAYBE**: inspect what they actually contain.

## Numbers

| Crate | Version | Licence | Verdict | Note |
|---|---|---|---|---|
| dashu (int/ratio/float) | 0.6.2 | MIT OR Apache-2.0 | **USE** | Pure Rust big int, rational, float. Best fit as the exact-number base. Check transcendental-function coverage |
| num-bigint / num-rational / num-traits | 0.5.1 / 0.4.2 / 0.2.19 | MIT OR Apache-2.0 | MAYBE | Ubiquitous fallback; benchmark against dashu |
| astro-float | 0.9.6 | MIT | **USE** (spike) | Arbitrary-precision floats incl. elementary functions; likely fills dashu-float gaps |
| malachite | 0.14.0 | LGPL-3.0-only | **NO** | Copyleft |
| rug | 1.30.0 | LGPL-3.0+ | **NO** | GMP/MPFR bindings, copyleft |
| ibig | 0.3.6 | (not fetched) | NO | Unmaintained since 2022 |
| inari (intervals) | 2.0.0 | MIT | MAYBE | I recall it pulls an LGPL GMP/MPFR crate transitively; **unverified**, check `cargo deny` before any use. Otherwise write own interval type |

## Language front end

| Crate | Version | Licence | Verdict | Note |
|---|---|---|---|---|
| ruff_python_parser | 0.0.21 (2026-11-05) | MIT | **USE** (spike) | Real Python parser with error recovery; matches D3. 0.0.x, API not semver-stable: pin and vendor-check. Also need the AST crate it depends on |
| rustpython-parser | 0.4.0 (2024-08) | n/a | NO | Stale |
| rustpython-vm | 0.6.0 | MIT | NO (for eval) | Full Python VM, no symbolic semantics, heavy; see Q5 |
| rhai / starlark | 1.27.0 / 0.14.2 | MIT/Apache; Apache | NO | Not Python-syntax-compatible enough; we need symbolic semantics |
| chumsky / winnow / logos / lalrpop | 0.13.0 / 1.0.5 / 0.16.1 / 0.23.2 | MIT; MIT; MIT/Apache; Apache/MIT | fallback | If we must parse our own extensions (e.g. math shorthand) |
| tree-sitter | 0.27.1 | not fetched | later | Editor syntax highlighting in egui |

## Symbolic engine support

| Crate | Version | Licence | Verdict | Note |
|---|---|---|---|---|
| egg (e-graphs) | 0.11.0 | MIT | MAYBE | Equality saturation for simplification; powerful but can be slow; spike for a simplifier |
| hashbrown, rustc-hash, ahash, smallvec, bumpalo, slotmap, indexmap | see crates.io | MIT/Apache (slotmap Zlib) | **USE** | Building blocks for an interned expression arena |
| im / imbl (persistent collections) | 15.1.0 / 7.0.2 | MPL-2.0+ | **NO** | MPL only allowed by named exception per ArtCraft policy |
| petgraph | 0.8.3 | MIT OR Apache-2.0 | **USE** | Notebook dependency DAG, cycle detection |
| rayon | 1.12.0 | MIT OR Apache-2.0 | **USE** | Parallel algebra |
| salsa | 0.28.5 | Apache-2.0 OR MIT | MAYBE | Incremental computation framework (rust-analyzer); possibly overkill for cell graph |
| CAS core (simplify, integrate, solve, factor) | n/a | n/a | **OWN** | No permissive Rust CAS identified. Not yet exhaustively searched: **TODO** survey remaining Rust CAS crates (names not verified) |

## Numerics

| Crate | Version | Licence | Verdict |
|---|---|---|---|
| nalgebra | 0.35.0 | Apache-2.0 | **USE** (dense linear algebra) |
| faer | 0.24.4 | MIT | **USE** (spike; fast dense/sparse decompositions, pure Rust) |
| ndarray | 0.17.2 | MIT OR Apache-2.0 | MAYBE (n-d arrays) |
| ndarray-linalg / lapack-src | 0.18.1 / 0.13.0 | MIT/Apache | NO (default) (needs external LAPACK; breaks pure-Rust and packaging) |
| statrs, special, libm | 0.19.1, 0.14.2, 0.2.16 | MIT; Apache/MIT; MIT | **USE** (distributions, special functions, libm) |
| rustfft / realfft | 6.4.1 / 3.5.1 | MIT/Apache; MIT | **USE** |
| roots, argmin, ode_solvers, peroxide, mathru | various | BSD-2; MIT/Apache; Apache; MIT/Apache; MIT | MAYBE (root finding, optimization, ODEs) |
| rand, rand_distr | 0.10.3 / 0.6.0 | MIT/Apache | **USE** |
| cranelift-jit | 0.137.1 | Apache-2.0 WITH LLVM-exception | MAYBE (JIT for numeric evaluators; allowed by deny.toml) |
| polars / arrow | 0.55.2 / 60.1.0 | MIT / Apache-2.0 | later (dataframes) |

## UI, plotting, math rendering (egui)

| Crate | Version | Licence | Verdict | Note |
|---|---|---|---|---|
| egui / eframe / egui_extras | 0.37 (ArtCraft pins 0.36/0.37, FigCraft uses a forked egui+winit) | MIT/Apache | **USE** | D2. Decide: stock or follow ArtCraft's fork |
| egui_plot | 0.38.0 | MIT OR Apache-2.0 | **USE** | 2D plots; 3D plotting TBD (wgpu) |
| wgpu | 30.0.1 | MIT OR Apache-2.0 | **USE** | Future 3D plots |
| latex-rust | 3.0.1 | MIT OR Apache-2.0 | MAYBE (spike) | LaTeX math -> box layout -> egui shapes; bundles STIX Two Math (OFL-1.1, fonts allowed). Older 1.0.1 pinned egui 0.28: **check compatibility with our egui**. Small project, check maturity |
| pulldown-latex | 0.8.0 | MIT | MAYBE | LaTeX math parser only |
| typst / typst-library | 0.15.1 | Apache-2.0 | MAYBE | Has serious math layout but is a heavy dependency |
| katex, latex2mathml, mitex-parser | stale or JS-bound | n/a | NO | |
| Own math layout (OpenType MATH table via ttf-parser) | | | **OWN fallback** | Highest-risk UI item; egui has no math text. Plan a spike early |
| parley, cosmic-text, swash, ttf-parser, fontdb | 0.12.0, 0.19.0, 0.2.10, 0.25.1, 0.24.0 | Apache/MIT; MIT/Apache; Apache/MIT; MIT/Apache; MIT | **USE** | Text and font access for custom typesetting |

## Build-system / hygiene

Copy from Craft Anything: `deny.toml`, `about.toml` (cargo-about licence bundle), `rustfmt.toml`, `clippy.toml`,
workspace lints (`unsafe_code = "forbid"`), `cargo xtask layers`. `[sources] unknown-git = "deny"` means no git
dependencies (FigCraft's egui fork needs a documented exception).

## Next steps for this survey

1. Run `cargo deny check licenses` on a scratch workspace holding the USE/MAYBE set to catch transitive licence problems.
2. Spike the three riskiest: ruff_python_parser as front end, latex-rust (or own layout) in egui, dashu+astro-float number coverage.
3. Survey Rust CAS crates for ideas only (permissive ones); we expect to write the CAS core ourselves.

## Spike results (2026-11-07, from building the prototype)

| Crate | Outcome |
|---|---|
| ruff_python_parser / ruff_python_ast / ruff_text_size 0.0.21 | **Adopted**, pinned with `=`. Parses everything we need with good error locations; lowering to our AST is ~300 lines in `crates/syntax/src/lower.rs`. Compiles for wasm32. Brings `ruff_unicode_names2` with licence `(MIT OR Apache-2.0) AND Unicode-DFS-2016` (not on the ArtCraft house allowlist, PLAN Q10). Deep nesting must be guarded before parsing (we pre-scan brackets) |
| dashu 0.6 | **Adopted** for exact integers and rationals. API notes: `BitTest` for `bit_len`, `RBig::pow` takes `isize`, `/` on `IBig` truncates. Fast enough: `factorial(5000)` in 4 ms |
| petgraph 0.8 | **Adopted** for cycle detection (tarjan SCC) in the notebook graph |
| thiserror 2 | Adopted |
| egui/eframe 0.37, egui_plot 0.38, rfd 0.17 | **Adopted** for the app. 0.37 API differs from 0.3x: `App::ui(&mut self, ui, frame)` replaces `update`, `Panel::top/bottom(..).show(ui, ..)`, `Id::unique`, `ctx.all_styles_mut`, `CharIndex` newtype, `fonts_mut`. `Context::run_ui` + `FullOutput::drop_without_applying_deltas` allow headless frame tests |
| Not used yet | astro-float, faer, nalgebra, latex-rust, Numerica, egg, cranelift-jit, rayon: reserved for typeset math (R4), numerics (R5) and performance (R7) |
| cargo-deny 0.20 | Installed locally; `cargo deny check` is clean once crates are `publish = false` and `Unicode-DFS-2016` is allowed |
| latex-rust 3.0.1 | **Rejected**. Depends on `egui =0.36.2` (we use 0.37) plus `skrifa =0.39`, and needs a LaTeX string we would have to generate from `Expr`. We lay out directly from `Expr` instead (D17) |
| egui_kittest 0.37 (wgpu, snapshot) | **Adopted, dev-only**: renders real frames to PNG for visual checks; `cargo deny` stays clean |
| Inter (rsms, OFL), JetBrains Mono (OFL), STIX Two Math 2.0.2 (OFL) | **Adopted**, bundled in `assets/fonts/` with their licences. egui 0.37 `FontTweak.coords` selects the Inter weight (semibold) from the variable font |
