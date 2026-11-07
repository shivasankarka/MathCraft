//! Distribute products over sums and expand integer powers of sums.

use crate::util::guard;
use mathcraft_core::{Expr, Kind, MathError, MathResult, budget};

/// Largest number of terms an expansion may produce.
pub const MAX_TERMS: usize = 100_000;

pub fn expand(e: &Expr) -> MathResult<Expr> {
    guard(e)?;
    match e.kind() {
        Kind::Num(_) | Kind::Sym(_) => Ok(e.clone()),
        Kind::Mul(fs) => {
            let parts = fs.iter().map(expand).collect::<MathResult<Vec<_>>>()?;
            distribute(&parts)
        }
        Kind::Pow(b, x) => {
            let nb = expand(b)?;
            let nx = expand(x)?;
            if let (Kind::Add(_), Some(n)) = (nb.kind(), nx.as_i64())
                && n >= 2
            {
                return power(&nb, n.unsigned_abs());
            }
            Ok(Expr::pow(&nb, &nx))
        }
        _ => e.map_children(expand),
    }
}

fn terms_of(e: &Expr) -> Vec<Expr> {
    match e.kind() {
        Kind::Add(v) => v.clone(),
        _ => vec![e.clone()],
    }
}

/// Multiply out a list of (already expanded) factors.
fn distribute(parts: &[Expr]) -> MathResult<Expr> {
    let mut acc: Vec<Expr> = vec![Expr::one()];
    for p in parts {
        let ts = terms_of(p);
        if acc.len().saturating_mul(ts.len()) > MAX_TERMS {
            return Err(MathError::too_large("expansion would produce too many terms"));
        }
        let mut next = Vec::with_capacity(acc.len() * ts.len());
        for a in &acc {
            for t in &ts {
                budget::tick()?;
                next.push(Expr::mul(vec![a.clone(), t.clone()]));
            }
        }
        // Adding combines like terms, keeping intermediate results small.
        acc = terms_of(&Expr::add(next));
    }
    Ok(Expr::add(acc))
}

/// `(sum) ** n` for a positive integer `n`, by repeated squaring.
fn power(base: &Expr, n: u64) -> MathResult<Expr> {
    let k = base.args().len() as u64;
    if n > 10_000 || estimate_terms(n, k) > MAX_TERMS as u128 {
        return Err(MathError::too_large("expansion would produce too many terms"));
    }
    let mut result = Expr::one();
    let mut b = base.clone();
    let mut m = n;
    while m > 0 {
        if m & 1 == 1 {
            result = distribute(&[result, b.clone()])?;
        }
        m >>= 1;
        if m > 0 {
            b = distribute(&[b.clone(), b.clone()])?;
        }
    }
    Ok(result)
}

/// Upper bound on the number of terms in `(k-term sum) ** n`: C(n + k - 1, k - 1).
fn estimate_terms(n: u64, k: u64) -> u128 {
    let mut c: u128 = 1;
    for i in 1..k.min(64) {
        c = c.saturating_mul(u128::from(n + i)) / u128::from(i);
        if c > u128::from(u64::MAX) {
            return u128::MAX;
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn y() -> Expr {
        Expr::sym("y")
    }

    #[test]
    fn binomial() {
        let e = Expr::pow(&Expr::add(vec![x(), y()]), &Expr::int(2));
        assert_eq!(expand(&e).unwrap().to_string(), "x**2 + 2*x*y + y**2");
    }

    #[test]
    fn product_of_sums() {
        let e = Expr::mul(vec![Expr::add(vec![x(), Expr::int(1)]), Expr::add(vec![x(), Expr::int(-1)])]);
        assert_eq!(expand(&e).unwrap().to_string(), "x**2 - 1");
    }

    #[test]
    fn huge_expansion_is_refused() {
        let e = Expr::pow(&Expr::add(vec![x(), y(), Expr::sym("z")]), &Expr::int(5000));
        assert!(matches!(expand(&e), Err(MathError::TooLarge(_))));
    }

    #[test]
    fn nested() {
        let inner = Expr::pow(&Expr::add(vec![x(), Expr::int(1)]), &Expr::int(3));
        let e = Expr::mul(vec![Expr::int(2), inner]);
        assert_eq!(expand(&e).unwrap().to_string(), "2*x**3 + 6*x**2 + 6*x + 2");
    }
}
