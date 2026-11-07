//! Scattering-amplitude algebra (layer L1): traces of Dirac matrices and contraction of Lorentz indices, in four
//! dimensions. Kept apart from the rest of MathCraft so it can become a package of its own.
//!
//! Objects, all ordinary expressions:
//! - `gamma(mu)`: the matrix γ^μ (only meaningful as an argument of [`gamma_trace`]),
//! - `slash(p)`: the matrix p̸ for a momentum symbol `p` (linear: `slash(p + 2*q)` is `slash(p) + 2*slash(q)`),
//! - `metric(mu, nu)`: the metric tensor g^{μν},
//! - `comp(p, mu)`: the component p^μ of a vector,
//! - `pdot(p, q)`: the Lorentz product p·q.
//!
//! The trace uses the standard recursion `Tr[a1 a2 ... an] = Σ_k (-1)^k (a1·ak) Tr[the rest]` with `Tr[1] = 4`, which
//! follows from the anticommutation relation `{γ^μ, γ^ν} = 2 g^{μν}` and the cyclic property of the trace.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use mathcraft_algebra::expand;
use mathcraft_core::{Expr, Kind, MathError, MathResult, budget};

/// Longest chain of matrices in one trace (the number of terms grows like `(n-1)!!`).
const MAX_CHAIN: usize = 12;
/// Most terms a trace or a contraction may produce before it is refused.
const MAX_TERMS: usize = 200_000;

pub fn gamma(index: &Expr) -> Expr {
    Expr::call("gamma", vec![index.clone()])
}

pub fn slash(momentum: &Expr) -> Expr {
    Expr::call("slash", vec![momentum.clone()])
}

fn sorted_pair(name: &str, a: &Expr, b: &Expr) -> Expr {
    if a <= b { Expr::call(name, vec![a.clone(), b.clone()]) } else { Expr::call(name, vec![b.clone(), a.clone()]) }
}

pub fn metric(a: &Expr, b: &Expr) -> Expr {
    sorted_pair("metric", a, b)
}

pub fn pdot(p: &Expr, q: &Expr) -> Expr {
    sorted_pair("pdot", p, q)
}

pub fn comp(p: &Expr, index: &Expr) -> Expr {
    Expr::call("comp", vec![p.clone(), index.clone()])
}

/// A matrix in a chain: an index (γ^μ) or a momentum (p̸).
#[derive(Clone)]
enum Item {
    Index(Expr),
    Momentum(Expr),
}

/// The scalar `a·b` for two chain items: metric, component or dot product.
fn pair(a: &Item, b: &Item) -> Expr {
    match (a, b) {
        (Item::Index(i), Item::Index(j)) => metric(i, j),
        (Item::Momentum(p), Item::Momentum(q)) => pdot(p, q),
        (Item::Momentum(p), Item::Index(i)) | (Item::Index(i), Item::Momentum(p)) => comp(p, i),
    }
}

fn is_symbol(e: &Expr) -> bool {
    matches!(e.kind(), Kind::Sym(_))
}

/// Rewrite every `slash(a*p + b*q)` as `a*slash(p) + b*slash(q)`.
fn linearize(e: &Expr) -> MathResult<Expr> {
    budget::tick()?;
    if let Kind::Call(name, args) = e.kind()
        && &**name == "slash"
        && let [arg] = args.as_slice()
    {
        let inner = expand(arg)?;
        let terms: Vec<Expr> = match inner.kind() {
            Kind::Add(t) => t.clone(),
            _ => vec![inner.clone()],
        };
        let mut out = Vec::new();
        for t in terms {
            let (coeff, rest) = t.split_coeff();
            if !is_symbol(&rest) {
                return Err(MathError::domain("slash() takes a momentum symbol, or a sum of them such as slash(p + q)"));
            }
            out.push(Expr::mul(vec![Expr::num(coeff), slash(&rest)]));
        }
        return Ok(Expr::add(out));
    }
    e.map_children(linearize)
}

/// One term of a chain argument: a scalar coefficient times at most one matrix.
fn split_term(term: &Expr) -> MathResult<(Expr, Option<Item>)> {
    let factors: Vec<Expr> = match term.kind() {
        Kind::Mul(f) => f.clone(),
        _ => vec![term.clone()],
    };
    let mut coeff = Vec::new();
    let mut item = None;
    for f in factors {
        let found = match f.kind() {
            Kind::Call(name, args) => match (&**name, args.as_slice()) {
                ("gamma", [i]) => Some(Item::Index(i.clone())),
                ("slash", [p]) => Some(Item::Momentum(p.clone())),
                _ => None,
            },
            _ => None,
        };
        match found {
            Some(i) if item.is_none() => item = Some(i),
            Some(_) => {
                return Err(MathError::domain("write each gamma matrix as a separate argument of gamma_trace, in order"));
            }
            None => coeff.push(f),
        }
    }
    Ok((Expr::mul(coeff), item))
}

/// Every complete pairing of the chain, with its sign, as a list of products.
fn pairings(chain: &[Item], sign: i64, factors: &mut Vec<Expr>, out: &mut Vec<Expr>) -> MathResult<()> {
    budget::tick()?;
    let Some((first, rest)) = chain.split_first() else {
        let mut f = factors.clone();
        f.push(Expr::int(sign));
        out.push(Expr::mul(f));
        if out.len() > MAX_TERMS {
            return Err(MathError::too_large("this trace has too many terms"));
        }
        return Ok(());
    };
    for k in 0..rest.len() {
        let mut others: Vec<Item> = rest.to_vec();
        let partner = others.remove(k);
        factors.push(pair(first, &partner));
        let s = if k % 2 == 0 { sign } else { -sign };
        pairings(&others, s, factors, out)?;
        factors.pop();
    }
    Ok(())
}

/// The trace of a product of matrices, written as the list `items` in order. Each item may be a sum, for instance
/// `slash(p) + m` for a propagator numerator. The result is a sum of products of `metric`, `comp` and `pdot`.
pub fn gamma_trace(items: &[Expr]) -> MathResult<Expr> {
    // each item becomes a list of (coefficient, optional matrix)
    let mut options: Vec<Vec<(Expr, Option<Item>)>> = Vec::new();
    for item in items {
        let e = expand(&linearize(item)?)?;
        let terms: Vec<Expr> = match e.kind() {
            Kind::Add(t) => t.clone(),
            _ => vec![e.clone()],
        };
        options.push(terms.iter().map(split_term).collect::<MathResult<Vec<_>>>()?);
    }
    let mut total = Vec::new();
    let mut choice = vec![0_usize; options.len()];
    loop {
        budget::tick()?;
        let mut coeff = Vec::new();
        let mut chain = Vec::new();
        for (o, c) in options.iter().zip(&choice) {
            if let Some((k, m)) = o.get(*c) {
                coeff.push(k.clone());
                if let Some(m) = m {
                    chain.push(m.clone());
                }
            }
        }
        if chain.len() > MAX_CHAIN {
            return Err(MathError::too_large(format!("a trace of more than {MAX_CHAIN} matrices is not supported")));
        }
        if chain.len() % 2 == 0 {
            let mut terms = Vec::new();
            pairings(&chain, 1, &mut Vec::new(), &mut terms)?;
            // Tr[1] = 4 for the empty remainder
            coeff.push(Expr::int(4));
            coeff.push(Expr::add(terms));
            total.push(Expr::mul(coeff));
            if total.len() > MAX_TERMS {
                return Err(MathError::too_large("this trace has too many terms"));
            }
        }
        // advance the odometer
        let mut i = options.len();
        loop {
            if i == 0 {
                return Ok(Expr::add(total));
            }
            i -= 1;
            let len = options.get(i).map_or(0, Vec::len);
            let Some(c) = choice.get_mut(i) else { return Ok(Expr::add(total)) };
            *c += 1;
            if *c < len {
                break;
            }
            *c = 0;
        }
    }
}

/// A tensor factor that carries indices.
#[derive(Clone)]
enum Atom {
    Metric(Expr, Expr),
    Comp(Expr, Expr),
}

impl Atom {
    fn indices(&self) -> Vec<&Expr> {
        match self {
            Atom::Metric(a, b) => vec![a, b],
            Atom::Comp(_, i) => vec![i],
        }
    }
    fn replace(&mut self, from: &Expr, to: &Expr) {
        let swap = |e: &mut Expr| {
            if *e == *from {
                *e = to.clone();
            }
        };
        match self {
            Atom::Metric(a, b) => {
                swap(a);
                swap(b);
            }
            Atom::Comp(_, i) => swap(i),
        }
    }
    fn to_expr(&self) -> Expr {
        match self {
            Atom::Metric(a, b) => metric(a, b),
            Atom::Comp(p, i) => comp(p, i),
        }
    }
}

fn as_atom(e: &Expr) -> Option<Atom> {
    let Kind::Call(name, args) = e.kind() else { return None };
    match (&**name, args.as_slice()) {
        ("metric", [a, b]) => Some(Atom::Metric(a.clone(), b.clone())),
        ("comp", [p, i]) => Some(Atom::Comp(p.clone(), i.clone())),
        _ => None,
    }
}

fn contract_term(term: &Expr) -> MathResult<Expr> {
    let factors: Vec<Expr> = match term.kind() {
        Kind::Mul(f) => f.clone(),
        _ => vec![term.clone()],
    };
    let mut scalars = Vec::new();
    let mut atoms: Vec<Atom> = Vec::new();
    for f in factors {
        if let Some(a) = as_atom(&f) {
            atoms.push(a);
            continue;
        }
        if let Kind::Pow(base, exp) = f.kind()
            && let Some(a) = as_atom(base)
            && let Some(n) = exp.as_i64()
            && (1..=16).contains(&n)
        {
            for _ in 0..n {
                atoms.push(a.clone());
            }
            continue;
        }
        scalars.push(f);
    }
    loop {
        budget::tick()?;
        // find an index that occurs at least twice
        let mut found: Option<(usize, usize, Expr)> = None;
        'search: for (i, a) in atoms.iter().enumerate() {
            for idx in a.indices() {
                for (j, b) in atoms.iter().enumerate() {
                    let count = b.indices().iter().filter(|x| ***x == *idx).count();
                    let same_atom_twice = i == j && a.indices().iter().filter(|x| ***x == *idx).count() >= 2;
                    if (i != j && count >= 1) || same_atom_twice {
                        found = Some((i, j, idx.clone()));
                        break 'search;
                    }
                }
            }
        }
        let Some((i, j, idx)) = found else { break };
        if i == j {
            // g(mu, mu) = 4
            atoms.remove(i);
            scalars.push(Expr::int(4));
            continue;
        }
        // prefer to eliminate a metric
        let (Some(ai), Some(aj)) = (atoms.get(i).cloned(), atoms.get(j).cloned()) else { break };
        let ((m, metric_atom), (o, other_atom)) = if matches!(ai, Atom::Metric(..)) { ((i, ai), (j, aj)) } else { ((j, aj), (i, ai)) };
        match (metric_atom, other_atom) {
            (Atom::Metric(a, b), mut other) => {
                let partner = if a == idx { b } else { a };
                other.replace(&idx, &partner);
                if let Some(slot) = atoms.get_mut(o) {
                    *slot = other;
                }
                atoms.remove(m);
            }
            (Atom::Comp(p, _), Atom::Comp(q, _)) => {
                scalars.push(pdot(&p, &q));
                atoms.remove(i.max(j));
                atoms.remove(i.min(j));
            }
            // a component against a metric is handled above, so this cannot occur
            _ => return Err(MathError::domain("an index appears more than twice in a product")),
        }
    }
    let mut out = scalars;
    out.extend(atoms.iter().map(Atom::to_expr));
    Ok(Expr::mul(out))
}

/// Sum over repeated Lorentz indices: `metric(mu, mu)` is 4, `metric(mu, nu)*comp(p, mu)` is `comp(p, nu)` and
/// `comp(p, mu)*comp(q, mu)` is `pdot(p, q)`. Expands products first.
pub fn contract(e: &Expr) -> MathResult<Expr> {
    let e = expand(e)?;
    let terms: Vec<Expr> = match e.kind() {
        Kind::Add(t) => t.clone(),
        _ => vec![e.clone()],
    };
    if terms.len() > MAX_TERMS {
        return Err(MathError::too_large("too many terms to contract"));
    }
    let done = terms.iter().map(contract_term).collect::<MathResult<Vec<_>>>()?;
    expand(&Expr::add(done))
}

#[cfg(test)]
mod tests;
