//! Elementary functions: automatic evaluation rules applied when a call is constructed.

use crate::expr::{Expr, Kind, SYM_E, SYM_PI};
use crate::number::Num;
use dashu::integer::IBig;

const UNARY: &[&str] = &["sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "log", "abs"];

pub fn is_builtin_function(name: &str) -> bool {
    matches!(name, "sqrt" | "exp") || UNARY.contains(&name)
}

pub(crate) fn apply(name: &str, args: Vec<Expr>) -> Expr {
    match (name, args.as_slice()) {
        ("sqrt", [x]) => Expr::pow(x, &Expr::half()),
        ("exp", [x]) => Expr::pow(&Expr::e(), x),
        (_, [x]) if UNARY.contains(&name) => unary(name, x),
        _ => Expr::raw_call(name, args),
    }
}

#[derive(PartialEq)]
enum Parity {
    Odd,
    Even,
    Neither,
}

fn parity(name: &str) -> Parity {
    match name {
        "sin" | "tan" | "asin" | "atan" | "sinh" | "tanh" => Parity::Odd,
        "cos" | "cosh" | "abs" => Parity::Even,
        _ => Parity::Neither,
    }
}

/// Machine-precision value of a built-in unary function, `None` outside its real domain.
pub fn f64_unary(name: &str, v: f64) -> Option<f64> {
    let r = match name {
        "sin" => v.sin(),
        "cos" => v.cos(),
        "tan" => v.tan(),
        "asin" => v.asin(),
        "acos" => v.acos(),
        "atan" => v.atan(),
        "sinh" => v.sinh(),
        "cosh" => v.cosh(),
        "tanh" => v.tanh(),
        "log" => v.ln(),
        "abs" => v.abs(),
        _ => return None,
    };
    (!r.is_nan()).then_some(r)
}

fn unary(name: &str, x: &Expr) -> Expr {
    let raw = || Expr::raw_call(name, vec![x.clone()]);
    if let Some(n) = x.as_num() {
        if !n.is_exact() {
            return match f64_unary(name, n.to_f64()) {
                Some(v) if v.is_finite() => Expr::float(v),
                _ => raw(),
            };
        }
        return exact_value(name, n).unwrap_or_else(raw);
    }
    // Parity: f(-u) = ±f(u).
    let (c, rest) = x.split_coeff();
    if c.is_negative() && !rest.is_one() && parity(name) != Parity::Neither {
        let flipped = Expr::mul(vec![Expr::num(c.neg()), rest]);
        let inner = unary(name, &flipped);
        return if parity(name) == Parity::Odd { inner.neg() } else { inner };
    }
    if matches!(name, "sin" | "cos" | "tan")
        && let Some(v) = pi_multiple(name, x)
    {
        return v;
    }
    match (name, x.kind()) {
        ("log", Kind::Sym(s)) if &**s == SYM_E => Expr::one(),
        ("log", Kind::Pow(b, y)) if b.as_sym() == Some(SYM_E) => y.clone(),
        ("abs", Kind::Call(n, _)) if &**n == "abs" => x.clone(),
        _ => raw(),
    }
}

fn exact_value(name: &str, n: &Num) -> Option<Expr> {
    let zero = n.is_zero();
    let one = n.is_one();
    match name {
        "sin" | "tan" | "asin" | "atan" | "sinh" | "tanh" if zero => Some(Expr::zero()),
        "cos" | "cosh" if zero => Some(Expr::one()),
        "acos" | "log" if one => Some(Expr::zero()),
        "atan" if one => Some(Expr::mul(vec![Expr::rational(1, 4)?, Expr::pi()])),
        "asin" | "acos" => {
            // arcsin/arccos at 0, ±1/2, ±1: the angle as a multiple of pi
            let (p, q) = n.parts().and_then(|(p, q)| Some((i64::try_from(&p).ok()?, i64::try_from(&IBig::from(q)).ok()?)))?;
            let sixths = match (p, q) {
                (0, _) => 0,
                (1, 1) => 3,
                (-1, 1) => -3,
                (1, 2) => 1,
                (-1, 2) => -1,
                _ => return None,
            };
            let s = if name == "asin" { sixths } else { 3 - sixths };
            Some(Expr::mul(vec![Expr::rational(s, 6)?, Expr::pi()]))
        }
        "abs" => Some(Expr::num(if n.is_negative() { n.neg() } else { n.clone() })),
        _ => None,
    }
}

/// `sin`, `cos`, `tan` of a rational multiple of pi, for the angles with closed forms (multiples of pi/6 and pi/4).
fn pi_multiple(name: &str, x: &Expr) -> Option<Expr> {
    let (c, rest) = x.split_coeff();
    if rest.as_sym() != Some(SYM_PI) || !c.is_exact() {
        return None;
    }
    let (p, q) = c.parts()?;
    let p = i64::try_from(&p).ok()?;
    let q = i64::try_from(&IBig::from(q)).ok()?;
    match name {
        "sin" => sin_pi(i128::from(p), i128::from(q)),
        "cos" => sin_pi(i128::from(p) * 2 + i128::from(q), i128::from(q) * 2),
        "tan" => {
            let s = sin_pi(i128::from(p), i128::from(q))?;
            let c = sin_pi(i128::from(p) * 2 + i128::from(q), i128::from(q) * 2)?;
            if c.is_zero() { None } else { Some(s.div(&c)) }
        }
        _ => None,
    }
}

/// `sin(p/q * pi)` when it has a simple closed form.
fn sin_pi(p: i128, q: i128) -> Option<Expr> {
    if q <= 0 {
        return None;
    }
    let mut n = p.rem_euclid(2 * q);
    let mut negative = false;
    if n > q {
        n -= q;
        negative = true;
    }
    if 2 * n > q {
        n = q - n;
    }
    let half = Expr::half();
    let v = if n == 0 {
        Expr::zero()
    } else if 6 * n == q {
        half
    } else if 4 * n == q {
        Expr::mul(vec![half, Expr::pow(&Expr::int(2), &Expr::half())])
    } else if 3 * n == q {
        Expr::mul(vec![half, Expr::pow(&Expr::int(3), &Expr::half())])
    } else if 2 * n == q {
        Expr::one()
    } else {
        return None;
    };
    Some(if negative { v.neg() } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn pi_times(n: i64, d: i64) -> Expr {
        Expr::mul(vec![Expr::rational(n, d).unwrap(), Expr::pi()])
    }

    #[test]
    fn special_angles() {
        assert_eq!(Expr::call("sin", vec![Expr::pi()]), Expr::zero());
        assert_eq!(Expr::call("cos", vec![Expr::pi()]), Expr::int(-1));
        assert_eq!(Expr::call("sin", vec![pi_times(1, 6)]).to_string(), "1/2");
        assert_eq!(Expr::call("cos", vec![pi_times(1, 4)]).to_string(), "sqrt(2)/2");
        assert_eq!(Expr::call("sin", vec![pi_times(7, 6)]).to_string(), "-1/2");
        assert_eq!(Expr::call("tan", vec![pi_times(1, 4)]), Expr::one());
        assert_eq!(Expr::call("sin", vec![pi_times(1, 7)]).to_string(), "sin(pi/7)");
    }

    #[test]
    fn parity_rules() {
        assert_eq!(Expr::call("sin", vec![x().neg()]).to_string(), "-sin(x)");
        assert_eq!(Expr::call("cos", vec![x().neg()]).to_string(), "cos(x)");
    }

    #[test]
    fn exp_and_log() {
        let ex = Expr::call("exp", vec![x()]);
        assert_eq!(Expr::call("log", vec![ex.clone()]), x());
        assert_eq!(Expr::call("exp", vec![Expr::call("log", vec![x()])]), x());
        assert_eq!(Expr::mul(vec![ex.clone(), ex]).to_string(), "exp(2*x)");
        assert_eq!(Expr::call("log", vec![Expr::e()]), Expr::one());
        // exp(c*log(a)) is a power; powers of exp(number) scale the exponent
        assert_eq!(Expr::call("exp", vec![Expr::mul(vec![Expr::int(3), Expr::call("log", vec![Expr::int(2)])])]), Expr::int(8));
        assert_eq!(Expr::pow(&Expr::call("exp", vec![Expr::int(4)]), &Expr::half()).to_string(), "exp(2)");
    }

    #[test]
    fn float_arguments_evaluate() {
        let v = Expr::call("sin", vec![Expr::float(0.0)]);
        assert!(v.is_zero());
        let w = Expr::call("cos", vec![Expr::float(0.0)]);
        assert!(w.is_one());
    }
}
