//! Rational functions: common denominators, partial fractions and their integrals.

use crate::expand::expand;
use crate::linalg::solve_linear;
use crate::poly::{factor, to_poly};
use crate::util::{guard, require_symbol, sign_of, split_fraction, subs};
use mathcraft_core::{Expr, Kind, MathError, MathResult};

/// Largest number of unknowns in a partial-fraction ansatz.
const MAX_UNKNOWNS: usize = 64;

/// `e` as one fraction `(numerator, denominator)`, both expanded. Sums get a common denominator (the product of
/// their distinct denominators, so it may not be the smallest one).
pub fn together(e: &Expr) -> MathResult<(Expr, Expr)> {
    guard(e)?;
    let Kind::Add(terms) = e.kind() else {
        let (n, d) = split_fraction(e);
        return Ok((expand(&n)?, expand(&d)?));
    };
    let parts: Vec<(Expr, Expr)> = terms.iter().map(split_fraction).collect();
    let mut dens: Vec<Expr> = Vec::new();
    for (_, d) in &parts {
        if !d.is_one() && !dens.contains(d) {
            dens.push(d.clone());
        }
    }
    let mut num = Vec::with_capacity(parts.len());
    for (n, d) in &parts {
        let mut factors = vec![n.clone()];
        factors.extend(dens.iter().filter(|o| *o != d).cloned());
        num.push(Expr::mul(factors));
    }
    Ok((expand(&Expr::add(num))?, expand(&Expr::mul(dens))?))
}

/// Polynomial division with remainder (coefficients lowest degree first).
fn divmod(n: &[Expr], d: &[Expr]) -> MathResult<(Vec<Expr>, Vec<Expr>)> {
    let Some(lead) = d.last() else { return Err(MathError::domain("division by the zero polynomial")) };
    let mut r = n.to_vec();
    let mut q = vec![Expr::zero(); (n.len() + 1).saturating_sub(d.len())];
    while r.len() >= d.len() && !r.is_empty() {
        mathcraft_core::budget::tick()?;
        let Some(top) = r.last() else { break };
        let c = expand(&top.div(lead))?;
        let shift = r.len() - d.len();
        if let Some(slot) = q.get_mut(shift) {
            *slot = c.clone();
        }
        for (i, dc) in d.iter().enumerate() {
            if let Some(slot) = r.get_mut(shift + i) {
                *slot = expand(&slot.sub(&Expr::mul(vec![c.clone(), dc.clone()])))?;
            }
        }
        r.pop();
    }
    while r.last().is_some_and(Expr::is_zero) {
        r.pop();
    }
    while q.last().is_some_and(Expr::is_zero) {
        q.pop();
    }
    Ok((q, r))
}

/// Monic greatest common divisor of two polynomials with numeric coefficients (Euclid); `None` for other coefficients.
fn poly_gcd(a: &[Expr], b: &[Expr]) -> MathResult<Option<Vec<Expr>>> {
    if !a.iter().chain(b).all(|c| c.is_num()) {
        return Ok(None);
    }
    let (mut a, mut b) = (a.to_vec(), b.to_vec());
    while !b.is_empty() {
        let (_, r) = divmod(&a, &b)?;
        a = b;
        b = r;
    }
    let Some(lead) = a.last().cloned() else { return Ok(None) };
    a.iter().map(|c| expand(&c.div(&lead))).collect::<MathResult<Vec<_>>>().map(Some)
}

/// Cancel common polynomial factors between numerator and denominator (one symbol at a time).
/// Returns the combined, expanded fraction when nothing cancels.
pub fn cancel(e: &Expr) -> MathResult<Expr> {
    let (n, d) = together(e)?;
    if d.is_one() {
        return Ok(n);
    }
    // integer polynomials in any number of symbols: the sparse gcd
    if let Some((qn, qd)) = crate::sparse::cancel_pair(&n, &d)? {
        return Ok(qn.div(&qd));
    }
    for name in e.free_symbols() {
        let x = Expr::sym(&name);
        let (Some(nc), Some(dc)) = (to_poly(&n, &x)?, to_poly(&d, &x)?) else { continue };
        let Some(g) = poly_gcd(&nc, &dc)? else { continue };
        if g.len() > 1 {
            let (qn, _) = divmod(&nc, &g)?;
            let (qd, _) = divmod(&dc, &g)?;
            return Ok(poly_expr(&qn, &x).div(&poly_expr(&qd, &x)));
        }
    }
    Ok(n.div(&d))
}

fn poly_expr(c: &[Expr], x: &Expr) -> Expr {
    crate::poly::from_poly(c, x)
}

/// Partial fractions of a rational function of `x`: a polynomial part plus one term per power of every factor of the
/// denominator. Returns `e` unchanged when it is not a rational function or the denominator does not factor.
pub fn apart(e: &Expr, x: &Expr) -> MathResult<Expr> {
    require_symbol(x)?;
    let (n, d) = together(e)?;
    if !d.contains(x) {
        return Ok(e.clone());
    }
    let (Some(np), Some(dp)) = (to_poly(&n, x)?, to_poly(&d, x)?) else { return Ok(e.clone()) };
    let (quot, rem) = divmod(&np, &dp)?;
    let poly_part = poly_expr(&quot, x);
    if rem.is_empty() {
        return Ok(poly_part);
    }
    let factored = factor(&d)?;
    let parts = match factored.kind() {
        Kind::Mul(v) => v.clone(),
        _ => vec![factored.clone()],
    };
    let (mut lead, mut facs): (Vec<Expr>, Vec<(Expr, i64)>) = (Vec::new(), Vec::new());
    for t in parts {
        match t.kind() {
            _ if !t.contains(x) => lead.push(t.clone()),
            Kind::Pow(b, k) if b.contains(x) && k.as_i64().is_some_and(|k| k > 0) => facs.push((b.clone(), k.as_i64().unwrap_or(1))),
            _ => facs.push((t.clone(), 1)),
        }
    }
    let lead = Expr::mul(lead);
    let rem_expr = poly_expr(&rem, x);
    if let [(b, 1)] = facs.as_slice() {
        return Ok(Expr::add(vec![poly_part, rem_expr.div(&Expr::mul(vec![lead, b.clone()]))]));
    }

    // Ansatz: rem = lead * sum over factors B^m and j <= m of N_j(x) * (Q / B^j), with N_j of degree < deg B.
    let mut unknowns: Vec<Expr> = Vec::new();
    let mut slots: Vec<(Expr, i64, Expr)> = Vec::new(); // (B, j, N_j)
    for (b, m) in &facs {
        let deg = to_poly(b, x)?.map_or(0, |c| c.len().saturating_sub(1));
        if deg == 0 {
            return Ok(e.clone());
        }
        for j in 1..=*m {
            let first = unknowns.len();
            let coeffs: Vec<Expr> = (0..deg).map(|i| Expr::sym(&format!("_pf{}", first + i + 1))).collect();
            unknowns.extend(coeffs.iter().cloned());
            slots.push((b.clone(), j, poly_expr(&coeffs, x)));
        }
    }
    if unknowns.len() > MAX_UNKNOWNS {
        return Err(MathError::too_large("the denominator has too many factors for partial fractions"));
    }
    let mut total = Vec::with_capacity(slots.len());
    for (b, j, nj) in &slots {
        let mut others: Vec<Expr> = Vec::new();
        for (ob, om) in &facs {
            let power = if ob == b { om - j } else { *om };
            others.push(Expr::pow(ob, &Expr::int(power)));
        }
        total.push(Expr::mul(vec![nj.clone(), Expr::mul(others)]));
    }
    let equation = expand(&Expr::add(vec![Expr::mul(vec![lead.clone(), Expr::add(total)]), rem_expr.neg()]))?;
    let Some(coeffs) = to_poly(&equation, x)? else { return Ok(e.clone()) };
    let Some(solution) = solve_linear(&coeffs, &unknowns)? else { return Ok(e.clone()) };
    let rules: Vec<(Expr, Expr)> = solution
        .iter()
        .filter_map(|eq| match eq.kind() {
            Kind::Eq(l, r) => Some((l.clone(), r.clone())),
            _ => None,
        })
        .collect();
    if rules.len() != unknowns.len() {
        return Err(MathError::unsupported("the partial-fraction system is underdetermined"));
    }
    let mut terms = vec![poly_part];
    for (b, j, nj) in slots {
        let value = expand(&subs(&nj, &rules)?)?;
        if !value.is_zero() {
            terms.push(value.div(&Expr::pow(&b, &Expr::int(j))));
        }
    }
    Ok(Expr::add(terms))
}

/// Integrate a rational function through its partial fractions. `rec` integrates the pieces that are not quadratic
/// over an irreducible quadratic (linear denominators and polynomials).
pub fn integrate_rational(e: &Expr, x: &Expr, rec: &mut dyn FnMut(&Expr) -> MathResult<Option<Expr>>) -> MathResult<Option<Expr>> {
    let (_, d) = together(e)?;
    if !d.contains(x) || to_poly(&d, x)?.is_none() {
        return Ok(None);
    }
    let a = apart(e, x)?;
    let split = &a != e;
    let terms = match a.kind() {
        Kind::Add(v) => v.clone(),
        _ => vec![a.clone()],
    };
    let mut parts = Vec::with_capacity(terms.len());
    for t in &terms {
        let piece = match quadratic_term(t, x)? {
            Some(r) => Some(r),
            None if split => rec(t)?,
            None => None,
        };
        match piece {
            Some(p) => parts.push(p),
            None => return Ok(None),
        }
    }
    Ok(Some(Expr::add(parts)))
}

/// `(B x + C) / (a x^2 + b x + c)` with an irreducible or irrational-root quadratic: log and arctangent (or log) forms.
fn quadratic_term(t: &Expr, x: &Expr) -> MathResult<Option<Expr>> {
    let (num, den) = split_fraction(t);
    let (Some(np), Some(dp)) = (to_poly(&num, x)?, to_poly(&den, x)?) else { return Ok(None) };
    let [c, b, a] = dp.as_slice() else { return Ok(None) };
    if np.len() > 2 || !matches!(den.kind(), Kind::Add(_)) || a.is_zero() {
        return Ok(None);
    }
    let (c0, c1) = (np.first().cloned().unwrap_or_else(Expr::zero), np.get(1).cloned().unwrap_or_else(Expr::zero));
    // Normalise to x^2 + p x + q over a, with numerator (B x + C) / a.
    let (p, q) = (b.div(a), c.div(a));
    let (bb, cc) = (c1.div(a), c0.div(a));
    let half_p = p.div(&Expr::int(2));
    let disc = expand(&q.sub(&Expr::mul(vec![half_p.clone(), half_p.clone()])))?;
    let Some(sd) = sign_of(&disc) else { return Ok(None) };
    let shifted = Expr::add(vec![x.clone(), half_p.clone()]);
    let log_part = Expr::mul(vec![bb.div(&Expr::int(2)), Expr::call("log", vec![den.clone()])]);
    let k = expand(&cc.sub(&Expr::mul(vec![bb, half_p])))?;
    let rest = if sd > 0 {
        let s = Expr::pow(&disc, &Expr::half());
        Expr::mul(vec![k, s.clone().div(&disc), Expr::call("atan", vec![shifted.div(&s)])])
    } else if sd < 0 {
        let s = Expr::pow(&disc.neg(), &Expr::half());
        let ratio = Expr::add(vec![shifted.clone(), s.neg()]).div(&Expr::add(vec![shifted, s.clone()]));
        Expr::mul(vec![k, Expr::call("log", vec![ratio]), Expr::pow(&Expr::mul(vec![Expr::int(2), s]), &Expr::int(-1))])
    } else {
        return Ok(None);
    };
    Ok(Some(Expr::add(vec![log_part, rest])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calculus::{diff, integrate};

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn i(v: i64) -> Expr {
        Expr::int(v)
    }

    #[test]
    fn partial_fractions_add_back_up() {
        // 1/(x^2-1), (x^2+1)/(x*(x-1)^2), x^3/(x^2-1) (needs the polynomial part)
        let cases = [
            i(1).div(&Expr::add(vec![Expr::pow(&x(), &i(2)), i(-1)])),
            Expr::add(vec![Expr::pow(&x(), &i(2)), i(1)]).div(&Expr::mul(vec![x(), Expr::pow(&Expr::add(vec![x(), i(-1)]), &i(2))])),
            Expr::pow(&x(), &i(3)).div(&Expr::add(vec![Expr::pow(&x(), &i(2)), i(-1)])),
        ];
        for e in cases {
            let a = apart(&e, &x()).unwrap();
            assert_ne!(a, e, "{e} was not split");
            let (n, d) = together(&a.sub(&e)).unwrap();
            assert!(n.is_zero() || crate::util::is_zero_expr(&n).unwrap(), "apart({e}) = {a} is not equal to it; difference numerator {n} over {d}");
        }
    }

    #[test]
    fn integrates_rational_functions() {
        let quad = Expr::add(vec![Expr::pow(&x(), &i(2)), i(1)]);
        let cases = [
            i(1).div(&Expr::add(vec![Expr::pow(&x(), &i(2)), i(-1)])),
            i(1).div(&Expr::mul(vec![x(), Expr::add(vec![x(), i(1)])])),
            Expr::add(vec![x(), i(3)]).div(&quad),
            i(1).div(&Expr::mul(vec![quad.clone(), Expr::add(vec![x(), i(-1)])])),
            i(1).div(&Expr::add(vec![Expr::pow(&x(), &i(2)), i(-2)])),
        ];
        for e in cases {
            let anti = integrate(&e, &x()).unwrap().unwrap_or_else(|| panic!("no antiderivative for {e}"));
            // differentiate back and compare numerically at a few points
            let back = diff(&anti, &x()).unwrap();
            for t in [0.5, 2.0, 3.7] {
                let env = |n: &str| (n == "x").then_some(t);
                let (a, b) = (mathcraft_core::numeric::eval_f64(&back, &env).unwrap(), mathcraft_core::numeric::eval_f64(&e, &env).unwrap());
                assert!((a - b).abs() < 1e-9, "d/dx of {anti} is {a} at x={t}, expected {b} (integrand {e})");
            }
        }
    }
}
