//! Trigonometric expansion: the addition formulas and multiple angles, `sin(x + y)` and `cos(3*x)` in terms of
//! `sin` and `cos` of the single symbols.

use crate::expand::expand;
use crate::util::guard;
use mathcraft_core::{Expr, Kind, MathResult};

/// Multiples beyond this are left alone: the expansion grows with the multiple.
const MAX_MULTIPLE: i64 = 24;

type Pair = (Expr, Expr);

fn product(a: &Expr, b: &Expr) -> Expr {
    Expr::mul(vec![a.clone(), b.clone()])
}

/// `(sin(a + b), cos(a + b))` from the parts.
fn add_angles((sa, ca): &Pair, (sb, cb): &Pair) -> Pair {
    (Expr::add(vec![product(sa, cb), product(ca, sb)]), Expr::add(vec![product(ca, cb), product(sa, sb).neg()]))
}

/// `(sin(u), cos(u))` with sums and integer multiples taken apart.
fn sin_cos(u: &Expr) -> MathResult<Pair> {
    guard(u)?;
    if let Kind::Add(terms) = u.kind() {
        let mut acc: Option<Pair> = None;
        for t in terms {
            let p = sin_cos(t)?;
            acc = Some(match acc {
                None => p,
                Some(a) => add_angles(&a, &p),
            });
        }
        if let Some(p) = acc {
            return Ok(p);
        }
    }
    let (c, rest) = u.split_coeff();
    if let Some(n) = c.as_i64().filter(|n| (2..=MAX_MULTIPLE).contains(&n.abs())) {
        let base = (Expr::call("sin", vec![rest.clone()]), Expr::call("cos", vec![rest.clone()]));
        let mut acc = base.clone();
        for _ in 1..n.abs() {
            acc = add_angles(&acc, &base);
        }
        // sin is odd and cos is even
        return Ok(if n < 0 { (acc.0.neg(), acc.1) } else { acc });
    }
    Ok((Expr::call("sin", vec![u.clone()]), Expr::call("cos", vec![u.clone()])))
}

/// Rewrites `sin`, `cos` and `tan` of sums and integer multiples with the addition formulas.
pub fn expand_trig(e: &Expr) -> MathResult<Expr> {
    guard(e)?;
    let inner = e.map_children(expand_trig)?;
    let Kind::Call(name, args) = inner.kind() else { return Ok(inner) };
    let ([u], "sin" | "cos" | "tan") = (args.as_slice(), name.as_ref()) else { return Ok(inner) };
    let (s, c) = sin_cos(u)?;
    match name.as_ref() {
        "sin" => expand(&s),
        "cos" => expand(&c),
        _ => Ok(Expr::mul(vec![expand(&s)?, Expr::pow(&expand(&c)?, &Expr::int(-1))])),
    }
}
