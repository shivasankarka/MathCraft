//! Real and imaginary parts of expressions built from `I`. Free symbols are taken to be real.

use crate::expand;
use mathcraft_core::{Expr, Kind, MathError, MathResult};

type Cx = (Expr, Expr);

fn cx_mul(x: &Cx, y: &Cx) -> MathResult<Cx> {
    let re = Expr::add(vec![Expr::mul(vec![x.0.clone(), y.0.clone()]), Expr::mul(vec![Expr::int(-1), x.1.clone(), y.1.clone()])]);
    let im = Expr::add(vec![Expr::mul(vec![x.0.clone(), y.1.clone()]), Expr::mul(vec![x.1.clone(), y.0.clone()])]);
    Ok((expand(&re)?, expand(&im)?))
}

/// `1 / z` as `conj(z) / |z|^2`.
fn cx_inv(z: &Cx) -> MathResult<Cx> {
    let norm = expand(&Expr::add(vec![Expr::pow(&z.0, &Expr::int(2)), Expr::pow(&z.1, &Expr::int(2))]))?;
    Ok((z.0.div(&norm), z.1.neg().div(&norm)))
}

fn call(name: &str, x: &Expr) -> Expr {
    Expr::call(name, vec![x.clone()])
}

/// The parts of `f(a + b*I)` for the elementary functions, from the addition formulas.
fn cx_function(name: &str, a: &Expr, b: &Expr) -> MathResult<Cx> {
    let (sa, ca, sb, cb) = (call("sin", a), call("cos", a), call("sin", b), call("cos", b));
    let (sha, cha, shb, chb) = (call("sinh", a), call("cosh", a), call("sinh", b), call("cosh", b));
    let m = |x: &Expr, y: &Expr| Expr::mul(vec![x.clone(), y.clone()]);
    let parts = match name {
        "sin" => (m(&sa, &chb), m(&ca, &shb)),
        "cos" => (m(&ca, &chb), m(&sa, &shb).neg()),
        "sinh" => (m(&sha, &cb), m(&cha, &sb)),
        "cosh" => (m(&cha, &cb), m(&sha, &sb)),
        "tan" | "tanh" => {
            let (s, c) = if name == "tan" { ("sin", "cos") } else { ("sinh", "cosh") };
            let (sn, cs) = (cx_function(s, a, b)?, cx_function(c, a, b)?);
            return cx_mul(&sn, &cx_inv(&cs)?);
        }
        "log" => {
            // ln|z| + i arg z; the angle is only known for numbers
            let modulus = expand(&Expr::add(vec![Expr::pow(a, &Expr::int(2)), Expr::pow(b, &Expr::int(2))]))?;
            (Expr::mul(vec![Expr::half(), call("log", &modulus)]), atan2(b, a)?)
        }
        _ => return Err(MathError::unsupported(format!("cannot separate the real and imaginary parts of `{name}` of a complex number"))),
    };
    Ok((expand(&parts.0)?, expand(&parts.1)?))
}

fn split(e: &Expr) -> MathResult<Cx> {
    let unit = Expr::imag();
    if !e.contains(&unit) {
        return Ok((e.clone(), Expr::zero()));
    }
    let fail = || MathError::unsupported(format!("cannot separate the real and imaginary parts of `{e}`"));
    let (re, im) = match e.kind() {
        Kind::Sym(_) => (Expr::zero(), Expr::one()),
        Kind::Add(v) => {
            let mut parts = Vec::with_capacity(v.len());
            for t in v {
                parts.push(split(t)?);
            }
            (Expr::add(parts.iter().map(|p| p.0.clone()).collect()), Expr::add(parts.iter().map(|p| p.1.clone()).collect()))
        }
        Kind::Mul(fs) => {
            let mut acc: Cx = (Expr::one(), Expr::zero());
            for f in fs {
                acc = cx_mul(&acc, &split(f)?)?;
            }
            acc
        }
        Kind::Pow(base, x) => {
            if *base == Expr::e() {
                // e^(a + bi) = e^a (cos b + i sin b)
                let (a, b) = split(x)?;
                let r = Expr::pow(base, &a);
                (Expr::mul(vec![r.clone(), call("cos", &b)]), Expr::mul(vec![r, call("sin", &b)]))
            } else if let Some(n) = x.as_i64().filter(|n| n.unsigned_abs() <= 4096) {
                let z = split(base)?;
                let mut acc: Cx = (Expr::one(), Expr::zero());
                let (mut sq, mut k) = (z, n.unsigned_abs());
                while k > 0 {
                    if k & 1 == 1 {
                        acc = cx_mul(&acc, &sq)?;
                    }
                    k >>= 1;
                    if k > 0 {
                        sq = cx_mul(&sq, &sq)?;
                    }
                }
                if n < 0 { cx_inv(&acc)? } else { acc }
            } else if !base.contains(&unit) && base.as_num().is_some_and(|n| !n.is_negative() && !n.is_zero()) {
                // a^(x) with a positive number: e^(x ln a)
                return split(&Expr::pow(&Expr::e(), &Expr::mul(vec![x.clone(), call("log", base)])));
            } else {
                return Err(fail());
            }
        }
        Kind::Call(name, args) => match args.as_slice() {
            [arg] => {
                let (a, b) = split(arg)?;
                cx_function(name, &a, &b)?
            }
            _ => return Err(fail()),
        },
        _ => return Err(fail()),
    };
    Ok((expand(&re)?, expand(&im)?))
}

/// `(re, im)` with `e == re + im*I`.
pub fn parts(e: &Expr) -> MathResult<(Expr, Expr)> {
    split(e)
}

pub fn conjugate(e: &Expr) -> MathResult<Expr> {
    let (re, im) = split(e)?;
    Ok(Expr::add(vec![re, Expr::mul(vec![Expr::int(-1), im, Expr::imag()])]))
}

/// The modulus `sqrt(re**2 + im**2)`.
pub fn abs(e: &Expr) -> MathResult<Expr> {
    let (re, im) = split(e)?;
    if im.is_zero() {
        return Ok(Expr::call("abs", vec![re]));
    }
    let sum = expand(&Expr::add(vec![Expr::pow(&re, &Expr::int(2)), Expr::pow(&im, &Expr::int(2))]))?;
    // `|e^(i x)|` is `sqrt(cos^2 + sin^2)`: let the Pythagorean identity act
    Ok(Expr::pow(&crate::simplify(&sum)?, &Expr::half()))
}

/// The argument (angle) of a number, in `(-pi, pi]`. Only numbers have one here.
pub fn arg(e: &Expr) -> MathResult<Expr> {
    let (re, im) = split(e)?;
    if re.as_num().is_none() || im.as_num().is_none() {
        return Err(MathError::unsupported("arg needs a number, such as 1 + I"));
    }
    atan2(&im, &re)
}

/// Rewrites `e**(a + b*I)` as `e**a * (cos(b) + I*sin(b))`: complex exponentials become trigonometric functions.
pub fn rewrite_trig(e: &Expr) -> MathResult<Expr> {
    let inner = e.map_children(rewrite_trig)?;
    if let Kind::Pow(base, x) = inner.kind()
        && *base == Expr::e()
        && x.contains(&Expr::imag())
    {
        let (re, im) = split(x)?;
        let angle = |f: &str| call(f, &im);
        return Ok(Expr::mul(vec![Expr::pow(base, &re), Expr::add(vec![angle("cos"), Expr::mul(vec![Expr::imag(), angle("sin")])])]));
    }
    Ok(inner)
}

/// Rewrites sines, cosines and their hyperbolic cousins with exponentials (`cos(x)` is `(e**(I*x) + e**(-I*x))/2`).
pub fn rewrite_exp(e: &Expr) -> MathResult<Expr> {
    let inner = e.map_children(rewrite_exp)?;
    let Kind::Call(name, args) = inner.kind() else { return Ok(inner) };
    let [u] = args.as_slice() else { return Ok(inner) };
    let ex = |t: &Expr| Expr::pow(&Expr::e(), t);
    let iu = Expr::mul(vec![Expr::imag(), u.clone()]);
    let half = Expr::half();
    Ok(match name.as_ref() {
        "cos" => Expr::mul(vec![half, Expr::add(vec![ex(&iu), ex(&iu.neg())])]),
        "sin" => Expr::mul(vec![Expr::add(vec![ex(&iu), ex(&iu.neg()).neg()]), Expr::mul(vec![Expr::int(-1), Expr::half(), Expr::imag()])]),
        "cosh" => Expr::mul(vec![half, Expr::add(vec![ex(u), ex(&u.neg())])]),
        "sinh" => Expr::mul(vec![half, Expr::add(vec![ex(u), ex(&u.neg()).neg()])]),
        "tan" | "tanh" => {
            let base = if name.as_ref() == "tan" { ("sin", "cos") } else { ("sinh", "cosh") };
            let (s, c) = (call(base.0, u), call(base.1, u));
            return Ok(rewrite_exp(&s)?.div(&rewrite_exp(&c)?));
        }
        _ => inner,
    })
}

/// The angle of the point `(x, y)` in `(-pi, pi]`, for numeric arguments: exact multiples of pi/4 come out exact.
pub fn atan2(y: &Expr, x: &Expr) -> MathResult<Expr> {
    let (Some(yn), Some(xn)) = (y.as_num(), x.as_num()) else {
        return Err(MathError::unsupported("atan2 needs numeric arguments"));
    };
    let pi = Expr::pi();
    let frac = |p: i64, q: i64| Expr::rational(p, q).map(|r| Expr::mul(vec![r, pi.clone()])).unwrap_or_else(Expr::zero);
    if xn.is_zero() {
        return match (yn.is_zero(), yn.is_negative()) {
            (true, _) => Err(MathError::domain("atan2(0, 0) is undefined")),
            (false, false) => Ok(frac(1, 2)),
            (false, true) => Ok(frac(-1, 2)),
        };
    }
    let base = Expr::call("atan", vec![y.div(x)]);
    Ok(match (xn.is_negative(), yn.is_negative(), yn.is_zero()) {
        (false, _, _) => base,
        (true, _, true) => pi,
        (true, false, _) => Expr::add(vec![base, pi]),
        (true, true, _) => Expr::add(vec![base, pi.neg()]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i() -> Expr {
        Expr::imag()
    }

    #[test]
    fn splits_parts() {
        let z = Expr::add(vec![Expr::int(3), Expr::mul(vec![Expr::int(4), i()])]);
        let (re, im) = parts(&z).unwrap();
        assert_eq!((re.to_string(), im.to_string()), ("3".into(), "4".into()));
        assert_eq!(abs(&z).unwrap().to_string(), "5");
        assert_eq!(conjugate(&z).unwrap().to_string(), "3 - 4*I");
        let w = Expr::mul(vec![Expr::add(vec![Expr::int(1), i()]), Expr::add(vec![Expr::int(1), i()])]);
        assert_eq!(parts(&w).unwrap().1.to_string(), "2");
    }
}
