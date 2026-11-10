# Changelog

User-facing changes per version. Planning notes and status live in `refs/` (not tracked).

## 0.2.0 (2026-11-10)

- Command line: `mathcraft-cli export notebook.py -o out.html` runs a notebook and saves it as a standalone HTML page or Markdown (`-f md|html`, or by the file extension). `eval -` and `run -` read from stdin, `--timeout <seconds>` sets the time limit, `--version` prints the version, and the REPL quits on `exit`.
- The command line tool is also a separate download on the Releases page (no window system or GUI libraries needed).
- Drag a tab to reorder it; the order is kept in the restored session.
- Drag a cell by the grip in its toolbar to reorder it; one drag is one Cmd+Z step.
- Cmd+Alt+Up / Cmd+Alt+Down move the keyboard to the previous or next code cell. Printed output longer than 24 lines is folded to 20 with a "Show N more lines" button.
- Command palette: Shift+Enter on a function inserts `name(` into the cell you were editing; Enter still opens the function list for it.
- Hover help: point at a built-in function name in a cell (`diff`, `integrate`, ...) to see its signature and summary.
- Math in text cells: `$x**2 - 2$` is typeset inline and a line `$$ integrate(1/(x**2+1), x) $$` is typeset on its own, centred. The text between the dollars is a MathCraft expression and it is evaluated (so `$exp(I*pi) + 1$` shows 0); something that is not an expression, or that takes too long, stays as plain text. `$5 and $6` is left alone, and `\$` is a literal dollar.
- The slider is named after its variable (also read by screen readers).
- Plots you can explore: drag to pan, pinch or Ctrl+scroll to zoom, right-drag a box to zoom into it, double-click to reset. The coordinates under the pointer show in the corner. Right-click a plot to reset the view, copy its points as CSV or save the view as an SVG picture. (PNG export is not there yet.)
- Sliders: `a = slider(0, 10, 2)` (optionally a step, `slider(0, 1, 0.5, 0.05)`) shows a slider under the cell. Dragging it rewrites the value in the cell and re-runs everything that uses `a`; while the kernel is busy a drag waits instead of piling up runs. See `examples/sliders.py`.
- Session restore: MathCraft reopens the tabs, the active tab, the folder and the sidebar from last time (unless you start it with a file or folder). File > Open recent and the command palette list the last 12 files.
- Undo and redo for cell changes: Cmd+Z / Cmd+Shift+Z (when no editor has focus), the Edit menu or the palette bring back a deleted cell or put a moved or added one back. Typing in other cells is never undone.
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
- Limits at infinity: polynomials such as `x**2 - 3*x` (the leading term decides) and pairs of logarithms with the same coefficient (`log(x**2 + x) - log(x**2)`). With this, `integrate(1/(x**4 + 1), (x, 0, oo))` is `pi*sqrt(2)/4`.
- `factor` of a fraction cancels it and factors the numerator and denominator; new `together(expr)`.
- New special functions `Ci`, `Ei`, `erfc`, with `integrate(cos(a*x)/x, x)` (`Ci`), `integrate(exp(a*x)/x, x)` (`Ei`) and exact `LambertW(e)`.
- The modular gcd copes with large coefficients (it uses up to 48 primes and only tries the exact division once the result has stopped changing): a degree-25 bivariate gcd with 12-digit coefficients takes about 0.1 s.
- `factor(expr, extension=sqrt(2))` factors a polynomial in one symbol over `Q(sqrt(2))`, `Q(I)` or `Q(2**(1/3))` (any single radical): `factor(x**4 + 1, extension=sqrt(2))` is `(x**2 - sqrt(2)*x + 1)*(x**2 + sqrt(2)*x + 1)`.
- More integrals: by parts with `log`, `atan`, `asin`... (`x*atan(x)`, `log(x)**3`), `x**n*exp(c*x**2)`, substitutions in `exp(c*x)` (`1/(1 + exp(x))`) and in linear arguments (`x*sqrt(x + 1)`), `log(x)/x`, and repeated irreducible quadratics (`1/(x**2 + 1)**2`).
- More limits at infinity: a decaying factor times a bounded oscillation (`exp(-x)*sin(x)`, `sin(x)/x`) and differences of square roots (`sqrt(x + 1) - sqrt(x)`).
- `solve` handles `a**x == b**x`-type equations (`solve(3**x - 2**x, x)`) by taking logarithms.

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
