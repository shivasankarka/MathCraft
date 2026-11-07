# MathCraft

A fast, friendly, open-source symbolic-computation notebook in pure Rust. Exact algebra and calculus, a Python-flavoured
language, and a reactive notebook where changing one cell updates every cell that depends on it.

**Status: working prototype (v0.0.1).** See `refs/STATUS.md` for what works and what does not yet.

## Try it

```sh
cargo run --release -p mathcraft                         # opens a welcome notebook
cargo run --release -p mathcraft -- examples/tour.py     # open a file
cargo run --release -p mathcraft-cli -- eval 'integrate(sin(x)**2, (x, 0, pi))'
cargo run --release -p mathcraft-cli -- run examples/tour.py
cargo run --release -p mathcraft-cli -- check examples/tour.py
```

In the app: **Shift+Enter** runs a cell and moves on, **⌘/Ctrl+Enter** runs without moving, **⌘⇧+Enter** runs all,
**⌘.** interrupts, **⌘S** saves, **F1** lists every function.

## The language

Python syntax, with maths-friendly meaning:

```python
f = sin(x) * exp(-x / 5)         # x is not defined, so it is a symbol
diff(f, x)                       # exp(-x/5)*cos(x) - exp(-x/5)*sin(x)/5
1/3 + 1/6                        # 1/2, exact
solve(x**2 - 5*x + 6, x)         # [2, 3]
integrate(sin(x)**2, (x, 0, pi)) # pi/2
A = [[1, 2], [3, 4]]
det(A), inv(A), A @ A
plot([sin(x), cos(x)], (x, 0, 10))
```

Use `**` for powers (`^` is XOR in Python and gives a helpful error). Declare symbols explicitly with
`x, y = symbols('x y')`, or turn on *Strict* in Settings to require it.

## Notebook rules (after marimo)

- A name can be defined in **one cell only**; no two cells may depend on each other in a cycle. Offending cells don't
  run and say why (`MC002` duplicate definition, `MC003` cycle, `MC001` syntax error).
- Running a cell re-runs the cells that use what it defines (or marks them stale in *lazy* mode).
- Names starting with `_` are private to a cell.
- Files are plain scripts with `# %%` markers, so they diff well in git and also run from the command line.
  Outputs are not stored in the file.

## Develop

Rules for contributors and agents: `AGENTS.md`. Plan and decisions: `refs/PLAN.md`.

```sh
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo deny check
```

Licence: MIT OR Apache-2.0 (proposed; see PLAN Q8).
