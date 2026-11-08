# %% [markdown]
# # Sliders
# Drag a slider and everything that uses its name re-runs. The value is written into the cell, so it is kept in the file.

# %%
a = slider(1, 5, 2)

# %%
b = slider(0, 6, 1.5, 0.25)

# %%
f = sin(a*x) * exp(-x/b)

# %%
plot(f, (x, 0, 10))

# %%
diff(f, x)
