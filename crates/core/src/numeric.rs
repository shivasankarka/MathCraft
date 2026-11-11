//! Numeric evaluation: full evaluation to `f64`, and partial approximation that keeps unknown symbols.

use crate::budget;
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, Kind, SYM_E, SYM_I, SYM_PI};
use crate::functions::f64_unary;

/// Evaluate to a machine float. `env` supplies values for free symbols.
pub fn eval_f64(e: &Expr, env: &dyn Fn(&str) -> Option<f64>) -> MathResult<f64> {
    budget::tick()?;
    match e.kind() {
        Kind::Num(n) => Ok(n.to_f64()),
        Kind::Sym(s) => match &**s {
            SYM_E => Ok(std::f64::consts::E),
            SYM_PI => Ok(std::f64::consts::PI),
            SYM_I => Err(MathError::domain("the value is complex, so it has no single real number")),
            name => env(name).ok_or_else(|| MathError::domain(format!("cannot evaluate numerically: `{name}` has no value"))),
        },
        Kind::Add(v) => v.iter().try_fold(0.0, |acc, t| Ok(acc + eval_f64(t, env)?)),
        Kind::Mul(v) => v.iter().try_fold(1.0, |acc, t| Ok(acc * eval_f64(t, env)?)),
        Kind::Pow(b, x) => {
            let r = eval_f64(b, env)?.powf(eval_f64(x, env)?);
            if r.is_nan() { Err(MathError::domain("result is not a real number")) } else { Ok(r) }
        }
        Kind::Call(name, args) => match args.as_slice() {
            [a] => {
                let v = eval_f64(a, env)?;
                f64_unary(name, v).ok_or_else(|| MathError::domain(format!("`{name}` is undefined at {v}")))
            }
            [y, x] if &**name == "atan2" => Ok(eval_f64(y, env)?.atan2(eval_f64(x, env)?)),
            _ => Err(MathError::unsupported(format!("cannot evaluate `{name}` numerically"))),
        },
        Kind::List(_) | Kind::Eq(..) => Err(MathError::unsupported("only numbers and formulas can be evaluated to a float")),
    }
}

/// Approximate every exact number and constant with a float, keeping free symbols and integer exponents.
/// `n(x**2 + sqrt(2))` is `x**2 + 1.4142135623730951`.
pub fn approximate(e: &Expr) -> MathResult<Expr> {
    budget::tick()?;
    Ok(match e.kind() {
        Kind::Num(n) => Expr::float(n.to_f64()),
        Kind::Sym(s) => match &**s {
            SYM_E => Expr::float(std::f64::consts::E),
            SYM_PI => Expr::float(std::f64::consts::PI),
            _ => e.clone(),
        },
        Kind::Pow(b, x) => {
            let keep = x.as_num().is_some_and(|n| n.is_integer() && n.is_exact());
            let nb = approximate(b)?;
            let nx = if keep { x.clone() } else { approximate(x)? };
            Expr::pow(&nb, &nx)
        }
        _ => e.map_children(approximate)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_formulas() {
        let x = Expr::sym("x");
        let f = Expr::add(vec![Expr::pow(&x, &Expr::int(2)), Expr::call("sin", vec![x.clone()])]);
        let v = eval_f64(&f, &|n| (n == "x").then_some(2.0)).unwrap();
        assert!((v - (4.0 + 2.0_f64.sin())).abs() < 1e-12);
        assert!(eval_f64(&f, &|_| None).is_err());
    }

    #[test]
    fn approximates_but_keeps_symbols() {
        let x = Expr::sym("x");
        let e = Expr::add(vec![Expr::pow(&x, &Expr::int(2)), Expr::pow(&Expr::int(2), &Expr::half())]);
        assert_eq!(approximate(&e).unwrap().to_string(), "x**2 + 1.4142135623730951");
    }
}
