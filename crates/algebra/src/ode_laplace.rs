//! Linear ODEs and systems of them with constant coefficients, through the Laplace transform. The constants
//! `C1, C2, ...` are the initial values `f1(0), f1'(0), ..., f2(0), ...` in that order, so any forcing term the
//! transform table covers works (also where variation of parameters gets stuck) and systems need no special case.

use crate::expand::expand;
use crate::laplace::{inverse_laplace, laplace};
use crate::linalg::solve_linear;
use crate::ode::{MAX_ORDER, constant, derivative_order};
use crate::poly::to_poly;
use crate::simplify::simplify;
use crate::util::{guard, require_symbol};
use mathcraft_core::{Expr, Kind, MathError, MathResult};

fn unsupported<T>(what: &str) -> MathResult<T> {
    Err(MathError::unsupported(format!("dsolve cannot solve this system: {what}")))
}

fn lifted(i: usize, k: usize) -> Expr {
    Expr::sym(&format!("_y{i}_{k}"))
}

fn lift(e: &Expr, fs: &[Expr], x: &Expr) -> MathResult<Expr> {
    guard(e)?;
    for (i, f) in fs.iter().enumerate() {
        if let Some(k) = derivative_order(e, f, x) {
            return if k <= MAX_ORDER { Ok(lifted(i, k)) } else { unsupported("the order is too high") };
        }
    }
    e.map_children(|c| lift(c, fs, x))
}

/// Solutions `f_i == ...` of the linear constant-coefficient equations `eqs` in the unknowns `fs` (each `f(x)`).
pub fn dsolve_laplace(eqs: &[Expr], fs: &[Expr], x: &Expr) -> MathResult<Vec<Expr>> {
    require_symbol(x)?;
    if eqs.len() != fs.len() {
        return unsupported("it needs as many equations as unknowns");
    }
    let s = Expr::sym("_s");
    let lifted_eqs: Vec<Expr> = eqs
        .iter()
        .map(|eq| {
            let zero = match eq.kind() {
                Kind::Eq(l, r) => l.sub(r),
                _ => eq.clone(),
            };
            lift(&zero, fs, x)
        })
        .collect::<MathResult<_>>()?;
    // highest derivative of every unknown
    let orders: Vec<usize> =
        (0..fs.len()).map(|i| (0..=MAX_ORDER).rev().find(|k| lifted_eqs.iter().any(|e| e.contains(&lifted(i, *k)))).unwrap_or(0)).collect();
    if orders.contains(&0) {
        return unsupported("an unknown does not appear differentiated");
    }
    // initial values C1.. in order
    let mut initial = Vec::new();
    let mut next = 1;
    for o in &orders {
        let mut row = Vec::new();
        for _ in 0..*o {
            row.push(constant(next));
            next += 1;
        }
        initial.push(row);
    }
    let transforms: Vec<Expr> = (0..fs.len()).map(|i| Expr::sym(&format!("_F{i}"))).collect();
    let mut algebraic = Vec::new();
    for e in &lifted_eqs {
        let mut rest = e.clone();
        let mut total = Vec::new();
        for (i, o) in orders.iter().enumerate() {
            for k in (0..=*o).rev() {
                let sym = lifted(i, k);
                let Some(c) = to_poly(&rest, &sym)? else { return unsupported("it is not linear") };
                if c.len() > 2 {
                    return unsupported("it is not linear");
                }
                let coef = c.get(1).cloned().unwrap_or_else(Expr::zero);
                rest = c.first().cloned().unwrap_or_else(Expr::zero);
                if coef.is_zero() {
                    continue;
                }
                if coef.contains(x) {
                    return unsupported("the coefficients must be constants");
                }
                // coef * (s^k F - sum_{j<k} s^(k-1-j) y^(j)(0))
                let mut term = Expr::mul(vec![Expr::pow(&s, &Expr::int(k as i64)), transforms.get(i).cloned().unwrap_or_else(Expr::zero)]);
                for j in 0..k {
                    let y0 = initial.get(i).and_then(|r| r.get(j)).cloned().unwrap_or_else(Expr::zero);
                    term = Expr::add(vec![term, Expr::mul(vec![Expr::pow(&s, &Expr::int((k - 1 - j) as i64)), y0]).neg()]);
                }
                total.push(Expr::mul(vec![coef, term]));
            }
        }
        if (0..fs.len()).any(|i| (0..=MAX_ORDER).any(|k| rest.contains(&lifted(i, k)))) {
            return unsupported("it is not linear");
        }
        if !rest.is_zero() {
            let Some(g) = laplace(&rest, x, &s)? else { return unsupported("the forcing term has no known Laplace transform") };
            total.push(g);
        }
        algebraic.push(expand(&Expr::add(total))?);
    }
    let Some(sol) = solve_linear(&algebraic, &transforms)? else { return unsupported("the transformed system has no solution") };
    if sol.len() != fs.len() {
        return unsupported("the transformed system is underdetermined");
    }
    let mut out = Vec::new();
    for (f, eq) in fs.iter().zip(sol) {
        let Kind::Eq(_, big) = eq.kind() else { continue };
        let Some(y) = inverse_laplace(big, &s, x)? else { return unsupported("the inverse transform has no closed form") };
        out.push(Expr::eq(f.clone(), simplify(&expand(&y)?)?));
    }
    Ok(out)
}
