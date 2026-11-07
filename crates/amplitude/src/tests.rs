#![allow(clippy::unwrap_used)]

use super::*;

fn s(n: &str) -> Expr {
    Expr::sym(n)
}

fn dot(a: &str, b: &str) -> Expr {
    pdot(&s(a), &s(b))
}

#[test]
fn trace_of_two_slashes_is_four_dot() {
    let t = gamma_trace(&[slash(&s("p")), slash(&s("q"))]).unwrap();
    assert_eq!(t, Expr::mul(vec![Expr::int(4), dot("p", "q")]));
}

#[test]
fn trace_of_two_gammas_is_four_metric() {
    let t = gamma_trace(&[gamma(&s("mu")), gamma(&s("nu"))]).unwrap();
    assert_eq!(t, Expr::mul(vec![Expr::int(4), metric(&s("mu"), &s("nu"))]));
}

#[test]
fn odd_traces_vanish() {
    let t = gamma_trace(&[slash(&s("p")), slash(&s("q")), slash(&s("k"))]).unwrap();
    assert!(t.is_zero());
}

#[test]
fn four_slash_trace_matches_the_standard_formula() {
    let t = gamma_trace(&[slash(&s("a")), slash(&s("b")), slash(&s("c")), slash(&s("d"))]).unwrap();
    let want = Expr::mul(vec![
        Expr::int(4),
        Expr::add(vec![
            Expr::mul(vec![dot("a", "b"), dot("c", "d")]),
            Expr::mul(vec![Expr::int(-1), dot("a", "c"), dot("b", "d")]),
            Expr::mul(vec![dot("a", "d"), dot("b", "c")]),
        ]),
    ]);
    assert_eq!(expand(&t).unwrap(), expand(&want).unwrap());
}

#[test]
fn mass_terms_are_kept() {
    // Tr[(p-slash + m)(q-slash + m)] = 4 p.q + 4 m^2
    let m = s("m");
    let t = gamma_trace(&[Expr::add(vec![slash(&s("p")), m.clone()]), Expr::add(vec![slash(&s("q")), m.clone()])]).unwrap();
    let want = Expr::add(vec![Expr::mul(vec![Expr::int(4), dot("p", "q")]), Expr::mul(vec![Expr::int(4), Expr::pow(&m, &Expr::int(2))])]);
    assert_eq!(expand(&t).unwrap(), expand(&want).unwrap());
}

#[test]
fn slash_of_a_sum_is_linear() {
    let p_plus_q = Expr::add(vec![s("p"), s("q")]);
    let t = gamma_trace(&[slash(&p_plus_q), slash(&s("k"))]).unwrap();
    let want = Expr::mul(vec![Expr::int(4), Expr::add(vec![dot("p", "k"), dot("q", "k")])]);
    assert_eq!(expand(&t).unwrap(), expand(&want).unwrap());
}

#[test]
fn contraction_rules() {
    let (mu, nu) = (s("mu"), s("nu"));
    assert_eq!(contract(&metric(&mu, &mu)).unwrap(), Expr::int(4));
    assert_eq!(contract(&Expr::mul(vec![metric(&mu, &nu), comp(&s("p"), &mu)])).unwrap(), comp(&s("p"), &nu));
    assert_eq!(contract(&Expr::mul(vec![comp(&s("p"), &mu), comp(&s("q"), &mu)])).unwrap(), dot("p", "q"));
    assert_eq!(contract(&Expr::mul(vec![metric(&mu, &nu), metric(&mu, &nu)])).unwrap(), Expr::int(4));
}

#[test]
fn gamma_contraction_identity() {
    // Tr[γ^μ a γ_μ b] = -2 Tr[a b] = -8 a.b
    let mu = s("mu");
    let t = gamma_trace(&[gamma(&mu), slash(&s("a")), gamma(&mu), slash(&s("b"))]).unwrap();
    let c = contract(&t).unwrap();
    assert_eq!(c, Expr::mul(vec![Expr::int(-8), dot("a", "b")]));
}

#[test]
fn electron_positron_to_muons_massless() {
    // (1/4) Σ|M|² = (e^4 / s^2) (1/4) L^{μν} H_{μν} with L = Tr[p2 γ^μ p1 γ^ν], H = Tr[p3 γ_μ p4 γ_ν]
    let (mu, nu) = (s("mu"), s("nu"));
    let l = gamma_trace(&[slash(&s("p2")), gamma(&mu), slash(&s("p1")), gamma(&nu)]).unwrap();
    let h = gamma_trace(&[slash(&s("p3")), gamma(&mu), slash(&s("p4")), gamma(&nu)]).unwrap();
    let lh = contract(&Expr::mul(vec![l, h])).unwrap();
    // 32 [(p1.p3)(p2.p4) + (p1.p4)(p2.p3)]
    let want =
        Expr::mul(vec![Expr::int(32), Expr::add(vec![Expr::mul(vec![dot("p1", "p3"), dot("p2", "p4")]), Expr::mul(vec![dot("p1", "p4"), dot("p2", "p3")])])]);
    assert_eq!(lh, expand(&want).unwrap());
}

#[test]
fn hostile_inputs_are_errors() {
    let many: Vec<Expr> = (0..20).map(|i| slash(&s(&format!("p{i}")))).collect();
    assert!(gamma_trace(&many).is_err());
    assert!(gamma_trace(&[slash(&Expr::int(3))]).is_err());
    assert!(gamma_trace(&[Expr::mul(vec![gamma(&s("a")), gamma(&s("b"))])]).is_err());
}
