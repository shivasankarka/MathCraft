# Changelog

User-facing changes per version. Planning notes and status live in `refs/` (not tracked).

## Unreleased

- Slices: `xs[1:3]`, `xs[::-1]`, `s[:4]` on lists, tuples, strings, ranges and vectors, with Python's rules for negative and out-of-range bounds.
- f-strings: `f"{x:.3f}"`, `f"{n:,d}"`, `f"{name:>8}"`, `{x!r}` and nested specs, plus new `format(x, spec)` and `repr(x)` builtins.

- Fixed a wrong limit: `limit((1+x)**(1/x), x, 0)` gave 1; it is now e (`1 ** (1/0)` no longer collapses to 1).
- More limits work: variable exponents like `(1+1/x)**x`, `x*sin(1/x)`, `exp(-1/x**2)/x**2` and root differences like `sqrt(x**2+x) - x`.
- Improper integrals work: `integrate(exp(-x), (x, 0, oo))` gives 1 (it used to return `-exp(-oo) + 1`).
- `solve` handles polynomial systems such as `solve([x**2 + y**2 - 25, x - y - 1], [x, y])` by elimination; the answer is a list of solutions, each a list of `var == value`. Linear systems keep their flat form.
- More integrals: powers of sin and cos (`cos(x)**3`, `sin(x)**2*cos(x)**2`), `exp(a*x)*sin(b*x)`, hyperbolic products, `abs`, `1/sqrt(1-x**2)`, `1/sqrt(x**2+1)` and Gaussians (`exp(-x**2)` gives `sqrt(pi)*erf(x)/2`, so `integrate(exp(-x**2), (x, -oo, oo))` is `sqrt(pi)`).
- New functions: `erf`, `asinh`, `acosh`, `atanh`, `sec`, `csc`, `cot`, `atan2`.
- Exact complex values: `exp(I*pi)` is -1, `exp(I*pi/3)` is `1/2 + I*sqrt(3)/2`, `log(-1)` is `I*pi`. `simplify` now proves any trigonometric polynomial identity in sin and cos (it reduces modulo sin² + cos² = 1).

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
