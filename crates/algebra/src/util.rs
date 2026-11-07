use crate::expand::expand;
use mathcraft_core::{Expr, Kind, MathError, MathResult, budget};

/// Refuse expressions nested deeper than this; recursion in the algebra routines is bounded by it.
pub const MAX_DEPTH: u32 = 1500;

/// Count a unit of work and refuse absurdly deep expressions. Every recursive routine calls this first.
pub fn guard(e: &Expr) -> MathResult<()> {
    budget::tick()?;
    if e.depth() > MAX_DEPTH { Err(MathError::too_large("expression is nested too deeply")) } else { Ok(()) }
}

/// Simultaneous structural substitution: every subexpression equal to a `from` is replaced by its `to`.
pub fn subs(e: &Expr, rules: &[(Expr, Expr)]) -> MathResult<Expr> {
    guard(e)?;
    for (from, to) in rules {
        if e == from {
            return Ok(to.clone());
        }
    }
    e.map_children(|c| subs(c, rules))
}

pub fn require_symbol(x: &Expr) -> MathResult<()> {
    if x.as_sym().is_some() && !x.is_constant_symbol() { Ok(()) } else { Err(MathError::domain(format!("expected a plain symbol such as x, got `{x}`"))) }
}

/// Is `e` identically zero after expansion?
pub fn is_zero_expr(e: &Expr) -> MathResult<bool> {
    Ok(e.is_zero() || expand(e)?.is_zero())
}

/// Does the expression contain a division by exact zero (`0 ** -n`) or a non-finite float?
pub fn has_singularity(e: &Expr) -> bool {
    match e.kind() {
        Kind::Pow(b, x) => (b.is_zero() && x.as_num().is_some_and(|n| n.is_negative())) || has_singularity(b) || has_singularity(x),
        Kind::Num(n) => !n.to_f64().is_finite(),
        _ => e.children().iter().any(has_singularity),
    }
}

/// Split a product into its numerator and denominator: factors with a negative numeric exponent go below the line.
pub fn split_fraction(e: &Expr) -> (Expr, Expr) {
    let factors: Vec<Expr> = match e.kind() {
        Kind::Mul(fs) => fs.clone(),
        _ => vec![e.clone()],
    };
    let (mut num, mut den) = (Vec::new(), Vec::new());
    for f in factors {
        match f.kind() {
            Kind::Pow(b, k) if k.as_num().is_some_and(|n| n.is_negative()) => den.push(Expr::pow(b, &k.neg())),
            _ => num.push(f),
        }
    }
    (Expr::mul(num), Expr::mul(den))
}

/// The sign of a symbol-free expression, when a float evaluation decides it (`None` when it does not).
pub fn sign_of(e: &Expr) -> Option<i8> {
    if !e.free_symbols().is_empty() {
        return None;
    }
    let v = mathcraft_core::numeric::eval_f64(e, &|_| None).ok()?;
    if v.abs() < 1e-12 {
        return is_zero_expr(e).ok().filter(|z| *z).map(|_| 0);
    }
    Some(if v > 0.0 { 1 } else { -1 })
}
