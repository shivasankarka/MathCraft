//! Differentiation, integration, series and limits.

use crate::expand::expand;
use crate::poly::linear_in;
use crate::util::{guard, has_singularity, require_symbol, split_fraction, subs};
use mathcraft_core::numeric::eval_f64;
use mathcraft_core::{Expr, Kind, MathError, MathResult};

// ---- differentiation ---------------------------------------------------------------------------------------------

/// Derivative of `e` with respect to the symbol `x`.
pub fn diff(e: &Expr, x: &Expr) -> MathResult<Expr> {
    require_symbol(x)?;
    d(e, x)
}

pub fn diff_n(e: &Expr, x: &Expr, n: u32) -> MathResult<Expr> {
    let mut cur = e.clone();
    for _ in 0..n {
        cur = diff(&cur, x)?;
    }
    Ok(cur)
}

fn d(e: &Expr, x: &Expr) -> MathResult<Expr> {
    guard(e)?;
    if !e.contains(x) {
        return Ok(Expr::zero());
    }
    match e.kind() {
        Kind::Num(_) => Ok(Expr::zero()),
        Kind::Sym(_) => Ok(Expr::one()),
        Kind::Add(ts) => Ok(Expr::add(ts.iter().map(|t| d(t, x)).collect::<MathResult<_>>()?)),
        Kind::Mul(fs) => {
            let mut terms = Vec::with_capacity(fs.len());
            for i in 0..fs.len() {
                let mut factors = fs.clone();
                if let Some(slot) = factors.get_mut(i) {
                    *slot = d(slot, x)?;
                }
                terms.push(Expr::mul(factors));
            }
            Ok(Expr::add(terms))
        }
        Kind::Pow(b, ex) => {
            if !ex.contains(x) {
                // d(b^n) = n * b^(n-1) * b'
                let lower = Expr::pow(b, &Expr::add(vec![ex.clone(), Expr::int(-1)]));
                Ok(Expr::mul(vec![ex.clone(), lower, d(b, x)?]))
            } else if !b.contains(x) {
                // d(c^u) = c^u * log(c) * u'
                Ok(Expr::mul(vec![e.clone(), Expr::call("log", vec![b.clone()]), d(ex, x)?]))
            } else {
                // d(b^u) = b^u * (u' log b + u b'/b)
                let left = Expr::mul(vec![d(ex, x)?, Expr::call("log", vec![b.clone()])]);
                let right = Expr::mul(vec![ex.clone(), d(b, x)?]).div(b);
                Ok(Expr::mul(vec![e.clone(), Expr::add(vec![left, right])]))
            }
        }
        Kind::Call(name, args) => match args.as_slice() {
            [u] => {
                let outer = derivative_of(name, u).ok_or_else(|| MathError::unsupported(format!("do not know the derivative of `{name}`")));
                match outer {
                    Ok(o) => Ok(Expr::mul(vec![o, d(u, x)?])),
                    // An unknown function of x stays as an unevaluated derivative.
                    Err(_) => Ok(Expr::call("diff", vec![e.clone(), x.clone()])),
                }
            }
            _ => Ok(Expr::call("diff", vec![e.clone(), x.clone()])),
        },
        Kind::List(v) => Ok(Expr::list(v.iter().map(|t| d(t, x)).collect::<MathResult<_>>()?)),
        Kind::Eq(l, r) => Ok(Expr::eq(d(l, x)?, d(r, x)?)),
    }
}

/// `f'(u)` for a built-in function `f`.
fn derivative_of(name: &str, u: &Expr) -> Option<Expr> {
    let one = Expr::one();
    let two = Expr::int(2);
    let c = |n: &str| Expr::call(n, vec![u.clone()]);
    let one_minus_u2 = || Expr::add(vec![Expr::one(), Expr::pow(u, &Expr::int(2)).neg()]);
    Some(match name {
        "sin" => c("cos"),
        "cos" => c("sin").neg(),
        "tan" => Expr::pow(&c("cos"), &Expr::int(-2)),
        "asin" => Expr::pow(&one_minus_u2(), &Expr::rational(-1, 2)?),
        "acos" => Expr::pow(&one_minus_u2(), &Expr::rational(-1, 2)?).neg(),
        "atan" => Expr::pow(&Expr::add(vec![one, Expr::pow(u, &two)]), &Expr::int(-1)),
        "sinh" => c("cosh"),
        "cosh" => c("sinh"),
        "tanh" => Expr::pow(&c("cosh"), &Expr::int(-2)),
        "log" => Expr::pow(u, &Expr::int(-1)),
        "abs" => u.div(&c("abs")),
        _ => return None,
    })
}

// ---- integration -------------------------------------------------------------------------------------------------

/// An antiderivative of `e` in `x`, or `None` when the integrators here cannot find one.
pub fn integrate(e: &Expr, x: &Expr) -> MathResult<Option<Expr>> {
    require_symbol(x)?;
    int(e, x, 0)
}

const MAX_INT_DEPTH: u32 = 6;

fn int(e: &Expr, x: &Expr, depth: u32) -> MathResult<Option<Expr>> {
    guard(e)?;
    if !e.contains(x) {
        return Ok(Some(Expr::mul(vec![e.clone(), x.clone()])));
    }
    let direct = match e.kind() {
        Kind::Add(ts) => {
            let mut parts = Vec::with_capacity(ts.len());
            for t in ts {
                match int(t, x, depth)? {
                    Some(p) => parts.push(p),
                    None => {
                        parts.clear();
                        break;
                    }
                }
            }
            (!parts.is_empty()).then(|| Expr::add(parts))
        }
        Kind::Mul(fs) => {
            let (consts, vars): (Vec<Expr>, Vec<Expr>) = fs.iter().cloned().partition(|f| !f.contains(x));
            if !consts.is_empty() {
                return Ok(int(&Expr::mul(vars), x, depth)?.map(|r| Expr::mul(vec![Expr::mul(consts), r])));
            }
            by_parts(fs, x, depth)?
        }
        _ => elementary(e, x)?,
    };
    if direct.is_some() {
        return Ok(direct);
    }
    if depth < MAX_INT_DEPTH
        && let Some(r) = crate::rational::integrate_rational(e, x, &mut |t| int(t, x, depth + 1))?
    {
        return Ok(Some(r));
    }
    // Fallbacks: expand and retry, then simplify (e.g. sin^2 + cos^2 = 1) and retry, then substitution.
    let ex = expand(e)?;
    if &ex != e
        && let Some(r) = int(&ex, x, depth)?
    {
        return Ok(Some(r));
    }
    if depth < MAX_INT_DEPTH {
        let sm = crate::simplify::simplify(e)?;
        if &sm != e
            && sm != ex
            && let Some(r) = int(&sm, x, depth + 1)?
        {
            return Ok(Some(r));
        }
    }
    if depth < MAX_INT_DEPTH { u_substitution(e, x, depth) } else { Ok(None) }
}

/// Antiderivatives of single elementary terms with a linear inner argument.
fn elementary(e: &Expr, x: &Expr) -> MathResult<Option<Expr>> {
    let lin = |u: &Expr| linear_in(u, x);
    Ok(match e.kind() {
        Kind::Sym(_) => Some(Expr::mul(vec![Expr::rational(1, 2).unwrap_or_else(Expr::one), Expr::pow(x, &Expr::int(2))])),
        Kind::Pow(b, k) if *k == Expr::int(2) && trig_arg(b).is_some() => {
            // power reduction: sin^2 u = (1 - cos 2u)/2, cos^2 u = (1 + cos 2u)/2
            let Some((is_sin, u)) = trig_arg(b) else { return Ok(None) };
            match lin(&u)? {
                Some((a, _)) => {
                    let half_u = Expr::mul(vec![Expr::half(), u.clone()]);
                    let quarter_sin =
                        Expr::mul(vec![Expr::rational(1, 4).unwrap_or_else(Expr::one), Expr::call("sin", vec![Expr::mul(vec![Expr::int(2), u])])]);
                    let anti = if is_sin { Expr::add(vec![half_u, quarter_sin.neg()]) } else { Expr::add(vec![half_u, quarter_sin]) };
                    Some(anti.div(&a))
                }
                None => None,
            }
        }
        Kind::Pow(b, k) if !k.contains(x) => {
            if let Some((a, _)) = lin(b)? {
                if *k == Expr::int(-1) {
                    Some(Expr::call("log", vec![b.clone()]).div(&a))
                } else {
                    let k1 = Expr::add(vec![k.clone(), Expr::one()]);
                    Some(Expr::pow(b, &k1).div(&Expr::mul(vec![a, k1])))
                }
            } else {
                None
            }
        }
        Kind::Pow(b, k) if b.as_sym() == Some(mathcraft_core::expr::SYM_E) => lin(k)?.map(|(a, _)| e.div(&a)),
        Kind::Pow(b, k) if !b.contains(x) => lin(k)?.map(|(a, _)| e.div(&Expr::mul(vec![a, Expr::call("log", vec![b.clone()])]))),
        Kind::Call(name, args) => match args.as_slice() {
            [u] => match lin(u)? {
                Some((a, _)) => {
                    let c = |n: &str| Expr::call(n, vec![u.clone()]);
                    let anti = match name.as_ref() {
                        "sin" => Some(c("cos").neg()),
                        "cos" => Some(c("sin")),
                        "tan" => Some(Expr::call("log", vec![c("cos")]).neg()),
                        "sinh" => Some(c("cosh")),
                        "cosh" => Some(c("sinh")),
                        "tanh" => Some(Expr::call("log", vec![c("cosh")])),
                        "log" => Some(Expr::add(vec![Expr::mul(vec![u.clone(), c("log")]), u.neg()])),
                        "atan" if u == x => Some(Expr::add(vec![
                            Expr::mul(vec![x.clone(), c("atan")]),
                            Expr::mul(vec![
                                Expr::rational(-1, 2).unwrap_or_else(Expr::one),
                                Expr::call("log", vec![Expr::add(vec![Expr::one(), Expr::pow(x, &Expr::int(2))])]),
                            ]),
                        ])),
                        "asin" if u == x => Some(Expr::add(vec![
                            Expr::mul(vec![x.clone(), c("asin")]),
                            Expr::pow(&Expr::add(vec![Expr::one(), Expr::pow(x, &Expr::int(2)).neg()]), &Expr::half()),
                        ])),
                        _ => None,
                    };
                    anti.map(|f| f.div(&a))
                }
                None => None,
            },
            _ => None,
        },
        _ => None,
    })
}

/// `(is_sin, u)` when `e` is `sin(u)` or `cos(u)`.
fn trig_arg(e: &Expr) -> Option<(bool, Expr)> {
    match e.kind() {
        Kind::Call(name, args) => match (name.as_ref(), args.as_slice()) {
            ("sin", [u]) => Some((true, u.clone())),
            ("cos", [u]) => Some((false, u.clone())),
            _ => None,
        },
        _ => None,
    }
}

/// `x**n * g(x)` where `g` is sin, cos, sinh, cosh, exp of a linear argument, or `log(x)`: integration by parts.
fn by_parts(fs: &[Expr], x: &Expr, depth: u32) -> MathResult<Option<Expr>> {
    let [a, b] = fs else { return Ok(None) };
    let (mono, g) = if power_of_x(a, x).is_some() { (a, b) } else { (b, a) };
    let Some(n) = power_of_x(mono, x) else { return Ok(None) };
    if let Kind::Call(name, args) = g.kind()
        && name.as_ref() == "log"
        && args.first() == Some(x)
        && n != -1
    {
        // ∫ x^n log x = x^(n+1)/(n+1) log x - x^(n+1)/(n+1)^2
        let n1 = Expr::int(n + 1);
        let xp = Expr::pow(x, &n1);
        let first = Expr::mul(vec![xp.clone(), g.clone()]).div(&n1);
        let second = xp.div(&Expr::pow(&n1, &Expr::int(2))).neg();
        return Ok(Some(Expr::add(vec![first, second])));
    }
    let by_parts_ok = match g.kind() {
        Kind::Call(name, _) => matches!(name.as_ref(), "sin" | "cos" | "sinh" | "cosh"),
        Kind::Pow(base, _) => base.as_sym() == Some(mathcraft_core::expr::SYM_E),
        _ => false,
    };
    if !by_parts_ok || n < 1 {
        return Ok(None);
    }
    let Some(big_g) = elementary(g, x)? else { return Ok(None) };
    // ∫ x^n g = x^n G - n ∫ x^(n-1) G
    let rest = Expr::mul(vec![Expr::pow(x, &Expr::int(n - 1)), big_g.clone()]);
    let Some(r) = int(&rest, x, depth + 1)? else { return Ok(None) };
    let total = Expr::add(vec![Expr::mul(vec![mono.clone(), big_g]), Expr::mul(vec![Expr::int(-n), r])]);
    Ok(Some(expand(&total)?))
}

fn power_of_x(e: &Expr, x: &Expr) -> Option<i64> {
    match e.kind() {
        Kind::Sym(_) if e == x => Some(1),
        Kind::Pow(b, k) if b == x => k.as_i64(),
        _ => None,
    }
}

/// Try substituting `u` for each candidate inner expression: `∫ f(u) u' dx = ∫ f(w) dw`.
fn u_substitution(e: &Expr, x: &Expr, depth: u32) -> MathResult<Option<Expr>> {
    let mut cands: Vec<Expr> = Vec::new();
    collect_inner(e, x, &mut cands);
    cands.sort_by_key(|c| std::cmp::Reverse(c.size()));
    cands.dedup();
    let w = Expr::sym("_u");
    for u in cands.into_iter().take(8) {
        let du = d(&u, x)?;
        if du.is_zero() {
            continue;
        }
        let q = subs(&e.div(&du), &[(u.clone(), w.clone())])?;
        if q.contains(x) {
            continue;
        }
        if let Some(f) = int(&q, &w, depth + 1)? {
            return Ok(Some(subs(&f, &[(w, u)])?));
        }
    }
    Ok(None)
}

fn collect_inner(e: &Expr, x: &Expr, out: &mut Vec<Expr>) {
    match e.kind() {
        Kind::Pow(b, k) => {
            if b.contains(x) && b != x {
                out.push(b.clone());
            }
            if k.contains(x) && k != x {
                out.push(k.clone());
            }
        }
        Kind::Call(_, args) => out.extend(args.iter().filter(|a| a.contains(x) && *a != x).cloned()),
        _ => {}
    }
    for c in e.children() {
        collect_inner(&c, x, out);
    }
}

/// Definite integral over `[a, b]`: exact via an antiderivative when possible, otherwise numeric for numeric limits.
pub fn integrate_definite(e: &Expr, x: &Expr, a: &Expr, b: &Expr) -> MathResult<Expr> {
    require_symbol(x)?;
    if let Some(f) = int(e, x, 0)? {
        // An endpoint where the antiderivative is not defined (oo, or a pole) is replaced by a one-sided limit.
        let end = |at: &Expr, side: crate::limits::Side| -> Option<Expr> {
            let direct = subs(&f, &[(x.clone(), at.clone())]).ok()?;
            if !has_singularity(&direct) {
                return Some(direct);
            }
            crate::limits::limit_side(&f, x, at, side).ok().filter(|v| !has_singularity(v))
        };
        if let (Some(hi), Some(lo)) = (end(b, crate::limits::Side::Left), end(a, crate::limits::Side::Right)) {
            return Ok(Expr::add(vec![hi, lo.neg()]));
        }
    }
    let none = |_: &str| None;
    match (eval_f64(a, &none), eval_f64(b, &none)) {
        (Ok(lo), Ok(hi)) => {
            let f = |t: f64| eval_f64(e, &|n| (x.as_sym() == Some(n)).then_some(t));
            Ok(Expr::float(adaptive_simpson(&f, lo, hi)?))
        }
        _ => Err(MathError::unsupported("no closed form found, and numeric integration needs numeric limits")),
    }
}

/// One Simpson interval: endpoints, the function values at both ends and the middle, and the interval's estimate.
struct Panel {
    a: f64,
    b: f64,
    fa: f64,
    fm: f64,
    fb: f64,
    whole: f64,
}

impl Panel {
    fn new(f: &dyn Fn(f64) -> MathResult<f64>, a: f64, b: f64) -> MathResult<Panel> {
        let (fa, fb, fm) = (f(a)?, f(b)?, f(0.5 * (a + b))?);
        Ok(Panel { a, b, fa, fm, fb, whole: (b - a) / 6.0 * (fa + 4.0 * fm + fb) })
    }
}

fn adaptive_simpson(f: &dyn Fn(f64) -> MathResult<f64>, a: f64, b: f64) -> MathResult<f64> {
    fn rec(f: &dyn Fn(f64) -> MathResult<f64>, p: &Panel, tol: f64, depth: u32) -> MathResult<f64> {
        let m = 0.5 * (p.a + p.b);
        let (flm, frm) = (f(0.5 * (p.a + m))?, f(0.5 * (m + p.b))?);
        let left = Panel { a: p.a, b: m, fa: p.fa, fm: flm, fb: p.fm, whole: (m - p.a) / 6.0 * (p.fa + 4.0 * flm + p.fm) };
        let right = Panel { a: m, b: p.b, fa: p.fm, fm: frm, fb: p.fb, whole: (p.b - m) / 6.0 * (p.fm + 4.0 * frm + p.fb) };
        let delta = left.whole + right.whole - p.whole;
        if depth == 0 || delta.abs() <= 15.0 * tol {
            return Ok(left.whole + right.whole + delta / 15.0);
        }
        Ok(rec(f, &left, tol / 2.0, depth - 1)? + rec(f, &right, tol / 2.0, depth - 1)?)
    }
    rec(f, &Panel::new(f, a, b)?, 1e-10, 40)
}

// ---- series and limits -------------------------------------------------------------------------------------------

/// Taylor coefficients `f^(k)(x0) / k!` for `k = 0..=order`.
fn taylor_coefficients(e: &Expr, x: &Expr, x0: &Expr, order: u32) -> MathResult<Vec<Expr>> {
    let mut out = Vec::new();
    let mut deriv = e.clone();
    let mut fact = Expr::one();
    for k in 0..=order {
        let at = expand(&subs(&deriv, &[(x.clone(), x0.clone())])?)?;
        if has_singularity(&at) {
            return Err(MathError::domain("the function is not analytic at the expansion point"));
        }
        out.push(at.div(&fact));
        deriv = diff(&deriv, x)?;
        fact = Expr::mul(vec![fact, Expr::int(i64::from(k) + 1)]);
    }
    Ok(out)
}

/// Taylor polynomial of `e` around `x = x0` up to and including `(x - x0) ** order`.
/// Quotients with a removable singularity, like `sin(x)/x`, are expanded by dividing the series of top and bottom.
pub fn series(e: &Expr, x: &Expr, x0: &Expr, order: u32) -> MathResult<Expr> {
    require_symbol(x)?;
    if order > 64 {
        return Err(MathError::too_large("series order is limited to 64"));
    }
    let coeffs = match taylor_coefficients(e, x, x0, order) {
        Ok(c) => c,
        Err(direct) => series_of_quotient(e, x, x0, order).map_err(|_| direct)?,
    };
    let dx = Expr::add(vec![x.clone(), x0.neg()]);
    Ok(Expr::add(coeffs.into_iter().enumerate().map(|(k, c)| Expr::mul(vec![c, Expr::pow(&dx, &Expr::int(k as i64))])).collect()))
}

fn series_of_quotient(e: &Expr, x: &Expr, x0: &Expr, order: u32) -> MathResult<Vec<Expr>> {
    let (num, den) = split_fraction(e);
    let extra = 16u32;
    let n = taylor_coefficients(&num, x, x0, order + extra)?;
    let d = taylor_coefficients(&den, x, x0, order + extra)?;
    let mut m = 0usize;
    while d.get(m).is_some_and(Expr::is_zero) {
        m += 1;
        if m > extra as usize {
            return Err(MathError::unsupported("the denominator vanishes to too high an order"));
        }
    }
    if n.iter().take(m).any(|c| !c.is_zero()) {
        return Err(MathError::domain("the function has a pole at the expansion point (a Laurent series is not supported yet)"));
    }
    let lead = d.get(m).cloned().ok_or_else(|| MathError::unsupported("cannot expand this quotient"))?;
    let mut q: Vec<Expr> = Vec::new();
    for i in 0..=(order as usize) {
        let mut acc = n.get(i + m).cloned().unwrap_or_else(Expr::zero);
        for j in 1..=i {
            let (dj, qj) = (d.get(j + m).cloned().unwrap_or_else(Expr::zero), q.get(i - j).cloned().unwrap_or_else(Expr::zero));
            acc = Expr::add(vec![acc, Expr::mul(vec![dj, qj]).neg()]);
        }
        q.push(expand(&acc.div(&lead))?);
    }
    Ok(q)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::limit;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn i(v: i64) -> Expr {
        Expr::int(v)
    }
    fn f(name: &str, a: Expr) -> Expr {
        Expr::call(name, vec![a])
    }
    fn p(b: &Expr, e: i64) -> Expr {
        Expr::pow(b, &i(e))
    }

    #[test]
    fn derivatives() {
        assert_eq!(diff(&p(&x(), 3), &x()).unwrap().to_string(), "3*x**2");
        assert_eq!(diff(&f("sin", x()), &x()).unwrap().to_string(), "cos(x)");
        assert_eq!(diff(&Expr::mul(vec![x(), f("sin", x())]), &x()).unwrap().to_string(), "x*cos(x) + sin(x)");
        assert_eq!(diff(&f("exp", Expr::mul(vec![i(2), x()])), &x()).unwrap().to_string(), "2*exp(2*x)");
        assert_eq!(diff(&f("log", x()), &x()).unwrap().to_string(), "1/x");
        assert_eq!(diff(&Expr::pow(&x(), &x()), &x()).unwrap().to_string(), "x**x*(log(x) + 1)");
        assert_eq!(diff(&f("sin", p(&x(), 2)), &x()).unwrap().to_string(), "2*x*cos(x**2)");
    }

    /// Antiderivatives are checked by differentiating back, which is independent of how they were found.
    #[test]
    fn integrals_differentiate_back() {
        let cases = vec![
            p(&x(), 3),
            Expr::add(vec![p(&x(), 2), i(3)]),
            f("sin", x()),
            f("cos", Expr::mul(vec![i(3), x()])),
            f("exp", x()),
            Expr::mul(vec![x(), f("exp", x())]),
            Expr::mul(vec![p(&x(), 2), f("sin", x())]),
            Expr::mul(vec![Expr::mul(vec![i(2), x()]), f("cos", p(&x(), 2))]),
            p(&Expr::add(vec![x(), i(1)]), 3),
            Expr::pow(&x(), &i(-1)),
            Expr::mul(vec![x(), p(&Expr::add(vec![p(&x(), 2), i(1)]), -1)]),
            f("log", x()),
            Expr::mul(vec![x(), f("log", x())]),
        ];
        for e in cases {
            let anti = integrate(&e, &x()).unwrap().unwrap_or_else(|| panic!("no antiderivative for {e}"));
            let back = expand(&diff(&anti, &x()).unwrap()).unwrap();
            let want = expand(&e).unwrap();
            let delta = expand(&Expr::add(vec![back.clone(), want.neg()])).unwrap();
            let ok = delta.is_zero() || {
                // trig/log forms may differ structurally; compare numerically.
                [0.3, 1.1, 2.7].iter().all(|t| {
                    let env = |n: &str| (n == "x").then_some(*t);
                    (eval_f64(&back, &env).unwrap() - eval_f64(&want, &env).unwrap()).abs() < 1e-9
                })
            };
            assert!(ok, "d/dx ∫ {e} = {back}, expected {want} (antiderivative {anti})");
        }
    }

    #[test]
    fn definite_integrals() {
        let one = integrate_definite(&p(&x(), 2), &x(), &i(0), &i(3)).unwrap();
        assert_eq!(one, i(9));
        let sin = integrate_definite(&f("sin", x()), &x(), &i(0), &Expr::pi()).unwrap();
        assert_eq!(sin, i(2));
        let gauss = integrate_definite(&f("exp", p(&x(), 2).neg()), &x(), &i(0), &i(1)).unwrap();
        assert!((gauss.as_num().unwrap().to_f64() - 0.746_824_132_812_427).abs() < 1e-8);
    }

    #[test]
    fn taylor_series() {
        let s = series(&f("exp", x()), &x(), &i(0), 3).unwrap();
        assert_eq!(expand(&s).unwrap().to_string(), "x**3/6 + x**2/2 + x + 1");
        let s = series(&f("sin", x()), &x(), &i(0), 5).unwrap();
        assert_eq!(expand(&s).unwrap().to_string(), "x**5/120 - x**3/6 + x");
    }

    #[test]
    fn limits() {
        let e = f("sin", x()).div(&x());
        assert_eq!(limit(&e, &x(), &i(0)).unwrap(), i(1));
        let e = Expr::add(vec![p(&x(), 2), i(-1)]).div(&Expr::add(vec![x(), i(-1)]));
        assert_eq!(limit(&e, &x(), &i(1)).unwrap(), i(2));
    }
}
