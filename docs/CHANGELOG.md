# Changelog

User-facing changes per version. Planning notes and status live in `refs/` (not tracked).

## Unreleased

- New `laplace(f, t, s=s)` and `inverse_laplace(F, s, t=t)`. The forward transform covers sums of `c*t**n*exp(a*t)*trig(w*t)` (`sin`, `cos`, `sinh`, `cosh`), powers `t**p` and products or powers of trig functions: `laplace(t**2*exp(-3*t), t)` is `2/(s + 3)**3`, `laplace(sin(t)**2, t)` is `2/(s**3 + 4*s)`. The inverse goes through partial fractions: linear factors to any power, irreducible quadratics up to the square (`inverse_laplace(1/(s**2 + 2*s + 5), s)` is `exp(-t)*sin(2*t)/2`), and `s**p` (`1/sqrt(s)` is `1/(sqrt(pi)*sqrt(t))`).
- New `fourier(f, t, w=w)` and `inverse_fourier(F, w, t=t)` (convention `integral f(t) exp(-I*w*t) dt`): Gaussians `exp(a*t**2 + b*t + c)`, `exp(-a*abs(t))` and `1/(t**2 + a**2)`, with a polynomial factor in `t` and sums of these. `fourier(exp(-t**2), t)` is `sqrt(pi)*exp(-w**2/4)`, `fourier(1/(t**2 + 4), t)` is `pi*exp(-2*abs(w))/2`.
- `dsolve` solves linear systems with constant coefficients through the Laplace transform: `dsolve([diff(x(t), t) == y(t), diff(y(t), t) == -x(t)], [x(t), y(t)], ics=[1, 0])` is `[x(t) == cos(t), y(t) == -sin(t)]`. Without `ics` the constants `C1, C2, ...` are the initial values in order. Single higher-order equations fall back to the same route when variation of parameters gets stuck (`y'' + 4*y == t*cos(2*t)`). `inverse_laplace` now inverts any power (up to 8) of an irreducible quadratic and cancels nested fractions first.
- New `rref(matrix)`, giving `(reduced row echelon form, [pivot columns])`, and `svd(matrix)`, giving `(U, [singular values], V)` with `matrix == U * diag(s) * V^T`, exactly (compact: only the non-zero singular values, largest first). Singular values are square roots of the eigenvalues of `A^T A`, so they need those in closed form; entries come out as nested radicals when the values are irrational. `eigenvects` and `nullspace` now work when the eigenvalues are irrational (a sum of fractions counts as zero when its common numerator does).
- Definite integrals of `sin`/`cos` fractions over a full period are exact instead of floats: `integrate(1/(2 + cos(x)), (x, 0, 2*pi))` is `2*pi*sqrt(3)/3`. Limits now combine the limits of the parts of a sum, product or function value when one of them is infinite (`atan(tan(x/2))` as `x -> pi` from the left is `pi/2`), which is what the jumps of the half-angle antiderivative needed.
- A definite integral with `abs(u)` for linear `u` is split where `u` changes sign: `integrate(1/sqrt(abs(x)), (x, -1, 1))` is `4` (it used to fail), `integrate(exp(-abs(x)), (x, -oo, oo))` is `2`.
- Trigonometric inequalities use the smallest period: `solve(tan(x) > 1, x)` is `(n*pi + pi/4, n*pi + pi/2)` instead of two intervals per `2*pi`.

## 0.4.0 (2026-11-15)

- Sums recognise the classic power series: `summation(x**k/factorial(k), (k, 0, oo))` is `exp(x)`, with a polynomial factor in `k` (`k**2*x**k/factorial(k)` is `x**2*exp(x) + x*exp(x)`), and likewise `sin`, `cos`, `sinh`, `cosh` (`x**(2*k+1)/factorial(2*k+1)` and friends), `-log(1 - x)` for `x**k/k`, and `atan`/`atanh` for `x**(2*k+1)/(2*k+1)`. So `summation((-1)**k/k, (k, 1, oo))` is `-log(2)`, `summation((-1)**k/(2*k+1), (k, 0, oo))` is `pi/4`, and `summation(1/(k*(k+1)*2**k), (k, 1, oo))` is `1 - log(2)`. A symbolic ratio is taken to converge; a numeric one outside the radius is refused. Fractions that telescope also sum to a symbolic end: `summation(1/(k*(k+1)), (k, 1, n))` is `1 - 1/(n + 1)`.
- New `product(f, (k, a, b))`: numeric bounds multiply out, and with a symbolic end the factors may be constants, powers with an exponent linear in `k`, or linear in `k` (through the Gamma function): `product(k, (k, 1, n))` is `Gamma(n + 1)`, `product(1 - 1/k**2, (k, 2, n))` is `(n + 1)/(2*n)`. `simplify` reduces a Gamma over another whose arguments differ by an integer (`Gamma(n + 3)/Gamma(n)` is `n*(n + 1)*(n + 2)`). `factorial(n)` of a symbol is `Gamma(n + 1)` instead of an error.
- Rational functions of `sin` and `cos` integrate through the half-angle substitution: `integrate(1/(2 + cos(x)), x)` is `2*sqrt(3)*atan(sqrt(3)*tan(x/2)/3)/3`. Its `tan(x/2)` jumps at odd multiples of `pi`, so a definite integral splits there; where the exact limits are out of reach it gives a number (`integrate(1/(2 + cos(x)), (x, 0, 2*pi))` is `3.62759...`). The numeric integrator now starts from 32 panels, because a single one could look converged by coincidence (`(-pi, pi)` gave `3.49` instead of `3.63`). `tan` at an odd multiple of `pi/2` counts as a pole.
- The tutorial notebook and the README no longer list trigonometric inequalities and `exp(x**2)` as missing.
- More integrals: odd powers of `sqrt(a - x**2)` and `sqrt(a + x**2)` times a rational factor, by trigonometric (and, where that gets stuck, hyperbolic) substitution. `integrate(x**2/sqrt(1 - x**2), x)` is `asin(x)/2 - x*sqrt(1 - x**2)/2`, `1/(x**2*sqrt(x**2 + 4))` is `-sqrt(x**2 + 4)/(4*x)`, and `x**2/sqrt(x**2 + 9)` is `x*sqrt(x**2 + 9)/2 - 9*asinh(x/3)/2`.
- New `erfi(x)`, and `integrate(exp(x**2), x)` is `sqrt(pi)*erfi(x)/2` (any `exp(a*x**2 + b*x + c)` with a positive `a`).
- `solve` handles trigonometric inequalities: `solve(sin(x) > 1/2, x)` is `(2*n*pi + pi/6, 2*n*pi + 5*pi/6)` for any integer `n`; it works for `cos`, `tan`, multiples and shifts of the angle (`cos(3*x - 1) > 1/2`) and polynomials in trig functions, and keeps poles open. Inequalities that mix `x` with `sin(x)` still give a message.
- `simplify` contracts double angles (`2*sin(x)*cos(x)` is `sin(2*x)`; `cos(x)**2 - sin(x)**2`, `1 - 2*sin(x)**2` and `2*cos(x)**2 - 1` are `cos(2*x)`) and takes squares out of roots (`sqrt(8*x**2 + 8)` is `2*sqrt(2*x**2 + 2)`). `solve` does the same for roots with other symbols, so a circle solved for y gives `sqrt(1 - x**2)` instead of `sqrt(-4*x**2 + 4)/2`. New `trigsimp(expr)`, the same as `simplify`.
- Fixed wrong definite integrals across a pole: `integrate(1/x**2, (x, -1, 1))` answered `-2` and `integrate(tan(x), (x, 0, pi))` answered `-I*pi`. They now stop with "the integral diverges", as do `1/x` on (0, 1) and `x` on (0, oo). A pole inside the interval splits it, so convergent cases such as `1/sqrt(x)` on (0, 1) and `log(x)` on (0, 1) stay exact, and `1/(x-2)` on (0, 1) is `-log(2)`. Numeric fallback no longer returns `nan` when the integrand blows up.
- Plots: right-click to show or hide the grid and the axes and to space the tick marks (automatic, far apart, close together). The choice is per plot and lasts until you close the app. Saved SVG and PNG pictures follow it, so they match what you see; without the grid they now carry short tick marks instead of grid lines.
- README rewritten for newcomers: who it is for, a two-minute install table, a first-five-minutes guide, and an honest list of what does not work yet.
- The interface size you set with Cmd or Ctrl and + / - (0 resets) is remembered between runs.
- Browser page: every cell has a Run button (so it works on a phone, where there is no Shift+Enter); Ctrl or Cmd+Enter also run.
- The notebook that opens on first start is now a tutorial and reference: the app (keys, palette, symbols window, action row, folders, git, results), exact numbers, Python, algebra, equations, calculus, trigonometry and complex numbers, special functions, matrices, numerics, plots and data, sliders, exporting code, Dirac traces, the command line, and a list of what does not work yet. Every cell runs.
- `dsolve` refuses `ics=[y(0) == 1]` (an equation or comparison) with a message instead of silently answering `y(x) == 0`; write `ics=[1]`.
- View > Changes since the last git commit: the open notebook compared with its committed version, line by line (added in green, removed in red, with two lines of context), updating as you edit. Formatting alone is not a change. Read-only; nothing is staged or committed.
- Symbols palette: a calculator keypad (digits, operators, brackets, √, π, e) at the top; each key types at the cursor in the code cell you were editing.
- Git, read-only: when the opened folder is in a git repository (and `git` is installed), the sidebar marks notebooks that are changed (M), new (U), staged (A), deleted (D) or in conflict (!), a closed folder shows a dot when something inside changed, and the status bar shows the branch with how far ahead or behind it is and how many files changed. It refreshes every few seconds and after saving.
- Symbols palette: a matrix of any size (up to 8 × 8): set the rows and columns, press Insert, and Tab through the entries.
- CSV and TSV files: `read_csv("data.csv", header=True, columns=True)` reads numbers as numbers (`x, y = read_csv(...)` feeds `scatter(x, y)`), and `write_csv("out.csv", rows, header=[...])` writes a list of rows or a matrix. Files are only reachable inside the notebook's own folder (the working directory in the command line tool): no absolute paths, no `..`, symbolic links are checked, and only `.csv`, `.tsv` and `.txt` are accepted. Save a new notebook first so it has a folder. The MCP server and the browser build have no file access.
- An action row under the menus: Simplify, Expand, Factor, Solve, Differentiate, Integrate, Numeric and Plot wrap the selected text (or the expression on the cursor's line, after any `name =`) in that call, guessing the variable and selecting it so it is easy to change. View > Show or hide the action row.
- Symbols palette: Tab moves to the next placeholder of the inserted template (`integrate(f, (x, a, b))`: f, x, a, b), and after the last one past the template.
- `integrate` handles a polynomial times `exp` times `sin` or `cos` (`x**2*exp(x)*sin(x)`), and integrals of powers of `sin` and `cos` come out as one flat sum (`sin(x)**5*cos(x)/6 - …`) instead of nested fractions.
- Charts of data: `scatter(xs, ys)` (dots), `bar(labels, heights)` and `hist(data, bins=10)`. They zoom, pan, copy as CSV and save as SVG or PNG like any plot.
- Windows: the app no longer opens a console window beside it.
- Numerics: `nintegrate(f, (x, a, b))` (adaptive quadrature; `oo` ends allowed, for integrals with no closed form), `fzero(f, (x, a, b))` (a root where `f` changes sign) and `minimize` / `maximize(f, (x, a, b))` (`[x, f(x)]`).
- Symbols palette (F2, or the Symbols button): buttons for integrals, derivatives, sums, limits, roots, fractions, trigonometry, constants, relations and matrices insert the code at the cursor with the first placeholder selected. Notebooks stay plain text.
- Derivatives are typeset as d/dx, dⁿ/dxⁿ and ∂/∂x (an unknown function's `diff(y(x), x)` too), in the app and in `latex()`. New `Derivative(f, x, n)` stays unevaluated, like `Integral`; `doit()` evaluates it.

## 0.3.0 (2026-11-13)

- Complex numbers: `re`, `im`, `conjugate` and `abs` work on `exp(I*x)`, `sin`, `cos`, `sinh`, `cosh`, `tan`, `tanh` and `log` of complex arguments; `(1 + I)**10` is `32*I`; `sin(I)` is `I*sinh(1)`; `N(exp(1 + 2*I))` gives a complex number. New `arg(z)`, `rewrite_trig` (exponentials to sin and cos) and `rewrite_exp`; `simplify((exp(I*x) + exp(-I*x))/2)` is `cos(x)`.
- `solve` handles polynomials in `sin` and `cos` of one angle (`sin(x)**2 - cos(x)**2`, `sin(x) + cos(x) == 1`, `3*sin(x) + 4*cos(x) == 5`); `asin(sqrt(2)/2)` is `pi/4`.
- `sqrt(3 + 4*I)` is `2 + I` (exact when the modulus is rational), and `re`, `im`, `abs` and `N` handle the square root of any complex number. `arg` and `atan2` of symbolic values stay as `atan2(y, x)` and evaluate numerically. A complex curve in `plot` is drawn as its real and imaginary parts.
- `solve(sin(x)*cos(x) == 1/4, x)` gives `[pi/12, 5*pi/12]`: quadratic forms in `sin` and `cos` go through the double angle.
- `expand_trig` (`sin(x + y)`, `cos(2*x)`, `sin(3*x)` in terms of `sin(x)` and `cos(x)`); `simplify` uses it (`sin(2*x)/sin(x)` is `2*cos(x)`, `sin(3*x) - 3*sin(x) + 4*sin(x)**3` is 0) and so does `solve` (`cos(2*x) == cos(x)`, `sin(x) + sin(3*x) == 0`). `cos(acos(x))`, `sin(acos(x))`, `tan(atan(x))` and the like are evaluated, and `atan(sqrt(3))` is `pi/3`.
- Limits: `limit(tan(x), x, pi/2, '-')` is `oo` (it returned `tan(pi/2)`), and `limit(abs(x)/x, x, 0, '+')` is 1.
- `simplify` makes a nested fraction one (`1/(1 + 1/x)` is `x/(x + 1)`); `solve(x**2 == 4*y, x)` gives `[-2*sqrt(y), 2*sqrt(y)]`; `diff(atan2(y, x), x)` works.
- Infinite sums: `summation(1/k**2, (k, 1, oo))` is `pi**2/6` (even zeta values), telescoping sums such as `1/(k*(k + 1))` give 1, and `k*2**(-k)`-type sums (a polynomial times a geometric term) are summed.
- `integrate` handles `sec` and `csc`, products of sines and cosines of different multiples (`sin(x)*cos(2*x)`), and `x**r*log(x)` for a fractional `r`.
- `solve(x**2 > 4, x)` solves inequalities in one real unknown (`<`, `<=`, `>`, `>=`): polynomials, rational functions, `log`, square roots and `abs`. The answer is an interval set such as `(-oo, -2) ∪ (2, oo)`, or `∅`; comparing symbols no longer errors but stays an inequality. Trigonometric inequalities are refused for now.
- `dsolve(eq, y(x), ics=[...], x0=0)` solves ordinary differential equations: first order linear, separable, Bernoulli (`y' = a y + b y**n`, one real branch) and homogeneous (`y' = F(y/x)`), and linear with constant coefficients (any order; a forcing term up to second order). `dsolve(diff(y(x), x) + y(x) == x, y(x))` is `y(x) == C1*exp(-x) + x - 1`; `ics` lists `y(x0)`, `y'(x0)`, ...
- Typeset: n-th roots with an index, long sums break into lines to fit the window, and `Integral(f, (x, a, b))`, `Sum(f, (k, a, b))`, `Limit(f, x, a)` stay unevaluated and read as mathematics (`doit()` evaluates them). They also copy as LaTeX.
- Menus: rows have shortcuts as keycaps, an accent highlight and dimmed disabled items.
- Plots can be saved as PNG (right-click a plot: Save as PNG…) as well as SVG.
- macOS: the release also ships `MathCraft.app` with the app icon.
- The engine builds to WebAssembly: a small demo page and `mathcraft.js` (`load(...)`, `run(code)` returning the value as text and LaTeX) are on the Releases page as `mathcraft-web-<version>.zip`.
- Faster exact polynomial division and polynomial gcd (about 1.8 times on the 7-variable benchmark).

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
