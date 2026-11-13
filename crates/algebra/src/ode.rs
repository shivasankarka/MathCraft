//! Ordinary differential equations: first order linear, separable, Bernoulli and homogeneous (`y' = F(y/x)`), and
//! linear with constant coefficients.
//!
//! The unknown `y(x)` and its derivatives (`diff(y(x), x)`, ...) are lifted to plain symbols, the equation is
//! classified on those, and the solution is returned as `y(x) == ...` with arbitrary constants `C1`, `C2`, ...

use crate::calculus::{diff, integrate};
use crate::complex::parts;
use crate::expand::expand;
use crate::poly::{factor, to_poly};
use crate::simplify::simplify;
use crate::solve::solve;
use crate::system::solve_system;
use crate::util::{is_zero_expr, require_symbol, subs};
use mathcraft_core::{Expr, Kind, MathError, MathResult};

/// The highest derivative order handled.
const MAX_ORDER: usize = 8;

fn lifted_symbol(k: usize) -> Expr {
    Expr::sym(&format!("_y{k}"))
}

fn constant(k: usize) -> Expr {
    Expr::sym(&format!("C{k}"))
}

fn unsupported<T>(what: &str) -> MathResult<T> {
    Err(MathError::unsupported(format!("dsolve cannot solve this equation: {what}")))
}

/// `Some(k)` when `e` is the `k`-th derivative of `f` with respect to `x`.
fn derivative_order(e: &Expr, f: &Expr, x: &Expr) -> Option<usize> {
    if e == f {
        return Some(0);
    }
    if let Kind::Call(name, args) = e.kind()
        && &**name == "diff"
        && let [inner, v] = args.as_slice()
        && v == x
    {
        return derivative_order(inner, f, x).map(|k| k + 1);
    }
    None
}

/// Replaces `f` and its derivatives by the symbols `_y0`, `_y1`, ...
fn lift(e: &Expr, f: &Expr, x: &Expr) -> MathResult<Expr> {
    if let Some(k) = derivative_order(e, f, x) {
        return if k <= MAX_ORDER { Ok(lifted_symbol(k)) } else { unsupported("the order is too high") };
    }
    e.map_children(|c| lift(c, f, x))
}

/// Splits `expr` as `sum(a_k * Y_k) + rest` for the lifted symbols `Y_0..Y_n`; `None` when it is not linear in them.
fn linear_parts(expr: &Expr, order: usize) -> MathResult<Option<(Vec<Expr>, Expr)>> {
    let mut rest = expr.clone();
    let mut coeffs = vec![Expr::zero(); order + 1];
    for k in (0..=order).rev() {
        let Some(c) = to_poly(&rest, &lifted_symbol(k))? else { return Ok(None) };
        if c.len() > 2 {
            return Ok(None);
        }
        if let Some(slot) = coeffs.get_mut(k) {
            *slot = c.get(1).cloned().unwrap_or_else(Expr::zero);
        }
        rest = c.first().cloned().unwrap_or_else(Expr::zero);
    }
    let clean = |e: &Expr| (0..=order).all(|k| !e.contains(&lifted_symbol(k)));
    Ok((coeffs.iter().all(clean) && clean(&rest)).then_some((coeffs, rest)))
}

/// Solutions `y(x) == ...` of the equation `eq` (an `==` or an expression equal to zero) in the unknown `f = y(x)`.
pub fn dsolve(eq: &Expr, f: &Expr) -> MathResult<Vec<Expr>> {
    let Kind::Call(_, args) = f.kind() else {
        return unsupported("the unknown must be written y(x)");
    };
    let [x] = args.as_slice() else { return unsupported("the unknown must be written y(x)") };
    require_symbol(x)?;
    let zero_form = match eq.kind() {
        Kind::Eq(l, r) => l.sub(r),
        _ => eq.clone(),
    };
    let lifted = lift(&zero_form, f, x)?;
    if lifted.contains(f) {
        return unsupported("the unknown appears with an argument other than the variable");
    }
    let Some(order) = (0..=MAX_ORDER).rev().find(|k| lifted.contains(&lifted_symbol(*k))) else {
        return unsupported("it does not contain the unknown");
    };
    if order == 0 {
        return unsupported("it has no derivative of the unknown");
    }
    let rhs_list = if let Some((a, g)) = linear_parts(&lifted, order)? {
        if order == 1 {
            vec![first_order_linear(&a, &g, x)?]
        } else if a.iter().all(|c| !c.contains(x)) {
            constant_coefficients(&a, &g, x)?
        } else {
            return unsupported("linear equations of order above one need constant coefficients");
        }
    } else if order == 1 {
        first_order_nonlinear(&lifted, x)?
    } else {
        return unsupported("it is not linear");
    };
    Ok(rhs_list.into_iter().map(|r| Expr::eq(f.clone(), r)).collect())
}

/// `a1 y' + a0 y + g = 0` through the integrating factor `exp(int a0/a1)`.
fn first_order_linear(a: &[Expr], g: &Expr, x: &Expr) -> MathResult<Expr> {
    let (Some(a0), Some(a1)) = (a.first(), a.get(1)) else { return unsupported("not a first order equation") };
    let p = simplify(&a0.div(a1))?;
    let q = simplify(&g.neg().div(a1))?;
    let Some(ip) = integrate(&p, x)? else { return unsupported("the integrating factor has no closed form") };
    let mu = Expr::pow(&Expr::e(), &ip);
    let Some(particular) = integrate(&expand(&Expr::mul(vec![mu.clone(), q]))?, x)? else {
        return unsupported("the integral for the particular solution has no closed form");
    };
    let inverse = Expr::pow(&mu, &Expr::int(-1));
    let sol = Expr::mul(vec![inverse, Expr::add(vec![particular, constant(1)])]);
    simplify(&expand(&sol)?)
}

/// The right side `F` of `y' = F(x, y)` for an equation linear in `y'`.
fn rate_of(lifted: &Expr) -> MathResult<Option<Expr>> {
    let Some(c) = to_poly(lifted, &lifted_symbol(1))? else { return Ok(None) };
    let (Some(c0), Some(c1)) = (c.first(), c.get(1)) else { return Ok(None) };
    if c.len() != 2 {
        return Ok(None);
    }
    Ok(Some(simplify(&c0.neg().div(c1))?))
}

/// Nonlinear first order equations solved for `y'`: separable, else Bernoulli, else homogeneous.
fn first_order_nonlinear(lifted: &Expr, x: &Expr) -> MathResult<Vec<Expr>> {
    let first = match separable(lifted, x) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    if let Some(rate) = rate_of(lifted)? {
        if let Some(v) = bernoulli(&rate, x)? {
            return Ok(v);
        }
        if let Some(v) = homogeneous(&rate, x)? {
            return Ok(v);
        }
    }
    Err(first)
}

/// `y' = a(x) y + b(x) y^n` (`n` a number other than 0 and 1): with `v = y^(1-n)` it is linear,
/// `v' = (1-n)(a v + b)`, and `y = v^(1/(1-n))`.
fn bernoulli(rate: &Expr, x: &Expr) -> MathResult<Option<Vec<Expr>>> {
    let y = lifted_symbol(0);
    let expanded = expand(rate)?;
    let terms = match expanded.kind() {
        Kind::Add(t) => t.clone(),
        _ => vec![expanded.clone()],
    };
    // the coefficient of each power of y, as a sum of the terms that carry it
    let mut powers: Vec<(Expr, Vec<Expr>)> = Vec::new();
    for t in terms {
        let factors = match t.kind() {
            Kind::Mul(f) => f.clone(),
            _ => vec![t.clone()],
        };
        let (mut exponent, mut coefficient) = (Expr::zero(), Vec::new());
        for f in factors {
            if !f.contains(&y) {
                coefficient.push(f);
            } else if f == y {
                exponent = Expr::add(vec![exponent, Expr::one()]);
            } else if let Kind::Pow(b, k) = f.kind()
                && *b == y
                && k.as_num().is_some()
            {
                exponent = Expr::add(vec![exponent, k.clone()]);
            } else {
                return Ok(None);
            }
        }
        let coefficient = Expr::mul(coefficient);
        match powers.iter_mut().find(|(k, _)| *k == exponent) {
            Some((_, c)) => c.push(coefficient),
            None => powers.push((exponent, vec![coefficient])),
        }
    }
    let [first, second] = powers.as_slice() else { return Ok(None) };
    let ((a, n, b), one) = (if first.0 == Expr::one() { (&first.1, &second.0, &second.1) } else { (&second.1, &first.0, &first.1) }, Expr::one());
    if (first.0 == one) == (second.0 == one) || n.is_zero() {
        return Ok(None);
    }
    let (a, b) = (Expr::add(a.clone()), Expr::add(b.clone()));
    let k = one.sub(n);
    // v' - k a v - k b = 0
    let a0 = simplify(&Expr::mul(vec![Expr::int(-1), k.clone(), a]))?;
    let g = simplify(&Expr::mul(vec![Expr::int(-1), k.clone(), b]))?;
    let v = first_order_linear(&[a0, Expr::one()], &g, x)?;
    let sol = Expr::pow(&v, &Expr::one().div(&k));
    Ok(Some(vec![simplify(&sol)?]))
}

/// `y' = F(y/x)`: with `y = u x` it is `x u' = F(u) - u`, which is separable.
fn homogeneous(rate: &Expr, x: &Expr) -> MathResult<Option<Vec<Expr>>> {
    let (y, u) = (lifted_symbol(0), lifted_symbol(0));
    let scaled = simplify(&subs(rate, &[(y, Expr::mul(vec![u.clone(), x.clone()]))])?)?;
    if scaled.contains(x) {
        return Ok(None);
    }
    let equation = lifted_symbol(1).sub(&simplify(&scaled.sub(&u).div(x))?);
    let Ok(sols) = separable(&equation, x) else { return Ok(None) };
    let times_x = |s: &Expr| simplify(&expand(&Expr::mul(vec![x.clone(), s.clone()]))?);
    Ok(Some(sols.iter().map(times_x).collect::<MathResult<_>>()?))
}

/// `y' = f(x) g(y)`: `int dy/g = int f dx + C1`, solved for `y` when possible.
fn separable(lifted: &Expr, x: &Expr) -> MathResult<Vec<Expr>> {
    let (y, yp) = (lifted_symbol(0), lifted_symbol(1));
    let Some(c) = to_poly(lifted, &yp)? else { return unsupported("it is not linear in y'") };
    let (Some(c0), Some(c1)) = (c.first(), c.get(1)) else { return unsupported("it is not solved for y'") };
    if c.len() != 2 {
        return unsupported("it is not linear in y'");
    }
    let rate = simplify(&c0.neg().div(c1))?;
    let (mut fx, mut gy) = (Vec::new(), Vec::new());
    let factored = factor(&rate)?;
    let factors = match factored.kind() {
        Kind::Mul(v) => v.clone(),
        _ => vec![factored.clone()],
    };
    for t in factors {
        match (t.contains(x), t.contains(&y)) {
            (true, true) => return unsupported("it is neither linear nor separable"),
            (_, true) => gy.push(t),
            _ => fx.push(t),
        }
    }
    let (fx, gy) = (Expr::mul(fx), Expr::mul(gy));
    let (Some(left), Some(right)) = (integrate(&Expr::one().div(&gy), &y)?, integrate(&fx, x)?) else {
        return unsupported("the separated integrals have no closed form");
    };
    let k = constant(1);
    let implicit = Expr::add(vec![left, right.neg(), k.neg()]);
    let Ok(roots) = solve(&implicit, &y) else { return unsupported("the implicit solution cannot be solved for y") };
    if roots.is_empty() {
        return unsupported("the implicit solution cannot be solved for y");
    }
    roots.iter().map(absorb_constant).collect()
}

/// Reparametrises `C1 -> log(C1)` when that turns `exp(a + C1)` into `C1 exp(a)`, which keeps the same family.
fn absorb_constant(sol: &Expr) -> MathResult<Expr> {
    let k = constant(1);
    let plain = simplify(&expand(sol)?)?;
    let tidy = simplify(&expand(&subs(sol, &[(k.clone(), Expr::call("log", vec![k.clone()]))])?)?)?;
    Ok(if tidy.size() <= plain.size() && !tidy.to_string().contains("log(C1)") { tidy } else { plain })
}

/// `a_n y^(n) + ... + a_0 y = -g` with numeric (x-free) coefficients.
fn constant_coefficients(a: &[Expr], g: &Expr, x: &Expr) -> MathResult<Vec<Expr>> {
    let order = a.len().saturating_sub(1);
    let r = Expr::sym("_r");
    let char_poly = Expr::add(a.iter().enumerate().map(|(k, c)| Expr::mul(vec![c.clone(), Expr::pow(&r, &Expr::int(k as i64))])).collect());
    let mut roots: Vec<Expr> = Vec::new();
    for root in solve(&char_poly, &r)? {
        let root = simplify(&root)?;
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    // multiplicity: how many successive derivatives of the characteristic polynomial vanish at the root
    let mut found = Vec::new();
    let mut total = 0;
    for root in &roots {
        let (mut m, mut p) = (0, char_poly.clone());
        while m <= order && is_zero_expr(&simplify(&subs(&p, &[(r.clone(), root.clone())])?)?)? {
            m += 1;
            p = diff(&p, &r)?;
        }
        total += m;
        found.push((root.clone(), m.max(1)));
    }
    if total != order {
        return unsupported("the characteristic polynomial has no closed-form roots");
    }
    let basis = basis_functions(&found, x)?;
    let mut homogeneous = Vec::with_capacity(basis.len());
    for (i, b) in basis.iter().enumerate() {
        homogeneous.push(Expr::mul(vec![constant(i + 1), b.clone()]));
    }
    let mut parts_sum = homogeneous;
    if !is_zero_expr(g)? {
        parts_sum.push(particular(a, g, &basis, x)?);
    }
    Ok(vec![simplify(&expand(&Expr::add(parts_sum))?)?])
}

/// The functions `x^j e^(a x)`, and `x^j e^(a x) cos(b x)`, `... sin(b x)` for a conjugate pair `a ± b I`.
fn basis_functions(roots: &[(Expr, usize)], x: &Expr) -> MathResult<Vec<Expr>> {
    let mut out = Vec::new();
    let mut used = vec![false; roots.len()];
    for (i, (root, m)) in roots.iter().enumerate() {
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        let (re, im) = parts(root)?;
        let growth = Expr::pow(&Expr::e(), &Expr::mul(vec![re.clone(), x.clone()]));
        let powers = (0..*m).map(|j| Expr::mul(vec![Expr::pow(x, &Expr::int(j as i64)), growth.clone()]));
        if im.is_zero() {
            out.extend(powers);
            continue;
        }
        let conj = simplify(&Expr::add(vec![re.clone(), Expr::mul(vec![Expr::int(-1), im.clone(), Expr::imag()])]))?;
        let Some(partner) = roots.iter().position(|(r, _)| *r == conj) else {
            return unsupported("complex roots do not come in conjugate pairs");
        };
        if let Some(flag) = used.get_mut(partner) {
            *flag = true;
        }
        // the positive frequency, so that `sin` comes out with a plain sign
        let im = if im.as_num().is_some_and(|n| n.is_negative()) { im.neg() } else { im };
        let angle = Expr::mul(vec![im, x.clone()]);
        for p in powers {
            out.push(Expr::mul(vec![p.clone(), Expr::call("cos", vec![angle.clone()])]));
            out.push(Expr::mul(vec![p, Expr::call("sin", vec![angle.clone()])]));
        }
    }
    Ok(out)
}

/// A particular solution by variation of parameters (second order).
fn particular(a: &[Expr], g: &Expr, basis: &[Expr], x: &Expr) -> MathResult<Expr> {
    let ([y1, y2], [_, _, lead]) = (basis, a) else {
        return unsupported("a forcing term is supported for second order equations");
    };
    let wronskian = simplify(&expand(&Expr::add(vec![Expr::mul(vec![y1.clone(), diff(y2, x)?]), Expr::mul(vec![Expr::int(-1), diff(y1, x)?, y2.clone()])]))?)?;
    let scale = Expr::mul(vec![lead.clone(), wronskian]);
    let forcing = g.neg();
    let mut total = Vec::new();
    for (sign, this, other) in [(-1, y1, y2), (1, y2, y1)] {
        let integrand = simplify(&expand(&Expr::mul(vec![Expr::int(sign), other.clone(), forcing.clone()]).div(&scale))?)?;
        let Some(u) = integrate(&integrand, x)? else { return unsupported("the variation of parameters integral has no closed form") };
        total.push(Expr::mul(vec![this.clone(), u]));
    }
    Ok(Expr::add(total))
}

/// Fixes the constants of `solution` (the right side of `y == ...`) from `y^(k)(x0) == values[k]`.
pub fn apply_conditions(solution: &Expr, x: &Expr, x0: &Expr, values: &[Expr]) -> MathResult<Vec<Expr>> {
    let mut constants = Vec::new();
    for k in 1..=MAX_ORDER + 1 {
        if solution.contains(&constant(k)) {
            constants.push(constant(k));
        }
    }
    if constants.len() != values.len() {
        return Err(MathError::domain(format!(
            "the solution has {} arbitrary constant(s) but {} initial condition(s) were given",
            constants.len(),
            values.len()
        )));
    }
    let mut eqs = Vec::new();
    let mut cur = solution.clone();
    for v in values {
        eqs.push(Expr::eq(subs(&cur, &[(x.clone(), x0.clone())])?, v.clone()));
        cur = diff(&cur, x)?;
    }
    let sols = solve_system(&eqs, &constants)?;
    sols.into_iter()
        .map(|s| {
            let rules: Vec<(Expr, Expr)> = constants.iter().cloned().zip(s).collect();
            simplify(&expand(&subs(solution, &rules)?)?)
        })
        .collect()
}
