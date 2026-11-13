# mathcraft notebook v1

# %% [markdown]
# # Welcome to MathCraft
# This notebook is the manual. Every cell below runs, so read it, change things, and press **Shift+Enter** to run a
# cell and move on. Use the search in the command palette (**Cmd+P**, Ctrl+P on Windows and Linux) to find any
# command, and **F1** for the list of every function. Sections:
#
# 1. The notebook and the app
# 2. Exact numbers
# 3. Python in MathCraft
# 4. Algebra
# 5. Equations
# 6. Calculus
# 7. Trigonometry and complex numbers
# 8. Special functions and number theory
# 9. Vectors and matrices
# 10. Numerics
# 11. Plots and data
# 12. Sliders
# 13. Taking results out
# 14. Physics: Dirac traces
# 15. Command line, AI assistants, browser
# 16. What does not work yet

# %%
f = sin(x) * exp(-x / 5)

# %%
diff(f, x)

# %%
plot(f, (x, 0, 20))

# %% [markdown]
# ## 1. The notebook and the app
# A notebook is a plain text file (`.py`): a `# %%` line starts a cell and `# %% [markdown]` starts a text cell
# like this one. You can edit it in any editor, keep it in git, and run it from the command line.
#
# **Running**
# - **Shift+Enter** runs the cell and moves to the next one; **Cmd+Enter** runs it and stays.
# - **Cmd+Shift+Enter** runs every cell, **Cmd+.** interrupts a computation. Run > Dependents only turn stale makes
#   changed cells wait until you run them (lazy mode).
# - Cells that read a name re-run when the cell that defines it changes. A name can be defined in **one** cell only;
#   start a name with `_` to keep it private to its cell. Names you never define (`x`, `y`, …) are symbols.
#
# **Editing**
# - Type two letters for completions; **Tab** or **Enter** accepts one. Point at a function name to see its signature.
# - **F1** lists every function; **Shift+Enter** in that list puts `name(` into your cell.
# - **F2** opens the symbols window: a calculator keypad, integrals, sums, limits, roots, fractions, trigonometry,
#   constants, relations and matrices of any size. **Tab** walks through the blanks of what you inserted.
# - The row under the menus (Simplify, Expand, Factor, Solve, Differentiate, Integrate, Numeric, Plot) wraps the
#   selected text, or the expression on the cursor's line, in that call. View > Action row hides it.
# - Optional Vim keys: Settings > Vim keys in the command palette.
#
# **Cells and tabs**
# - Drag a cell by the grip in its toolbar, or a tab by its title, to reorder. **Cmd+Z** brings back a deleted or
#   moved cell. **Cmd+Alt+Up/Down** jumps to the previous or next code cell.
# - **Cmd+Shift+T** switches to Text view (the raw file); Notebook view is the default. **Cmd+B** shows the sidebar.
# - MathCraft remembers your tabs, folder and settings between sessions. Theme and accent colour are in the palette;
#   `theme.json` can override any colour.
#
# **Folders and git**
# - Open a folder (File > Open folder) and the sidebar lists its notebooks. If it is a git repository, notebooks
#   show M (changed), U (new), A (staged), D (deleted) or ! (conflict), and the status bar shows the branch.
# - View > Changes since the last git commit shows what you changed in this notebook, line by line.
#
# **Results**
# - Right-click a result to copy it as text, LaTeX, Python, NumPy or Rust.
# - Right-click a plot to copy its points as CSV or save it as SVG or PNG; drag to pan, Ctrl+scroll or pinch to zoom,
#   right-drag a box to zoom into it, double-click to reset.
# - Text cells understand Markdown: `# headings`, `**bold**`, `*italic*`, `` `code` ``, lists, quotes, and maths.
#   Write `$x**2 - 2$` for inline maths and `$$ integrate(1/(x**2+1), x) $$` on its own line for displayed maths:
#   the text between the dollars is a MathCraft expression and it is evaluated, so $exp(I*pi) + 1$ shows 0 and
#   $$ integrate(1/(x**2+1), x) $$

# %% [markdown]
# ## 2. Exact numbers
# Integers and fractions are exact and as large as you like. A decimal appears only when you ask for one.

# %%
1/3 + 1/6, sqrt(8), sin(pi/6), 2**100

# %%
factorial(30), binomial(50, 25), gcd(84, 36), lcm(4, 6)

# %%
N(pi), float(sqrt(2)), N(sin(x) + sqrt(2))

# %%
sqrt(2)**2, (1 + sqrt(2)) * (1 - sqrt(2)), exp(I*pi), log(exp(3))

# %% [markdown]
# ## 3. Python in MathCraft
# The language is Python-flavoured: functions, loops, conditions, comprehensions, tuples, slices and f-strings work
# as you expect. Not available: classes, imports (everything is already loaded), dictionaries, sets, `try`, and
# assigning to one element of a list (build a new value instead).

# %%
def fib(n):
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a

[fib(k) for k in range(12)]

# %%
[n for n in range(2, 40) if all([n % d != 0 for d in range(2, n)])]

# %%
def collatz(n):
    steps = 0
    while n != 1:
        n = n // 2 if n % 2 == 0 else 3*n + 1
        steps += 1
    return steps

collatz(27), max([collatz(k) for k in range(1, 30)])

# %%
words = ["alpha", "beta", "gamma"]
[len(w) for w in words], words[::-1], len(words[0]), sorted(words, reverse=True)

# %%
for _i, _w in enumerate(words):
    print(f"{_i}: {_w:>8}  {N(pi):.4f}  {2**(_i + 10):,d}")

# %%
square = lambda t: t**2
[square(k) for k in range(5)], sum([1/k for k in range(1, 6)])

# %% [markdown]
# ## 4. Algebra
# `expand`, `factor` (full factorisation of integer polynomials in any number of variables), `simplify`, `cancel`,
# `together`, `apart`, `subs`, `coeffs`, `degree`.

# %%
expand((x + y)**4)

# %%
factor(x**3 - 6*x**2 + 11*x - 6), factor(x**4 + 1, extension=sqrt(2))

# %%
simplify(sin(x)**2 + cos(x)**2 + x*(x + 1) - x**2)

# %%
cancel((x**2 - 1) / (x - 1)), together(1/x + 1/y), apart(x**3 / (x**2 - 1))

# %%
subs(x**2 + y, [x == 3, y == 1])

# %%
coeffs(x**3 - 2*x + 5, x), degree(x**3 - 2*x + 5, x), free_symbols(a*x + b*y)

# %% [markdown]
# **Assumptions.** Symbols are real by default. Declare more with `symbols('n', positive=True)` (also `negative`,
# `nonnegative`, `nonpositive`, `nonzero`, `integer`); `refine` and `simplify` use them.

# %%
p = symbols('p', positive=True)
simplify(sqrt(p**2)), simplify(log(p**3)), is_positive(p)

# %% [markdown]
# **Patterns.** A name ending in `_` is a wildcard. `replace` rewrites every match, `pattern_match` shows what each
# wildcard matched, `findall` lists the matching parts.

# %%
replace(sin(t)**2 + cos(t)**2 + 5, sin(a_)**2 + cos(a_)**2, 1), findall(sin(x) + sin(2*y), sin(a_))

# %% [markdown]
# ## 5. Equations
# `solve` handles polynomials (also of degree above 4 when they factor), equations with exp, log, roots, `abs` and
# trigonometry, linear and polynomial systems, and inequalities in one real unknown. Symbols may stay in the answer.

# %%
solve(x**2 - 5*x + 6, x), solve(x**2 + 2*x + 5, x)

# %%
solve(x**3 - 3*x + 1, x)

# %%
solve(exp(2*x) - 3*exp(x) + 2, x), solve(log(x) + log(x - 1) - log(6), x), solve(sqrt(x + 1) - x, x)

# %%
solve(2*sin(x) + 1, x), solve(cos(2*x) == cos(x), x)

# %%
solve([x + y == 3, x - y == 1], [x, y])

# %%
solve([x + y == a, x - y == b], [x, y])

# %%
solve([x**2 + y**2 - 25, x - y - 1], [x, y])

# %%
solve(x**2 > 4, x), solve(abs(x - 1) <= 2, x)

# %% [markdown]
# **Differential equations.** Declare the unknown function first, then use `dsolve` (first-order linear, separable,
# Bernoulli and homogeneous equations, and linear equations with constant coefficients of any order). `ics` lists the
# values y(0), y'(0), … (at `x0`, which is 0 unless you say otherwise).

# %%
y = symbols('y')
dsolve(diff(y(x), x) + y(x) == x, y(x))

# %%
dsolve(diff(y(x), x, 2) + 4*y(x), y(x), ics=[1, 0])

# %% [markdown]
# For what has no closed form: `nsolve(eq, x, guess)` for one number and `nroots(poly, x)` for every complex root.

# %%
nsolve(cos(x) == x, x, 1), nroots(x**5 - x - 1, x)

# %% [markdown]
# ## 6. Calculus

# %%
diff(x**x * sin(x), x, 2)

# %%
integrate(x**2 * exp(x), x), integrate(sin(x)**2, (x, 0, pi))

# %%
integrate(1 / (x**2 - 1), x), integrate((x + 3) / (x**2 + x + 1), x)

# %%
integrate(exp(-x**2), (x, -oo, oo)), integrate(sin(x) / x, (x, 0, oo))

# %%
integrate(x**2 * exp(x) * sin(x), (x, 0, 1))

# %%
limit(sin(x) / x, x, 0), limit((1 + 1/x)**x, x, oo), limit(x * exp(-x), x, oo)

# %%
limit(1/x, x, 0, dir='+'), limit(1/x, x, 0, dir='-')

# %%
series(sin(x) / x, x, 0, 8), series(1 / sin(x), x, 0, 4)

# %%
summation(k**2, (k, 1, n)), summation(1/k**2, (k, 1, oo)), summation(2**(-k), (k, 1, oo))

# %% [markdown]
# `Integral`, `Sum`, `Derivative` and `Limit` stay unevaluated and read as mathematics; `doit` evaluates them.

# %%
Integral(sin(x)**2, (x, 0, pi))

# %%
doit(Integral(sin(x)**2, (x, 0, pi))), doit(Derivative(x**3, x, 2))

# %% [markdown]
# ## 7. Trigonometry and complex numbers
# `I` (or `3j`) is the imaginary unit. `expand_trig` uses the addition formulas; `rewrite_trig` and `rewrite_exp`
# convert between exponentials and sines and cosines.

# %%
expand_trig(sin(x + y)), expand_trig(cos(3*x))

# %%
simplify(sin(2*x) / sin(x)), rewrite_trig(exp(I*x)), rewrite_exp(cos(x))

# %%
z = expand((1 + 2*I) * (3 - I))
z, conjugate(z), abs(z), re(z), im(z), arg(1 + I)

# %%
sqrt(-4), 1 / (1 + I), sqrt(3 + 4*I), (1 + I)**10

# %% [markdown]
# ## 8. Special functions and number theory

# %%
erf(x), Gamma(5), Gamma(1/2), LambertW(1), Si(x), Ci(x), Ei(x)

# %%
integrate(exp(-x**2), x), solve(x * exp(x) - 1, x)

# %%
isprime(97), nextprime(100), totient(36), factorint(360)

# %% [markdown]
# ## 9. Vectors and matrices
# `vec` and `matrix` hold exact numbers and symbols by default. Pick an element type with `dtype=` (`i8 i16 i32 i64
# f32 f64`). Types are never mixed silently: `i32` plus `f64` is an error until you call `astype`. Use `@` for the
# matrix product and `*` for scaling.

# %%
A = matrix([[1, 2], [3, 4]])
A @ A, det(A), inv(A), transpose(A), rank(A)

# %%
eigenvals(matrix([[2, 1], [1, 2]])), charpoly(matrix([[2, 1], [1, 2]]), x)

# %%
eigenvects(matrix([[2, 1], [1, 2]])), nullspace(matrix([[1, 2], [2, 4]]))

# %%
vec([x, y]) * 3 + vec([1, 2]), A @ vec([1, 1]), shape(A), eye(3)

# %%
B = astype(A, f64)
inv(B)

# %%
v = vec([1, 2, 3], dtype=i32)
v * 2, dot(v, v), norm(vec([3, 4])), zeros((2, 2)), ones(3)

# %% [markdown]
# ## 10. Numerics
# For integrals and equations with no closed form. `N(expr)` approximates; the others search numerically.

# %%
nintegrate(exp(-x**2) * cos(x), (x, -oo, oo))

# %%
fzero(cos(x) - x, (x, 0, 1)), minimize(x**2 - 2*x + 5, (x, -3, 3)), maximize(sin(x), (x, 0, 3))

# %% [markdown]
# ## 11. Plots and data
# `plot` draws one or more curves; `scatter`, `bar` and `hist` draw data. All of them zoom, pan, and save as SVG or
# PNG.

# %%
plot([sin(x), cos(x), sin(x) * cos(x)], (x, -6, 6))

# %%
taylor = sum([(-1)**k * x**(2*k + 1) / factorial(2*k + 1) for k in range(5)])
plot([sin(x), taylor], (x, -6, 6))

# %%
scatter([1, 2, 3, 4, 5], [1, 4, 9, 16, 25])

# %%
bar(["Mon", "Tue", "Wed"], [3, 5, 2])

# %%
hist([1, 2, 2, 3, 3, 3, 4, 4, 5], bins=5)

# %% [markdown]
# **Files.** Once the notebook is saved, it can read and write CSV and TSV files in its own folder (no absolute paths,
# no `..`):
#
# ```
# xs, ys = read_csv("data.csv", header=True, columns=True)
# scatter(xs, ys)
# write_csv("out.csv", [[1, 2], [3, 4]], header=["a", "b"])
# ```

# %% [markdown]
# ## 12. Sliders
# `slider(lo, hi, value, step)` is a number you can drag. Everything that uses its name re-runs as it moves, and the
# value is written back into the cell.

# %%
amp = slider(1, 5, 2)

# %%
decay = slider(0, 6, 1.5, 0.25)

# %%
g = amp * sin(x) * exp(-x / (decay + 1))
plot(g, (x, 0, 10))

# %% [markdown]
# ## 13. Taking results out
# `latex`, `pycode` and `rustcode` turn an expression into source text. Copy it into a paper or a script
# (`numpy=True` writes NumPy code).

# %%
latex(integrate(1 / (x**2 + 1), x)), latex(matrix([[1, 2], [3, 4]]))

# %%
print(pycode(solve(x**2 - 5*x + 6 == a, x), [a]))

# %%
print(rustcode(sin(x) * exp(-x), [x]))

# %% [markdown]
# ## 14. Physics: Dirac traces
# `gamma`, `slash`, `gamma_trace`, `metric`, `comp`, `pdot` and `contract` compute traces of Dirac matrices and sum
# over Lorentz indices. The file `examples/amplitudes.py` derives the cross-section of e⁺e⁻ → μ⁺μ⁻.

# %%
gamma_trace(slash(p), gamma(mu), slash(q), gamma(nu))

# %%
contract(gamma_trace(gamma(mu), gamma(nu)) * metric(mu, nu))

# %% [markdown]
# ## 15. Command line, AI assistants, browser
# - `mathcraft-cli eval 'integrate(sin(x)**2, (x, 0, pi))'` evaluates code; `mathcraft-cli run notebook.py` runs a
#   notebook; `check` looks for problems; `export notebook.py -o out.html` writes a page or Markdown; `repl` is an
#   interactive prompt.
# - `mathcraft-cli mcp` serves the engine to AI assistants over MCP, with no access to your files.
# - The same engine builds to WebAssembly for a web page (the release's `mathcraft-web` zip).
# - Settings > Strict names makes an unknown name an error instead of a symbol, which catches typos.

# %% [markdown]
# ## 16. What does not work yet
# Things you may try that fail today, each with a clear message:
#
# - **Solving**: polynomials of degree 5 and up that do not factor (use `nroots`), equations like `cos(x) == x`
#   (use `nsolve`), most nonlinear systems that are not polynomial, inequalities with `sin`, `cos` or `tan`.
# - **Integrals** of `exp(x**2)` and some other special functions come back as `integrate(...)` unevaluated; there is
#   no `+ C`, and `1/x` integrates to `log(x)`, not `log|x|`.
# - **Differential equations**: second-order equations with variable coefficients, systems, and partial equations.
# - **Transforms**: Laplace and Fourier transforms.
# - **Matrices**: SVD, assigning to one element (`A[0, 0] = 5`), and `*` between a matrix and a vector (use `@`).
# - **Python**: classes, imports, dictionaries, sets, `try`, and generators.
# - **Plots**: 3D surfaces, contour plots and overlaying several data series.
