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
- `solve` handles equations with square roots (`solve(sqrt(x+1) - x, x)`), removing extra roots from squaring.
- Linear algebra: `eigenvals`, `eigenvects`, `nullspace` and `charpoly` for exact matrices.
- Number theory: `isprime`, `nextprime`, `totient`, `factorint`. Special functions: `Gamma` (exact for integers and half-integers; `gamma` stays the Dirac matrix).
- `summation(f, (k, a, b))`: sums with numeric bounds, polynomials in k (symbolic upper limit), geometric series (also to `oo`).
- Assumptions: `x = symbols('x', positive=True)` (also `negative`, `nonnegative`, `nonpositive`, `nonzero`, `integer`). With them `simplify(sqrt(x**2))` is `x`, `log(x**3)` is `3*log(x)`, `abs(x)` is `x`, `solve` drops roots that contradict them, and `sin(n*pi)` is 0 for integer `n`. New `refine(expr)` and `is_positive`, `is_negative`, `is_nonnegative`, `is_nonpositive`, `is_nonzero`, `is_integer` (True, False, or None when unknown). Without assumptions every symbol is still real.
- Pattern matching and rewrite rules: names ending in `_` are wildcards. `replace(expr, pattern, new)` (or a list of `(pattern, new)` rules) rewrites every match, e.g. `replace(sin(t)**2 + cos(t)**2 + 5, sin(a_)**2 + cos(a_)**2, 1)` is 6. `pattern_match(expr, pattern)` shows what each wildcard matched and `findall(expr, pattern)` lists the matching subexpressions. Sums and products match in any order and a pattern may match part of a longer sum or product.
- `factor` is now a real factoriser: any polynomial with integer coefficients, in one or several variables, splits into irreducibles over the integers (`x**4 + 4`, `x**8 - 1`, `x**2*y - y**3`, `x**3 + y**3 + z**3 - 3*x*y*z`). It used to find only rational roots, and only in one variable.
- `gcd`, `lcm` and `cancel` are much faster on large multi-variable polynomials: a sparse modular (Zippel-style) gcd now runs before the Euclidean method, so gcds of products with tens of thousands of terms in 7 variables finish in a few seconds instead of giving up.
- Converting a long sum into a polynomial is no longer quadratic, which also speeds up `expand`, `cancel` and `factor` on large sums.
- `solve` splits a polynomial into irreducible factors over the integers and solves each, so `solve((x**2 - 2)*(x**3 - 3), x)` and `solve(x**6 - 1, x)` work. (`apart` already used the new factoriser through `factor`.)
- New `LambertW(x)` (principal branch, with derivative); `solve` uses it for `a*x*exp(k*x) == b` (`solve(x*exp(x) - 1, x)` is `[LambertW(1)]`).
- New `Si(x)`, the sine integral: `integrate(sin(a*x)/x, x)` is `Si(a*x)` and `integrate(sin(x)/x, (x, 0, oo))` is `pi/2`.
- `series` gives Laurent series at poles: `series(1/sin(x), x, 0, 4)` is `1/x + x/6 + 7*x**3/360`.
- `integrate` handles `1/(x**4 + 1)` and other quartics `x**4 + b*x**2 + c` that split over a square root (log and arctangent forms).

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
