//! Polynomial systems: solve several equations in several unknowns by elimination. A variable that appears in more
//! than one equation is removed with resultants, the smaller system is solved first, and each of its solutions is put
//! back to find the eliminated variable, keeping only candidates that satisfy every equation.

use crate::complex::parts;
use crate::expand::expand;
use crate::linalg::{self, Matrix};
use crate::poly::to_poly;
use crate::rational::together;
use crate::solve::solve;
use crate::util::{guard, has_singularity, subs};
use mathcraft_core::numeric::eval_f64;
use mathcraft_core::{Expr, Kind, MathError, MathResult};

const MAX_SOLUTIONS: usize = 256;
const MAX_DEPTH: usize = 12;

fn unsupported(why: &str) -> MathError {
    MathError::unsupported(format!("cannot solve this system: {why}"))
}

/// `lhs - rhs` of an equation, as a polynomial numerator.
fn zero_form(eq: &Expr) -> MathResult<Expr> {
    let f = match eq.kind() {
        Kind::Eq(l, r) => l.sub(r),
        _ => eq.clone(),
    };
    let (num, _) = together(&f)?;
    expand(&num)
}

/// Does `e` vanish? Exactly if the algebra says so, otherwise numerically (complex values included).
fn vanishes(e: &Expr) -> bool {
    let Ok(e) = expand(e) else { return false };
    if e.is_zero() {
        return true;
    }
    if has_singularity(&e) {
        return false;
    }
    let near = |v: &Expr| eval_f64(v, &|_| None).is_ok_and(|n| n.is_finite() && n.abs() < 1e-7);
    parts(&e).is_ok_and(|(re, im)| near(&re) && near(&im))
}

fn degree_in(f: &Expr, v: &Expr) -> MathResult<Option<usize>> {
    Ok(to_poly(f, v)?.map(|c| c.len().saturating_sub(1)))
}

/// Resultant of `f` and `g` with respect to `v`: the determinant of their Sylvester matrix.
fn resultant(f: &Expr, g: &Expr, v: &Expr) -> MathResult<Expr> {
    let (Some(mut a), Some(mut b)) = (to_poly(f, v)?, to_poly(g, v)?) else { return Err(unsupported("only polynomial equations")) };
    a.reverse();
    b.reverse();
    let (m, n) = (a.len() - 1, b.len() - 1);
    if m + n > 12 {
        return Err(MathError::too_large("the equations have too high a degree to eliminate a variable"));
    }
    let size = m + n;
    let mut rows: Matrix = Vec::with_capacity(size);
    for i in 0..n {
        let mut row = vec![Expr::zero(); size];
        for (j, c) in a.iter().enumerate() {
            if let Some(cell) = row.get_mut(i + j) {
                *cell = c.clone();
            }
        }
        rows.push(row);
    }
    for i in 0..m {
        let mut row = vec![Expr::zero(); size];
        for (j, c) in b.iter().enumerate() {
            if let Some(cell) = row.get_mut(i + j) {
                *cell = c.clone();
            }
        }
        rows.push(row);
    }
    expand(&linalg::det(&rows)?)
}

/// All solutions of the polynomial system `eqs == 0` in `vars`, each as values aligned with `vars`.
/// Every variable must be determined: a system with free parameters is refused.
pub fn solve_system(eqs: &[Expr], vars: &[Expr]) -> MathResult<Vec<Vec<Expr>>> {
    let zero_forms = eqs.iter().map(zero_form).collect::<MathResult<Vec<_>>>()?;
    let mut sols = rec(zero_forms, vars.to_vec(), 0)?;
    // The elimination can introduce candidates that fail an original equation (poles): check them all.
    sols.retain(|s| {
        let rules: Vec<(Expr, Expr)> = vars.iter().cloned().zip(s.iter().cloned()).collect();
        eqs.iter().all(|eq| {
            let f = match eq.kind() {
                Kind::Eq(l, r) => l.sub(r),
                _ => eq.clone(),
            };
            subs(&f, &rules).is_ok_and(|v| vanishes(&v))
        })
    });
    // A stable order: by the numeric (real, then imaginary) parts of the values, first unknown first.
    let key = |s: &Vec<Expr>| -> Vec<f64> {
        s.iter()
            .flat_map(|v| {
                let (re, im) = parts(v).unwrap_or_else(|_| (v.clone(), Expr::zero()));
                [re, im].map(|p| eval_f64(&p, &|_| None).unwrap_or(0.0))
            })
            .collect()
    };
    sols.sort_by(|a, b| key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal));
    sols.into_iter().map(|s| s.iter().map(expand).collect()).collect()
}

fn rec(eqs: Vec<Expr>, vars: Vec<Expr>, depth: usize) -> MathResult<Vec<Vec<Expr>>> {
    if depth > MAX_DEPTH {
        return Err(MathError::too_large("the system needs too many elimination steps"));
    }
    let mut live = Vec::new();
    for e in eqs {
        guard(&e)?;
        let e = expand(&e)?;
        if e.is_zero() {
            continue;
        }
        if !vars.iter().any(|v| e.contains(v)) {
            // a constraint with no unknowns: it holds or the system has no solution
            if vanishes(&e) {
                continue;
            }
            return Ok(Vec::new());
        }
        live.push(e);
    }
    let Some(v) = pick_variable(&live, &vars)? else {
        return if vars.is_empty() { Ok(vec![Vec::new()]) } else { Err(unsupported("some unknowns are not determined by the equations")) };
    };
    let rest_vars: Vec<Expr> = vars.iter().filter(|w| **w != v).cloned().collect();
    let (with, without): (Vec<Expr>, Vec<Expr>) = live.iter().cloned().partition(|e| e.contains(&v));
    let mut ranked = Vec::new();
    for e in with {
        match degree_in(&e, &v)? {
            Some(d) => ranked.push((d, e)),
            None => return Err(unsupported("only polynomial equations are supported")),
        }
    }
    ranked.sort_by_key(|(d, _)| *d);
    let eqs_v: Vec<Expr> = ranked.into_iter().map(|(_, e)| e).collect();
    let Some((first, others)) = eqs_v.split_first() else { return Err(unsupported("nothing to eliminate")) };

    // The equations without `v`, plus one resultant per further equation containing it.
    let mut reduced = without;
    for g in others {
        reduced.push(resultant(first, g, &v)?);
    }
    let sub_solutions = if reduced.is_empty() && rest_vars.is_empty() { vec![Vec::new()] } else { rec(reduced, rest_vars.clone(), depth + 1)? };

    let mut out: Vec<Vec<Expr>> = Vec::new();
    for s in sub_solutions {
        let rules: Vec<(Expr, Expr)> = rest_vars.iter().cloned().zip(s.iter().cloned()).collect();
        let mut candidates: Option<Vec<Expr>> = None;
        let mut checks = Vec::new();
        for e in &eqs_v {
            let f = expand(&subs(e, &rules)?)?;
            if f.is_zero() {
                continue;
            }
            if candidates.is_none() {
                candidates = Some(solve(&f, &v)?);
            } else {
                checks.push(f);
            }
        }
        let Some(cands) = candidates else { return Err(unsupported("some unknowns are not determined by the equations")) };
        for r in cands {
            if checks.iter().all(|c| subs(c, &[(v.clone(), r.clone())]).is_ok_and(|x| vanishes(&x))) {
                // assemble in the order of `vars`
                let mut row = Vec::with_capacity(vars.len());
                let mut it = s.iter();
                for w in &vars {
                    row.push(if *w == v { r.clone() } else { it.next().cloned().unwrap_or_else(Expr::zero) });
                }
                if !out.contains(&row) {
                    out.push(row);
                }
            }
        }
        if out.len() > MAX_SOLUTIONS {
            return Err(MathError::too_large("the system has too many solutions"));
        }
    }
    Ok(out)
}

/// The variable to eliminate next: one that sits alone in an equation (so it can be solved for directly), else the
/// one with the lowest total degree across the equations.
fn pick_variable(eqs: &[Expr], vars: &[Expr]) -> MathResult<Option<Expr>> {
    for e in eqs {
        let present: Vec<&Expr> = vars.iter().filter(|v| e.contains(v)).collect();
        if let [only] = present.as_slice() {
            return Ok(Some((*only).clone()));
        }
    }
    let mut best: Option<(usize, Expr)> = None;
    for v in vars {
        let mut cost = 0;
        let mut any = false;
        for e in eqs.iter().filter(|e| e.contains(v)) {
            any = true;
            cost += degree_in(e, v)?.unwrap_or(99);
        }
        if any && best.as_ref().is_none_or(|(c, _)| cost < *c) {
            best = Some((cost, v.clone()));
        }
    }
    Ok(best.map(|(_, v)| v))
}
