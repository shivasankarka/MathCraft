//! Polynomials in one symbol: coefficient extraction, rational roots, factoring and solving.

use crate::expand::expand;
use crate::util::{guard, is_zero_expr, require_symbol};
use dashu::integer::IBig;
use mathcraft_core::number::small_factors;
use mathcraft_core::{Expr, Kind, MathError, MathResult, Num};

const MAX_DEGREE: usize = 4096;

/// Coefficients (lowest degree first) of `e` as a polynomial in `x`, or `None` when `e` is not polynomial in `x`.
/// The zero polynomial is the empty vector.
pub fn to_poly(e: &Expr, x: &Expr) -> MathResult<Option<Vec<Expr>>> {
    require_symbol(x)?;
    let ex = expand(e)?;
    let terms: Vec<Expr> = match ex.kind() {
        Kind::Add(v) => v.clone(),
        _ => vec![ex.clone()],
    };
    let mut coeffs: Vec<Vec<Expr>> = Vec::new();
    for t in &terms {
        guard(t)?;
        let Some((deg, c)) = term_degree(t, x) else { return Ok(None) };
        if deg > MAX_DEGREE {
            return Ok(None);
        }
        if coeffs.len() <= deg {
            coeffs.resize(deg + 1, Vec::new());
        }
        if let Some(slot) = coeffs.get_mut(deg) {
            slot.push(c);
        }
    }
    let mut out: Vec<Expr> = coeffs.into_iter().map(Expr::add).collect();
    while out.last().is_some_and(Expr::is_zero) {
        out.pop();
    }
    Ok(Some(out))
}

fn term_degree(t: &Expr, x: &Expr) -> Option<(usize, Expr)> {
    if !t.contains(x) {
        return Some((0, t.clone()));
    }
    match t.kind() {
        Kind::Sym(_) if t == x => Some((1, Expr::one())),
        Kind::Pow(b, k) if b == x => Some((usize::try_from(k.as_i64()?).ok()?, Expr::one())),
        Kind::Mul(fs) => {
            let mut deg = None;
            let mut rest = Vec::new();
            for f in fs {
                if f.contains(x) {
                    if deg.is_some() {
                        return None;
                    }
                    deg = Some(term_degree(f, x)?.0);
                } else {
                    rest.push(f.clone());
                }
            }
            Some((deg?, Expr::mul(rest)))
        }
        _ => None,
    }
}

pub fn from_poly(coeffs: &[Expr], x: &Expr) -> Expr {
    Expr::add(coeffs.iter().enumerate().map(|(i, c)| Expr::mul(vec![c.clone(), Expr::pow(x, &Expr::int(i as i64))])).collect())
}

// ---- integer polynomial helpers ----------------------------------------------------------------------------------

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.unsigned_abs(), b.unsigned_abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    i128::try_from(a).unwrap_or(1)
}

fn to_i128(v: &IBig) -> Option<i128> {
    i128::try_from(v).ok()
}

/// Scale rational coefficients to a primitive integer polynomial: `coeffs = scale * result`.
fn int_poly(coeffs: &[Num]) -> Option<(Vec<i128>, Num)> {
    let mut parts = Vec::with_capacity(coeffs.len());
    let mut l: i128 = 1;
    for c in coeffs {
        let (n, d) = c.parts()?;
        let (n, d) = (to_i128(&n)?, to_i128(&IBig::from(d))?);
        l = l.checked_mul(d / gcd(l, d))?;
        parts.push((n, d));
    }
    let mut ints = Vec::with_capacity(parts.len());
    for (n, d) in parts {
        ints.push(n.checked_mul(l / d)?);
    }
    let g = ints.iter().fold(0i128, |g, v| gcd(g, *v)).max(1);
    for v in &mut ints {
        *v /= g;
    }
    Some((ints, Num::ratio(IBig::from(g), IBig::from(l))?))
}

fn divisors(n: i128) -> Option<Vec<i128>> {
    let n = u64::try_from(n.unsigned_abs()).ok()?;
    if n == 0 {
        return None;
    }
    let mut ds: Vec<u64> = vec![1];
    for (p, k) in small_factors(n) {
        let base = ds.clone();
        let mut pk = 1u64;
        for _ in 0..k {
            pk = pk.checked_mul(p)?;
            ds.extend(base.iter().filter_map(|d| d.checked_mul(pk)));
        }
        if ds.len() > 20_000 {
            return None;
        }
    }
    ds.into_iter().map(i128::from).map(Some).collect()
}

/// `q^n * P(p/q)` is zero?
fn is_root(poly: &[i128], p: i128, q: i128) -> bool {
    let n = poly.len().saturating_sub(1);
    let mut qpow = vec![1i128; n + 1];
    for i in 1..=n {
        match qpow.get(i - 1).and_then(|v| v.checked_mul(q)) {
            Some(v) => {
                if let Some(slot) = qpow.get_mut(i) {
                    *slot = v;
                }
            }
            None => return false,
        }
    }
    let mut acc = match poly.last() {
        Some(a) => *a,
        None => return false,
    };
    for i in (0..n).rev() {
        let (Some(a), Some(qp)) = (poly.get(i), qpow.get(n - i)) else { return false };
        let Some(next) = acc.checked_mul(p).and_then(|v| a.checked_mul(*qp).and_then(|w| v.checked_add(w))) else { return false };
        acc = next;
    }
    acc == 0
}

/// Divide by `(q x - p)`; `None` when not exact.
fn deflate(poly: &[i128], p: i128, q: i128) -> Option<Vec<i128>> {
    let n = poly.len().checked_sub(1)?;
    if n == 0 {
        return None;
    }
    let mut s = vec![0i128; n];
    let top = *poly.last()?;
    if top % q != 0 {
        return None;
    }
    *s.get_mut(n - 1)? = top / q;
    for i in (1..n).rev() {
        let si = *s.get(i)?;
        let v = poly.get(i)?.checked_add(p.checked_mul(si)?)?;
        if v % q != 0 {
            return None;
        }
        *s.get_mut(i - 1)? = v / q;
    }
    (*poly.first()? == p.checked_mul(*s.first()?)?.checked_neg()?).then_some(s)
}

/// Rational roots `(p, q, multiplicity)` of a primitive integer polynomial, and what remains after removing them.
fn rational_roots(poly: &[i128]) -> (Vec<(i128, i128, usize)>, Vec<i128>) {
    let mut p = poly.to_vec();
    let mut roots = Vec::new();
    let mut zeros = 0;
    while p.len() > 1 && p.first() == Some(&0) {
        p.remove(0);
        zeros += 1;
    }
    if zeros > 0 {
        roots.push((0, 1, zeros));
    }
    'search: while p.len() > 1 {
        let (Some(a0), Some(an)) = (p.first().copied(), p.last().copied()) else { break };
        let (Some(dp), Some(dq)) = (divisors(a0), divisors(an)) else { break };
        for &num in &dp {
            for &den in &dq {
                if gcd(num, den) != 1 {
                    continue;
                }
                for sign in [1i128, -1] {
                    let (cp, cq) = (num * sign, den);
                    if !is_root(&p, cp, cq) {
                        continue;
                    }
                    let mut mult = 0;
                    while let Some(next) = deflate(&p, cp, cq) {
                        p = next;
                        mult += 1;
                        if p.len() <= 1 || !is_root(&p, cp, cq) {
                            break;
                        }
                    }
                    if mult > 0 {
                        roots.push((cp, cq, mult));
                        continue 'search;
                    }
                }
            }
        }
        break;
    }
    (roots, p)
}

fn num_coeffs(coeffs: &[Expr]) -> Option<Vec<Num>> {
    coeffs.iter().map(|c| c.as_num().filter(|n| n.is_exact()).cloned()).collect()
}

fn int_expr(v: i128) -> Expr {
    Expr::big(IBig::from(v))
}

fn ints_to_exprs(v: &[i128]) -> Vec<Expr> {
    v.iter().map(|c| int_expr(*c)).collect()
}

// ---- factoring ---------------------------------------------------------------------------------------------------

/// Factor a polynomial in one symbol over the rationals as far as rational roots allow.
/// Expressions it cannot handle are returned unchanged.
pub fn factor(e: &Expr) -> MathResult<Expr> {
    // a fraction: cancel it, then factor the numerator and the denominator
    if matches!(e.kind(), Kind::Mul(_) | Kind::Add(_) | Kind::Pow(..)) {
        let (_, d) = crate::rational::together(e)?;
        if !d.is_one() {
            let (n, d) = crate::rational::together(&crate::rational::cancel(e)?)?;
            return Ok(factor(&n)?.div(&factor(&d)?));
        }
    }
    if let Some(f) = crate::sparse::factor(e)? {
        return Ok(f);
    }
    let syms = e.free_symbols();
    let [name] = syms.iter().collect::<Vec<_>>()[..] else { return Ok(e.clone()) };
    let x = Expr::sym(name);
    let Some(coeffs) = to_poly(e, &x)? else { return Ok(e.clone()) };
    let Some(nums) = num_coeffs(&coeffs) else { return Ok(e.clone()) };
    if coeffs.len() < 2 {
        return Ok(e.clone());
    }
    let Some((ints, scale)) = int_poly(&nums) else { return Ok(e.clone()) };
    let (roots, rest) = rational_roots(&ints);
    let mut lead = scale;
    let mut factors: Vec<Expr> = Vec::new();
    for (p, q, mult) in roots {
        // (q x - p) ** mult
        let lin = Expr::add(vec![Expr::mul(vec![int_expr(q), x.clone()]), int_expr(-p)]);
        factors.push(Expr::pow(&lin, &Expr::int(mult as i64)));
    }
    if rest.len() > 1 {
        let g = rest.iter().fold(0i128, |g, v| gcd(g, *v)).max(1);
        let sign = if rest.last().is_some_and(|v| *v < 0) { -1 } else { 1 };
        let prim: Vec<i128> = rest.iter().map(|v| v / g * sign).collect();
        lead = lead.mul(&Num::Int(IBig::from(g * sign)));
        factors.push(from_poly(&ints_to_exprs(&prim), &x));
    } else if let Some(c) = rest.first() {
        lead = lead.mul(&Num::Int(IBig::from(*c)));
    }
    // Each root factor (q x - p) is q times a monic factor; the integer polynomial already contains them, so no
    // correction is needed: `ints = prod (q x - p) * rest`.
    factors.push(Expr::num(lead));
    Ok(Expr::mul(factors))
}

// ---- solving -----------------------------------------------------------------------------------------------------

/// Real solutions of `f == 0` for a polynomial `f` in `x`.
pub fn solve_polynomial(f: &Expr, x: &Expr) -> MathResult<Vec<Expr>> {
    let Some(coeffs) = to_poly(f, x)? else {
        return Err(MathError::unsupported("solve currently handles polynomial equations (and linear systems)"));
    };
    if coeffs.is_empty() {
        return Err(MathError::domain("the equation is true for every value (0 == 0)"));
    }
    let mut roots: Vec<Expr> = Vec::new();
    let mut remaining = coeffs.clone();
    if let Some(nums) = num_coeffs(&coeffs)
        && let Some((ints, _)) = int_poly(&nums)
    {
        let (found, rest) = rational_roots(&ints);
        for (p, q, _) in found {
            if let Some(r) = Expr::rational(i64::try_from(p).unwrap_or(0), i64::try_from(q).unwrap_or(1)) {
                roots.push(r);
            }
        }
        remaining = ints_to_exprs(&rest);
    }
    if remaining.len() > 4
        && let Some(found) = solve_by_factors(&remaining, x)?
    {
        roots.extend(found);
        sort_roots(&mut roots);
        roots.dedup();
        return Ok(roots);
    }
    match remaining.len() {
        0 | 1 => {}
        2 => roots.push(linear_root(&remaining)?),
        3 => roots.extend(quadratic_roots(&remaining)?),
        4 if let Some(cubic) = crate::solve::cubic_roots(&remaining)? => roots.extend(cubic),
        _ if let Some(found) = power_substitution(&remaining)? => roots.extend(found),
        5 if let Some(found) = crate::solve::quartic_roots(&remaining)? => roots.extend(found),
        n => {
            return Err(MathError::unsupported(format!(
                "cannot solve a degree-{} polynomial in closed form yet (rational roots were {})",
                n - 1,
                if roots.is_empty() { "not found".to_string() } else { "extracted".to_string() }
            )));
        }
    }
    sort_roots(&mut roots);
    roots.dedup();
    Ok(roots)
}

/// Split `c` into irreducible factors over the integers and solve each one. `None` when it does not split (the
/// caller then tries the closed forms for the whole polynomial) or when some factor cannot be solved.
fn solve_by_factors(c: &[Expr], x: &Expr) -> MathResult<Option<Vec<Expr>>> {
    let factored = factor(&from_poly(c, x))?;
    let parts: Vec<Expr> = match factored.kind() {
        Kind::Mul(v) => v.clone(),
        _ => return Ok(None),
    };
    let mut out = Vec::new();
    let mut split = false;
    for t in parts {
        let base = match t.kind() {
            Kind::Pow(b, k) if k.as_i64().is_some_and(|k| k > 0) => b.clone(),
            _ => t.clone(),
        };
        if !base.contains(x) {
            continue;
        }
        let Some(sub) = to_poly(&base, x)? else { return Ok(None) };
        if sub.len() >= c.len() {
            return Ok(None);
        }
        split = true;
        match solve_polynomial(&base, x) {
            Ok(r) => out.extend(r),
            Err(_) => return Ok(None),
        }
    }
    Ok(split.then_some(out))
}

/// `c(x^k)` with k > 1 (such as `x^4 - 5x^2 + 4`): solve for `u = x^k`, then take k-th roots (k = 2, 3, 4).
fn power_substitution(c: &[Expr]) -> MathResult<Option<Vec<Expr>>> {
    let k = c.iter().enumerate().filter(|(_, v)| !v.is_zero()).fold(0usize, |g, (i, _)| gcd(g as i128, i as i128) as usize);
    if !(2..=4).contains(&k) || c.len() <= 3 || !c.len().saturating_sub(1).is_multiple_of(k) {
        return Ok(None);
    }
    let reduced: Vec<Expr> = c.iter().step_by(k).cloned().collect();
    let u = Expr::sym("_u");
    let mut out = Vec::new();
    for r in solve_polynomial(&from_poly(&reduced, &u), &u)? {
        let root = Expr::pow(&r, &Expr::rational(1, k as i64).unwrap_or_else(Expr::one));
        let w = Expr::mul(vec![Expr::half(), Expr::pow(&Expr::int(3), &Expr::half())]);
        let cube = |s: i64| Expr::add(vec![Expr::rational(-1, 2).unwrap_or_else(Expr::one), Expr::mul(vec![Expr::int(s), Expr::imag(), w.clone()])]);
        let units: Vec<Expr> = match k {
            2 => vec![Expr::one(), Expr::int(-1)],
            3 => vec![Expr::one(), cube(1), cube(-1)],
            _ => vec![Expr::one(), Expr::int(-1), Expr::imag(), Expr::imag().neg()],
        };
        let negative = r.as_num().is_some_and(Num::is_negative);
        for unit in units {
            let base = if negative && k % 2 == 1 { Expr::pow(&r.neg(), &Expr::rational(1, k as i64).unwrap_or_else(Expr::one)).neg() } else { root.clone() };
            out.push(expand(&Expr::mul(vec![unit, base]))?);
        }
    }
    Ok(Some(out))
}

fn at(c: &[Expr], i: usize) -> Expr {
    c.get(i).cloned().unwrap_or_else(Expr::zero)
}

fn linear_root(c: &[Expr]) -> MathResult<Expr> {
    Ok(at(c, 0).neg().div(&at(c, 1)))
}

pub(crate) fn quadratic_roots(c: &[Expr]) -> MathResult<Vec<Expr>> {
    let (a, b, cc) = (at(c, 2), at(c, 1), at(c, 0));
    let disc = expand(&Expr::add(vec![Expr::pow(&b, &Expr::int(2)), Expr::mul(vec![Expr::int(-4), a.clone(), cc])]))?;
    let numeric = a.is_num() && b.is_num() && disc.is_num();
    if disc.as_num().is_some_and(Num::is_zero) {
        return Ok(vec![b.neg().div(&Expr::mul(vec![Expr::int(2), a]))]);
    }
    let sq = Expr::pow(&disc, &Expr::half());
    let two_a = Expr::mul(vec![Expr::int(2), a]);
    let mut out = Vec::new();
    for s in [Expr::int(-1), Expr::int(1)] {
        let r = Expr::add(vec![b.neg(), Expr::mul(vec![s, sq.clone()])]).div(&two_a);
        out.push(if numeric { expand(&r)? } else { r });
    }
    Ok(out)
}

pub(crate) fn sort_roots(roots: &mut [Expr]) {
    // By (real part, imaginary part); roots with no numeric value come last in canonical order: a total order.
    let key = |e: &Expr| {
        let (re, im) = crate::complex::parts(e).ok()?;
        let value = |p: &Expr| mathcraft_core::numeric::eval_f64(p, &|_| None).ok();
        Some((value(&re)?, value(&im)?))
    };
    roots.sort_by(|a, b| match (key(a), key(b)) {
        (Some((ar, ai)), Some((br, bi))) => ar.total_cmp(&br).then(ai.total_cmp(&bi)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.cmp(b),
    });
}

/// Is the expression (a polynomial in `x`) of degree at most one? Returns `(a, b)` for `a*x + b` with `a != 0`.
pub fn linear_in(u: &Expr, x: &Expr) -> MathResult<Option<(Expr, Expr)>> {
    if !u.contains(x) {
        return Ok(None);
    }
    Ok(match to_poly(u, x)? {
        Some(c) if c.len() == 2 && !is_zero_expr(&at(&c, 1))? => Some((at(&c, 1), at(&c, 0))),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn poly(c: &[i64]) -> Expr {
        from_poly(&c.iter().map(|v| Expr::int(*v)).collect::<Vec<_>>(), &x())
    }

    #[test]
    fn extracts_coefficients() {
        let e = Expr::mul(vec![Expr::add(vec![x(), Expr::int(1)]), Expr::add(vec![x(), Expr::int(2)])]);
        let c = to_poly(&e, &x()).unwrap().unwrap();
        assert_eq!(c.iter().map(|v| v.to_string()).collect::<Vec<_>>(), vec!["2", "3", "1"]);
        assert!(to_poly(&Expr::call("sin", vec![x()]), &x()).unwrap().is_none());
    }

    #[test]
    fn factors_with_rational_roots() {
        assert_eq!(factor(&poly(&[-1, 0, 1])).unwrap().to_string(), "(x - 1)*(x + 1)");
        assert_eq!(factor(&poly(&[1, 2, 1])).unwrap().to_string(), "(x + 1)**2");
        assert_eq!(factor(&poly(&[-2, 0, 1])).unwrap().to_string(), "x**2 - 2");
        assert_eq!(factor(&poly(&[-1, 0, 4])).unwrap().to_string(), "(2*x - 1)*(2*x + 1)");
    }

    #[test]
    fn solves_polynomials() {
        let show = |c: &[i64]| solve_polynomial(&poly(c), &x()).unwrap().iter().map(|v| v.to_string()).collect::<Vec<_>>();
        assert_eq!(show(&[-4, 0, 1]), vec!["-2", "2"]);
        assert_eq!(show(&[-6, 1, 1]), vec!["-3", "2"]);
        assert_eq!(show(&[-2, 0, 1]), vec!["-sqrt(2)", "sqrt(2)"]);
        assert_eq!(show(&[1, 0, 1]), vec!["-I", "I"]);
        assert_eq!(show(&[-6, 11, -6, 1]), vec!["1", "2", "3"]);
        assert_eq!(show(&[-8, 0, 0, 1]), vec!["-1 - I*sqrt(3)", "-1 + I*sqrt(3)", "2"]);
    }
}
