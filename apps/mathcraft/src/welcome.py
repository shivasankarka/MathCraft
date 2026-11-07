# mathcraft notebook v1

# %% [markdown]
# # Welcome to MathCraft
# Edit a cell and press **Shift+Enter** to run it and move on. When a cell changes, the cells that use its
# variables update automatically. A name can be defined in one cell only. Unknown names such as `x` are symbols.
# Type two letters for completions, press F1 for the list of functions. Every cell below runs; the last section
# lists what does *not* work yet.

# %%
f = sin(x) * exp(-x / 5)

# %%
diff(f, x)

# %%
plot(f, (x, 0, 20))

# %% [markdown]
# ## Exact numbers
# Integers and fractions are exact and as large as you like. Decimals appear only when you ask for them.

# %%
1/3 + 1/6, sqrt(8), sin(pi/6), 2**100

# %%
factorial(30), binomial(50, 25), gcd(84, 36)

# %%
N(pi), float(sqrt(2)), N(sin(x) + sqrt(2))

# %% [markdown]
# ## Algebra

# %%
expand((x + y)**4)

# %%
factor(x**3 - 6*x**2 + 11*x - 6)

# %%
simplify(sin(x)**2 + cos(x)**2 + x*(x + 1) - x**2)

# %%
subs(x**2 + y, [x == 3, y == 1])

# %%
coeffs(x**3 - 2*x + 5, x), degree(x**3 - 2*x + 5, x)

# %% [markdown]
# ## Solving
# Polynomials (rational roots, quadratics, cubics, complex roots), equations with exp/log/trig, and linear systems, also with symbols.

# %%
solve(x**2 - 5*x + 6, x)

# %%
solve(x**2 + 2*x + 5, x)

# %%
solve(x**3 - 3*x + 1, x)

# %%
solve(exp(2*x) - 3*exp(x) + 2, x)

# %%
solve(2*sin(x) + 1, x)

# %%
nsolve(cos(x) == x, x, 1)

# %% [markdown]
# **Take a result into Python.** `pycode` writes a function for an expression or a list of them (`numpy=True` for NumPy, `rustcode` for Rust). Copy it into your own script.

# %%
print(pycode(solve(x**2 - 5*x + 6 == a, x), [a]))

# %%
solve([x + y == 3, x - y == 1], [x, y])

# %%
solve([x + y == a, x - y == b], [x, y])

# %% [markdown]
# ## Calculus

# %%
integrate(x**2 * exp(x), x)

# %%
integrate(sin(x)**2, (x, 0, pi))

# %%
diff(x**x * sin(x), x, 2)

# %%
series(sin(x) / x, x, 0, 8)

# %%
limit(sin(x) / x, x, 0), limit((x**2 - 1) / (x - 1), x, 1)

# %%
limit((x**2 + 1) / (2*x**2 + x), x, oo), limit(x * exp(-x), x, oo), limit(atan(x), x, -oo)

# %%
limit(1/x, x, 0, dir='+'), limit(1/x, x, 0, dir='-')

# %% [markdown]
# ## Rational functions
# Partial fractions, and integrals that use them.

# %%
apart(x**3 / (x**2 - 1))

# %%
integrate(1 / (x**2 - 1), x)

# %%
integrate((x + 3) / (x**2 + x + 1), x)

# %% [markdown]
# ## Complex numbers
# `I` (or `3j`) is the imaginary unit. Symbols are taken to be real.

# %%
z = expand((1 + 2*I) * (3 - I))
z, conjugate(z), abs(z), re(z), im(z)

# %%
sqrt(-4), 1 / (1 + I)

# %% [markdown]
# ## Vectors and matrices
# `vec` and `matrix` hold exact numbers and symbols by default. Pick an element type with `dtype=` (`i8 i16 i32 i64
# f32 f64`). Types are never mixed behind your back: `i32` plus `f64` is an error until you call `astype`.

# %%
A = matrix([[1, 2], [3, 4]])
A @ A, det(A), inv(A), transpose(A)

# %%
vec([x, y]) * 3 + vec([1, 2])

# %%
B = astype(A, f64)
inv(B)

# %%
v = vec([1, 2, 3], dtype=i32)
v * 2, dot(v, v), norm(vec([3, 4]))

# %% [markdown]
# ## Programming
# Functions, loops, conditions and comprehensions work as in Python.

# %%
def fib(n):
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a

[fib(k) for k in range(12)]

# %%
[n for n in range(2, 40) if all([n % d != 0 for d in range(2, n)])]

# %% [markdown]
# ## Reactive cells
# Change `rate` and run it: the cells below re-run by themselves.

# %%
rate = 3

# %%
total = rate * 10

# %%
total + 1

# %% [markdown]
# ## Plots

# %%
plot([sin(x), cos(x), sin(x) * cos(x)], (x, -6, 6))

# %% [markdown]
# ## Not working yet
# Things you may try that do not work today. Each has a clear error or a known limit.
#
# - **Solving** polynomials of degree 5 and up, equations needing log rules (`log(x) + log(x-1) == log(6)`), mixed ones like `cos(x) == x` (use `nsolve`), nonlinear systems, inequalities. Trig equations give one period of solutions.
# - **Integrals** of `exp(x**2)` and other special functions; no `+ C`; `log(x)` rather than `log|x|`.
# - **Limits** of the `(1 + 1/x)**x` kind, and `sin(x)/x` as x goes to infinity.
# - **Complex** `exp(I*x)` rewriting, `arg`, polar form.
# - **Matrices**: eigenvalues, SVD, float `rank`, assigning to one element (`A[0, 0] = 5`), and mixing a matrix with a vector.
# - **Scalars with types**: `x: i32 = 1` is not supported yet; only vectors and matrices carry a type.
# - **Python**: classes, imports, dicts, sets, slices, f-strings, `try`, generators.
# - **Interactive** sliders (`Manipulate`), 3D plots, saving outputs in the file, undo for deleting or moving cells.
