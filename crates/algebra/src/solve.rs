//! `solve` for one unknown: polynomials (with cubics), equations that become polynomials after a substitution
//! (`exp(2x) - 3 exp(x) + 2`, `sin(x)**2 - sin(x)`, `x - 3 sqrt(x) + 2`), and equations that can be inverted step by
//! step (`exp(x) == 5`, `2*sin(x) + 1 == 0`). Every candidate is checked against the equation numerically.

use crate::expand::expand;
use crate::poly::{solve_polynomial, sort_roots, to_poly};
use crate::rational::together;
use crate::util::{guard, has_singularity, subs};
use dashu::integer::IBig;
use mathcraft_core::numeric::eval_f64;
use mathcraft_core::{Expr, Kind, MathError, MathResult};

const MAX_DEPTH: u32 = 12;

/// Solutions of `f == 0` in the symbol `x`. Trigonometric inverses give the principal solutions (one period).
pub fn solve(f: &Expr, x: &Expr) -> MathResult<Vec<Expr>> {
    guard(f)?;
    if to_poly(f, x)?.is_some() {
        return solve_polynomial(f, x);
    }
    let (num, den) = together(f)?;
    let target = if den.contains(x) { num } else { f.clone() };
    if to_poly(&target, x)?.is_some() {
        let roots = solve_polynomial(&target, x)?;
        return Ok(filter(f, x, roots));
    }
    if let Some(g) = combine_logs(&target, x) {
        let roots = solve(&g, x)?;
        return Ok(filter(f, x, roots));
    }
    let mut roots = match isolate(&target, &Expr::zero(), x, 0)? {
        Some(r) => r,
        None => match substitute(&target, x)? {
            Some(r) => r,
            None => {
                return Err(MathError::unsupported(
                    "solve could not find a closed form for this equation (try nsolve, or nroots for all roots of a polynomial)",
                ));
            }
        },
    };
    roots = filter(f, x, roots);
    sort_roots(&mut roots);
    roots.dedup();
    Ok(roots)
}

/// `log(a) + log(b) - log(c) == 0` as the equation `a*b/c - 1 == 0`: several logarithms (with integer weights) and
/// terms without the unknown become one logarithm, `log(P) == k` is `P == exp(k)`. Roots that make an argument
/// negative are removed by the caller's check.
fn combine_logs(f: &Expr, x: &Expr) -> Option<Expr> {
    let Kind::Add(terms) = f.kind() else { return None };
    let (mut product, mut count, mut rest) = (Vec::new(), 0, Vec::new());
    for t in terms {
        let (c, body) = t.split_coeff();
        match body.kind() {
            Kind::Call(name, args) if &**name == "log" && args.len() == 1 && c.is_integer() => {
                let arg = args.first()?;
                product.push(Expr::pow(arg, &Expr::num(c)));
                count += 1;
            }
            _ => rest.push(t.clone()),
        }
    }
    if count < 2 || rest.iter().any(|t| t.contains(x)) || !product.iter().any(|p| p.contains(x)) {
        return None;
    }
    let target = Expr::call("exp", vec![Expr::add(rest).neg()]);
    Some(Expr::add(vec![Expr::mul(product), target.neg()]))
}

/// All complex roots of a polynomial with numeric coefficients, as floats (`a + b*I`), for equations that have no
/// closed form (a general quintic). Aberth's method; the roots come back sorted by real, then imaginary part.
pub fn nroots(f: &Expr, x: &Expr) -> MathResult<Vec<Expr>> {
    guard(f)?;
    let Some(coeffs) = to_poly(f, x)? else { return Err(MathError::unsupported("nroots needs a polynomial in the unknown")) };
    let mut c: Vec<f64> = Vec::new();
    for k in &coeffs {
        c.push(eval_f64(k, &|_| None).map_err(|_| MathError::unsupported("nroots needs numeric coefficients"))?);
    }
    while c.last().is_some_and(|v| *v == 0.0) {
        c.pop();
    }
    let n = c.len().saturating_sub(1);
    if n == 0 {
        return Ok(Vec::new());
    }
    // c is lowest degree first; evaluate p and p' by Horner
    let eval = |z: (f64, f64)| -> ((f64, f64), (f64, f64)) {
        let (mut p, mut dp) = ((0.0, 0.0), (0.0, 0.0));
        for a in c.iter().rev() {
            dp = (dp.0 * z.0 - dp.1 * z.1 + p.0, dp.0 * z.1 + dp.1 * z.0 + p.1);
            p = (p.0 * z.0 - p.1 * z.1 + a, p.0 * z.1 + p.1 * z.0);
        }
        (p, dp)
    };
    let div = |a: (f64, f64), b: (f64, f64)| {
        let d = b.0 * b.0 + b.1 * b.1;
        ((a.0 * b.0 + a.1 * b.1) / d, (a.1 * b.0 - a.0 * b.1) / d)
    };
    // start on a circle whose radius bounds the roots (Cauchy), at an irregular angle so no root is symmetric to it
    let lead = c.last().copied().unwrap_or(1.0).abs();
    let radius = 1.0 + c.iter().take(n).map(|v| v.abs() / lead).fold(0.0, f64::max);
    let mut z: Vec<(f64, f64)> = (0..n)
        .map(|k| {
            let t = 2.0 * std::f64::consts::PI * (k as f64) / (n as f64) + 0.4;
            (radius * 0.5 * t.cos(), radius * 0.5 * t.sin())
        })
        .collect();
    for _ in 0..500 {
        let mut moved = 0.0f64;
        for i in 0..n {
            let Some(&zi) = z.get(i) else { continue };
            let (p, dp) = eval(zi);
            if p == (0.0, 0.0) {
                continue;
            }
            let ratio = div(p, dp);
            let mut s = (0.0, 0.0);
            for (j, zj) in z.iter().enumerate() {
                if j != i {
                    let d = (zi.0 - zj.0, zi.1 - zj.1);
                    let inv = div((1.0, 0.0), d);
                    s = (s.0 + inv.0, s.1 + inv.1);
                }
            }
            // w = ratio / (1 - ratio * s)
            let denom = (1.0 - (ratio.0 * s.0 - ratio.1 * s.1), -(ratio.0 * s.1 + ratio.1 * s.0));
            let w = div(ratio, denom);
            if !w.0.is_finite() || !w.1.is_finite() {
                continue;
            }
            if let Some(slot) = z.get_mut(i) {
                *slot = (zi.0 - w.0, zi.1 - w.1);
            }
            moved = moved.max(w.0.abs() + w.1.abs());
        }
        if moved < 1e-14 * radius {
            break;
        }
    }
    let tidy = |v: f64| if v.abs() < 1e-12 { 0.0 } else { v };
    let mut out: Vec<(f64, f64)> = z.into_iter().map(|(a, b)| (tidy(a), tidy(b))).collect();
    out.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.total_cmp(&q.1)));
    Ok(out
        .into_iter()
        .map(|(a, b)| if b == 0.0 { Expr::float(a) } else { Expr::add(vec![Expr::float(a), Expr::mul(vec![Expr::float(b), Expr::imag()])]) })
        .collect())
}

/// Drop candidates that do not satisfy the equation (extra roots from squaring, poles, wrong branches).
fn filter(f: &Expr, x: &Expr, roots: Vec<Expr>) -> Vec<Expr> {
    roots.into_iter().filter(|r| satisfies(f, x, r)).collect()
}

fn satisfies(f: &Expr, x: &Expr, r: &Expr) -> bool {
    let Ok(v) = subs(f, &[(x.clone(), r.clone())]) else { return true };
    if has_singularity(&v) || !logs_positive(f, x, r) {
        return false;
    }
    match eval_f64(&v, &|_| None) {
        Ok(n) if n.is_finite() => n.abs() < 1e-6,
        Ok(_) => r.free_symbols().contains("I"),
        Err(_) => true,
    }
}

/// Every logarithm in `f` has a positive real argument at `x = r` (when the argument is a real number we can evaluate).
/// Looked at before substitution, because `log` of a negative number now folds to a complex value.
fn logs_positive(f: &Expr, x: &Expr, r: &Expr) -> bool {
    if let Kind::Call(name, args) = f.kind()
        && &**name == "log"
        && let [a] = args.as_slice()
        && subs(a, &[(x.clone(), r.clone())]).is_ok_and(|v| eval_f64(&v, &|_| None).is_ok_and(|n| n <= 0.0))
    {
        return false;
    }
    f.children().iter().all(|c| logs_positive(c, x, r))
}

fn is_negative(e: &Expr) -> Option<bool> {
    eval_f64(e, &|_| None).ok().map(|v| v < 0.0)
}

/// `y ** (1/3)` for the real cube root, also when `y` is negative.
fn real_cbrt(y: &Expr) -> Expr {
    let third = Expr::rational(1, 3).unwrap_or_else(Expr::one);
    if is_negative(y) == Some(true) { Expr::pow(&y.neg(), &third).neg() } else { Expr::pow(y, &third) }
}

// ---- cubics ------------------------------------------------------------------------------------------------------

/// Cardano's formulas for `c3 x^3 + c2 x^2 + c1 x + c0` with numeric coefficients; `None` when they are not numbers.
pub fn cubic_roots(c: &[Expr]) -> MathResult<Option<Vec<Expr>>> {
    let [d, cc, b, a] = c else { return Ok(None) };
    if ![a, b, cc, d].iter().all(|v| v.is_num()) {
        return Ok(None);
    }
    let int = Expr::int;
    let a2 = Expr::mul(vec![a.clone(), a.clone()]);
    let p = expand(
        &Expr::add(vec![Expr::mul(vec![int(3), a.clone(), cc.clone()]), Expr::mul(vec![int(-1), b.clone(), b.clone()])])
            .div(&Expr::mul(vec![int(3), a2.clone()])),
    )?;
    let q_top = Expr::add(vec![
        Expr::mul(vec![int(2), b.clone(), b.clone(), b.clone()]),
        Expr::mul(vec![int(-9), a.clone(), b.clone(), cc.clone()]),
        Expr::mul(vec![int(27), a2, d.clone()]),
    ]);
    let q = expand(&q_top.div(&Expr::mul(vec![int(27), a.clone(), a.clone(), a.clone()])))?;
    let shift = b.neg().div(&Expr::mul(vec![int(3), a.clone()]));
    let half_q = Expr::mul(vec![Expr::half(), q.clone()]);
    let third_p = Expr::mul(vec![Expr::rational(1, 3).unwrap_or_else(Expr::one), p.clone()]);
    let delta = expand(&Expr::add(vec![Expr::pow(&half_q, &int(2)), Expr::pow(&third_p, &int(3))]))?;
    let Some(dv) = eval_f64(&delta, &|_| None).ok() else { return Ok(None) };
    let mut out = Vec::new();
    if dv >= 0.0 {
        let root = Expr::pow(&delta, &Expr::half());
        let u = real_cbrt(&Expr::add(vec![half_q.neg(), root.clone()]));
        let v = real_cbrt(&Expr::add(vec![half_q.neg(), root.neg()]));
        let t1 = Expr::add(vec![u.clone(), v.clone()]);
        out.push(Expr::add(vec![t1.clone(), shift.clone()]));
        let re = Expr::add(vec![Expr::mul(vec![Expr::rational(-1, 2).unwrap_or_else(Expr::one), t1]), shift]);
        let im = Expr::mul(vec![Expr::half(), Expr::pow(&int(3), &Expr::half()), Expr::add(vec![u, v.neg()])]);
        for s in [-1, 1] {
            out.push(Expr::add(vec![re.clone(), Expr::mul(vec![int(s), Expr::imag(), im.clone()])]));
        }
    } else {
        // three real roots: trigonometric form
        let m = Expr::mul(vec![int(2), Expr::pow(&p.neg().div(&int(3)), &Expr::half())]);
        let ratio = Expr::mul(vec![int(3), q]).div(&Expr::mul(vec![int(2), p.clone()]));
        let scale = Expr::pow(&int(-3).div(&p), &Expr::half());
        let angle = Expr::call("acos", vec![Expr::mul(vec![ratio, scale])]);
        for k in 0..3 {
            let phase = Expr::add(vec![
                Expr::mul(vec![Expr::rational(1, 3).unwrap_or_else(Expr::one), angle.clone()]),
                Expr::mul(vec![Expr::rational(-2 * k, 3).unwrap_or_else(Expr::zero), Expr::pi()]),
            ]);
            out.push(Expr::add(vec![Expr::mul(vec![m.clone(), Expr::call("cos", vec![phase])]), shift.clone()]));
        }
    }
    Ok(Some(out))
}

// ---- quartics ----------------------------------------------------------------------------------------------------

/// Ferrari's method for `c4 x^4 + ... + c0` with numeric coefficients: a resolvent cubic gives a real `m > 0`, which
/// splits the depressed quartic into two quadratics. `None` when the coefficients are not numbers.
pub fn quartic_roots(c: &[Expr]) -> MathResult<Option<Vec<Expr>>> {
    let [e0, d0, c0, b0, a] = c else { return Ok(None) };
    if ![e0, d0, c0, b0, a].iter().all(|v| v.is_num()) {
        return Ok(None);
    }
    let int = Expr::int;
    let frac = |n: i64, d: i64| Expr::rational(n, d).unwrap_or_else(Expr::one);
    let (b, cc, d, e) = (expand(&b0.div(a))?, expand(&c0.div(a))?, expand(&d0.div(a))?, expand(&e0.div(a))?);
    let sq = |x: &Expr| Expr::pow(x, &int(2));
    // x = y - b/4 gives y^4 + p y^2 + q y + r
    let p = expand(&Expr::add(vec![cc.clone(), Expr::mul(vec![frac(-3, 8), sq(&b)])]))?;
    let q = expand(&Expr::add(vec![Expr::mul(vec![frac(1, 8), Expr::pow(&b, &int(3))]), Expr::mul(vec![frac(-1, 2), b.clone(), cc.clone()]), d.clone()]))?;
    let r = expand(&Expr::add(vec![
        Expr::mul(vec![frac(-3, 256), Expr::pow(&b, &int(4))]),
        Expr::mul(vec![frac(1, 16), sq(&b), cc.clone()]),
        Expr::mul(vec![frac(-1, 4), b.clone(), d]),
        e,
    ]))?;
    let shift = Expr::mul(vec![frac(-1, 4), b]);
    let back = |roots: Vec<Expr>| -> MathResult<Vec<Expr>> { roots.into_iter().map(|y| expand(&Expr::add(vec![y, shift.clone()]))).collect() };
    if q.is_zero() {
        // y^4 + p y^2 + r: solve as a quadratic in y^2
        let ys = solve_polynomial(&crate::poly::from_poly(&[r, Expr::zero(), p, Expr::zero(), Expr::one()], &Expr::sym("_y")), &Expr::sym("_y"))?;
        return back(ys).map(Some);
    }
    // resolvent: m^3 + p m^2 + (p^2/4 - r) m - q^2/8 = 0
    let resolvent =
        [expand(&Expr::mul(vec![frac(-1, 8), sq(&q)]))?, expand(&Expr::add(vec![Expr::mul(vec![frac(1, 4), sq(&p)]), r.neg()]))?, p.clone(), Expr::one()];
    let Some(candidates) = cubic_roots(&resolvent)? else { return Ok(None) };
    let Some(m) = candidates.into_iter().find(|m| eval_f64(m, &|_| None).is_ok_and(|v| v > 1e-12)) else { return Ok(None) };
    let s = Expr::pow(&Expr::mul(vec![int(2), m.clone()]), &Expr::half());
    let half_p_m = Expr::add(vec![Expr::mul(vec![Expr::half(), p]), m]);
    let t = Expr::mul(vec![q, Expr::pow(&Expr::mul(vec![int(2), s.clone()]), &int(-1))]);
    let first = [Expr::add(vec![half_p_m.clone(), t.neg()]), s.clone(), Expr::one()];
    let second = [Expr::add(vec![half_p_m, t]), s.neg(), Expr::one()];
    let mut ys = crate::poly::quadratic_roots(&first)?;
    ys.extend(crate::poly::quadratic_roots(&second)?);
    back(ys).map(Some)
}

// ---- inversion ---------------------------------------------------------------------------------------------------

/// Solve `f == rhs` by undoing the outermost operation that involves `x`. `None` when `x` is spread over several terms.
fn isolate(f: &Expr, rhs: &Expr, x: &Expr, depth: u32) -> MathResult<Option<Vec<Expr>>> {
    guard(f)?;
    if depth > MAX_DEPTH {
        return Ok(None);
    }
    if f == x {
        return Ok(Some(vec![rhs.clone()]));
    }
    if !f.contains(x) {
        return Ok(None);
    }
    let rec = |g: &Expr, r: &Expr| isolate(g, r, x, depth + 1);
    match f.kind() {
        Kind::Add(terms) => {
            let (with, without): (Vec<&Expr>, Vec<&Expr>) = terms.iter().partition(|t| t.contains(x));
            let [one] = with[..] else { return Ok(None) };
            let rest = Expr::add(without.into_iter().cloned().collect());
            rec(one, &Expr::add(vec![rhs.clone(), rest.neg()]))
        }
        Kind::Mul(factors) => {
            let (with, without): (Vec<&Expr>, Vec<&Expr>) = factors.iter().partition(|t| t.contains(x));
            let rest = Expr::mul(without.into_iter().cloned().collect());
            if rhs.is_zero() {
                // a product is zero when one factor is; poles are removed by the final check
                let mut all = Vec::new();
                for g in with {
                    if let Kind::Pow(_, k) = g.kind()
                        && k.as_num().is_some_and(|n| n.is_negative())
                    {
                        continue;
                    }
                    match rec(g, rhs)? {
                        Some(r) => all.extend(r),
                        None => return Ok(None),
                    }
                }
                return Ok(Some(all));
            }
            let [one] = with[..] else { return Ok(None) };
            rec(one, &rhs.div(&rest))
        }
        Kind::Pow(b, e) => match (b.contains(x), e.contains(x)) {
            (false, true) => {
                if rhs.is_zero() {
                    return Ok(Some(Vec::new()));
                }
                let lg = match is_negative(rhs) {
                    Some(true) => Expr::add(vec![Expr::call("log", vec![rhs.neg()]), Expr::mul(vec![Expr::imag(), Expr::pi()])]),
                    _ => Expr::call("log", vec![rhs.clone()]),
                };
                let target = if b.as_sym() == Some(mathcraft_core::expr::SYM_E) { lg } else { lg.div(&Expr::call("log", vec![b.clone()])) };
                // 2**x == 8 is exactly 3, not log(8)/log(2)
                let exact = eval_f64(&target, &|_| None)
                    .ok()
                    .map(f64::round)
                    .filter(|k| k.abs() < 1e6)
                    .map(|k| Expr::int(k as i64))
                    .filter(|k| &Expr::pow(b, k) == rhs);
                rec(e, &exact.unwrap_or(target))
            }
            (true, false) => {
                let Some(n) = e.as_num().filter(|n| n.is_exact()) else { return Ok(None) };
                if n.is_zero() {
                    return Ok(None);
                }
                let Some(inv) = n.recip() else { return Ok(None) };
                let root = Expr::pow(rhs, &Expr::num(inv));
                let even = n.parts().is_some_and(|(p, _)| p % IBig::from(2) == IBig::ZERO);
                let mut all = Vec::new();
                for g in if even { vec![root.clone(), root.neg()] } else { vec![root] } {
                    if let Some(r) = rec(b, &g)? {
                        all.extend(r);
                    }
                }
                Ok(Some(all))
            }
            _ => Ok(None),
        },
        Kind::Call(name, args) => {
            let [arg] = &args[..] else { return Ok(None) };
            let int = Expr::int;
            let inverses: Vec<Expr> = match &**name {
                "log" => vec![Expr::pow(&Expr::e(), rhs)],
                "sin" => vec![Expr::call("asin", vec![rhs.clone()]), Expr::pi().sub(&Expr::call("asin", vec![rhs.clone()]))],
                "cos" => vec![Expr::call("acos", vec![rhs.clone()]), Expr::call("acos", vec![rhs.clone()]).neg()],
                "tan" => vec![Expr::call("atan", vec![rhs.clone()])],
                "asin" => vec![Expr::call("sin", vec![rhs.clone()])],
                "acos" => vec![Expr::call("cos", vec![rhs.clone()])],
                "atan" => vec![Expr::call("tan", vec![rhs.clone()])],
                "sinh" => {
                    vec![Expr::call("log", vec![Expr::add(vec![rhs.clone(), Expr::pow(&Expr::add(vec![Expr::pow(rhs, &int(2)), int(1)]), &Expr::half())])])]
                }
                "cosh" => {
                    let l = Expr::call("log", vec![Expr::add(vec![rhs.clone(), Expr::pow(&Expr::add(vec![Expr::pow(rhs, &int(2)), int(-1)]), &Expr::half())])]);
                    vec![l.clone(), l.neg()]
                }
                "tanh" => {
                    vec![Expr::mul(vec![Expr::half(), Expr::call("log", vec![Expr::add(vec![int(1), rhs.clone()]).div(&Expr::add(vec![int(1), rhs.neg()]))])])]
                }
                "abs" => {
                    if is_negative(rhs) == Some(true) {
                        return Ok(Some(Vec::new()));
                    }
                    vec![rhs.clone(), rhs.neg()]
                }
                _ => return Ok(None),
            };
            let mut all = Vec::new();
            for inv in inverses {
                if let Some(r) = rec(arg, &inv)? {
                    all.extend(r);
                }
            }
            Ok(Some(all))
        }
        _ => Ok(None),
    }
}

// ---- substitution ------------------------------------------------------------------------------------------------

/// Try to see the equation as a polynomial in some subexpression `g(x)` and then solve `g(x) == root`.
fn substitute(f: &Expr, x: &Expr) -> MathResult<Option<Vec<Expr>>> {
    let u = Expr::sym("_u");
    // x ** (p/q) with q > 1: x = u ** q
    let mut q = 1i64;
    collect_fraction_powers(f, x, &mut q);
    if q > 1 {
        let swapped = replace_powers(f, x, &u, q);
        if swapped.contains(x) {
            return Ok(None);
        }
        if to_poly(&swapped, &u)?.is_some_and(|c| c.len() > 2) {
            let mut all = Vec::new();
            for r in solve_polynomial(&swapped, &u)? {
                if is_negative(&r) != Some(true) {
                    all.push(Expr::pow(&r, &Expr::int(q)));
                }
            }
            return Ok(Some(all));
        }
    }
    let mut atoms = Vec::new();
    collect_atoms(f, x, &mut atoms);
    atoms.sort_by_key(Expr::size);
    atoms.dedup();
    for g in atoms {
        let replaced = match exp_step(&g) {
            Some(step) => replace_exp(f, x, &step, &u),
            None => Some(subs(f, &[(g.clone(), u.clone())])?),
        };
        let Some(replaced) = replaced else { continue };
        if replaced.contains(x) {
            continue;
        }
        if !to_poly(&replaced, &u)?.is_some_and(|c| c.len() > 2) {
            continue;
        }
        let target = exp_step(&g).map_or(g.clone(), |s| Expr::pow(&Expr::e(), &Expr::mul(vec![s, x.clone()])));
        let mut all = Vec::new();
        for r in solve_polynomial(&replaced, &u)? {
            if let Some(sol) = isolate(&target, &r, x, 1)? {
                all.extend(sol);
            }
        }
        return Ok(Some(all));
    }
    Ok(None)
}

/// Rewrite `x ** k` as `u ** (k q)` and `x` as `u ** q`.
fn replace_powers(e: &Expr, x: &Expr, u: &Expr, q: i64) -> Expr {
    if e == x {
        return Expr::pow(u, &Expr::int(q));
    }
    if let Kind::Pow(b, k) = e.kind()
        && b == x
    {
        return Expr::pow(u, &Expr::mul(vec![k.clone(), Expr::int(q)]));
    }
    let kids = e.children();
    if kids.is_empty() { e.clone() } else { e.rebuild(kids.iter().map(|c| replace_powers(c, x, u, q)).collect()) }
}

fn collect_fraction_powers(e: &Expr, x: &Expr, q: &mut i64) {
    if let Kind::Pow(b, k) = e.kind()
        && b == x
        && let Some(n) = k.as_num()
        && let Some((_, den)) = n.parts()
        && let Ok(den) = i64::try_from(den)
        && den > 1
    {
        *q = lcm(*q, den);
    }
    for c in e.children() {
        collect_fraction_powers(&c, x, q);
    }
}

fn lcm(a: i64, b: i64) -> i64 {
    fn gcd(a: i64, b: i64) -> i64 {
        if b == 0 { a } else { gcd(b, a % b) }
    }
    a / gcd(a, b).max(1) * b
}

/// Function calls and powers of `e` that involve `x`, as substitution candidates.
fn collect_atoms(e: &Expr, x: &Expr, out: &mut Vec<Expr>) {
    if !e.contains(x) {
        return;
    }
    let candidate = match e.kind() {
        Kind::Call(..) => true,
        Kind::Pow(b, k) => b.as_sym() == Some(mathcraft_core::expr::SYM_E) && k.contains(x),
        _ => false,
    };
    if candidate {
        out.push(e.clone());
    }
    for c in e.children() {
        collect_atoms(&c, x, out);
    }
}

/// For `exp(c*x)`: the coefficient `c`.
fn exp_step(g: &Expr) -> Option<Expr> {
    match g.kind() {
        Kind::Pow(b, k) if b.as_sym() == Some(mathcraft_core::expr::SYM_E) => {
            let (c, _) = k.split_coeff();
            Some(Expr::num(c))
        }
        _ => None,
    }
}

/// Rewrite `exp(n*step*x)` as `u ** n` throughout; `None` when some exponent is not a whole multiple of `step`.
fn replace_exp(e: &Expr, x: &Expr, step: &Expr, u: &Expr) -> Option<Expr> {
    if !e.contains(x) {
        return Some(e.clone());
    }
    if let Kind::Pow(b, k) = e.kind()
        && b.as_sym() == Some(mathcraft_core::expr::SYM_E)
    {
        let (c, rest) = k.split_coeff();
        if &rest != x {
            return None;
        }
        let ratio = c.to_f64() / step.as_num()?.to_f64();
        let n = ratio.round();
        if (ratio - n).abs() > 1e-9 || n.abs() > 64.0 || n == 0.0 {
            return None;
        }
        return Some(Expr::pow(u, &Expr::int(n as i64)));
    }
    let kids = e.children();
    if kids.is_empty() {
        return Some(e.clone());
    }
    let mapped: Option<Vec<Expr>> = kids.iter().map(|c| replace_exp(c, x, step, u)).collect();
    Some(e.rebuild(mapped?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: Expr) -> Vec<String> {
        solve(&f, &Expr::sym("x")).unwrap().iter().map(|r| r.to_string()).collect()
    }
    fn x() -> Expr {
        Expr::sym("x")
    }

    #[test]
    fn logarithm_rules_combine_and_domains_are_checked() {
        let lg = |e: Expr| Expr::call("log", vec![e]);
        let x1 = Expr::add(vec![x(), Expr::int(-1)]);
        // log(x) + log(x - 1) = log(2): x = 2 (x = -1 would need the log of a negative number)
        let f = Expr::add(vec![lg(x()), lg(x1), lg(Expr::int(2)).neg()]);
        assert_eq!(run(f), ["2"]);
        // 2 log(x) = log(4x - 3): x = 1 or 3
        let g = Expr::add(vec![Expr::mul(vec![Expr::int(2), lg(x())]), lg(Expr::add(vec![Expr::mul(vec![Expr::int(4), x()]), Expr::int(-3)])).neg()]);
        assert_eq!(run(g), ["1", "3"]);
    }

    #[test]
    fn nroots_finds_every_root_of_a_quintic() {
        // x^5 - x - 1 has no closed form: one real root near 1.1673 and four complex ones
        let f = Expr::add(vec![Expr::pow(&x(), &Expr::int(5)), x().neg(), Expr::int(-1)]);
        let roots = nroots(&f, &x()).unwrap();
        assert_eq!(roots.len(), 5);
        for r in &roots {
            let v = subs(&f, &[(x(), r.clone())]).unwrap();
            let (re, im) = complex_parts(&v);
            assert!(re.abs() < 1e-9 && im.abs() < 1e-9, "{r} gives {v}");
        }
        assert!(roots.iter().any(|r| (eval_f64(r, &|_| None).unwrap_or(0.0) - 1.1673).abs() < 1e-3));
        assert!(solve(&f, &x()).is_err(), "no closed form, so solve says so");
    }

    fn complex_parts(e: &Expr) -> (f64, f64) {
        let (re, im) = crate::complex::parts(e).unwrap();
        (eval_f64(&re, &|_| None).unwrap_or(f64::NAN), eval_f64(&im, &|_| None).unwrap_or(f64::NAN))
    }

    #[test]
    fn quartics_have_correct_real_roots() {
        // x^4 - x - 1 has two real roots, about 1.2207 and -0.7245
        let f = Expr::add(vec![Expr::pow(&x(), &Expr::int(4)), x().neg(), Expr::int(-1)]);
        let roots = solve(&f, &x()).unwrap();
        assert_eq!(roots.len(), 4);
        let mut real: Vec<f64> = roots.iter().filter_map(|r| eval_f64(r, &|_| None).ok()).collect();
        real.sort_by(f64::total_cmp);
        assert!(real.len() >= 2, "{roots:?}");
        let g = |v: f64| v.powi(4) - v - 1.0;
        assert!(real.iter().all(|v| g(*v).abs() < 1e-9), "{real:?}");
        assert!((real[0] + 0.7245).abs() < 1e-3 && (real[1] - 1.2207).abs() < 1e-3, "{real:?}");
        // a quartic with q != 0 whose roots are all real: (x-1)(x-2)(x-3)(x-5) is found by rational roots, so use
        // x^4 + 2x^3 - 7x^2 - 8x + 3 (no rational roots)
        let h = Expr::add(vec![
            Expr::pow(&x(), &Expr::int(4)),
            Expr::mul(vec![Expr::int(2), Expr::pow(&x(), &Expr::int(3))]),
            Expr::mul(vec![Expr::int(-7), Expr::pow(&x(), &Expr::int(2))]),
            Expr::mul(vec![Expr::int(-8), x()]),
            Expr::int(3),
        ]);
        let roots = solve(&h, &x()).unwrap();
        let vals: Vec<f64> = roots.iter().filter_map(|r| eval_f64(r, &|_| None).ok()).collect();
        assert!(vals.len() >= 2 && vals.iter().all(|v| (v.powi(4) + 2.0 * v.powi(3) - 7.0 * v * v - 8.0 * v + 3.0).abs() < 1e-8), "{vals:?}");
    }

    #[test]
    fn inverts_exponentials_and_logs() {
        assert_eq!(run(Expr::call("exp", vec![x()]).sub(&Expr::int(5))), vec!["log(5)"]);
        assert_eq!(run(Expr::call("log", vec![x()]).sub(&Expr::int(2))), vec!["exp(2)"]);
    }

    #[test]
    fn splits_products_and_drops_poles() {
        let f = Expr::mul(vec![x(), Expr::call("sin", vec![x()])]);
        assert!(run(f).contains(&"0".to_string()));
    }

    #[test]
    fn substitution_finds_quadratics_in_exp_and_sin() {
        let ex = Expr::call("exp", vec![x()]);
        let f = Expr::add(vec![Expr::pow(&ex, &Expr::int(2)), Expr::mul(vec![Expr::int(-3), ex]), Expr::int(2)]);
        assert_eq!(run(f), vec!["0", "log(2)"]);
        let s = Expr::call("sin", vec![x()]);
        let g = Expr::add(vec![Expr::pow(&s, &Expr::int(2)), s.neg()]);
        assert_eq!(run(g), vec!["0", "pi/2", "pi"]);
    }

    #[test]
    fn sqrt_substitution_rejects_extra_roots() {
        let f = Expr::add(vec![x(), Expr::mul(vec![Expr::int(-3), Expr::pow(&x(), &Expr::half())]), Expr::int(2)]);
        assert_eq!(run(f), vec!["1", "4"]);
        // sqrt(x) = -1 has no real solution
        assert!(run(Expr::add(vec![Expr::pow(&x(), &Expr::half()), Expr::one()])).is_empty());
    }
}
