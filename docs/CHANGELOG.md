# Changelog

User-facing changes per version. Planning notes and status live in `refs/` (not tracked).

## Unreleased

- Slices: `xs[1:3]`, `xs[::-1]`, `s[:4]` on lists, tuples, strings, ranges and vectors, with Python's rules for negative and out-of-range bounds.
- f-strings: `f"{x:.3f}"`, `f"{n:,d}"`, `f"{name:>8}"`, `{x!r}` and nested specs, plus new `format(x, spec)` and `repr(x)` builtins.

- Fixed a wrong limit: `limit((1+x)**(1/x), x, 0)` gave 1; it is now e (`1 ** (1/0)` no longer collapses to 1).
- More limits work: variable exponents like `(1+1/x)**x`, `x*sin(1/x)`, `exp(-1/x**2)/x**2` and root differences like `sqrt(x**2+x) - x`.
- Improper integrals work: `integrate(exp(-x), (x, 0, oo))` gives 1 (it used to return `-exp(-oo) + 1`).

## 0.1.1 (2026-11-08)

- Scattering amplitudes (first step): `gamma_trace`, `slash`, `gamma`, `metric`, `comp`, `pdot` and `contract` compute Dirac traces and sum over Lorentz indices. `examples/amplitudes.py` derives the e⁺e⁻ → μ⁺μ⁻ cross-section 4πα²/(3s).
- Fixed a timing race in the UI tests that failed on slow machines (Windows CI).

## 0.1.0 (2026-11-07)

First public release.

- Notebook and Text views, tabs, folder sidebar, command palette (Cmd+P), optional Vim mode.
- Solver: polynomials to degree 4 in closed form, log and exponential equations, `nroots` for any degree.
- Right-click a result to copy it as text, LaTeX, Python, NumPy or Rust. `latex()`, `pycode()`, `rustcode()`.
- `x^2` is accepted as a power.
- Settings are remembered (Vim keys, appearance, accent colour); `theme.json` can override colours.
- New app icon.
- Much faster `expand` for large integer polynomials; `gcd` and `lcm` of polynomials; new `cancel`.
