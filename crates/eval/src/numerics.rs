//! Numerical routines on plain `f64` functions: adaptive quadrature (Gauss–Kronrod 7/15), a bracketing root finder
//! (Brent) and a bounded minimiser (golden section with parabolic steps, Brent). Each takes the function as a closure
//! and counts its own work, so the caller can stop a runaway call through the closure's error.

/// Kronrod nodes and weights (15 points) and the embedded Gauss weights (7 points), on `[-1, 1]`.
const XGK: [f64; 8] = [
    0.991_455_371_120_812_6,
    0.949_107_912_342_758_5,
    0.864_864_423_359_769_1,
    0.741_531_185_599_394_4,
    0.586_087_235_467_691_1,
    0.405_845_151_377_397_2,
    0.207_784_955_007_898_47,
    0.0,
];
const WGK: [f64; 8] = [
    0.022_935_322_010_529_225,
    0.063_092_092_629_978_55,
    0.104_790_010_322_250_18,
    0.140_653_259_715_525_92,
    0.169_004_726_639_267_9,
    0.190_350_578_064_785_4,
    0.204_432_940_075_298_9,
    0.209_482_141_084_727_82,
];
/// The 7-point Gauss weights at the Kronrod nodes they share (zero elsewhere); the last entry is the centre node.
const WG: [f64; 8] = [0.0, 0.129_484_966_168_869_7, 0.0, 0.279_705_391_489_276_7, 0.0, 0.381_830_050_505_118_9, 0.0, 0.417_959_183_673_469_4];

/// One Gauss–Kronrod panel: the integral over `[a, b]` and an error estimate.
fn panel(f: &mut dyn FnMut(f64) -> f64, a: f64, b: f64) -> (f64, f64) {
    let (centre, half) = ((a + b) / 2.0, (b - a) / 2.0);
    let fc = f(centre);
    let mut kronrod = fc * WGK[7];
    let mut gauss = fc * WG[7];
    for ((x, w), wg) in XGK.iter().zip(WGK.iter()).zip(WG.iter()).take(7) {
        let (f1, f2) = (f(centre - half * x), f(centre + half * x));
        kronrod += w * (f1 + f2);
        gauss += wg * (f1 + f2);
    }
    (kronrod * half, ((kronrod - gauss) * half).abs())
}

/// `∫ f` over `[a, b]` (either end may be infinite) to a relative tolerance of about `1e-10`. `None` when the integrand
/// is not finite or the subdivision limit is reached before the estimate settles.
pub fn integrate(f: &mut dyn FnMut(f64) -> f64, a: f64, b: f64) -> Option<f64> {
    if a == b {
        return Some(0.0);
    }
    if a > b {
        return integrate(f, b, a).map(|v| -v);
    }
    // Map an infinite range onto a finite one.
    let value = match (a.is_finite(), b.is_finite()) {
        (true, true) => adaptive(f, a, b),
        (true, false) => adaptive(&mut |t| infinite_tail(f, a, t), 0.0, 1.0),
        (false, true) => adaptive(&mut |t| infinite_tail(&mut |x| f(2.0 * b - x), b, t), 0.0, 1.0),
        (false, false) => adaptive(&mut |t| f(t / (1.0 - t * t)) * (1.0 + t * t) / ((1.0 - t * t) * (1.0 - t * t)), -1.0, 1.0),
    }?;
    value.is_finite().then_some(value)
}

/// `x = a + t/(1-t)` for `t` in `[0, 1)`, with its Jacobian.
fn infinite_tail(f: &mut dyn FnMut(f64) -> f64, a: f64, t: f64) -> f64 {
    let u = 1.0 - t;
    f(a + t / u) / (u * u)
}

fn adaptive(f: &mut dyn FnMut(f64) -> f64, a: f64, b: f64) -> Option<f64> {
    let mut parts = vec![(a, b, panel(f, a, b))];
    for _ in 0..2000 {
        let total: f64 = parts.iter().map(|p| p.2.0).sum();
        let error: f64 = parts.iter().map(|p| p.2.1).sum();
        if !total.is_finite() {
            return None;
        }
        if error <= 1e-10 * total.abs().max(1e-300) || error < 1e-14 {
            return Some(total);
        }
        // split the panel with the largest error
        let worst = parts.iter().enumerate().max_by(|x, y| x.1.2.1.total_cmp(&y.1.2.1)).map(|(i, _)| i)?;
        let (lo, hi, _) = parts.swap_remove(worst);
        let mid = (lo + hi) / 2.0;
        if mid <= lo || mid >= hi {
            return Some(total);
        }
        parts.push((lo, mid, panel(f, lo, mid)));
        parts.push((mid, hi, panel(f, mid, hi)));
    }
    None
}

/// A root of `f` in `[a, b]`, which needs `f(a)` and `f(b)` of opposite signs (Brent's method).
pub fn find_root(f: &mut dyn FnMut(f64) -> f64, a: f64, b: f64) -> Option<f64> {
    let (mut a, mut b) = (a, b);
    let (mut fa, mut fb) = (f(a), f(b));
    if !fa.is_finite() || !fb.is_finite() {
        return None;
    }
    if fa == 0.0 {
        return Some(a);
    }
    if fb == 0.0 {
        return Some(b);
    }
    if fa.signum() == fb.signum() {
        return None;
    }
    let (mut c, mut fc, mut d, mut e) = (a, fa, b - a, b - a);
    for _ in 0..200 {
        if fb.signum() == fc.signum() {
            (c, fc, d, e) = (a, fa, b - a, b - a);
        }
        if fc.abs() < fb.abs() {
            (a, b, c) = (b, c, b);
            (fa, fb, fc) = (fb, fc, fb);
        }
        let tol = 2.0 * f64::EPSILON * b.abs() + 1e-15;
        let m = (c - b) / 2.0;
        if m.abs() <= tol || fb == 0.0 {
            return Some(b);
        }
        if e.abs() < tol || fa.abs() <= fb.abs() {
            (d, e) = (m, m);
        } else {
            let s = fb / fa;
            let (mut p, mut q) = if a == c {
                (2.0 * m * s, 1.0 - s)
            } else {
                let (q, r) = (fa / fc, fb / fc);
                (s * (2.0 * m * q * (q - r) - (b - a) * (r - 1.0)), (q - 1.0) * (r - 1.0) * (s - 1.0))
            };
            if p > 0.0 {
                q = -q;
            } else {
                p = -p;
            }
            if 2.0 * p < (3.0 * m * q - (tol * q).abs()).min((e * q).abs()) {
                (e, d) = (d, p / q);
            } else {
                (d, e) = (m, m);
            }
        }
        (a, fa) = (b, fb);
        b += if d.abs() > tol { d } else { tol.copysign(m) };
        fb = f(b);
        if !fb.is_finite() {
            return None;
        }
    }
    None
}

/// The point of least `f` in `[a, b]` and its value, found by golden-section search on a coarse grid's best cell, so a
/// function with several dips finds the deepest one on the grid. Local at the resolution of the grid.
pub fn minimize(f: &mut dyn FnMut(f64) -> f64, a: f64, b: f64) -> Option<(f64, f64)> {
    const CELLS: usize = 64;
    let step = (b - a) / CELLS as f64;
    let mut best: Option<(usize, f64)> = None;
    for i in 0..=CELLS {
        let v = f(a + step * i as f64);
        if v.is_finite() && best.is_none_or(|(_, bv)| v < bv) {
            best = Some((i, v));
        }
    }
    let (i, _) = best?;
    let (mut lo, mut hi) = (a + step * i.saturating_sub(1) as f64, (a + step * (i + 1) as f64).min(b));
    let phi = (5.0_f64.sqrt() - 1.0) / 2.0;
    let (mut x1, mut x2) = (hi - phi * (hi - lo), lo + phi * (hi - lo));
    let (mut f1, mut f2) = (f(x1), f(x2));
    for _ in 0..200 {
        if (hi - lo).abs() <= 1e-12 * (1.0 + lo.abs() + hi.abs()) {
            break;
        }
        if f1 < f2 {
            (hi, x2, f2) = (x2, x1, f1);
            x1 = hi - phi * (hi - lo);
            f1 = f(x1);
        } else {
            (lo, x1, f1) = (x1, x2, f2);
            x2 = lo + phi * (hi - lo);
            f2 = f(x2);
        }
    }
    let x = (lo + hi) / 2.0;
    let fx = f(x);
    // the end points of the interval are candidates too (a minimum on the boundary)
    let edge = [a, b].into_iter().map(|e| (e, f(e))).filter(|(_, v)| v.is_finite()).min_by(|p, q| p.1.total_cmp(&q.1));
    match edge {
        Some((e, v)) if fx.is_nan() || fx.partial_cmp(&v) == Some(std::cmp::Ordering::Greater) => Some((e, v)),
        _ => fx.is_finite().then_some((x, fx)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrates_smooth_and_infinite_ranges() {
        let v = integrate(&mut |x| x.sin(), 0.0, std::f64::consts::PI).unwrap();
        assert!((v - 2.0).abs() < 1e-10, "{v}");
        let v = integrate(&mut |x| (-x * x).exp(), f64::NEG_INFINITY, f64::INFINITY).unwrap();
        assert!((v - std::f64::consts::PI.sqrt()).abs() < 1e-8, "{v}");
        let v = integrate(&mut |x| (-x).exp(), 0.0, f64::INFINITY).unwrap();
        assert!((v - 1.0).abs() < 1e-9, "{v}");
        // an integrable endpoint singularity
        let v = integrate(&mut |x| 1.0 / x.sqrt(), 0.0, 1.0).unwrap();
        assert!((v - 2.0).abs() < 1e-6, "{v}");
        assert!((integrate(&mut |x| x, 1.0, 0.0).unwrap() + 0.5).abs() < 1e-12);
    }

    #[test]
    fn finds_roots_and_minima() {
        let r = find_root(&mut |x| x * x - 2.0, 0.0, 2.0).unwrap();
        assert!((r - 2.0_f64.sqrt()).abs() < 1e-12, "{r}");
        assert!(find_root(&mut |x| x * x + 1.0, -1.0, 1.0).is_none());
        let (x, v) = minimize(&mut |x| (x - 1.5) * (x - 1.5) + 2.0, -5.0, 5.0).unwrap();
        assert!((x - 1.5).abs() < 1e-6 && (v - 2.0).abs() < 1e-10, "{x} {v}");
        // the deeper of two dips, and a minimum on the boundary
        let (x, _) = minimize(&mut |x| (x * x - 1.0).powi(2) + 0.3 * x, -2.0, 2.0).unwrap();
        assert!(x < 0.0, "{x}");
        let (x, _) = minimize(&mut |x| x, 0.0, 3.0).unwrap();
        assert!(x.abs() < 1e-6, "{x}");
    }
}
