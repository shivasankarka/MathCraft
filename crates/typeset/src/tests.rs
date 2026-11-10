use super::*;

struct Fixed;
impl Metrics for Fixed {
    fn advance(&self, text: &str, size: f32) -> f32 {
        0.5 * size * text.chars().count() as f32
    }
}

fn x() -> Expr {
    Expr::sym("x")
}
fn n(v: i64) -> Expr {
    Expr::int(v)
}
fn lay(e: &Expr) -> Layout {
    let l = layout(&Doc::Expr(e.clone()), &Fixed, 20.0);
    assert!(l.is_some(), "no layout for {e}");
    l.unwrap_or_default()
}
fn texts(l: &Layout) -> Vec<String> {
    l.items
        .iter()
        .filter_map(|i| match i {
            Item::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}
fn rules(l: &Layout) -> usize {
    l.items.iter().filter(|i| matches!(i, Item::Rule { .. })).count()
}
fn strokes(l: &Layout) -> usize {
    l.items.iter().filter(|i| matches!(i, Item::Stroke { .. })).count()
}

/// Every drawn item must sit inside the reported bounds, and all numbers must be finite.
fn assert_inside(l: &Layout, what: &str) {
    let eps = 0.5;
    assert!(l.width.is_finite() && l.ascent.is_finite() && l.descent.is_finite(), "{what}: bounds not finite");
    for item in &l.items {
        match item {
            Item::Text { x, y, size, .. } => {
                assert!(x.is_finite() && y.is_finite(), "{what}: text position");
                assert!(*x >= -eps && *x <= l.width + eps, "{what}: text x {x} outside 0..{}", l.width);
                assert!(*y - 0.74 * size >= -l.ascent - eps && *y + 0.0 <= l.descent + 0.22 * size + eps, "{what}: text y {y}");
            }
            Item::Rule { x, y, w, h, .. } => {
                assert!(*x >= -eps && x + w <= l.width + eps, "{what}: rule x");
                assert!(*y >= -l.ascent - eps && y + h <= l.descent + eps, "{what}: rule y");
            }
            Item::Stroke { points, .. } => {
                for [px, py] in points {
                    assert!(*px >= -eps && *px <= l.width + eps, "{what}: stroke x {px}");
                    assert!(*py >= -l.ascent - eps && *py <= l.descent + eps, "{what}: stroke y {py}");
                }
            }
        }
    }
}

#[test]
fn variables_are_math_italic_and_constants_upright() {
    assert_eq!(texts(&lay(&x())), vec!["𝑥"]);
    assert_eq!(texts(&lay(&Expr::sym("h"))), vec!["ℎ"]);
    assert_eq!(texts(&lay(&Expr::pi())), vec!["π"]);
    assert_eq!(texts(&lay(&Expr::e())), vec!["e"]);
    assert_eq!(texts(&lay(&Expr::sym("speed"))), vec!["speed"]);
}

#[test]
fn greek_names_and_subscripts() {
    assert_eq!(texts(&lay(&Expr::sym("alpha"))), vec!["𝛼"]);
    assert_eq!(texts(&lay(&Expr::sym("Omega"))), vec!["Ω"]);
    let l = lay(&Expr::sym("x1"));
    assert_eq!(texts(&l), vec!["𝑥", "1"]);
    let l = lay(&Expr::sym("theta_n"));
    assert_eq!(texts(&l), vec!["𝜃", "𝑛"]);
    // a subscript is smaller and lower than its base
    if let [Item::Text { y: y0, size: s0, .. }, Item::Text { y: y1, size: s1, .. }] = l.items.as_slice() {
        assert!(y1 > y0 && s1 < s0);
    } else {
        assert!(l.items.len() == 2, "expected base and subscript");
    }
    // multi-letter names keep their digits
    assert_eq!(texts(&lay(&Expr::sym("speed2"))), vec!["speed2"]);
}

#[test]
fn a_rational_is_a_stacked_fraction() {
    let l = lay(&Expr::rational(1, 3).unwrap_or_else(Expr::one));
    assert_eq!(rules(&l), 1);
    let ys: Vec<f32> = l
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Text { y, .. } => Some(*y),
            _ => None,
        })
        .collect();
    assert!(ys.len() == 2 && ys.iter().any(|y| *y < 0.0) && ys.iter().any(|y| *y > 0.0), "numerator above, denominator below: {ys:?}");
    assert_inside(&l, "1/3");
}

#[test]
fn a_power_has_a_raised_smaller_exponent() {
    let l = lay(&Expr::pow(&x(), &n(2)));
    if let [Item::Text { y: y0, size: s0, .. }, Item::Text { y: y1, size: s1, text, .. }] = l.items.as_slice() {
        assert_eq!(text, "2");
        assert!(y1 < y0 && s1 < s0);
    } else {
        assert!(l.items.len() == 2, "expected base and exponent, got {:?}", l.items);
    }
}

#[test]
fn roots_and_exponentials() {
    let l = lay(&Expr::pow(&n(2), &Expr::half()));
    assert_eq!(strokes(&l), 1);
    assert_eq!(texts(&l), vec!["2"]);
    // exp(x) is e^x
    let l = lay(&Expr::pow(&Expr::e(), &x()));
    assert_eq!(texts(&l), vec!["e", "𝑥"]);
}

#[test]
fn cube_roots_have_an_index() {
    let l = lay(&Expr::pow(&x(), &Expr::rational(1, 3).unwrap()));
    assert_eq!(strokes(&l), 1);
    assert_eq!(texts(&l), vec!["𝑥", "3"]);
    assert_inside(&l, "cube root");
    // other fractions stay powers
    let l = lay(&Expr::pow(&x(), &Expr::rational(2, 3).unwrap()));
    assert_eq!(strokes(&l), 0);
}

#[test]
fn sums_use_a_real_minus_and_keep_display_order() {
    let e = Expr::add(vec![Expr::pow(&x(), &n(2)), Expr::mul(vec![n(-3), x()]), n(2)]);
    let t = texts(&lay(&e));
    assert_eq!(t, vec!["𝑥", "2", "\u{2212}", "3", "𝑥", "+", "2"], "{t:?}");
}

#[test]
fn sums_in_a_base_get_parentheses() {
    let e = Expr::pow(&Expr::add(vec![x(), n(1)]), &n(2));
    let t = texts(&lay(&e));
    assert_eq!(t, vec!["(", "𝑥", "+", "1", ")", "2"], "{t:?}");
}

#[test]
fn trig_powers_read_sin_squared_of_x() {
    let e = Expr::pow(&Expr::call("sin", vec![x()]), &n(2));
    let t = texts(&lay(&e));
    assert_eq!(t, vec!["sin", "2", "(", "𝑥", ")"], "{t:?}");
}

#[test]
fn function_names_are_tidied() {
    let t = texts(&lay(&Expr::call("asin", vec![x()])));
    assert_eq!(t[0], "arcsin");
    let l = lay(&Expr::call("abs", vec![x()]));
    assert_eq!(strokes(&l) + texts(&l).iter().filter(|s| *s == "|").count(), 2);
}

#[test]
fn quotients_and_negative_powers_become_fractions() {
    let e = Expr::div(&Expr::add(vec![x(), n(1)]), &Expr::pow(&x(), &n(2)));
    let l = lay(&e);
    assert_eq!(rules(&l), 1);
    assert_inside(&l, "(x+1)/x^2");
    let one_over_x = Expr::pow(&x(), &n(-1));
    assert_eq!(rules(&lay(&one_over_x)), 1);
    assert!(texts(&lay(&one_over_x)).contains(&"1".to_string()));
}

#[test]
fn matrices_have_tall_brackets_and_aligned_columns() {
    let row = |a: i64, b: i64| Expr::list(vec![n(a), n(b)]);
    let l = lay(&Expr::list(vec![row(1, 22), row(333, 4)]));
    assert_eq!(strokes(&l), 2, "two bracket strokes");
    assert_eq!(texts(&l).len(), 4);
    assert_inside(&l, "matrix");
    // a plain list is not a matrix
    let l = lay(&Expr::list(vec![n(1), n(2), n(3)]));
    assert_eq!(texts(&l), vec!["[", "1", ",", "2", ",", "3", "]"]);
}

#[test]
fn floats_use_scientific_layout() {
    let t = texts(&lay(&Expr::float(1.5e-10)));
    assert_eq!(t.first().map(String::as_str), Some("1.5"), "{t:?}");
    assert!(t.iter().any(|s| s.contains("10")) && t.iter().any(|s| s.contains('\u{2212}')), "{t:?}");
}

#[test]
fn everything_stays_inside_its_bounds() {
    let sinx = Expr::call("sin", vec![x()]);
    let samples = [
        Expr::add(vec![Expr::pow(&x(), &n(3)), Expr::mul(vec![Expr::rational(-1, 2).unwrap_or_else(Expr::one), x()]), n(7)]),
        Expr::div(&sinx, &Expr::add(vec![Expr::pow(&x(), &n(2)), n(1)])),
        Expr::pow(&Expr::add(vec![x(), Expr::sym("y")]), &Expr::rational(3, 2).unwrap_or_else(Expr::one)),
        Expr::mul(vec![Expr::pow(&Expr::e(), &Expr::mul(vec![n(-1), Expr::pow(&x(), &n(2))])), sinx.clone()]),
        Expr::pow(&Expr::add(vec![Expr::pow(&x(), &Expr::half()), n(1)]), &n(2)),
        Expr::eq(Expr::sym("y"), Expr::add(vec![x(), n(1)])),
    ];
    for (i, e) in samples.iter().enumerate() {
        assert_inside(&lay(e), &format!("sample {i}: {e}"));
    }
}

#[test]
fn huge_or_deep_input_gives_up_instead_of_hanging() {
    let big = Expr::add((0..100_000).map(|i| Expr::mul(vec![n(i + 2), Expr::sym(&format!("v{i}"))])).collect());
    assert!(layout(&Doc::Expr(big), &Fixed, 20.0).is_none());
    let mut deep = x();
    for _ in 0..400 {
        deep = Expr::call("f", vec![deep]);
    }
    assert!(layout(&Doc::Expr(deep), &Fixed, 20.0).is_none());
}

#[test]
fn documents_for_values() {
    let d = Doc::List(vec![Doc::Expr(n(1)), Doc::Text("True".into())]);
    let l = layout(&d, &Fixed, 20.0).unwrap_or_default();
    assert_eq!(texts(&l), vec!["[", "1", ",", "True", "]"]);
    let m = Doc::Matrix(vec![vec![Doc::Expr(n(1)), Doc::Expr(n(2))], vec![Doc::Expr(n(3)), Doc::Expr(n(4))]]);
    assert_eq!(strokes(&layout(&m, &Fixed, 20.0).unwrap_or_default()), 2);
}

#[test]
fn imaginary_unit_is_an_upright_i() {
    let z = Expr::add(vec![n(3), Expr::mul(vec![n(4), Expr::imag()])]);
    let t = texts(&lay(&z));
    assert!(t.iter().any(|s| s == "i"), "{t:?}");
    assert!(t.iter().any(|s| s == "3"), "{t:?}");
}

#[test]
fn long_sums_wrap_into_lines() {
    let terms: Vec<Expr> = (1..=12).map(|k| Expr::mul(vec![n(k + 1), Expr::pow(&x(), &n(k))])).collect();
    let e = Expr::add(terms);
    let doc = Doc::Expr(e.clone());
    let whole = layout(&doc, &Fixed, 20.0).unwrap_or_default();
    let wrapped = layout_wrapped(&doc, &Fixed, 20.0, whole.width / 3.0).unwrap_or_default();
    // the same glyphs, no wider than asked (the indent and one term's overshoot aside), and taller than before
    assert_eq!(texts(&wrapped).len(), texts(&whole).len());
    assert!(wrapped.width <= whole.width / 3.0 + 60.0, "{} vs {}", wrapped.width, whole.width);
    assert!(wrapped.height() > 2.0 * whole.height());
    assert_inside(&wrapped, "wrapped sum");
    // a sum that fits, and a product that does not, are not touched
    let fits = layout_wrapped(&doc, &Fixed, 20.0, whole.width + 1.0).unwrap_or_default();
    assert_eq!(fits.items.len(), whole.items.len());
    assert!((fits.height() - whole.height()).abs() < 0.01);
}
