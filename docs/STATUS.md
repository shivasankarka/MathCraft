# Status

What works today and how it was checked. Last updated 2026-11-07, prototype v0.0.2, built and run on macOS (Apple
silicon) only. The plan is in [PLAN.md](PLAN.md).

## Verification summary

| Check | Result |
|---|---|
| `cargo test --workspace` | all tests pass (see last run in the hand-off) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean, with ArtCraft's never-crash lints denied |
| `cargo deny check` | advisories, bans, licenses, sources all ok (one documented licence exception, see below) |
| Hostile-input run (23 inputs through the CLI: 5000-deep brackets, infinite loops, `2**(2**1024)`, `(x+1)**100000`, runaway recursion, 100k-deep expression building, ...) | every case ended in a clear error or a bounded result; no crash, no hang |
| Headless GUI frames (egui context, real layout code) | welcome notebook renders, all cells run, plot produced; editing and dependent re-run through the app's action path |
| `cargo check --target wasm32-unknown-unknown` for core, syntax, algebra, eval | compiles. **Not run in a browser**, see gaps |
| Visual check | `MATHCRAFT_SHOTS=<dir> cargo test -p mathcraft screenshots -- --ignored` renders the notebook with the GPU (egui_kittest + wgpu) in dark and light to PNG. Reviewed by the developer-agent, not yet by the user. **Interaction (typing, Tab/Enter, focus, dialogs) is still untested** |

## What works

- **Language**: Python syntax via ruff's parser (lowered to our own AST): assignment (incl. tuple unpacking, augmented), `def`
  with defaults, closures, lambdas, `if/elif/else`, `for`, `while`, `break/continue/return`, list comprehensions with
  conditions, lists, tuples, strings, indexing, chained comparisons, `and/or/not`, `@` matrix product. Unsupported Python
  (classes, imports, `try`, dicts/sets, slices, f-strings, generators) gives a clear message with the line number.
- **Numbers**: exact integers and rationals (arbitrary size, dashu), floats; `1/3 + 1/6` is exactly `1/2`; `sqrt(8)` is
  `2*sqrt(2)`; sin/cos/tan of multiples of pi/6 and pi/4 are exact.
- **Expressions**: canonical form (sorted, combined like terms and powers, deterministic), `E ** x` is `exp(x)`.
- **Algebra**: `expand`, `factor` (rational roots), `simplify` (expand/factor/Pythagorean, keeps the smallest), `subs`,
  `solve` (polynomials: rational roots + quadratic formula; linear systems incl. symbolic), `coeffs`, `degree`, `N`.
- **Calculus**: `diff` (chain/product/power rules, many functions), `integrate` (linearity, power/exp/trig, `sin^2/cos^2`,
  integration by parts for `x^n` times exp/sin/cos/log, u-substitution; definite integrals exact when an antiderivative
  is found, otherwise numeric), `limit` (substitution + L'Hopital), `series` (Taylor, plus quotients with removable
  singularities such as `sin(x)/x`). Antiderivatives are checked in tests by differentiating back.
- **Linear algebra** (lists of lists): `det`, `inv`, `transpose`, `rank`, `eye`, `@`.
- **Plotting**: `plot(f, (x, a, b))` and lists of curves; breaks the curve at poles.
- **Notebook**: `# %%` text format; static analysis of defs/refs (functions' free variables, comprehensions, `_private`
  names); dependency graph; rules MC001 syntax, MC002 duplicate definition, MC003 cycle, MC004 ambiguous reference;
  run-cell/run-all/run-stale; auto vs lazy mode; stale tracking; upstream failures block dependents; running a cell first
  runs never-run or stale upstream cells; failed cells commit nothing; `mathcraft-cli check` for CI.
- **Kernel**: worker thread (256 MB stack), per-cell timeout (60 s) and step budget, interrupt (tested on an infinite
  loop). It is written to survive an engine panic by resetting its state and reporting it, but **that path has no test**
  because nothing in the engine panics on purpose.
- **App**: cells with syntax highlighting, Tab/auto-indent, state chips, "defines / uses cell N" hints, problem banners,
  plots, markdown cells (headings and lists), save/open/new with unsaved-changes prompts, atomic save, function list (F1),
  settings for run mode and strict mode.
- **CLI**: `eval`, `run`, `check`, `repl`.

## Performance baseline (release build, M-series Mac, best of 3, includes process start)

startup 10 ms; `expand((x+y+z)**30)` (496 terms) 23 ms; `expand((x+y+z+w)**20)` (1771 terms) 71 ms; sum of 200k squares
53 ms; `fib(25)` recursive 129 ms; 12th derivative of `x**x*sin(x)` 41 ms; symbolic 7x7 determinant 33 ms; exact 12x12
Hilbert inverse 5 ms. The interpreter is a tree walker with hash-map scopes and is roughly 4x slower than CPython on
call-heavy loops (no benchmark of CPython was run here; this is an estimate from fib(25)). Algebra is the strength.

## Known gaps and limitations (be aware when testing)

- **Complex numbers** are expressions `a + b*I` with `I**2 = -1` built into the canonical form (`3j` works too). `sqrt` of a negative rational, `1/(1+I)`, `solve` with complex roots (polynomials up to what `solve` already handles), `re`, `im`, `conjugate`, `abs`, `N`. **Limits:** symbols are assumed real (so `re(x)` is `x`); no `exp(I*x)` to `cos + I*sin` rewriting, no `arg`/polar form, no complex-valued `sin`/`log` evaluation, `plot` of a complex value is an error.
- **Typeset math**: done for expressions, lists, tuples and matrices (fractions, powers, roots, trig, subscripts, Greek, matrices with tall brackets). Missing: line-breaking of very long sums (they scroll horizontally), inline `$...$` in text cells, copy as LaTeX, integral/sum/limit notation (the engine has no unevaluated forms yet), nth roots (shown as powers).
- **UI polish (design pass 1)**: chrome (toolbar, status bar) one step off the paper-white/near-black document with hairline separators; push-button Run all, file name with "Edited"; soft code panels (no outline in light) with a macOS-style accent focus halo; outputs aligned with the code text; gutter numbers only on code cells; warning/error banners with icon, coloured rule and normal-colour text; themed plot series colours; completion popup with shadow, kind badges and accent-tinted selection; centred Functions window (signature over summary, "no match" state); empty-notebook state with keycaps; 1.5 leading in text cells. All colours, the shadow, radii and plot palette are theme data. Screenshots: `screenshots`, `completion_screenshot`, `help_screenshot` (also renders the empty state). Not covered: hover/pressed states and menus (static renders); the native macOS titlebar is unchanged (no unified toolbar).
- **Theme**: Dark/Light/System work; the choice is not persisted between runs and custom theme files are not loadable yet. Plot axes are bright in the dark theme.
- **solve** (`crates/algebra/src/solve.rs`): polynomials (rational roots, quadratic, Cardano cubic with numeric coefficients (trig form for three real roots), `x^k` substitution for biquadratics and similar), equations that become polynomial after substituting `exp(c x)`, a trig/log call or `sqrt(x)`, and step-by-step inversion (`exp`, `log`, `sin/cos/tan`, inverse trig, `sinh/cosh/tanh`, `abs`, powers, products equal to zero). Every candidate is checked numerically, so extra roots are dropped. Trig equations return one period of solutions. `nsolve(eq, x, guess)` is Newton's method. Not supported: general quartics, log-combining (`log(x)+log(x-1)==log(6)`), nonlinear systems, inequalities.
- **Limits** (`crates/algebra/src/limits.rs`): `oo`/`-oo` as a constant; limits at infinity (rational functions, exp, log, atan, powers, `x*exp(-x)` style 0*oo via L'Hopital); poles give `oo`/`-oo`; one-sided limits with `dir='+'`/`'-'`; a two-sided limit with different sides is an error that says so. Not supported: `1**oo` forms such as `(1+1/x)**x`, bounded oscillation (`sin(x)/x` at oo), oo - oo beyond rational cases. `oo` is a plain symbol: no arithmetic simplification (`oo + 1` stays).
- **Partial fractions**: `apart(f, x)` (rational-root factors, any multiplicity, polynomial part); `integrate` uses it for rational functions, including `atan`/`log` forms for quadratic factors (no repeated quadratic factors, no `log|x|`).
- **integrate** has no special functions (`integrate(exp(x**2), x)` stays unevaluated), no `+ C`, and
  returns `log(x)` rather than `log|x|`. **simplify** is heuristic: it tries expand, factor, the Pythagorean identity, `sin/cos` to `tan`, `sqrt(u**2)` to `abs(u)` and cancelling common polynomial factors in fractions (numeric coefficients, one symbol at a time), and keeps the smallest. It does not combine logs or sums of fractions. 
- **Vectors and matrices (D16/D20)**: `vec(...)`, `matrix(...)`, `zeros`, `ones`, `eye`, `astype`, `tolist`, `shape`, `dot`, `norm`, `det`, `inv`, `transpose`, `rank`, `@`, elementwise `+ - * / **`, scalar broadcasting, indexing `A[i]`, `A[i, j]`, iteration, functions applied elementwise (`sin(v)`). Element types (`dtype=`): `exact` (default: exact numbers and symbols), `i8 i16 i32 i64` (overflow is an error), `f32 f64`. **Types are never mixed**: `i32` with `f64`, or two different widths, is an error that names the fix (`astype`). A plain number in the program adopts the array's type if it fits exactly. **Limits:** float linear algebra is our own simple loops (no `faer` yet, so large matrices are slow); no float `rank`, eigenvalues, SVD or `solve` of exact systems with arrays yet; no assignment into elements; no broadcasting between a matrix and a vector; scalars have no dtype (an element read from an `i32` array is an exact integer, from an `f64` array a float); lists stay Python lists (`[1,2] + [3]` concatenates) and lists of lists are still accepted by `det`/`inv`/`transpose`/`rank`.
- **Not yet in the language**: classes, imports, dicts/sets, slices, f-strings, `try`, generators, keyword-only args.
- **Editor**: autocompletion popup (2+ letters; notebook names first, then built-ins with signatures; Up/Down, Tab/Enter accept, Esc closes; quiet in comments and strings); undo/redo (Cmd+Z, Cmd+Shift+Z) are egui's per-editor history. Covered by headless interaction tests (typing, Tab, arrows, Esc, Enter indentation, Shift+Enter run, undo/redo). Not covered: mouse clicks on the popup, notebook-level undo (deleting or moving a cell).
- **Vim engine** (`crates/vim`; wired into cell editors and Text view via `vim_edit.rs`, switched on in Settings > Vim keys or with `MATHCRAFT_VIM=1`; mode and pending keys show in the status bar; `:w :q :wq :x :q! :run` work; the block cursor and visual selection are drawn with egui's selection; arrow keys act as hjkl outside insert mode; the setting is not remembered between runs yet): modes normal/insert/visual/visual-line/command; counts; motions `h j k l w b e 0 ^ $ gg G f F t T ; , n N`; operators `d c y` (also doubled); `x X D C s S r J ~ p P u Ctrl-r`; `i a I A o O`; `:` command line returned to the caller; `/` search. 9 tests including a random-key no-panic run. Missing: dot repeat, text objects, marks, macros, named registers, `%`, `{ }`.
- **Tabs** (`doc.rs`, `tabs.rs`): several documents open at once, each with its own kernel, path and unsaved state; the strip shows from two tabs on. Cmd+N new, Cmd+O open (switches if the file is already open; reuses an untouched blank tab), Cmd+W close, Ctrl+Tab / Ctrl+Shift+Tab cycle, middle-click closes, dirty dot, unsaved-changes prompts on close and quit. Background tabs keep computing. Covered by tests; no drag-to-reorder, no session restore.
- **Folder sidebar** (`workspace.rs`, `sidebar.rs`): File > Open Folder (or `mathcraft <dir>`) lists the `.py` notebooks (subfolders up to 4 levels, folders first; hidden/target/venv skipped) in a resizable left panel; click opens in a tab (or switches), dots mark open and unsaved files, + creates `untitled.py` in the folder, refresh button, Cmd+B shows/hides. The list rescans on save/create/refresh only (no file watching). Known issue: with a long folder name the "open another folder" button is pushed out of view.
- **Text view** (`text_view.rs`, View menu or Cmd+Shift+T): the whole `# %%` file in one highlighted editor with the results of every code cell in a panel on the right, the cell under the cursor marked. Shift+Enter or Cmd+Enter runs the cell under the cursor. Edits are synced into the notebook as you type, keeping the identity (outputs, kernel variables) of cells that did not change, also when a cell is inserted or removed in the middle; switching back to Notebook view loses nothing. Not done: scrolling the results to follow the cursor, jumping between cells with keys.
- **Notebook**: outputs are not saved to the file; no cell-level undo; markdown
  rendering is minimal (headings, bullets; no bold/italic); no interactive sliders (`Manipulate` equivalent) yet; `_private`
  names are removed when their cell re-runs but are visible to other cells at runtime (they just never create dependencies).
- **Interrupt granularity**: cancellation is checked at budget checkpoints (loops, evaluation steps, algebra recursion). A
  single huge operation that does not tick, for example a very large exact `**`, cannot be interrupted until it returns.
- **Expressions are `Arc` trees**, not hash-consed; fine for the prototype, a candidate for the performance milestone.
- **WASM**: lower layers compile for `wasm32-unknown-unknown`, but `Instant::now()` in the budget panics on that target,
  so the timeout needs a WASM-safe clock before any browser build. Not run in a browser.
- **ArtCraft rules not yet satisfied**: no `xtask layers`/`secrets` checks, no CI, no `ATTRIBUTION.md`/`NOTICE`, no
  `about.toml` licence bundle, no Craft Anything tool registry/MCP server yet (PLAN R0, R6).
- **Licence exception**: `Unicode-DFS-2016` is allowed in `deny.toml` (it comes with the Python parser's
  `ruff_unicode_names2`). It is permissive but not on ArtCraft's house allowlist; needs maintainer sign-off before merging.
- **Dependency risk**: `ruff_python_parser` is a 0.0.x internal crate of ruff, pinned with `=0.0.21`; every upgrade may
  break the lowering code in `crates/syntax/src/lower.rs`, which is the only place that touches it.
- **GUI interaction is unverified by automated tests**: typing, Tab/Enter handling, Shift+Enter focus movement, file
  dialogs and the look of the window were not exercised beyond the headless render and action tests.
