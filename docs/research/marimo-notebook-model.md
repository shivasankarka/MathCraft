# marimo notebook model (2026-11-07)

Source: docs.marimo.io pages (reactivity, understanding errors, lint rules, coming from Jupyter, home) read via
fetch on 2026-11-07. **Not read:** marimo's source code, the setup-cell guide, the multiple-definitions fix guide,
caching and SQL details. Read the source before we finalise our analysis algorithm.

## The rules

1. **Static analysis, no execution.** Per cell: *definitions* (globals it defines) and *references* (globals it reads but does not define).
2. **DAG.** Edge from cell A to cell B if B references something A defines. Running a cell runs (or marks stale) its descendants. Order on the page is irrelevant; helper functions can sit at the bottom.
3. **One definition per name.** A global may be defined in exactly one cell. Violation = error `MB002`, the cell does not run.
4. **No cycles** (`MB003`). `a` defined in cell 1 reading `b`, `b` defined in cell 2 reading `a` is rejected.
5. **No `import *`** so every name has a visible origin.
6. **Underscore-prefixed names are cell-local** and do not enter the graph. Wrapping temporaries in a function also keeps them local.
7. **Mutations are not tracked** (`list.append`, `obj.attr = v` trigger nothing). Workaround: mutate in the defining cell or make a new variable. marimo's docs: reliable mutation tracking in Python is impossible.
8. **Deleting a cell removes its variables** from memory; dependents re-run or go stale.
9. **Runtime modes:** automatic (default) or lazy (dependents marked stale, run on demand); autorun on startup and on execution can be disabled.
10. **Disabled cells** block their descendants; re-enabling runs cells whose ancestors ran meanwhile. Interrupting a cell cancels queued dependents.
11. **Expensive cells:** guard with a stop condition and a run button (`mo.stop`, `run_button`).
12. **Reactive UI elements** (sliders, dropdowns, ...) bind to variables with no callbacks.
13. **File format:** plain `.py`, git-friendly. Header with `__generated_with`, `app = marimo.App(...)`, each cell is an `@app.cell` function whose **parameters are the references and return tuple is the definitions**, `if __name__ == "__main__": app.run()`. Runs as a script, an app, or under pytest. **Outputs are not stored** in the file.
14. **No IPython magics or `!` shell commands.**
15. **Linter `marimo check`:** breaking `MB001` unparsable cell, `MB002` multiple definitions, `MB003` cycles, `MB004` setup-cell dependencies, `MB005` syntax error; runtime `MR001` to `MR004` (self-import, branch expression output, reusable-definition order, private import alias); formatting `MF001` to `MF007`.

## Mapping to MathCraft

| marimo rule | MathCraft adaptation |
|---|---|
| 1 to 4, 8, 9 | Adopt as is. Implement in headless `notebook` crate with `petgraph`; analysis works on our parsed AST |
| 7 mutations | Mostly dissolved: expressions are immutable values, so "mutation" reduces to rebinding, which rule 3 already forbids. We still need a policy for mutable containers (lists, dicts, arrays) |
| 3 vs CAS symbols | **Core design tension (Q1, Q2).** Undefined `x` in a CAS is a symbol, in marimo a reference. Proposed: explicit `symbols(...)` declaration, substitution via `.subs`, unresolved name is an error with a hint |
| 12 UI elements | Natural analogue of Mathematica `Manipulate`: slider bound to a variable, dependents recompute. Needs cheap re-evaluation, so ties to performance work |
| 13 file format | Open (Q3, Q4). Option A: marimo-style `@cell` functions (valid Python, explicit). Option B: plain script with `# %%` cell markers, references inferred. Leaning B |
| 15 linter | Provide `mathcraft check` with stable rule ids, usable in CI; same engine as the notebook |
| 11 expensive cells | Symbolic computation can blow up: need cancellation, timeouts, step/memory budgets in the kernel |
| Setup cell, SQL, apps, WASM | Later milestones; WASM works with low layers WASM-safe (D7) |

## Pitfalls worth designing for

- Static analysis cannot see dynamic names (`globals()`, `exec`); simply do not support them.
- Autorun of heavy cells: default to stale/lazy for cells over a time budget.
- No stored outputs hurts users who want results in the file: provide export (HTML/PDF/notebook) and an optional output sidecar.
- Reproducibility claim depends on determinism: our algebra results (term ordering, simplification) must be deterministic across runs and platforms.
