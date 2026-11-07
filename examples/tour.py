# mathcraft notebook v1

# %% [markdown]
# # A tour of MathCraft
# Every `# %%` line starts a cell. This file is plain Python-flavoured code, so it also runs from the command line:
# `mathcraft-cli run examples/tour.py`. Cells that read a variable re-run when the cell that defines it changes.

# %% [markdown]
# ## Exact algebra

# %%
(x + y)**3

# %%
expand((x + y)**3)

# %%
factor(x**3 - 6*x**2 + 11*x - 6)

# %%
1/3 + 1/6, sqrt(8), 2**100

# %% [markdown]
# ## Calculus

# %%
f = sin(x) * exp(-x / 5)

# %%
diff(f, x)

# %%
integrate(x**2 * exp(x), x)

# %%
integrate(sin(x)**2, (x, 0, pi))

# %%
integrate(exp(-x**2), (x, 0, 1))

# %%
limit(sin(x) / x, x, 0)

# %%
series(exp(x), x, 0, 5)

# %% [markdown]
# ## Equations and linear algebra

# %%
solve(x**2 - 5*x + 6, x)

# %%
solve([x + y == 3, x - y == 1], [x, y])

# %%
A = [[2, 1], [1, 3]]

# %%
det(A), inv(A), A @ A

# %% [markdown]
# ## Programming

# %%
def fib(n):
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a

# %%
[fib(k) for k in range(15)]

# %%
taylor = sum([x**k / factorial(k) for k in range(6)])
taylor

# %%
plot([sin(x), taylor], (x, -4, 4))
