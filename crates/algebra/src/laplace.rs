//! Laplace transforms. `laplace` handles sums of `c * t**n * exp(a*t) * trig(w*t)` (`trig` one of `sin`, `cos`,
//! `sinh`, `cosh`; `n` a non-negative integer, or any exponent above -1 when nothing else depends on `t`) through
//! the shift rule and `t**n f <-> (-1)**n F^(n)`; products and powers of trig functions go through exponentials.
//! `inverse_laplace` goes the other way for rational functions (partial fractions, quadratics up to the square)
//! and for powers `s**p`.

use crate::calculus::{diff, diff_n};
use crate::complex::rewrite_exp;
use crate::expand::expand;
use crate::poly::{factor, linear_in, to_poly};
use crate::rational::{apart, cancel, together_deep};
use crate::simplify::simplify;
use crate::util::{guard, require_symbol, sign_of, split_fraction, subs};
use mathcraft_core::{Expr, Kind, MathResult};

/// The highest power of an irreducible quadratic that `inverse_laplace` inverts.
const MAX_QUADRATIC_POWER: i64 = 8;

/// The Laplace transform `integral_0^oo f(t) exp(-s t) dt`, or `None` when no rule applies.
pub fn laplace(f: &Expr, t: &Expr, s: &Expr) -> MathResult<Option<Expr>> {
    require_symbol(t)?;
    require_symbol(s)?;
    guard(f)?;
    let expanded = expand(f)?;
    let terms = match expanded.kind() {
        Kind::Add(v) => v.clone(),
        _ => vec![expanded.clone()],
    };
    let mut total = Vec::with_capacity(terms.len());
    for term in &terms {
        let piece = match step_term(term, t, s)? {
            Some(p) => p,
            None => match transform_term(term, t, s)? {
                Some(p) => p,
                None => match exponential_pieces(term, t, s)? {
                    Some(p) => p,
                    None => return Ok(None),
                },
            },
        };
        total.push(piece);
    }
    Ok(Some(simplify(&Expr::add(total))?))
}

/// A term with a `Heaviside(t - c)` or `DiracDelta(t - c)` factor (`c >= 0`, a symbolic `c` is taken to be): the second
/// shift rule `L[H(t - c) h(t - c)] = exp(-c s) L[h]` and the sifting property `L[delta(t - c) g] = g(c) exp(-c s)`.
fn step_term(term: &Expr, t: &Expr, s: &Expr) -> MathResult<Option<Expr>> {
    let factors = match term.kind() {
        Kind::Mul(v) => v.clone(),
        _ => vec![term.clone()],
    };
    let Some(pos) =
        factors.iter().position(|f| matches!(f.kind(), Kind::Call(n, a) if matches!(&**n, "Heaviside" | "DiracDelta") && a.iter().any(|u| u.contains(t))))
    else {
        return Ok(None);
    };
    let Some(Kind::Call(name, args)) = factors.get(pos).map(Expr::kind) else { return Ok(None) };
    let [u] = args.as_slice() else { return Ok(None) };
    let Some((k, m)) = linear_in(u, t)? else { return Ok(None) };
    if sign_of(&k) != Some(1) {
        return Ok(None);
    }
    let c = expand(&m.neg().div(&k))?;
    let rest = Expr::mul(factors.iter().enumerate().filter(|(i, _)| *i != pos).map(|(_, f)| f.clone()).collect());
    let delay = Expr::pow(&Expr::e(), &Expr::mul(vec![c.clone(), s.clone()]).neg());
    if sign_of(&c) == Some(-1) {
        // the step is already on for t > 0, and the delta is outside the range
        return if &**name == "Heaviside" { Ok(laplace(&rest, t, s)?) } else { Ok(Some(Expr::zero())) };
    }
    if &**name == "DiracDelta" {
        let value = expand(&subs(&rest, &[(t.clone(), c)])?)?;
        return Ok(Some(Expr::mul(vec![value, Expr::pow(&k, &Expr::int(-1)), delay])));
    }
    let shifted = expand(&subs(&rest, &[(t.clone(), Expr::add(vec![t.clone(), c]))])?)?;
    let Some(inner) = laplace(&shifted, t, s)? else { return Ok(None) };
    Ok(Some(Expr::mul(vec![delay, inner])))
}

/// A term with several trig factors or powers of them, through `sin(x) = (e**(I x) - e**(-I x))/(2 I)`.
fn exponential_pieces(term: &Expr, t: &Expr, s: &Expr) -> MathResult<Option<Expr>> {
    if !has_trig(term) {
        return Ok(None);
    }
    let rewritten = expand(&rewrite_exp(term)?)?;
    let pieces = match rewritten.kind() {
        Kind::Add(v) => v.clone(),
        _ => vec![rewritten.clone()],
    };
    let mut total = Vec::with_capacity(pieces.len());
    for p in &pieces {
        if has_trig(p) {
            return Ok(None);
        }
        let Some(v) = transform_term(p, t, s)? else { return Ok(None) };
        total.push(v);
    }
    Ok(Some(cancel(&Expr::add(total))?))
}

pub(crate) fn has_trig(e: &Expr) -> bool {
    matches!(e.kind(), Kind::Call(n, _) if is_trig(n)) || e.children().iter().any(has_trig)
}

fn is_trig(name: &str) -> bool {
    matches!(name, "sin" | "cos" | "sinh" | "cosh")
}

/// One term `c * t**n * exp(a*t) * trig(w*t)`.
fn transform_term(term: &Expr, t: &Expr, s: &Expr) -> MathResult<Option<Expr>> {
    let factors = match term.kind() {
        Kind::Mul(v) => v.clone(),
        _ => vec![term.clone()],
    };
    let (mut coef, mut power, mut rate) = (Vec::new(), Vec::new(), Vec::new());
    let mut trig: Option<(String, Expr)> = None;
    for f in &factors {
        if !f.contains(t) {
            coef.push(f.clone());
            continue;
        }
        match f.kind() {
            _ if f == t => power.push(Expr::one()),
            Kind::Pow(b, p) if b == t && p.as_num().is_some_and(|n| n.is_exact()) => power.push(p.clone()),
            Kind::Pow(b, u) if *b == Expr::e() => {
                let Some((k, c)) = linear_in(u, t)? else { return Ok(None) };
                if !c.is_zero() {
                    coef.push(Expr::pow(&Expr::e(), &c));
                }
                rate.push(k);
            }
            Kind::Call(name, args) if is_trig(name) && trig.is_none() => {
                let [arg] = args.as_slice() else { return Ok(None) };
                let Some((w, c)) = linear_in(arg, t)? else { return Ok(None) };
                if !c.is_zero() {
                    return Ok(None);
                }
                trig = Some((name.to_string(), w));
            }
            _ => return Ok(None),
        }
    }
    let n = Expr::add(power);
    let a = Expr::add(rate);
    let c = Expr::mul(coef);
    let shifted = Expr::add(vec![s.clone(), a.neg()]);
    let base = match &trig {
        None if n.as_i64().is_none() => {
            // t**p alone: Gamma(p + 1)/s**(p + 1).
            let p1 = Expr::add(vec![n.clone(), Expr::one()]);
            if !a.is_zero() || !n.is_num() || sign_of(&p1) != Some(1) {
                return Ok(None);
            }
            let g = Expr::call("Gamma", vec![p1.clone()]);
            return Ok(Some(Expr::mul(vec![c, g, Expr::pow(s, &p1.neg())])));
        }
        _ if n.as_i64().is_none_or(|k| k < 0) => return Ok(None),
        None => Expr::pow(&shifted, &Expr::int(-1)),
        Some((name, w)) => {
            let w2 = Expr::pow(w, &Expr::int(2));
            let u2 = Expr::pow(&shifted, &Expr::int(2));
            let (num, den) = match name.as_str() {
                "sin" => (w.clone(), Expr::add(vec![u2, w2])),
                "cos" => (shifted.clone(), Expr::add(vec![u2, w2])),
                "sinh" => (w.clone(), Expr::add(vec![u2, w2.neg()])),
                _ => (shifted.clone(), Expr::add(vec![u2, w2.neg()])),
            };
            num.div(&den)
        }
    };
    let order = n.as_i64().unwrap_or(0);
    let g = if order == 0 { base } else { diff_n(&base, s, u32::try_from(order).unwrap_or(0))? };
    let sign = if order % 2 == 0 { Expr::one() } else { Expr::int(-1) };
    Ok(Some(Expr::mul(vec![c, sign, g])))
}

/// The inverse Laplace transform of `f(s)` as a function of `t` (for `t > 0`), or `None` when no rule applies.
/// A factor `exp(-c*s)` delays the result: `Heaviside(t - c)` times the inverse, shifted by `c`.
pub fn inverse_laplace(f: &Expr, s: &Expr, t: &Expr) -> MathResult<Option<Expr>> {
    require_symbol(s)?;
    require_symbol(t)?;
    guard(f)?;
    // group the terms by their delay
    let expanded = expand(f)?;
    let terms = match expanded.kind() {
        Kind::Add(v) => v.clone(),
        _ => vec![expanded.clone()],
    };
    let mut groups: Vec<(Expr, Vec<Expr>)> = Vec::new();
    for term in &terms {
        let (delay, rest) = split_delay(term, s)?;
        match groups.iter_mut().find(|(d, _)| *d == delay) {
            Some((_, v)) => v.push(rest),
            None => groups.push((delay, vec![rest])),
        }
    }
    let mut total = Vec::new();
    for (delay, rests) in groups {
        let Some(base) = inverse_rational(&Expr::add(rests), s, t)? else { return Ok(None) };
        if delay.is_zero() {
            total.push(base);
            continue;
        }
        let shifted = expand(&subs(&base, &[(t.clone(), Expr::add(vec![t.clone(), delay.neg()]))])?)?;
        let step = Expr::call("Heaviside", vec![Expr::add(vec![t.clone(), delay.neg()])]);
        let parts = match shifted.kind() {
            Kind::Add(v) => v.clone(),
            _ => vec![shifted.clone()],
        };
        for p in parts {
            total.push(if contains_call(&p, "DiracDelta") { p } else { Expr::mul(vec![p, step.clone()]) });
        }
    }
    Ok(Some(expand(&Expr::add(total))?))
}

fn contains_call(e: &Expr, name: &str) -> bool {
    matches!(e.kind(), Kind::Call(n, _) if &**n == name) || e.children().iter().any(|c| contains_call(c, name))
}

/// `(c, rest)` for a term `exp(-c*s) * rest` (`c` zero when there is no such factor).
fn split_delay(term: &Expr, s: &Expr) -> MathResult<(Expr, Expr)> {
    let factors = match term.kind() {
        Kind::Mul(v) => v.clone(),
        _ => vec![term.clone()],
    };
    let (mut delay, mut rest) = (Vec::new(), Vec::new());
    for f in factors {
        if let Kind::Pow(b, u) = f.kind()
            && *b == Expr::e()
            && u.contains(s)
            && let Some((k, m)) = linear_in(u, s)?
            && m.is_zero()
            && sign_of(&k) != Some(1)
        {
            delay.push(k.neg());
            continue;
        }
        rest.push(f);
    }
    Ok((expand(&Expr::add(delay))?, Expr::mul(rest)))
}

fn inverse_rational(f: &Expr, s: &Expr, t: &Expr) -> MathResult<Option<Expr>> {
    let (n, d) = together_deep(f)?;
    let parted = apart(&cancel(&n.div(&d))?, s)?;
    let terms = match parted.kind() {
        Kind::Add(v) => v.clone(),
        _ => vec![parted.clone()],
    };
    let mut total = Vec::with_capacity(terms.len());
    for term in &terms {
        match inverse_term(term, s, t)? {
            Some(v) => total.push(v),
            None => return Ok(None),
        }
    }
    Ok(Some(expand(&Expr::add(total))?))
}

/// `c / B**m` (with `B` linear, or quadratic over `m <= 8`) or `c * s**p`.
fn inverse_term(term: &Expr, s: &Expr, t: &Expr) -> MathResult<Option<Expr>> {
    if !term.contains(s) {
        return Ok(Some(Expr::mul(vec![term.clone(), Expr::call("DiracDelta", vec![t.clone()])])));
    }
    // c * s**p with p a negative non-integer: t**(-p - 1)/Gamma(-p).
    let factors = match term.kind() {
        Kind::Mul(v) => v.clone(),
        _ => vec![term.clone()],
    };
    let (free, dep): (Vec<Expr>, Vec<Expr>) = factors.into_iter().partition(|f| !f.contains(s));
    if let [only] = dep.as_slice()
        && let Kind::Pow(b, p) = only.kind()
        && b == s
        && let Some(pn) = p.as_num()
        && pn.is_exact()
        && !pn.is_integer()
        && pn.is_negative()
    {
        let q = p.neg();
        let value = Expr::mul(vec![
            Expr::mul(free),
            Expr::pow(t, &Expr::add(vec![q.clone(), Expr::int(-1)])),
            Expr::pow(&Expr::call("Gamma", vec![q]), &Expr::int(-1)),
        ]);
        return Ok(Some(value));
    }
    let (num, den) = split_fraction(term);
    if num.contains(s) && to_poly(&num, s)?.is_none_or(|c| c.len() > 2) {
        return Ok(None);
    }
    let factored = factor(&den)?;
    let parts = match factored.kind() {
        Kind::Mul(v) => v.clone(),
        _ => vec![factored.clone()],
    };
    let (mut lead, mut base, mut m) = (Vec::new(), None, 1i64);
    for p in &parts {
        if !p.contains(s) {
            lead.push(p.clone());
            continue;
        }
        let (b, k) = match p.kind() {
            Kind::Pow(b, k) => (b.clone(), k.as_i64()),
            _ => (p.clone(), Some(1)),
        };
        let Some(k) = k.filter(|k| *k >= 1) else { return Ok(None) };
        if base.is_some() {
            return Ok(None);
        }
        base = Some(b);
        m = k;
    }
    let Some(b) = base else { return Ok(None) };
    let Some(bc) = to_poly(&b, s)? else { return Ok(None) };
    let scale = Expr::mul(lead);
    let at = |i: usize| bc.get(i).cloned().unwrap_or_else(Expr::zero);
    let (p0, q0, r0) = (at(2), at(1), at(0));
    let Some(np) = to_poly(&num, s)? else { return Ok(None) };
    let (n1, n0) = (np.get(1).cloned().unwrap_or_else(Expr::zero), np.first().cloned().unwrap_or_else(Expr::zero));
    match bc.len() {
        2 => {
            // c/(q0 s + r0)**m, a numerator constant after `apart`.
            if !n1.is_zero() {
                return Ok(None);
            }
            let a = r0.neg().div(&q0);
            let k = Expr::mul(vec![n0, Expr::pow(&Expr::mul(vec![scale, Expr::pow(&q0, &Expr::int(m))]), &Expr::int(-1))]);
            let value = Expr::mul(vec![
                k,
                Expr::pow(t, &Expr::int(m - 1)),
                Expr::pow(&Expr::e(), &Expr::mul(vec![a, t.clone()])),
                Expr::rational(1, factorial(m - 1)?).unwrap_or_else(Expr::one),
            ]);
            Ok(Some(value))
        }
        3 if m <= MAX_QUADRATIC_POWER => {
            let a = q0.neg().div(&Expr::mul(vec![Expr::int(2), p0.clone()]));
            let w2 = expand(&Expr::add(vec![r0.div(&p0), Expr::pow(&a, &Expr::int(2)).neg()]))?;
            let trig = match sign_of(&w2) {
                Some(1) => true,
                Some(-1) => false,
                _ if !w2.is_num() => true, // a symbolic frequency: assume it is real
                _ => return Ok(None),
            };
            let w = match w2.kind() {
                Kind::Pow(b, k) if trig && k.as_i64() == Some(2) => b.clone(),
                _ => Expr::pow(&if trig { w2.clone() } else { w2.neg() }, &Expr::half()),
            };
            let k = Expr::pow(&Expr::mul(vec![scale, Expr::pow(&p0, &Expr::int(m))]), &Expr::int(-1));
            // (n1 s + n0) = n1 (s - a) + (n0 + n1 a)
            let (u_coef, one_coef) = (n1.clone(), expand(&Expr::add(vec![n0, Expr::mul(vec![n1, a.clone()])]))?);
            let wt = Expr::mul(vec![w.clone(), t.clone()]);
            let (c_fn, s_fn) = if trig {
                (Expr::call("cos", vec![wt.clone()]), Expr::call("sin", vec![wt.clone()]))
            } else {
                (Expr::call("cosh", vec![wt.clone()]), Expr::call("sinh", vec![wt.clone()]))
            };
            // L^-1 of u/(u^2 +- w^2)^m and 1/(u^2 +- w^2)^m, before the shift, by the recurrence
            // h_m = t g_(m-1)/(2 (m - 1)) and g_m = (g_(m-1) -+ h_m')/w^2.
            let w2_inv = Expr::pow(&w2_pos(&w, trig), &Expr::int(-1));
            let (mut f_u, mut f_one) = (c_fn, Expr::mul(vec![s_fn, Expr::pow(&w, &Expr::int(-1))]));
            for j in 2..=m {
                let h = Expr::mul(vec![t.clone(), f_one.clone(), Expr::rational(1, 2 * (j - 1)).unwrap_or_else(Expr::one)]);
                let dh = diff(&h, t)?;
                let g = if trig { Expr::add(vec![f_one, dh.neg()]) } else { Expr::add(vec![dh, f_one.neg()]) };
                f_one = expand(&Expr::mul(vec![g, w2_inv.clone()]))?;
                f_u = expand(&h)?;
            }
            let inner = Expr::add(vec![Expr::mul(vec![u_coef, f_u]), Expr::mul(vec![one_coef, f_one])]);
            let shift = Expr::pow(&Expr::e(), &Expr::mul(vec![a, t.clone()]));
            Ok(Some(Expr::mul(vec![k, shift, inner])))
        }
        _ => Ok(None),
    }
}

/// `w**2` as it appears in `u**2 + w**2` (`trig`) or `u**2 - w**2`; the recurrence divides by it either way.
fn w2_pos(w: &Expr, _trig: bool) -> Expr {
    Expr::pow(w, &Expr::int(2))
}

fn factorial(n: i64) -> MathResult<i64> {
    Ok((1..=n.clamp(0, 20)).product())
}
