//! Closed-form sums `sum(f, k = a..b)`: finite sums with numeric bounds by direct addition, polynomials in `k` by
//! Newton interpolation of the power sums (Faulhaber), and geometric series `c * r**k`, finite or infinite.

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
    geometric(f, k, a, b)
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
