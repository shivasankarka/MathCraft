//! Notebooks bundled with the app, opened from File > Open example or the command palette.

/// Name shown in the menu, one line about it, and the notebook text.
pub const EXAMPLES: [(&str, &str, &str); 4] = [
    ("Tour", "calculus, algebra, matrices and plots in a few minutes", include_str!("../../../examples/tour.py")),
    ("Transforms and special functions", "Laplace, Fourier, ODEs, steps, zeta and polylog", include_str!("../../../examples/transforms.py")),
    ("Sliders", "drag a value and watch the plot follow", include_str!("../../../examples/sliders.py")),
    ("Amplitudes", "gamma matrices and traces", include_str!("../../../examples/amplitudes.py")),
];
