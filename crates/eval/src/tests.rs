use crate::{Value, run_source};
use mathcraft_core::budget::{self, Budget};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Run source in a fresh environment and show the final value (or the error) as text.
fn ev(src: &str) -> String {
    let mut g = HashMap::new();
    show(run_source(&mut g, src, false))
}

fn show(o: crate::Outcome) -> String {
    match o.result {
        Ok(Some(v)) => v.to_string(),
        Ok(None) => o.stdout.trim_end().to_string(),
        Err(e) => format!("ERROR {e}"),
    }
}

#[test]
fn exact_arithmetic() {
    assert_eq!(ev("1/3 + 1/6"), "1/2");
    assert_eq!(ev("2**100"), "1267650600228229401496703205376");
    assert_eq!(ev("sqrt(8)"), "2*sqrt(2)");
    assert_eq!(ev("0.1 + 0.2"), "0.30000000000000004");
    assert_eq!(ev("7 // 2"), "3");
    assert_eq!(ev("-7 // 2"), "-4");
    assert_eq!(ev("-7 % 3"), "2");
    assert_eq!(ev("1/0"), "ERROR line 1: division by zero");
}

#[test]
fn undefined_names_are_symbols() {
    assert_eq!(ev("x + x"), "2*x");
    assert_eq!(ev("(x + 1)**2"), "(x + 1)**2");
    assert_eq!(ev("expand((x + 1)**2)"), "x**2 + 2*x + 1");
    assert_eq!(ev("((x + 1)**2).expand()"), "x**2 + 2*x + 1");
}

#[test]
fn calculus_calls() {
    assert_eq!(ev("diff(x**3 + sin(x), x)"), "3*x**2 + cos(x)");
    assert_eq!(ev("diff(x**4, x, 2)"), "12*x**2");
    assert_eq!(ev("diff(x**2)"), "2*x");
    assert_eq!(ev("integrate(x**2, x)"), "x**3/3");
    assert_eq!(ev("integrate(sin(x)**2 + cos(x)**2, (x, 0, pi))"), "pi");
    assert_eq!(ev("integrate(x**2, (x, 0, 3))"), "9");
    assert_eq!(ev("limit(sin(x)/x, x, 0)"), "1");
    assert_eq!(ev("series(exp(x), x, 0, 4)"), "x**4/24 + x**3/6 + x**2/2 + x + 1");
}

#[test]
fn solving() {
    assert_eq!(ev("solve(x**2 - 4, x)"), "[-2, 2]");
    assert_eq!(ev("solve(Eq(x**2, 2), x)"), "[-sqrt(2), sqrt(2)]");
    assert_eq!(ev("solve([x + y == 3, x - y == 1], [x, y])"), "[x == 2, y == 1]");
    assert_eq!(ev("subs(x**2 + y, [x == 3, y == 1])"), "10");
    assert_eq!(ev("subs(x**2, x, 3)"), "9");
}

#[test]
fn factoring_and_simplifying() {
    assert_eq!(ev("factor(x**2 - 1)"), "(x - 1)*(x + 1)");
    assert_eq!(ev("simplify(sin(x)**2 + cos(x)**2)"), "1");
    assert_eq!(ev("N(sqrt(2))"), "1.4142135623730951");
    assert_eq!(ev("float(pi)"), "3.141592653589793");
}

#[test]
fn matrices() {
    assert_eq!(ev("A = [[1, 2], [3, 4]]\ndet(A)"), "-2");
    assert_eq!(ev("A = [[1, 2], [3, 4]]\nA @ A"), "[[7, 10], [15, 22]]");
    assert_eq!(ev("inv([[1, 2], [3, 4]])"), "[[-2, 1], [3/2, -1/2]]");
}

#[test]
fn python_control_flow_and_functions() {
    assert_eq!(ev("def f(n):\n    if n < 2:\n        return n\n    return f(n - 1) + f(n - 2)\nf(15)"), "610");
    assert_eq!(ev("total = 0\nfor i in range(5):\n    total += i * i\ntotal"), "30");
    assert_eq!(ev("[i**2 for i in range(6) if i % 2 == 0]"), "[0, 4, 16]");
    assert_eq!(ev("sq = lambda t: t * t\nsq(7)"), "49");
    assert_eq!(ev("a, b = 3, 4\nsqrt(a**2 + b**2)"), "5");
    assert_eq!(ev("print('hi', 1 + 1)"), "hi 2");
    assert_eq!(ev("i = 0\nwhile i < 3:\n    i += 1\ni"), "3");
    assert_eq!(ev("sum([1/n for n in range(1, 5)])"), "25/12");
    assert_eq!(ev("def g(x, k=2):\n    return x**k\ng(3) + g(2, k=3)"), "17");
}

#[test]
fn closures_capture_the_enclosing_function() {
    assert_eq!(ev("def adder(n):\n    return lambda x: x + n\nadder(5)(10)"), "15");
}

#[test]
fn comparisons_are_symbolic_or_decided() {
    assert_eq!(ev("3 > 2"), "True");
    assert_eq!(ev("x == x"), "True");
    assert_eq!(ev("x**2 == 4"), "x**2 == 4");
    assert_eq!(ev("sin(pi/7)**2 + cos(pi/7)**2 == 1"), "True");
    assert!(ev("x < 3").starts_with("ERROR"));
    assert!(ev("if x == 3:\n    pass").contains("equation"));
}

#[test]
fn declared_symbols_work_as_unknown_functions() {
    assert_eq!(ev("f = symbols('f')\nf(x) + f(x)"), "2*f(x)");
    assert_eq!(ev("x, y = symbols('x y')\nx * y"), "x*y");
    assert!(ev("foo(3)").contains("not defined"));
    assert!(ev("sinn(3)").contains("Did you mean 'sin'"));
}

#[test]
fn helpful_errors_with_line_numbers() {
    assert_eq!(ev("y = 2 ^ 3\ny"), "8", "^ is a power");
    assert_eq!(ev("a = 1\nb = [1, 2][5]"), "ERROR line 2: index 5 is out of range (length 2)");
    assert!(ev("import numpy").contains("imports"));
    assert!(ev("len(3)").contains("no length"));
    assert!(ev("diff(x**2, 3)").contains("symbol"));
}

#[test]
fn failed_cells_do_not_commit_assignments() {
    let mut g = HashMap::new();
    let o = run_source(&mut g, "a = 1\nb = 1/0", false);
    assert!(o.result.is_err());
    assert!(g.is_empty());
    let o = run_source(&mut g, "a = 1\nb = 2", false);
    assert!(o.result.is_ok());
    assert_eq!(g.get("b"), Some(&Value::int(2)));
}

#[test]
fn infinite_loops_stop_at_the_step_budget() {
    let _g = budget::install(Budget::new(None, Arc::new(AtomicBool::new(false)), 50_000));
    let out = ev("while True:\n    pass");
    assert!(out.contains("step budget"), "{out}");
}

#[test]
fn runaway_recursion_is_an_error_not_a_crash() {
    let out = ev("def f(n):\n    return f(n + 1)\nf(0)");
    assert!(out.contains("recursion"), "{out}");
}

#[test]
fn huge_results_are_refused_or_stay_symbolic() {
    assert_eq!(ev("len(str(2**100000))"), "30103");
    // too large to evaluate exactly: stays as a power instead of hanging
    assert_eq!(ev("2**100000000"), "2**100000000");
    assert!(ev("expand((x + y + z)**3000)").contains("too many terms"));
}

#[test]
fn plots_sample_points_and_split_at_poles() {
    let mut g = HashMap::new();
    let o = run_source(&mut g, "plot(1/x, (x, -1, 1))", false);
    let Ok(Some(Value::Plot(p))) = o.result else { panic!("expected a plot") };
    assert_eq!(p.series.len(), 1);
    assert!(p.series[0].segments.len() >= 2, "the pole at 0 must break the curve");
    assert!(!run_source(&mut g, "plot(a*x, (x, 0, 1))", false).result.is_ok());
}

#[test]
fn help_lists_functions() {
    assert!(ev("help('diff')").contains("derivative"));
    assert!(ev("help()").contains("integrate"));
}

#[test]
fn strict_mode_requires_declarations() {
    let mut g = HashMap::new();
    let o = run_source(&mut g, "x + 1", true);
    assert!(o.result.unwrap_err().message.contains("symbols('x')"));
    let o = run_source(&mut g, "x = symbols('x')\nx + 1", true);
    assert_eq!(o.result.unwrap().unwrap().to_string(), "x + 1");
}

#[test]
fn complex_numbers() {
    assert_eq!(ev("I**2"), "-1");
    assert_eq!(ev("expand((1 + 2*I)*(3 - I))"), "5 + 5*I");
    assert_eq!(ev("sqrt(-4)"), "2*I");
    assert_eq!(ev("1/(1 + I)"), "1/2 - I/2");
    assert_eq!(ev("abs(3 + 4*I)"), "5");
    assert_eq!(ev("conjugate(3 + 4*I)"), "3 - 4*I");
    assert_eq!(ev("re(2 + 3*I) + im(2 + 3*I)"), "5");
    assert_eq!(ev("3j"), "3*I");
    assert_eq!(ev("solve(x**2 + 1, x)"), "[-I, I]");
    assert_eq!(ev("solve(x**2 + 2*x + 5, x)"), "[-1 - 2*I, -1 + 2*I]");
}

#[test]
fn vectors_and_matrices() {
    assert_eq!(ev("vec([1, 2, 3]) * 2"), "vec([2, 4, 6])");
    assert_eq!(ev("vec([1, 2]) + vec([3, 4])"), "vec([4, 6])");
    assert_eq!(ev("vec([1, 2, 3]) / 2"), "vec([1/2, 1, 3/2])");
    assert_eq!(ev("vec([x, 2]) * y"), "vec([x*y, 2*y])");
    assert_eq!(ev("matrix([[1, 2], [3, 4]]) @ matrix([[1, 2], [3, 4]])"), "matrix([[7, 10], [15, 22]])");
    assert_eq!(ev("matrix([[1, 2], [3, 4]]) @ vec([1, 1])"), "vec([3, 7])");
    assert_eq!(ev("dot(vec([1, 2, 3]), vec([4, 5, 6]))"), "32");
    assert_eq!(ev("norm(vec([3, 4]))"), "5");
    assert_eq!(ev("det(matrix([[1, 2], [3, 4]]))"), "-2");
    assert_eq!(ev("inv(matrix([[1, 2], [3, 4]]))"), "matrix([[-2, 1], [3/2, -1/2]])");
    assert_eq!(ev("transpose(matrix([[1, 2, 3], [4, 5, 6]]))"), "matrix([[1, 4], [2, 5], [3, 6]])");
    assert_eq!(ev("matrix([[1, 2], [3, 4]])[1, 0]"), "3");
    assert_eq!(ev("matrix([[1, 2], [3, 4]])[0]"), "vec([1, 2])");
    assert_eq!(ev("sin(vec([0, pi/2]))"), "vec([0, 1])");
    assert_eq!(ev("sum(vec([1, 2, 3]))"), "6");
    assert_eq!(ev("[v * 2 for v in vec([1, 2])]"), "[2, 4]");
    assert_eq!(ev("eye(2)"), "matrix([[1, 0], [0, 1]])");
    assert_eq!(ev("zeros((2, 3))"), "matrix([[0, 0, 0], [0, 0, 0]])");
    assert_eq!(ev("shape(ones(4))"), "(4,)");
    assert_eq!(ev("tolist(vec([1, 2]))"), "[1, 2]");
    assert!(ev("matrix([[1, 2], [3]])").contains("same length"));
}

#[test]
fn typed_arrays_never_mix() {
    assert_eq!(ev("vec([1, 2], dtype=f64) * 2"), "vec([2.0, 4.0], dtype=f64)");
    assert_eq!(ev("astype(vec([1, 2]), f32)"), "vec([1.0, 2.0], dtype=f32)");
    assert_eq!(ev("vec([1, 2], dtype=i32) + vec([1, 2], dtype=i32)"), "vec([2, 4], dtype=i32)");
    assert!(ev("vec([1], dtype=i32) + vec([1.0], dtype=f64)").contains("never mixed"));
    assert!(ev("vec([1], dtype=i32) + vec([1])").contains("never mixed"));
    assert!(ev("vec([100], dtype=i8) * 2").contains("overflow"));
    assert!(ev("vec([1], dtype=i32) / 2").contains("astype"));
    assert!(ev("vec([1], dtype=i32) * 0.5").contains("does not fit"));
    assert!(ev("vec([x], dtype=f64)").contains("symbols"));
    assert!(ev("vec([1.5], dtype=i32)").contains("whole number"));
    assert!(ev("vec([300], dtype=i8)").contains("does not fit"));
    assert_eq!(ev("det(matrix([[1, 2], [3, 4]], dtype=f64))"), "-2.0");
    assert_eq!(ev("matrix([[2, 0], [0, 4]], dtype=f64) @ vec([1, 1], dtype=f64)"), "vec([2.0, 4.0], dtype=f64)");
    assert!(ev("det(matrix([[1, 2], [3, 4]], dtype=i32))").contains("astype"));
    assert!(ev("inv(matrix([[1, 2], [2, 4]], dtype=f64))").contains("singular"));
    assert_eq!(ev("astype(astype(vec([1, 2]), f64), i32)"), "vec([1, 2], dtype=i32)");
}

#[test]
fn infinity_one_sided_limits_and_partial_fractions() {
    assert_eq!(ev("limit((x**2 + 1)/(2*x**2 + x), x, oo)"), "1/2");
    assert_eq!(ev("limit(exp(-x), x, oo)"), "0");
    assert_eq!(ev("limit(atan(x), x, -oo)"), "-pi/2");
    assert_eq!(ev("limit(1/x, x, 0, dir='+')"), "oo");
    assert_eq!(ev("limit(1/x, x, 0, dir='-')"), "-oo");
    assert_eq!(ev("limit(1/x**2, x, 0)"), "oo");
    assert!(ev("limit(1/x, x, 0)").contains("differs on the two sides"));
    assert!(ev("limit(1/x, x, 0, dir='up')").contains("dir must be"));
    assert_eq!(ev("apart(1/(x**2 - 1), x)"), "1/(2*(x - 1)) - 1/(2*(x + 1))");
    assert_eq!(ev("apart(x**3/(x**2 - 1))"), "x + 1/(2*(x - 1)) + 1/(2*(x + 1))");
    assert_eq!(ev("integrate(1/(x**2 - 1), x)"), "log(x - 1)/2 - log(x + 1)/2");
    assert_eq!(ev("abs(N(subs(diff(integrate(1/(x**2 + x + 1), x), x) - 1/(x**2 + x + 1), x, 2))) < 1e-9"), "True");
    assert_eq!(ev("free_symbols(oo*x)"), "[x]");
}

#[test]
fn solves_cubics_and_transcendental_equations() {
    let v = |src: &str| ev(src);
    assert_eq!(v("solve(exp(x) == 5, x)"), "[log(5)]");
    assert_eq!(v("solve(2*sin(x) + 1, x)"), "[-pi/6, 7*pi/6]");
    assert_eq!(v("solve(x**4 - 5*x**2 + 4, x)"), "[-2, -1, 1, 2]");
    // every cubic root satisfies the equation
    assert_eq!(v("abs(N(subs(x**3 + x + 1, x, solve(x**3 + x + 1, x)[0]))) < 1e-9"), "True");
    assert!(v("nsolve(cos(x) == x, x, 1)").starts_with("0.73908513"));
}

#[test]
fn exports_functions_as_python_and_rust() {
    assert!(ev("nroots(x**5 - x - 1, x)").starts_with("[-0.7"), "five numeric roots, sorted");
    assert_eq!(ev("len(nroots(x**5 - x - 1))"), "5");
    assert_eq!(ev("solve(log(x) + log(x - 1) - log(2), x)"), "[2]");
    assert_eq!(ev("solve(2**x - 8, x)"), "[3]");
    assert_eq!(ev("diff(x^3 + x^2, x)"), ev("diff(x**3 + x**2, x)"), "^ is a power");
    assert_eq!(ev("latex(x**2/3)"), "'\\frac{x^{2}}{3}'");
    assert!(ev("latex([[1, 2], [3, x]])").contains("\\begin{bmatrix}\n  1 & 2 \\\\\n  3 & x\n\\end{bmatrix}"));
    assert_eq!(ev("pycode(x**2 + 1, x)"), "'import math\n\n\ndef f(x):\n    return x**2 + 1\n'");
    assert!(ev("pycode(solve(x**2 - a, x), a, 'roots')").contains("def roots(a):"));
    assert!(ev("pycode(sin(x), x, numpy=True)").contains("np.sin(x)"));
    assert!(ev("rustcode(x**2 + 1, x)").contains("x.powi(2) + 1.0"));
    assert!(ev("pycode(sin(x)*y)").contains("def f(x, y)"), "arguments default to the free symbols, sorted");
}

#[test]
fn quartics_match_known_roots_and_float_sqrt_is_imaginary() {
    assert_eq!(ev("sqrt(-4.0)"), "2.0*I");
    // x**4 + x + 1: all four roots are complex; check one against the known value
    let r = ev("abs(subs(x**4 + x + 1, x, N(solve(x**4 + x + 1, x)[3]))) < 1e-9");
    assert_eq!(r, "True");
}
