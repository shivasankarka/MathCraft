//! Matrices as lists of lists of expressions: products, determinants, inverses, rank and linear systems.

use crate::calculus::diff;
use crate::expand::expand;
use crate::util::{is_zero_expr, subs};
use mathcraft_core::{Expr, Kind, MathError, MathResult};

pub type Matrix = Vec<Vec<Expr>>;

const MAX_LAPLACE: usize = 7;

pub fn to_matrix(e: &Expr) -> MathResult<Matrix> {
    let Kind::List(rows) = e.kind() else { return Err(MathError::domain("expected a matrix (a list of equal-length lists)")) };
    let mut m: Matrix = Vec::with_capacity(rows.len());
    for r in rows {
        let Kind::List(cells) = r.kind() else { return Err(MathError::domain("expected a matrix (a list of equal-length lists)")) };
        m.push(cells.clone());
    }
    let width = m.first().map_or(0, Vec::len);
    if m.iter().any(|r| r.len() != width) {
        return Err(MathError::domain("matrix rows must all have the same length"));
    }
    Ok(m)
}

pub fn from_matrix(m: &Matrix) -> Expr {
    Expr::list(m.iter().map(|r| Expr::list(r.clone())).collect())
}

fn cell(m: &Matrix, i: usize, j: usize) -> Expr {
    m.get(i).and_then(|r| r.get(j)).cloned().unwrap_or_else(Expr::zero)
}

fn set(m: &mut Matrix, i: usize, j: usize, v: Expr) {
    if let Some(slot) = m.get_mut(i).and_then(|r| r.get_mut(j)) {
        *slot = v;
    }
}

pub fn identity(n: usize) -> Matrix {
    (0..n).map(|i| (0..n).map(|j| if i == j { Expr::one() } else { Expr::zero() }).collect()).collect()
}

pub fn transpose(m: &Matrix) -> Matrix {
    let width = m.first().map_or(0, Vec::len);
    (0..width).map(|j| (0..m.len()).map(|i| cell(m, i, j)).collect()).collect()
}

pub fn matmul(a: &Matrix, b: &Matrix) -> MathResult<Matrix> {
    let inner = a.first().map_or(0, Vec::len);
    if inner != b.len() {
        return Err(MathError::domain(format!("cannot multiply a matrix with {inner} columns by one with {} rows", b.len())));
    }
    let width = b.first().map_or(0, Vec::len);
    let mut out = Vec::with_capacity(a.len());
    for i in 0..a.len() {
        let mut row = Vec::with_capacity(width);
        for j in 0..width {
            mathcraft_core::budget::tick()?;
            let terms = (0..inner).map(|k| Expr::mul(vec![cell(a, i, k), cell(b, k, j)])).collect();
            row.push(expand(&Expr::add(terms))?);
        }
        out.push(row);
    }
    Ok(out)
}

/// Gauss-Jordan elimination on the first `ncols` columns. Returns the pivot columns.
fn rref(m: &mut Matrix, ncols: usize) -> MathResult<Vec<usize>> {
    let rows = m.len();
    let cols = m.first().map_or(0, Vec::len);
    let mut pivots = Vec::new();
    let mut r = 0;
    for c in 0..ncols {
        if r >= rows {
            break;
        }
        let mut found = None;
        for p in r..rows {
            if !is_zero_expr(&cell(m, p, c))? {
                found = Some(p);
                break;
            }
        }
        let Some(p) = found else { continue };
        m.swap(p, r);
        let pivot = cell(m, r, c);
        for j in 0..cols {
            let v = expand(&cell(m, r, j).div(&pivot))?;
            set(m, r, j, v);
        }
        for i in 0..rows {
            if i == r {
                continue;
            }
            let factor = cell(m, i, c);
            if is_zero_expr(&factor)? {
                continue;
            }
            for j in 0..cols {
                let v = expand(&Expr::add(vec![cell(m, i, j), Expr::mul(vec![factor.neg(), cell(m, r, j)])]))?;
                set(m, i, j, v);
            }
        }
        pivots.push(c);
        r += 1;
    }
    Ok(pivots)
}

/// A basis of the null space: vectors `v` with `m * v == 0`.
pub fn nullspace(m: &Matrix) -> MathResult<Vec<Vec<Expr>>> {
    let cols = m.first().map_or(0, Vec::len);
    let mut work = m.clone();
    let pivots = rref(&mut work, cols)?;
    let mut basis = Vec::new();
    for free in (0..cols).filter(|c| !pivots.contains(c)) {
        let mut v = vec![Expr::zero(); cols];
        if let Some(slot) = v.get_mut(free) {
            *slot = Expr::one();
        }
        for (row, &p) in pivots.iter().enumerate() {
            if let Some(slot) = v.get_mut(p) {
                *slot = cell(&work, row, free).neg();
            }
        }
        basis.push(v);
    }
    Ok(basis)
}

/// `det(m - x*I)` expanded: the characteristic polynomial in `x` (up to a sign of `(-1)**n` relative to `det(x*I - m)`).
pub fn charpoly(m: &Matrix, x: &Expr) -> MathResult<Expr> {
    let n = m.len();
    if m.iter().any(|r| r.len() != n) {
        return Err(MathError::domain("only a square matrix has eigenvalues"));
    }
    let shifted: Matrix = m
        .iter()
        .enumerate()
        .map(|(i, row)| row.iter().enumerate().map(|(j, v)| if i == j { Expr::add(vec![v.clone(), x.neg()]) } else { v.clone() }).collect())
        .collect();
    expand(&det(&shifted)?)
}

/// Eigenvalues with multiplicity (a repeated root appears as many times as it repeats).
pub fn eigenvalues(m: &Matrix) -> MathResult<Vec<Expr>> {
    let x = Expr::sym("_lambda");
    let p = charpoly(m, &x)?;
    let roots = crate::solve(&p, &x)?;
    let mut out = Vec::new();
    for r in roots {
        let (mut d, mut k) = (p.clone(), 0);
        loop {
            if !is_zero_expr(&subs(&d, &[(x.clone(), r.clone())])?)? || k > m.len() {
                break;
            }
            k += 1;
            d = diff(&d, &x)?;
        }
        for _ in 0..k.max(1) {
            out.push(r.clone());
        }
    }
    if out.len() != m.len() {
        return Err(MathError::unsupported("could not find all the eigenvalues in closed form (try nroots on charpoly)"));
    }
    Ok(out)
}

pub fn rank(m: &Matrix) -> MathResult<usize> {
    let mut work = m.clone();
    let cols = work.first().map_or(0, Vec::len);
    Ok(rref(&mut work, cols)?.len())
}

pub fn det(m: &Matrix) -> MathResult<Expr> {
    let n = m.len();
    if m.iter().any(|r| r.len() != n) {
        return Err(MathError::domain("the determinant needs a square matrix"));
    }
    if m.iter().flatten().all(Expr::is_num) {
        return det_numeric(m);
    }
    if n > MAX_LAPLACE {
        return Err(MathError::unsupported(format!("symbolic determinants are limited to {MAX_LAPLACE}x{MAX_LAPLACE}")));
    }
    expand(&laplace(m)?)
}

fn laplace(m: &Matrix) -> MathResult<Expr> {
    mathcraft_core::budget::tick()?;
    match m.len() {
        0 => Ok(Expr::one()),
        1 => Ok(cell(m, 0, 0)),
        n => {
            let mut terms = Vec::with_capacity(n);
            for j in 0..n {
                let minor: Matrix = m.iter().skip(1).map(|r| r.iter().enumerate().filter(|(k, _)| *k != j).map(|(_, v)| v.clone()).collect()).collect();
                let sign = if j % 2 == 0 { 1 } else { -1 };
                terms.push(Expr::mul(vec![Expr::int(sign), cell(m, 0, j), laplace(&minor)?]));
            }
            Ok(Expr::add(terms))
        }
    }
}

fn det_numeric(m: &Matrix) -> MathResult<Expr> {
    let n = m.len();
    let mut a = m.clone();
    let mut det = Expr::one();
    for c in 0..n {
        let Some(p) = (c..n).find(|&p| !cell(&a, p, c).is_zero()) else { return Ok(Expr::zero()) };
        if p != c {
            a.swap(p, c);
            det = det.neg();
        }
        let pivot = cell(&a, c, c);
        det = Expr::mul(vec![det, pivot.clone()]);
        for i in (c + 1)..n {
            let factor = cell(&a, i, c).div(&pivot);
            for j in c..n {
                let v = Expr::add(vec![cell(&a, i, j), Expr::mul(vec![factor.neg(), cell(&a, c, j)])]);
                set(&mut a, i, j, v);
            }
        }
    }
    Ok(det)
}

pub fn inverse(m: &Matrix) -> MathResult<Matrix> {
    let n = m.len();
    if m.iter().any(|r| r.len() != n) {
        return Err(MathError::domain("only a square matrix has an inverse"));
    }
    let id = identity(n);
    let mut aug: Matrix = m.iter().zip(id.iter()).map(|(a, b)| a.iter().chain(b.iter()).cloned().collect()).collect();
    let pivots = rref(&mut aug, n)?;
    if pivots.len() != n {
        return Err(MathError::domain("the matrix is singular (its determinant is zero)"));
    }
    Ok(aug.into_iter().map(|r| r.into_iter().skip(n).collect()).collect())
}

/// Solve a system of linear equations in `vars`. Each equation is `lhs == rhs` or an expression equal to zero.
/// Returns `None` when the system is inconsistent, otherwise one `var == value` per determined variable.
pub fn solve_linear(eqs: &[Expr], vars: &[Expr]) -> MathResult<Option<Vec<Expr>>> {
    let n = vars.len();
    let zero_rules: Vec<(Expr, Expr)> = vars.iter().map(|v| (v.clone(), Expr::zero())).collect();
    let mut m: Matrix = Vec::with_capacity(eqs.len());
    for eq in eqs {
        let f = match eq.kind() {
            Kind::Eq(l, r) => l.sub(r),
            _ => eq.clone(),
        };
        let f = expand(&f)?;
        let mut row = Vec::with_capacity(n + 1);
        let mut rebuilt = Vec::with_capacity(n + 1);
        for v in vars {
            let c = diff(&f, v)?;
            if vars.iter().any(|w| c.contains(w)) {
                return Err(MathError::unsupported("only linear systems are supported (a variable appears nonlinearly)"));
            }
            rebuilt.push(Expr::mul(vec![c.clone(), v.clone()]));
            row.push(c);
        }
        let constant = subs(&f, &zero_rules)?;
        rebuilt.push(constant.clone());
        if !is_zero_expr(&Expr::add(vec![f.clone(), Expr::add(rebuilt).neg()]))? {
            return Err(MathError::unsupported("only linear systems are supported"));
        }
        row.push(constant.neg());
        m.push(row);
    }
    let pivots = rref(&mut m, n)?;
    // A row of zeros equal to a nonzero constant means no solution.
    for r in 0..m.len() {
        if (0..n).all(|j| cell(&m, r, j).is_zero()) && !cell(&m, r, n).is_zero() {
            return Ok(None);
        }
    }
    let mut out = Vec::new();
    for (r, &c) in pivots.iter().enumerate() {
        let mut value = cell(&m, r, n);
        for j in 0..n {
            if j != c && !pivots.contains(&j) {
                value = Expr::add(vec![value, Expr::mul(vec![cell(&m, r, j).neg(), vars.get(j).cloned().unwrap_or_else(Expr::zero)])]);
            }
        }
        if let Some(v) = vars.get(c) {
            out.push(Expr::eq(v.clone(), expand(&value)?));
        }
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(rows: &[&[i64]]) -> Matrix {
        rows.iter().map(|r| r.iter().map(|v| Expr::int(*v)).collect()).collect()
    }
    fn show(m: &Matrix) -> String {
        from_matrix(m).to_string()
    }

    #[test]
    fn products_and_transpose() {
        let a = m(&[&[1, 2], &[3, 4]]);
        assert_eq!(show(&matmul(&a, &a).unwrap()), "[[7, 10], [15, 22]]");
        assert_eq!(show(&transpose(&a)), "[[1, 3], [2, 4]]");
        assert!(matmul(&a, &m(&[&[1, 2, 3]])).is_err());
    }

    #[test]
    fn determinants() {
        assert_eq!(det(&m(&[&[1, 2], &[3, 4]])).unwrap(), Expr::int(-2));
        assert_eq!(det(&m(&[&[2, 0, 1], &[1, 3, 2], &[1, 1, 1]])).unwrap(), Expr::int(2 + (1 - 3)));
        let (a, b) = (Expr::sym("a"), Expr::sym("b"));
        let sym: Matrix = vec![vec![a.clone(), b.clone()], vec![b.clone(), a.clone()]];
        assert_eq!(det(&sym).unwrap().to_string(), "a**2 - b**2");
    }

    #[test]
    fn inverse_and_rank() {
        let a = m(&[&[1, 2], &[3, 4]]);
        assert_eq!(show(&inverse(&a).unwrap()), "[[-2, 1], [3/2, -1/2]]");
        assert!(inverse(&m(&[&[1, 2], &[2, 4]])).is_err());
        assert_eq!(rank(&m(&[&[1, 2], &[2, 4]])).unwrap(), 1);
    }

    #[test]
    fn linear_systems() {
        let (x, y) = (Expr::sym("x"), Expr::sym("y"));
        let e1 = Expr::eq(Expr::add(vec![x.clone(), y.clone()]), Expr::int(3));
        let e2 = Expr::eq(Expr::add(vec![x.clone(), y.neg()]), Expr::int(1));
        let sol = solve_linear(&[e1, e2], &[x.clone(), y.clone()]).unwrap().unwrap();
        assert_eq!(sol.iter().map(|e| e.to_string()).collect::<Vec<_>>(), vec!["x == 2", "y == 1"]);
        let bad = solve_linear(&[Expr::eq(x.clone(), Expr::int(1)), Expr::eq(x.clone(), Expr::int(2))], &[x]).unwrap();
        assert!(bad.is_none());
    }
}
