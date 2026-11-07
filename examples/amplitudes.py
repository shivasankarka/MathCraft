# %% [markdown]
# # e⁺e⁻ → μ⁺μ⁻ at tree level
# Massless QED with one photon exchange. The spin-summed squared amplitude is a product of two Dirac traces,
# one for the electron line and one for the muon line, contracted over the photon's Lorentz indices.

# %%
# Electron line L^{mu nu} and muon line H_{mu nu}
L = gamma_trace(slash(p2), gamma(mu), slash(p1), gamma(nu))
H = gamma_trace(slash(p3), gamma(mu), slash(p4), gamma(nu))
LH = contract(L * H)
LH

# %% [markdown]
# Kinematics: all four particles are massless, so every dot product is a Mandelstam variable divided by two.

# %%
s_, t_, u_ = symbols('s t u')
dots = [
    (pdot(p1, p2), s_ / 2), (pdot(p3, p4), s_ / 2),
    (pdot(p1, p3), -t_ / 2), (pdot(p2, p4), -t_ / 2),
    (pdot(p1, p4), -u_ / 2), (pdot(p2, p3), -u_ / 2),
]
# average over the 4 initial spin states; the photon propagator gives 1/s^2 and each vertex a factor e
M2 = e_charge**4 / s_**2 * subs(LH, dots) / 4
factor(M2)

# %% [markdown]
# In the centre-of-mass frame, t = -s(1 - cos θ)/2 and u = -s(1 + cos θ)/2.

# %%
c = symbols('c')
M2c = simplify(subs(M2, [(t_, -s_ * (1 - c) / 2), (u_, -s_ * (1 + c) / 2)]))
M2c

# %% [markdown]
# dσ/dΩ = |M|² / (64 π² s), then integrate over the solid angle: 2π ∫ d(cos θ).

# %%
dsigma = M2c / (64 * pi**2 * s_)
sigma = 2 * pi * integrate(dsigma, (c, -1, 1))
simplify(sigma)

# %% [markdown]
# With α = e²/4π this is the textbook result σ = 4πα²/(3s):

# %%
alpha = symbols('alpha')
simplify(subs(sigma, e_charge, sqrt(4 * pi * alpha)))
