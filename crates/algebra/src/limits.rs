//! Limits: at finite points (two-sided and one-sided, including poles) and at plus or minus infinity.

use crate::calculus::diff;
use crate::expand::expand;
use crate::util::{guard, has_singularity, is_zero_expr, require_symbol, sign_of, split_fraction, subs};
use mathcraft_core::{Expr, Kind, MathError, MathResult};

/// Which side a finite limit is approached from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Both,
    /// From above: `x -> a+`.
    Right,
    /// From below: `x -> a-`.
    Left,
}

/// A limit value: a finite expression, or infinity with a sign.
enum Lim {
    Fin(Expr),
    Inf(i8),
}

const MAX_ROUNDS: usize = 8;
const MAX_DEPTH: u32 = 24;

fn unsupported() -> MathError {
    MathError::unsupported("could not determine the limit")
}

fn lim_expr(l: Lim) -> Expr {
    match l {
        Lim::Fin(e) => e,
        Lim::Inf(s) if s > 0 => Expr::infinity(),
        Lim::Inf(_) => Expr::infinity().neg(),
    }
}

fn infinity_sign(a: &Expr) -> Option<i8> {
    if a == &Expr::infinity() {
        Some(1)
    } else if a == &Expr::infinity().neg() {
        Some(-1)
    } else {
        None
    }
}

/// Two-sided limit of `e` as `x -> a` (`a` may be `oo` or `-oo`).
pub fn limit(e: &Expr, x: &Expr, a: &Expr) -> MathResult<Expr> {
    limit_side(e, x, a, Side::Both)
}

/// Limit of `e` as `x -> a` from the given side. Direct substitution, then L'Hôpital's rule, then the sign of a pole.
pub fn limit_side(e: &Expr, x: &Expr, a: &Expr, side: Side) -> MathResult<Expr> {
    require_symbol(x)?;
    if let Some(s) = infinity_sign(a) {
        return Ok(lim_expr(at_infinity(e, x, s, 0)?));
    }
    finite(e, x, a, side)
}

fn finite(e: &Expr, x: &Expr, a: &Expr, side: Side) -> MathResult<Expr> {
    let at = |t: &Expr| subs(t, &[(x.clone(), a.clone())]);
    let direct = at(e)?;
    if !has_singularity(&direct) {
        return Ok(direct);
    }
    let (mut num, mut den) = split_fraction(e);
    for _ in 0..MAX_ROUNDS {
        let n0 = expand(&at(&num)?)?;
        let d0 = expand(&at(&den)?)?;
        if has_singularity(&n0) || has_singularity(&d0) {
            return via_infinity(e, x, a, side);
        }
        if !d0.is_zero() {
            return Ok(n0.div(&d0));
        }
        if !n0.is_zero() && !is_zero_expr(&n0)? {
            return pole(&n0, &den, x, a, side);
        }
        num = diff(&num, x)?;
        den = diff(&den, x)?;
    }
    via_infinity(e, x, a, side)
}

/// Last resort at a finite point: put `x = a + 1/t` and take `t -> +-oo`, where the growth rules (exp, log, powers,
/// `0 * oo`) are stronger than L'Hôpital's rule on the original expression.
fn via_infinity(e: &Expr, x: &Expr, a: &Expr, side: Side) -> MathResult<Expr> {
    let t = Expr::sym("t_limit");
    let go = |s: i8| -> MathResult<Expr> {
        let inv = Expr::pow(&t, &Expr::int(-1));
        let moved = subs(e, &[(x.clone(), Expr::add(vec![a.clone(), inv]))])?;
        Ok(lim_expr(at_infinity(&moved, &t, s, 0)?))
    };
    if e.contains(&t) {
        return Err(unsupported());
    }
    match side {
        Side::Right => go(1),
        Side::Left => go(-1),
        Side::Both => {
            let (r, l) = (go(1)?, go(-1)?);
            if r == l { Ok(r) } else { Err(MathError::domain("the limit differs on the two sides; give a side, e.g. dir='+'")) }
        }
    }
}

/// The numerator is a non-zero `n0` at `a` and the denominator vanishes there: the result is `+oo` or `-oo`, decided by
/// the signs of the numerator and of the denominator's first non-zero derivative.
fn pole(n0: &Expr, den: &Expr, x: &Expr, a: &Expr, side: Side) -> MathResult<Expr> {
    let sn = sign_of(n0).ok_or_else(unsupported)?;
    let mut d = den.clone();
    for k in 1..=12u32 {
        d = diff(&d, x)?;
        let v = expand(&subs(&d, &[(x.clone(), a.clone())])?)?;
        if is_zero_expr(&v)? {
            continue;
        }
        let sc = sign_of(&v).ok_or_else(unsupported)?;
        let odd = k % 2 == 1;
        let flip = match side {
            Side::Right => 1,
            Side::Left if odd => -1,
            Side::Left => 1,
            Side::Both if odd => {
                return Err(MathError::domain("the limit differs on the two sides (oo on one, -oo on the other); give a side, e.g. dir='+'"));
            }
            Side::Both => 1,
        };
        return Ok(lim_expr(Lim::Inf(sn * sc * flip)));
    }
    Err(unsupported())
}

// ---- infinity ----------------------------------------------------------------------------------------------------

fn at_infinity(e: &Expr, x: &Expr, s: i8, depth: u32) -> MathResult<Lim> {
    guard(e)?;
    if depth > MAX_DEPTH {
        return Err(unsupported());
    }
    if !e.contains(x) {
        return Ok(Lim::Fin(e.clone()));
    }
    if e == x {
        return Ok(Lim::Inf(s));
    }
    match e.kind() {
        Kind::Add(ts) => {
            let (mut fin, mut inf): (Vec<Expr>, Option<i8>) = (Vec::new(), None);
            for t in ts {
                match at_infinity(t, x, s, depth + 1)? {
                    Lim::Fin(v) => fin.push(v),
                    Lim::Inf(d) => match inf {
                        None => inf = Some(d),
                        Some(p) if p == d => {}
                        // oo - oo with radicals: factor the dominant power out of each root, then out of the sum.
                        Some(_) if s > 0 && depth < MAX_DEPTH / 2 && factor_out_x(e, x)?.is_some() => {
                            return match factor_out_x(e, x)? {
                                Some(f) => at_infinity(&f, x, s, depth + 8),
                                None => Err(unsupported()),
                            };
                        }
                        // oo - oo: combine into one fraction and use L'Hôpital.
                        Some(_) => {
                            let (n, d) = crate::rational::together(e)?;
                            return if d.is_one() { Err(unsupported()) } else { ratio(&n, &d, x, s, depth + 1) };
                        }
                    },
                }
            }
            Ok(inf.map_or_else(|| Lim::Fin(Expr::add(fin)), Lim::Inf))
        }
        Kind::Mul(fs) => {
            let (num, den) = split_fraction(e);
            if !den.is_one() {
                return ratio(&num, &den, x, s, depth + 1);
            }
            let (mut finite, mut zeros, mut infs, mut sign) = (Vec::new(), Vec::new(), Vec::new(), 1i8);
            for f in fs {
                match at_infinity(f, x, s, depth + 1)? {
                    Lim::Fin(v) if is_zero_expr(&v)? => zeros.push(f.clone()),
                    Lim::Fin(v) => finite.push(v),
                    Lim::Inf(d) => {
                        sign *= d;
                        infs.push(f.clone());
                    }
                }
            }
            if infs.is_empty() {
                return Ok(Lim::Fin(if zeros.is_empty() { Expr::mul(finite) } else { Expr::zero() }));
            }
            if !zeros.is_empty() {
                // 0 * oo: write it as a quotient (either way round) and use L'Hôpital.
                let (zero, inf) = (Expr::mul(zeros), Expr::mul(infs));
                let inverse = |t: &Expr| Expr::pow(t, &Expr::int(-1));
                return ratio(&inf, &inverse(&zero), x, s, depth + 1).or_else(|_| ratio(&zero, &inverse(&inf), x, s, depth + 1));
            }
            let c = sign_of(&Expr::mul(finite)).ok_or_else(unsupported)?;
            Ok(Lim::Inf(sign * c))
        }
        Kind::Pow(b, k) => {
            if k.contains(x) {
                // b ** k with a constant positive base: grows or decays according to k and whether b is above 1.
                let base = if b.contains(x) { None } else { mathcraft_core::numeric::eval_f64(b, &|_| None).ok().filter(|v| *v > 0.0) };
                let Some(base) = base else {
                    // x-dependent base and exponent: b ** k = exp(k * log(b)).
                    let log_b = Expr::call("log", vec![b.clone()]);
                    let rewritten = Expr::pow(&Expr::e(), &Expr::mul(vec![k.clone(), log_b]));
                    return if depth < MAX_DEPTH / 2 { at_infinity(&rewritten, x, s, depth + 8) } else { Err(unsupported()) };
                };
                return Ok(match at_infinity(k, x, s, depth + 1)? {
                    Lim::Fin(v) => Lim::Fin(Expr::pow(b, &v)),
                    Lim::Inf(d) => {
                        let up = (base > 1.0) == (d > 0);
                        if (base - 1.0).abs() < f64::EPSILON {
                            Lim::Fin(Expr::one())
                        } else if up {
                            Lim::Inf(1)
                        } else {
                            Lim::Fin(Expr::zero())
                        }
                    }
                });
            }
            let kn = k.as_num().ok_or_else(unsupported)?;
            match at_infinity(b, x, s, depth + 1)? {
                Lim::Fin(v) if kn.is_negative() && is_zero_expr(&v)? => Err(unsupported()),
                Lim::Fin(v) => Ok(Lim::Fin(Expr::pow(&v, k))),
                Lim::Inf(_) if kn.is_negative() => Ok(Lim::Fin(Expr::zero())),
                Lim::Inf(d) if kn.is_integer() => Ok(Lim::Inf(if k.as_i64().is_some_and(|n| n % 2 == 0) { 1 } else { d })),
                Lim::Inf(d) if d > 0 => Ok(Lim::Inf(1)),
                Lim::Inf(_) => Err(unsupported()),
            }
        }
        Kind::Call(name, args) => {
            let [u] = args.as_slice() else { return Err(unsupported()) };
            let lu = at_infinity(u, x, s, depth + 1)?;
            match (&**name, lu) {
                ("log", Lim::Fin(v)) if v.is_zero() => Ok(Lim::Inf(-1)),
                (_, Lim::Fin(v)) => Ok(Lim::Fin(Expr::call(name, vec![v]))),
                ("exp", Lim::Inf(d)) => Ok(if d > 0 { Lim::Inf(1) } else { Lim::Fin(Expr::zero()) }),
                ("log", Lim::Inf(1)) => Ok(Lim::Inf(1)),
                ("atan", Lim::Inf(d)) => Ok(Lim::Fin(Expr::mul(vec![Expr::rational(i64::from(d), 2).unwrap_or_else(Expr::zero), Expr::pi()]))),
                ("tanh" | "erf", Lim::Inf(d)) => Ok(Lim::Fin(Expr::int(i64::from(d)))),
                ("asinh", Lim::Inf(d)) => Ok(Lim::Inf(d)),
                ("acosh", Lim::Inf(1)) => Ok(Lim::Inf(1)),
                _ => Err(MathError::domain(format!("`{name}` has no limit here (it oscillates or is undefined)"))),
            }
        }
        _ => Err(unsupported()),
    }
}

/// For `x -> +oo`: rewrite every root `b ** (p/q)` of a polynomial `b` of degree `n` as `x ** (n*p/q) * (b / x**n) ** (p/q)`,
/// then, if every term of the resulting sum carries a factor `x`, pull it out: `sqrt(x**2 + x) - x` becomes
/// `x * (sqrt(1 + 1/x) - 1)`, a `oo * 0` form the rest of the machinery can handle.
fn factor_out_x(e: &Expr, x: &Expr) -> MathResult<Option<Expr>> {
    fn pull(e: &Expr, x: &Expr) -> MathResult<Expr> {
        if let Kind::Pow(b, k) = e.kind()
            && b.contains(x)
            && !k.contains(x)
            && k.as_num().is_some_and(|n| !n.is_integer())
            && let Some(coeffs) = crate::poly::to_poly(b, x)?
            && coeffs.len() > 1
        {
            let n = Expr::int(i64::try_from(coeffs.len() - 1).unwrap_or(1));
            let inner = expand(&b.div(&Expr::pow(x, &n)))?;
            return Ok(Expr::mul(vec![Expr::pow(x, &Expr::mul(vec![n, k.clone()])), Expr::pow(&inner, k)]));
        }
        e.map_children(|c| pull(c, x))
    }
    let pulled = pull(e, x)?;
    let Kind::Add(terms) = pulled.kind() else { return Ok(None) };
    let divided: Vec<Expr> = terms.iter().map(|t| t.div(x)).collect();
    let has_x = |t: &Expr| t.contains(x);
    // every term must have shed an x (no term left with a bare 1/x it cannot absorb), and something must have changed
    if divided.iter().any(|t| matches!(t.kind(), Kind::Num(_)) && !has_x(t)) && !divided.iter().any(has_x) {
        return Ok(None);
    }
    let sum = Expr::add(divided);
    Ok((&pulled != e).then(|| Expr::mul(vec![x.clone(), sum])))
}

/// `num / den` at infinity: compare the limits of both, using L'Hôpital's rule for `oo/oo` and `0/0`.
fn ratio(num: &Expr, den: &Expr, x: &Expr, s: i8, depth: u32) -> MathResult<Lim> {
    let (mut n, mut d) = (num.clone(), den.clone());
    for _ in 0..MAX_ROUNDS {
        match (at_infinity(&n, x, s, depth)?, at_infinity(&d, x, s, depth)?) {
            (Lim::Fin(a), Lim::Fin(b)) => {
                if !is_zero_expr(&b)? {
                    return Ok(Lim::Fin(a.div(&b)));
                }
                if !is_zero_expr(&a)? {
                    return Err(unsupported());
                }
            }
            (Lim::Fin(_), Lim::Inf(_)) => return Ok(Lim::Fin(Expr::zero())),
            (Lim::Inf(t), Lim::Fin(b)) => {
                if is_zero_expr(&b)? {
                    return Err(unsupported());
                }
                return Ok(Lim::Inf(t * sign_of(&b).ok_or_else(unsupported)?));
            }
            (Lim::Inf(_), Lim::Inf(_)) => {}
        }
        let (dn, dd) = (diff(&n, x)?, diff(&d, x)?);
        // Cancel the new quotient so common factors (like -1/x**2 in both) do not keep it indeterminate.
        (n, d) = match crate::rational::cancel(&dn.div(&dd)) {
            Ok(q) if !q.divides_by_zero() => split_fraction(&q),
            _ => (dn, dd),
        };
    }
    Err(unsupported())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn lim(e: &Expr, a: &Expr, side: Side) -> String {
        limit_side(e, &x(), a, side).map_or_else(|err| format!("error: {err}"), |v| v.to_string())
    }
    fn oo() -> Expr {
        Expr::infinity()
    }

    #[test]
    fn limits_at_infinity() {
        let i = Expr::int;
        let rational = Expr::add(vec![Expr::pow(&x(), &i(2)), i(1)]).div(&Expr::add(vec![Expr::mul(vec![i(2), Expr::pow(&x(), &i(2))]), x()]));
        assert_eq!(lim(&rational, &oo(), Side::Both), "1/2");
        assert_eq!(lim(&Expr::pow(&x(), &i(3)).div(&Expr::pow(&x(), &i(2))), &oo(), Side::Both), "oo");
        assert_eq!(lim(&Expr::pow(&x(), &i(3)), &oo().neg(), Side::Both), "-oo");
        assert_eq!(lim(&Expr::call("exp", vec![x().neg()]), &oo(), Side::Both), "0");
        assert_eq!(lim(&Expr::mul(vec![x(), Expr::call("exp", vec![x().neg()])]), &oo(), Side::Both), "0");
        assert_eq!(lim(&Expr::call("atan", vec![x()]), &oo(), Side::Both), "pi/2");
        assert_eq!(lim(&Expr::pow(&x(), &i(2)).div(&Expr::add(vec![x(), i(1)])).sub(&x()), &oo(), Side::Both), "-1");
    }

    #[test]
    fn poles_and_one_sided_limits() {
        let i = Expr::int;
        let inv = Expr::pow(&x(), &i(-1));
        assert_eq!(lim(&inv, &i(0), Side::Right), "oo");
        assert_eq!(lim(&inv, &i(0), Side::Left), "-oo");
        assert!(lim(&inv, &i(0), Side::Both).starts_with("error: the limit differs"));
        assert_eq!(lim(&Expr::pow(&x(), &i(-2)), &i(0), Side::Both), "oo");
        assert_eq!(lim(&Expr::mul(vec![i(-1), Expr::pow(&x(), &i(-2))]), &i(0), Side::Both), "-oo");
    }
}
