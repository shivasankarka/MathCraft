//! Closed-form sums `sum(f, k = a..b)`: finite sums with numeric bounds by direct addition, polynomials in `k` by
//! Newton interpolation of the power sums (Faulhaber), geometric series `c * r**k`, finite or infinite, and the classic
//! power series (`exp`, `sin`, `cos`, `sinh`, `cosh`, `log`, `atan`, `atanh`) with a polynomial or rational factor.
//! `product(f, k = a..b)` covers constants, powers, and products of linear factors through the Gamma function.

use crate::expand::expand;
use crate::poly::to_poly;
use crate::util::{guard, is_zero_expr, require_symbol, subs};
use mathcraft_core::numeric::eval_f64;
use mathcraft_core::{Expr, Kind, MathError, MathResult};

const MAX_DIRECT: i64 = 5000;

/// `sum(f for k in a..=b)`, or `None` when no closed form is known. `b` may be `oo` for convergent geometric series.
pub fn summation(f: &Expr, k: &Expr, a: &Expr, b: &Expr) -> MathResult<Option<Expr>> {
    require_symbol(k)?;
    guard(f)?;
    if let (Some(lo), Some(hi)) = (a.as_i64(), b.as_i64()) {
        if hi < lo {
            return Ok(Some(Expr::zero()));
        }
        if hi - lo <= MAX_DIRECT {
            let mut terms = Vec::new();
            for i in lo..=hi {
                terms.push(subs(f, &[(k.clone(), Expr::int(i))])?);
            }
            return Ok(Some(expand(&Expr::add(terms))?));
        }
    }
    if !f.contains(k) {
        // constant summand: (b - a + 1) * f
        return Ok(Some(Expr::mul(vec![Expr::add(vec![b.clone(), a.neg(), Expr::one()]), f.clone()])));
    }
    if let Some(coeffs) = to_poly(f, k)? {
        if *b == Expr::infinity() {
            return Ok(None);
        }
        let upper = Expr::add(vec![b.clone()]);
        let lower = Expr::add(vec![a.clone(), Expr::int(-1)]);
        let mut total = Vec::new();
        for (p, c) in coeffs.iter().enumerate() {
            if is_zero_expr(c)? {
                continue;
            }
            let s = power_sum(p)?;
            let n = Expr::sym("_n");
            let diff = Expr::add(vec![subs(&s, &[(n.clone(), upper.clone())])?, subs(&s, &[(n, lower.clone())])?.neg()]);
            total.push(Expr::mul(vec![c.clone(), diff]));
        }
        return Ok(Some(expand(&Expr::add(total))?));
    }
    if *b == Expr::infinity()
        && let Some(lo) = a.as_i64()
    {
        if let Some(v) = polynomial_times_geometric(f, k, lo)? {
            return Ok(Some(v));
        }
        if let Some(v) = rational_tail(f, k, lo)? {
            return Ok(Some(v));
        }
        if let Some(v) = power_series(f, k, lo)? {
            return Ok(Some(v));
        }
    } else if let Some(lo) = a.as_i64()
        && let Some(v) = rational_telescoping(f, k, lo, b)?
    {
        return Ok(Some(v));
    }
    geometric(f, k, a, b)
}

/// `sum(P(k) * r**k, k = a..oo)` with `|r| < 1` a number: the geometric series `q**a/(1 - q)` differentiated with `q d/dq`
/// once per power of `k`.
fn polynomial_times_geometric(f: &Expr, k: &Expr, a: i64) -> MathResult<Option<Expr>> {
    let factors: Vec<Expr> = match f.kind() {
        Kind::Mul(fs) => fs.clone(),
        _ => vec![f.clone()],
    };
    let mut found = None;
    for (i, t) in factors.iter().enumerate() {
        if let Kind::Pow(b0, e) = t.kind()
            && !b0.contains(k)
            && e.contains(k)
            && let Some((m, c0)) = crate::poly::linear_in(e, k)?
        {
            found = Some((i, Expr::pow(b0, &m), Expr::pow(b0, &c0)));
            break;
        }
    }
    let Some((at, base, shift)) = found else { return Ok(None) };
    let rest = Expr::mul(factors.iter().enumerate().filter(|(i, _)| *i != at).map(|(_, t)| t.clone()).collect());
    if !rest.contains(k) || !eval_f64(&base, &|_| None).is_ok_and(|v| v.abs() < 1.0) {
        return Ok(None);
    }
    let Some(coeffs) = to_poly(&rest, k)? else { return Ok(None) };
    if coeffs.len() > 12 {
        return Ok(None);
    }
    let q = Expr::sym("_q");
    let mut g = Expr::pow(&q, &Expr::int(a)).div(&Expr::add(vec![Expr::one(), q.neg()]));
    let mut total = Vec::new();
    for c in &coeffs {
        total.push(Expr::mul(vec![c.clone(), subs(&g, &[(q.clone(), base.clone())])?]));
        g = Expr::mul(vec![q.clone(), crate::calculus::diff(&g, &q)?]);
    }
    Ok(Some(expand(&Expr::mul(vec![shift, Expr::add(total)]))?))
}

/// `pi**(2n)` times a rational: the values of the zeta function at the even integers up to 12.
fn zeta_even(p: i64) -> Option<Expr> {
    let (num, den) = match p {
        2 => (1, 6),
        4 => (1, 90),
        6 => (1, 945),
        8 => (1, 9450),
        10 => (1, 93555),
        12 => (691, 638_512_875),
        _ => return None,
    };
    Some(Expr::mul(vec![Expr::rational(num, den)?, Expr::pow(&Expr::pi(), &Expr::int(p))]))
}

/// `1 + 1/2**p + ... + 1/n**p`.
fn harmonic(n: i64, p: i64) -> Expr {
    Expr::add((1..=n).map(|j| Expr::pow(&Expr::int(j), &Expr::int(-p))).collect())
}

/// `sum(f, k = a..oo)` for a rational `f` whose partial fractions are `c/(k + m)**p` with integer `m`: the `p = 1` terms
/// telescope (their coefficients must add up to zero) and the others give `zeta(p)` (even `p` only) minus a finite sum.
fn rational_tail(f: &Expr, k: &Expr, a: i64) -> MathResult<Option<Expr>> {
    let parts = crate::rational::apart(f, k)?;
    let terms: Vec<Expr> = match parts.kind() {
        Kind::Add(ts) => ts.clone(),
        _ => vec![parts.clone()],
    };
    let (mut linear_sum, mut total) = (Vec::new(), Vec::new());
    for t in &terms {
        let factors: Vec<Expr> = match t.kind() {
            Kind::Mul(fs) => fs.clone(),
            _ => vec![t.clone()],
        };
        let (with, without): (Vec<Expr>, Vec<Expr>) = factors.into_iter().partition(|g| g.contains(k));
        let [one] = with.as_slice() else { return Ok(None) };
        let (base, p) = match one.kind() {
            Kind::Pow(b, e) if e.as_i64().is_some_and(|e| e < 0) => (b.clone(), -e.as_i64().unwrap_or(-1)),
            _ => return Ok(None),
        };
        let Some((slope, icept)) = crate::poly::linear_in(&base, k)? else { return Ok(None) };
        let Some(m) = icept.div(&slope).as_i64() else { return Ok(None) };
        // 1/(s*k + t)**p = s**-p / (k + t/s)**p
        let c = Expr::mul(vec![Expr::mul(without), Expr::pow(&slope, &Expr::int(-p))]);
        if a + m < 1 {
            return Ok(None);
        }
        if p == 1 {
            linear_sum.push(c.clone());
            total.push(Expr::mul(vec![c, harmonic(a + m - 1, 1).neg()]));
        } else {
            let Some(z) = zeta_even(p) else { return Ok(None) };
            total.push(Expr::mul(vec![c, Expr::add(vec![z, harmonic(a + m - 1, p).neg()])]));
        }
    }
    if !is_zero_expr(&expand(&Expr::add(linear_sum))?)? {
        return Ok(None);
    }
    Ok(Some(expand(&Expr::add(total))?))
}

/// The polynomial `S_p(_n) = 0**p + 1**p + ... + _n**p`, by Newton's forward-difference form through `p + 2` points.
fn power_sum(p: usize) -> MathResult<Expr> {
    let n = Expr::sym("_n");
    let mut values: Vec<Expr> = Vec::new();
    let mut running = Expr::zero();
    for m in 0..=(p + 1) {
        let term = if p == 0 { Expr::one() } else { Expr::pow(&Expr::int(m as i64), &Expr::int(p as i64)) };
        running = expand(&Expr::add(vec![running, term]))?;
        values.push(running.clone());
    }
    // forward differences at 0, then P(n) = sum_j d_j * n(n-1)...(n-j+1)/j!
    let mut result = Vec::new();
    let mut basis = Expr::one();
    let mut row = values;
    for j in 0..=(p + 1) {
        let lead = row.first().cloned().unwrap_or_else(Expr::zero);
        result.push(Expr::mul(vec![lead, basis.clone()]));
        row = row
            .windows(2)
            .map(|w| expand(&Expr::add(vec![w.get(1).cloned().unwrap_or_else(Expr::zero), w.first().cloned().unwrap_or_else(Expr::zero).neg()])))
            .collect::<MathResult<_>>()?;
        basis = Expr::mul(vec![basis, Expr::add(vec![n.clone(), Expr::int(-(j as i64))]), Expr::rational(1, j as i64 + 1).unwrap_or_else(Expr::one)]);
    }
    expand(&Expr::add(result))
}

/// `c * r**k` with `r` free of `k`: `c * (r**(b+1) - r**a) / (r - 1)`, or `c * r**a / (1 - r)` for `b = oo` and `|r| < 1`.
fn geometric(f: &Expr, k: &Expr, a: &Expr, b: &Expr) -> MathResult<Option<Expr>> {
    let (c, r) = match f.kind() {
        Kind::Mul(fs) => {
            let (with, without): (Vec<Expr>, Vec<Expr>) = fs.iter().cloned().partition(|t| t.contains(k));
            let [one] = with.as_slice() else { return Ok(None) };
            (Expr::mul(without), one.clone())
        }
        _ => (Expr::one(), f.clone()),
    };
    // r must be base ** (m*k + c0) with a base free of k: c * base**c0 * (base**m)**k
    let Kind::Pow(b0, e) = r.kind() else { return Ok(None) };
    if b0.contains(k) {
        return Ok(None);
    }
    let Some((m, c0)) = crate::poly::linear_in(e, k)? else { return Ok(None) };
    let c = Expr::mul(vec![c, Expr::pow(b0, &c0)]);
    let base = Expr::pow(b0, &m);
    if base.is_one() {
        return Ok(None);
    }
    let one_minus = Expr::add(vec![Expr::one(), base.neg()]);
    if *b == Expr::infinity() {
        // a symbolic ratio is taken to have |r| < 1; a numeric one is checked
        if eval_f64(&base, &|_| None).is_ok_and(|v| v.abs() >= 1.0) {
            return Err(MathError::domain("the series does not converge (|r| must be below 1)"));
        }
        let first = Expr::pow(&base, a);
        return Ok(Some(Expr::mul(vec![c, first]).div(&one_minus)));
    }
    let top = Expr::pow(&base, &Expr::add(vec![b.clone(), Expr::one()]));
    let bottom = Expr::pow(&base, a);
    Ok(Some(Expr::mul(vec![c, Expr::add(vec![bottom, top.neg()])]).div(&one_minus)))
}

/// `1/(k + m)` summed from `k = a` to a symbolic `n` for a rational `f` whose partial fractions are all `c/(k + m)` with
/// integer `m` and `c` adding up to zero: the harmonic numbers cancel, leaving a finite sum of `1/(n + j)`.
fn rational_telescoping(f: &Expr, k: &Expr, a: i64, n: &Expr) -> MathResult<Option<Expr>> {
    if to_poly(f, k)?.is_some() {
        return Ok(None);
    }
    let Some(parts) = linear_pole_terms(f, k)? else { return Ok(None) };
    let Some(lowest) = parts.iter().map(|(_, m)| *m).min() else { return Ok(None) };
    if parts.iter().any(|(_, m)| a + m < 1) || !is_zero_expr(&Expr::add(parts.iter().map(|(c, _)| c.clone()).collect()))? {
        return Ok(None);
    }
    let mut total = Vec::new();
    for (c, m) in &parts {
        let mut inner: Vec<Expr> = (lowest + 1..=*m).map(|j| recip(&Expr::add(vec![n.clone(), Expr::int(j)]))).collect();
        inner.push(harmonic(a + m - 1, 1).neg());
        total.push(Expr::mul(vec![c.clone(), Expr::add(inner)]));
    }
    Ok(Some(expand(&Expr::add(total))?))
}

/// The partial fractions `c/(k + m)` of a rational function with integer `m`, as pairs `(c, m)`; `None` when anything else
/// appears (higher powers, a polynomial part, non-integer shifts).
fn linear_pole_terms(f: &Expr, k: &Expr) -> MathResult<Option<Vec<(Expr, i64)>>> {
    let parts = crate::rational::apart(f, k)?;
    let terms: Vec<Expr> = match parts.kind() {
        Kind::Add(ts) => ts.clone(),
        _ => vec![parts.clone()],
    };
    let mut out = Vec::new();
    for t in &terms {
        let factors: Vec<Expr> = match t.kind() {
            Kind::Mul(fs) => fs.clone(),
            _ => vec![t.clone()],
        };
        let (with, without): (Vec<Expr>, Vec<Expr>) = factors.into_iter().partition(|g| g.contains(k));
        let [one] = with.as_slice() else { return Ok(None) };
        let Kind::Pow(base, e) = one.kind() else { return Ok(None) };
        if e.as_i64() != Some(-1) {
            return Ok(None);
        }
        let Some((slope, icept)) = crate::poly::linear_in(base, k)? else { return Ok(None) };
        let Some(m) = icept.div(&slope).as_i64() else { return Ok(None) };
        out.push((Expr::mul(vec![Expr::mul(without), recip(&slope)]), m));
    }
    Ok(Some(out))
}

/// `n!` for small `n`, as a number.
fn factorial(n: i64) -> Expr {
    Expr::int((1..=n).product::<i64>())
}

/// A square root `w` of `z` for which the series in `w` are even, and whether `z = -w**2` (trigonometric rather than
/// hyperbolic): `x**2` gives `x`, `-x**2` gives `x`, and a number gives its principal root.
fn series_root(z: &Expr) -> (Expr, bool) {
    let even_root = |e: &Expr| match e.kind() {
        Kind::Pow(b, p) => p.as_i64().filter(|p| *p % 2 == 0).map(|p| Expr::pow(b, &Expr::int(p / 2))),
        _ => None,
    };
    if let Some(w) = even_root(z) {
        return (w, false);
    }
    let positive = z.neg();
    if let Some(w) = even_root(&positive) {
        return (w, true);
    }
    match crate::util::sign_of(z) {
        Some(-1) => (Expr::pow(&positive, &Expr::half()), true),
        _ => (Expr::pow(z, &Expr::half()), false),
    }
}

/// Is a numeric ratio inside `[-1, 1)` (or `(-1, 1)` when `open`)? Symbolic ratios are taken to converge.
fn converges(z: &Expr, open: bool) -> bool {
    if !z.free_symbols().is_empty() {
        return true;
    }
    eval_f64(z, &|_| None).is_ok_and(|v| v.abs() < 1.0 || (!open && v == -1.0))
}

/// `sum_{k=a}^oo` of `c * z**k * R(k)` for the classic series: `R` a polynomial over `(k + d)!` or `(2k + d)!` (`exp`,
/// `cosh`/`cos`, `sinh`/`cos`), or a rational function whose poles are at integers (`-log(1 - z)`) or half-integers
/// (`atanh`/`atan`), plus any polynomial part.
fn power_series(f: &Expr, k: &Expr, a: i64) -> MathResult<Option<Expr>> {
    let factors: Vec<Expr> = match f.kind() {
        Kind::Mul(fs) => fs.clone(),
        _ => vec![f.clone()],
    };
    let (mut z, mut c, mut rest) = (Expr::one(), Vec::new(), Vec::new());
    for t in &factors {
        if let Kind::Pow(b0, e) = t.kind()
            && !b0.contains(k)
            && e.contains(k)
            && let Some((m, c0)) = crate::poly::linear_in(e, k)?
        {
            z = Expr::mul(vec![z, Expr::pow(b0, &m)]);
            c.push(Expr::pow(b0, &c0));
        } else if t.contains(k) {
            rest.push(t.clone());
        } else {
            c.push(t.clone());
        }
    }
    let c = Expr::mul(c);
    if let Some(v) = factorial_series(&rest, k, a, &z)? {
        return Ok(Some(expand(&Expr::mul(vec![c, v]))?));
    }
    if z.is_one() {
        return Ok(None);
    }
    let r = Expr::mul(rest);
    let parts = crate::rational::apart(&r, k)?;
    let terms: Vec<Expr> = match parts.kind() {
        Kind::Add(ts) => ts.clone(),
        _ => vec![parts.clone()],
    };
    let (mut polynomial, mut total) = (Vec::new(), Vec::new());
    for t in &terms {
        let Some((coef, slope, icept)) = simple_pole(t, k)? else {
            polynomial.push(t.clone());
            continue;
        };
        let shift = icept.div(&slope);
        let coef = coef.div(&slope);
        if let Some(m) = shift.as_i64() {
            // sum z**k/(k + m) = z**-m * (-log(1 - z) - sum_{j=1}^{a+m-1} z**j/j)
            if a + m < 1 || !converges(&z, false) || z.is_one() {
                return Ok(None);
            }
            let head = (1..a + m).map(|j| Expr::pow(&z, &Expr::int(j)).div(&Expr::int(j))).collect::<Vec<_>>();
            let one_minus = Expr::add(vec![Expr::one(), z.neg()]);
            let log = match recip(&one_minus).as_i64() {
                Some(q) if q > 1 => Expr::call("log", vec![Expr::int(q)]),
                _ => Expr::call("log", vec![one_minus]).neg(),
            };
            let body = Expr::add(vec![log, Expr::add(head).neg()]);
            total.push(Expr::mul(vec![coef, Expr::pow(&z, &Expr::int(-m)), body]));
        } else if shift == Expr::half() && a >= 0 {
            // sum z**k/(k + 1/2) = 2 * atanh(w)/w for z = w**2, atan(w)/w for z = -w**2
            let (w, trig) = series_root(&z);
            if !converges(&z, !trig) {
                return Ok(None);
            }
            let func = Expr::call(if trig { "atan" } else { "atanh" }, vec![w.clone()]);
            let full = Expr::mul(vec![Expr::int(2), func, recip(&w)]);
            let head = (0..a).map(|j| Expr::pow(&z, &Expr::int(j)).div(&Expr::add(vec![Expr::int(j), Expr::half()]))).collect::<Vec<_>>();
            total.push(Expr::mul(vec![coef, Expr::add(vec![full, Expr::add(head).neg()])]));
        } else {
            return Ok(None);
        }
    }
    if !polynomial.is_empty() {
        let q = Expr::mul(vec![Expr::add(polynomial), Expr::pow(&z, k), c.clone()]);
        let Some(v) = polynomial_times_geometric(&q, k, a)? else { return Ok(None) };
        total.push(v);
    }
    Ok(Some(expand(&Expr::mul(vec![c, Expr::add(total)]))?))
}

/// A term `coef/(slope*k + icept)` of a partial-fraction expansion.
type Pole = (Expr, Expr, Expr);

fn simple_pole(t: &Expr, k: &Expr) -> MathResult<Option<Pole>> {
    let factors: Vec<Expr> = match t.kind() {
        Kind::Mul(fs) => fs.clone(),
        _ => vec![t.clone()],
    };
    let (with, without): (Vec<Expr>, Vec<Expr>) = factors.into_iter().partition(|g| g.contains(k));
    let [one] = with.as_slice() else { return Ok(None) };
    let Kind::Pow(base, e) = one.kind() else { return Ok(None) };
    if e.as_i64() != Some(-1) {
        return Ok(None);
    }
    let Some((slope, icept)) = crate::poly::linear_in(base, k)? else { return Ok(None) };
    Ok(Some((Expr::mul(without), slope, icept)))
}

/// `sum_{k=a}^oo P(k) * z**k / (s*k + d)!` for `s` 1 or 2, given the non-geometric factors `rest` of the summand.
fn factorial_series(rest: &[Expr], k: &Expr, a: i64, z: &Expr) -> MathResult<Option<Expr>> {
    let mut gamma_arg = None;
    let mut poly_factors = Vec::new();
    for t in rest {
        if let Kind::Pow(g, e) = t.kind()
            && e.as_i64() == Some(-1)
            && let Kind::Call(name, args) = g.kind()
            && &**name == "Gamma"
            && let [arg] = args.as_slice()
            && gamma_arg.is_none()
        {
            gamma_arg = Some(arg.clone());
        } else {
            poly_factors.push(t.clone());
        }
    }
    let Some(arg) = gamma_arg else { return Ok(None) };
    let Some((slope, icept)) = crate::poly::linear_in(&arg, k)? else { return Ok(None) };
    let (Some(s), Some(t)) = (slope.as_i64(), icept.as_i64()) else { return Ok(None) };
    let d = t - 1;
    if !(1..=2).contains(&s) || !(0..=3).contains(&d) || !(0..=20).contains(&a) || (s == 2 && d > 1) {
        return Ok(None);
    }
    let p = Expr::mul(poly_factors);
    let Some(coeffs) = (if p.contains(k) { to_poly(&p, k)? } else { Some(vec![p.clone()]) }) else { return Ok(None) };
    if coeffs.len() > 8 || (s == 2 && coeffs.len() > 1) {
        return Ok(None);
    }
    let term = |j: i64| -> MathResult<Expr> {
        let top = subs(&Expr::mul(vec![p.clone(), Expr::pow(z, k)]), &[(k.clone(), Expr::int(j))])?;
        Ok(top.div(&factorial(s * j + d)))
    };
    let head = Expr::add((0..a).map(term).collect::<MathResult<Vec<_>>>()?);
    if s == 1 {
        // sum_{k>=a} P(k) z**k/(k+d)! = z**-d * sum_{j>=a+d} P(j-d) z**j/j!, and sum_j j**n z**j/j! = (q d/dq)**n exp(q)
        let shifted = subs(&p, &[(k.clone(), Expr::add(vec![k.clone(), Expr::int(-d)]))])?;
        let Some(shifted) = to_poly(&shifted, k)? else { return Ok(None) };
        let q = Expr::sym("_q");
        let mut g = Expr::call("exp", vec![q.clone()]);
        let mut parts = Vec::new();
        for c in &shifted {
            parts.push(Expr::mul(vec![c.clone(), subs(&g, &[(q.clone(), z.clone())])?]));
            g = Expr::mul(vec![q.clone(), crate::calculus::diff(&g, &q)?]);
        }
        let poly_at = |j: i64| Expr::add(shifted.iter().enumerate().map(|(i, c)| Expr::mul(vec![c.clone(), Expr::int(j.pow(i as u32))])).collect());
        let head = Expr::add((0..a + d).map(|j| Expr::mul(vec![poly_at(j), Expr::pow(z, &Expr::int(j))]).div(&factorial(j))).collect());
        return Ok(Some(Expr::mul(vec![Expr::pow(z, &Expr::int(-d)), Expr::add(vec![Expr::add(parts), head.neg()])])));
    }
    let (w, trig) = series_root(z);
    let value = match (trig, d) {
        (false, 0) => Expr::call("cosh", vec![w]),
        (true, 0) => Expr::call("cos", vec![w]),
        (false, _) => Expr::call("sinh", vec![w.clone()]).div(&w),
        (true, _) => Expr::call("sin", vec![w.clone()]).div(&w),
    };
    let lead = coeffs.first().cloned().unwrap_or_else(Expr::one);
    Ok(Some(Expr::mul(vec![lead, Expr::add(vec![value, head.neg()])])))
}

fn recip(e: &Expr) -> Expr {
    Expr::pow(e, &Expr::int(-1))
}

/// `prod(f for k in a..=b)`, or `None` when no closed form is known. Numeric bounds multiply directly; otherwise `f` is
/// factored and each factor is a constant, a power with exponent linear in `k`, or a power of a linear factor
/// `(s*k + t)`, whose product is a ratio of Gamma functions.
pub fn product(f: &Expr, k: &Expr, a: &Expr, b: &Expr) -> MathResult<Option<Expr>> {
    require_symbol(k)?;
    guard(f)?;
    if let (Some(lo), Some(hi)) = (a.as_i64(), b.as_i64()) {
        if hi < lo {
            return Ok(Some(Expr::one()));
        }
        if hi - lo <= MAX_DIRECT {
            let mut terms = Vec::new();
            for i in lo..=hi {
                terms.push(subs(f, &[(k.clone(), Expr::int(i))])?);
            }
            return Ok(Some(Expr::mul(terms)));
        }
    }
    if *b == Expr::infinity() {
        return Ok(None);
    }
    let count = Expr::add(vec![b.clone(), a.neg(), Expr::one()]);
    let factored = crate::poly::factor(f)?;
    let factors: Vec<Expr> = match factored.kind() {
        Kind::Mul(fs) => fs.clone(),
        _ => vec![factored.clone()],
    };
    let mut out = Vec::new();
    for t in &factors {
        if !t.contains(k) {
            out.push(Expr::pow(t, &count));
            continue;
        }
        let (base, power) = match t.kind() {
            Kind::Pow(b0, e) => (b0.clone(), e.clone()),
            _ => (t.clone(), Expr::one()),
        };
        if !base.contains(k) {
            let Some(total) = summation(&power, k, a, b)? else { return Ok(None) };
            out.push(Expr::pow(&base, &total));
        } else if power.contains(k) {
            return Ok(None);
        } else if let Some((slope, icept)) = crate::poly::linear_in(&base, k)? {
            // prod (s*k + t) = s**count * Gamma(b + m + 1)/Gamma(a + m) with m = t/s
            let m = icept.div(&slope);
            let top = Expr::call("Gamma", vec![Expr::add(vec![b.clone(), m.clone(), Expr::one()])]);
            let bottom = Expr::call("Gamma", vec![Expr::add(vec![a.clone(), m])]);
            let each = Expr::mul(vec![Expr::pow(&slope, &count), top, recip(&bottom)]);
            out.push(Expr::pow(&each, &power));
        } else {
            return Ok(None);
        }
    }
    Ok(Some(crate::simplify::gamma_ratio(&Expr::mul(out))?))
}
