//! Differentiation, integration, series and limits.

use crate::expand::expand;
use crate::poly::linear_in;
use crate::util::{guard, has_singularity, require_symbol, sign_of, split_fraction, subs};
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
        "asinh" => Expr::pow(&Expr::add(vec![one, Expr::pow(u, &two)]), &Expr::rational(-1, 2)?),
        "acosh" => Expr::pow(&Expr::add(vec![Expr::pow(u, &two), Expr::int(-1)]), &Expr::rational(-1, 2)?),
        "atanh" => Expr::pow(&one_minus_u2(), &Expr::int(-1)),
        "erf" => Expr::mul(vec![Expr::int(2), Expr::pow(&Expr::pi(), &Expr::rational(-1, 2)?), Expr::pow(&Expr::e(), &Expr::pow(u, &two).neg())]),
        "LambertW" => c("LambertW").div(&Expr::mul(vec![u.clone(), Expr::add(vec![one.clone(), c("LambertW")])])),
        "Si" => c("sin").div(u),
        "Ci" => c("cos").div(u),
        "Ei" => c("exp").div(u),
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
            match sinc(fs, x)? {
                Some(r) => Some(r),
                None => by_parts(fs, x, depth)?,
            }
        }
        _ => elementary(e, x)?,
    };
    if direct.is_some() {
        return Ok(direct);
    }
    if depth < MAX_INT_DEPTH
        && let Some(r) = families(e, x, depth)?
    {
        return Ok(Some(r));
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

/// `sin(a*x)/x` is `Si(a*x)`, `cos(a*x)/x` is `Ci(a*x)` (for `a > 0`) and `exp(a*x)/x` is `Ei(a*x)`.
fn sinc(fs: &[Expr], x: &Expr) -> MathResult<Option<Expr>> {
    let [a, b] = fs else { return Ok(None) };
    let inverse = Expr::pow(x, &Expr::int(-1));
    let s = if b == &inverse {
        a
    } else if a == &inverse {
        b
    } else {
        return Ok(None);
    };
    let (kind, u) = match s.kind() {
        Kind::Call(name, args) if matches!(&**name, "sin" | "cos") => match args.as_slice() {
            [u] => (&**name, u.clone()),
            _ => return Ok(None),
        },
        Kind::Pow(base, u) if base.as_sym() == Some(mathcraft_core::expr::SYM_E) => ("exp", u.clone()),
        _ => return Ok(None),
    };
    let Some((slope, shift)) = linear_in(&u, x)? else { return Ok(None) };
    if !shift.is_zero() {
        return Ok(None);
    }
    let name = match kind {
        "sin" => "Si",
        "exp" => "Ei",
        _ if sign_of(&slope) == Some(1) => "Ci",
        _ => return Ok(None),
    };
    Ok(Some(Expr::call(name, vec![u])))
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

// ---- recognised families -----------------------------------------------------------------------------------------

/// Integrals of whole families: powers of sin and cos, exp times sin or cos, `|u|`, hyperbolic functions (through
/// exponentials), square roots of quadratics (inverse trigonometric and hyperbolic results) and Gaussians (erf).
fn families(e: &Expr, x: &Expr, depth: u32) -> MathResult<Option<Expr>> {
    if let Some(r) = trig_powers(e, x)? {
        return Ok(Some(r));
    }
    if let Some(r) = exp_trig(e, x)? {
        return Ok(Some(r));
    }
    if let Some(r) = quadratic_root(e, x)? {
        return Ok(Some(r));
    }
    if let Some(r) = gaussian(e, x)? {
        return Ok(Some(r));
    }
    if let Kind::Call(name, args) = e.kind()
        && name.as_ref() == "abs"
        && let [u] = args.as_slice()
        && let Some((a, _)) = linear_in(u, x)?
    {
        return Ok(Some(Expr::mul(vec![u.clone(), e.clone()]).div(&Expr::mul(vec![Expr::int(2), a]))));
    }
    if has_call(e, &["sinh", "cosh"]) && !has_call(e, &["tanh"]) {
        let rewritten = expand(&hyperbolic_to_exp(e)?)?;
        if &rewritten != e {
            return int(&rewritten, x, depth + 1);
        }
    }
    Ok(None)
}

fn has_call(e: &Expr, names: &[&str]) -> bool {
    match e.kind() {
        Kind::Call(n, _) if names.contains(&n.as_ref()) => true,
        _ => e.children().iter().any(|c| has_call(c, names)),
    }
}

fn hyperbolic_to_exp(e: &Expr) -> MathResult<Expr> {
    if let Kind::Call(name, args) = e.kind()
        && let [u] = args.as_slice()
        && matches!(name.as_ref(), "sinh" | "cosh")
    {
        let (p, m) = (Expr::pow(&Expr::e(), u), Expr::pow(&Expr::e(), &u.neg()));
        let second = if name.as_ref() == "sinh" { m.neg() } else { m };
        return Ok(Expr::add(vec![p, second]).div(&Expr::int(2)));
    }
    e.map_children(hyperbolic_to_exp)
}

/// `sin(u)**m * cos(u)**n` for a linear `u` and whole `m, n >= 0` with `m + n >= 2` (and `1/cos**2`, `1/sin**2`).
fn trig_powers(e: &Expr, x: &Expr) -> MathResult<Option<Expr>> {
    let factors: Vec<Expr> = match e.kind() {
        Kind::Mul(fs) => fs.clone(),
        _ => vec![e.clone()],
    };
    let (mut m, mut n, mut arg) = (0i64, 0i64, None::<Expr>);
    for f in &factors {
        let (base, k) = match f.kind() {
            Kind::Pow(b, k) => (b.clone(), k.as_i64()),
            _ => (f.clone(), Some(1)),
        };
        let (Some((is_sin, u)), Some(k)) = (trig_arg(&base), k) else { return Ok(None) };
        if arg.get_or_insert_with(|| u.clone()) != &u {
            return Ok(None);
        }
        if k == -2 && factors.len() == 1 {
            let Some((a, _)) = linear_in(&u, x)? else { return Ok(None) };
            let anti = if is_sin { Expr::call("cos", vec![u.clone()]).div(&Expr::call("sin", vec![u])).neg() } else { Expr::call("tan", vec![u]) };
            return Ok(Some(anti.div(&a)));
        }
        if k < 1 {
            return Ok(None);
        }
        if is_sin { m += k } else { n += k }
    }
    let Some(u) = arg else { return Ok(None) };
    if m + n < 2 || m + n > 40 {
        return Ok(None);
    }
    let Some((a, _)) = linear_in(&u, x)? else { return Ok(None) };
    Ok(Some(trig_antiderivative(m, n, &u)?.div(&a)))
}

/// An antiderivative with respect to `u` of `sin(u)**m * cos(u)**n`.
fn trig_antiderivative(m: i64, n: i64, u: &Expr) -> MathResult<Expr> {
    guard(u)?;
    let (s, c) = (Expr::call("sin", vec![u.clone()]), Expr::call("cos", vec![u.clone()]));
    let frac = |p: i64, q: i64| Expr::rational(p, q).unwrap_or_else(Expr::one);
    if m == 0 && n == 0 {
        return Ok(u.clone());
    }
    // an odd power peels one factor off: substitute w = the other function and integrate a polynomial in w
    let poly_in = |w_power: i64, k: i64, at: &Expr, sign: i64| -> MathResult<Expr> {
        let w = Expr::sym("_w");
        let p = expand(&Expr::mul(vec![
            Expr::pow(&w, &Expr::int(w_power)),
            Expr::pow(&Expr::add(vec![Expr::one(), Expr::pow(&w, &Expr::int(2)).neg()]), &Expr::int(k)),
        ]))?;
        let coeffs = crate::poly::to_poly(&p, &w)?.unwrap_or_default();
        let anti = Expr::add(
            coeffs.iter().enumerate().map(|(j, cj)| Expr::mul(vec![cj.clone(), Expr::pow(&w, &Expr::int(j as i64 + 1)), frac(1, j as i64 + 1)])).collect(),
        );
        Ok(Expr::mul(vec![Expr::int(sign), subs(&anti, &[(w, at.clone())])?]))
    };
    if n % 2 == 1 {
        return poly_in(m, (n - 1) / 2, &s, 1);
    }
    if m % 2 == 1 {
        return poly_in(n, (m - 1) / 2, &c, -1);
    }
    if n == 0 {
        let first = Expr::mul(vec![Expr::pow(&s, &Expr::int(m - 1)), c, frac(-1, m)]);
        return Ok(Expr::add(vec![first, Expr::mul(vec![frac(m - 1, m), trig_antiderivative(m - 2, 0, u)?])]));
    }
    if m == 0 {
        let first = Expr::mul(vec![Expr::pow(&c, &Expr::int(n - 1)), s, frac(1, n)]);
        return Ok(Expr::add(vec![first, Expr::mul(vec![frac(n - 1, n), trig_antiderivative(0, n - 2, u)?])]));
    }
    // both even, both positive: sin^m cos^n = sin^m cos^(n-2) - sin^(m+2) cos^(n-2)
    Ok(Expr::add(vec![trig_antiderivative(m, n - 2, u)?, trig_antiderivative(m + 2, n - 2, u)?.neg()]))
}

/// `exp(p) * sin(q)` and `exp(p) * cos(q)` with linear `p` and `q`.
fn exp_trig(e: &Expr, x: &Expr) -> MathResult<Option<Expr>> {
    let Kind::Mul(fs) = e.kind() else { return Ok(None) };
    let [f, g] = fs.as_slice() else { return Ok(None) };
    let (ex, tr) = if matches!(f.kind(), Kind::Pow(b, _) if b.as_sym() == Some(mathcraft_core::expr::SYM_E)) { (f, g) } else { (g, f) };
    let (Kind::Pow(b, p), Some((is_sin, q))) = (ex.kind(), trig_arg(tr)) else { return Ok(None) };
    if b.as_sym() != Some(mathcraft_core::expr::SYM_E) {
        return Ok(None);
    }
    let (Some((a, _)), Some((bb, _))) = (linear_in(p, x)?, linear_in(&q, x)?) else { return Ok(None) };
    let (s, c) = (Expr::call("sin", vec![q.clone()]), Expr::call("cos", vec![q]));
    let inner = if is_sin {
        Expr::add(vec![Expr::mul(vec![a.clone(), s]), Expr::mul(vec![bb.clone(), c]).neg()])
    } else {
        Expr::add(vec![Expr::mul(vec![a.clone(), c]), Expr::mul(vec![bb.clone(), s])])
    };
    let norm = Expr::add(vec![Expr::pow(&a, &Expr::int(2)), Expr::pow(&bb, &Expr::int(2))]);
    Ok(Some(Expr::mul(vec![ex.clone(), inner]).div(&norm)))
}

/// A quadratic `A*x**2 + B*x + C` with rational coefficients, as `(A, h, D)` with `q = A*(x + h)**2 + D`.
fn complete_square(q: &Expr, x: &Expr) -> MathResult<Option<(Expr, Expr, Expr)>> {
    let Some(c) = crate::poly::to_poly(q, x)? else { return Ok(None) };
    let [c0, c1, c2] = c.as_slice() else { return Ok(None) };
    if [c0, c1, c2].iter().any(|t| t.as_num().is_none_or(|n| !n.is_exact())) || c2.is_zero() {
        return Ok(None);
    }
    let h = c1.div(&Expr::mul(vec![Expr::int(2), c2.clone()]));
    let d = expand(&Expr::add(vec![c0.clone(), Expr::mul(vec![c2.clone(), Expr::pow(&h, &Expr::int(2))]).neg()]))?;
    Ok(Some((c2.clone(), h, d)))
}

/// `1/sqrt(q)` and `sqrt(q)` for a quadratic `q`.
fn quadratic_root(e: &Expr, x: &Expr) -> MathResult<Option<Expr>> {
    let Kind::Pow(q, k) = e.kind() else { return Ok(None) };
    let inverse = if *k == Expr::rational(-1, 2).unwrap_or_else(Expr::zero) {
        true
    } else if *k == Expr::half() {
        false
    } else {
        return Ok(None);
    };
    let Some((a, h, d)) = complete_square(q, x)? else { return Ok(None) };
    let (Some(sa), Some(sd)) = (sign_of(&a), sign_of(&d)) else { return Ok(None) };
    let v = Expr::add(vec![x.clone(), h]);
    let sqrt = |t: &Expr| Expr::pow(t, &Expr::half());
    let inv = match (sa, sd) {
        (1, 1) => Expr::call("asinh", vec![Expr::mul(vec![v.clone(), sqrt(&a.div(&d))])]).div(&sqrt(&a)),
        (1, -1) => Expr::call("log", vec![Expr::add(vec![Expr::mul(vec![sqrt(&a), v.clone()]), sqrt(q)])]).div(&sqrt(&a)),
        (-1, 1) => Expr::call("asin", vec![Expr::mul(vec![v.clone(), sqrt(&a.neg().div(&d))])]).div(&sqrt(&a.neg())),
        _ => return Ok(None),
    };
    if inverse {
        return Ok(Some(inv));
    }
    // ∫ sqrt(q) = v*sqrt(q)/2 + D/2 * ∫ 1/sqrt(q)
    Ok(Some(Expr::add(vec![Expr::mul(vec![v, sqrt(q), Expr::half()]), Expr::mul(vec![d, Expr::half(), inv])])))
}

/// `exp(-a*(x + h)**2 + D)` with a positive rational `a`: a multiple of `erf`.
fn gaussian(e: &Expr, x: &Expr) -> MathResult<Option<Expr>> {
    let Kind::Pow(b, k) = e.kind() else { return Ok(None) };
    if b.as_sym() != Some(mathcraft_core::expr::SYM_E) {
        return Ok(None);
    }
    let Some((a, h, d)) = complete_square(k, x)? else { return Ok(None) };
    if sign_of(&a) != Some(-1) {
        return Ok(None);
    }
    let r = Expr::pow(&a.neg(), &Expr::half());
    let v = Expr::add(vec![x.clone(), h]);
    let scale = Expr::mul(vec![Expr::pow(&Expr::e(), &d), Expr::pow(&Expr::pi(), &Expr::half()), Expr::half()]).div(&r);
    Ok(Some(Expr::mul(vec![scale, Expr::call("erf", vec![Expr::mul(vec![r, v])])])))
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
    let (shift, coeffs) = match taylor_coefficients(e, x, x0, order) {
        Ok(c) => (0, c),
        Err(direct) => series_of_quotient(e, x, x0, order).map_err(|_| direct)?,
    };
    let dx = Expr::add(vec![x.clone(), x0.neg()]);
    Ok(Expr::add(coeffs.into_iter().enumerate().map(|(k, c)| Expr::mul(vec![c, Expr::pow(&dx, &Expr::int(k as i64 + shift))])).collect()))
}

/// Index of the first nonzero coefficient, or `None` when all of them are zero.
fn valuation(c: &[Expr]) -> Option<usize> {
    c.iter().position(|v| !v.is_zero())
}

/// Series of a quotient by dividing the series of top and bottom. Returns the power of `(x - x0)` of the first
/// coefficient (negative for a pole, which gives a Laurent series) and the coefficients up to `(x - x0) ** order`.
fn series_of_quotient(e: &Expr, x: &Expr, x0: &Expr, order: u32) -> MathResult<(i64, Vec<Expr>)> {
    let (num, den) = split_fraction(e);
    let probe = 12u32;
    let (n0, d0) = (taylor_coefficients(&num, x, x0, probe)?, taylor_coefficients(&den, x, x0, probe)?);
    let k = valuation(&n0).ok_or_else(|| MathError::unsupported("cannot expand this quotient"))?;
    let m = valuation(&d0).ok_or_else(|| MathError::unsupported("the denominator vanishes to too high an order"))?;
    let shift = k as i64 - m as i64;
    let count = i64::from(order) - shift + 1;
    if count <= 0 {
        return Ok((0, vec![Expr::zero()]));
    }
    let count = count as usize;
    let n = taylor_coefficients(&num, x, x0, (k + count) as u32)?;
    let d = taylor_coefficients(&den, x, x0, (m + count) as u32)?;
    let lead = d.get(m).cloned().ok_or_else(|| MathError::unsupported("cannot expand this quotient"))?;
    let mut q: Vec<Expr> = Vec::new();
    for i in 0..count {
        let mut acc = n.get(i + k).cloned().unwrap_or_else(Expr::zero);
        for j in 1..=i {
            let (dj, qj) = (d.get(j + m).cloned().unwrap_or_else(Expr::zero), q.get(i - j).cloned().unwrap_or_else(Expr::zero));
            acc = Expr::add(vec![acc, Expr::mul(vec![dj, qj]).neg()]);
        }
        q.push(expand(&acc.div(&lead))?);
    }
    Ok((shift, q))
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
        // exact now: sqrt(pi)*erf(1)/2
        assert!((eval_f64(&gauss, &|_| None).unwrap() - 0.746_824_132_812_427).abs() < 1e-8);
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
