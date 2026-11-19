# %% [markdown]
# # Transforms, differential equations and special functions
# Laplace and Fourier transforms, steps and impulses, solving ODEs, and sums that end in `zeta` and `polylog`.

# %% [markdown]
# ## Laplace transform

# %%
laplace(t**2 * exp(-3*t), t)

# %%
inverse_laplace(1 / (s**2 + 2*s + 5), s)

# %%
inverse_laplace(exp(-s) / (s**2 + 1), s)

# %% [markdown]
# ## Fourier transform

# %%
fourier(exp(-t**2), t)

# %%
fourier(exp(-abs(t)), t)

# %% [markdown]
# ## Steps and impulses
# `Heaviside` is the unit step and `DiracDelta` the unit impulse.

# %%
laplace(Heaviside(t - 2), t)

# %%
y = symbols('y')
dsolve(diff(y(t), t) + 2*y(t) == DiracDelta(t - 1), y(t), ics=[0])

# %% [markdown]
# ## Differential equations
# Constants `C1`, `C2` stand for the initial values when `ics` is given.

# %%
dsolve(diff(y(t), t, 2) + y(t), y(t))

# %%
dsolve(diff(y(t), t, 2) + 4*y(t) == sin(t), y(t), ics=[0, 1])

# %% [markdown]
# ## Sums, products and special functions

# %%
summation(1 / k**2, (k, 1, oo))

# %%
summation(1 / k**3, (k, 1, oo))

# %%
summation(x**k / k**2, (k, 1, oo))

# %%
N(polylog(2, 1/2))

# %%
product(1 - x**2 / k**2, (k, 1, oo))

# %% [markdown]
# ## Linear algebra

# %%
A = matrix([[2, 1], [1, 3]])
rref(A)

# %%
svd(matrix([[3, 0], [0, 2]]))
