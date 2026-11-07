//! Real and imaginary parts of expressions built from `I`. Free symbols are taken to be real.

use crate::expand;
use mathcraft_core::{Expr, Kind, MathError, MathResult, SYM_I};

fn split(e: &Expr) -> MathResult<(Expr, Expr)> {
    let unit = Expr::imag();
    let expanded = expand(e)?;
    let terms: Vec<Expr> = match expanded.kind() {
        Kind::Add(v) => v.clone(),
        _ => vec![expanded.clone()],
    };
    let (mut re, mut im) = (Vec::new(), Vec::new());
    for t in terms {
        if !t.contains(&unit) {
            re.push(t);
            continue;
        }
        let coefficient = match t.kind() {
            Kind::Sym(_) => Some(Expr::one()),
            Kind::Mul(fs) => {
                let rest: Vec<Expr> = fs.iter().filter(|f| f.as_sym() != Some(SYM_I)).cloned().collect();
                (rest.len() + 1 == fs.len() && !rest.iter().any(|f| f.contains(&unit))).then(|| Expr::mul(rest))
            }
            _ => None,
        };
        match coefficient {
            Some(c) => im.push(c),
            None => return Err(MathError::unsupported(format!("cannot separate the real and imaginary parts of `{t}`"))),
        }
    }
    Ok((Expr::add(re), Expr::add(im)))
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
    Ok(Expr::pow(&sum, &Expr::half()))
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
