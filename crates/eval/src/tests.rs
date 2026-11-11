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
    // a complex curve is drawn as its real and imaginary parts
    let Ok(Some(Value::Plot(p))) = run_source(&mut g, "plot(exp(I*x), (x, 0, 6))", false).result else { panic!("expected a plot") };
    assert_eq!(p.series.len(), 2);
    assert!(p.series.iter().all(|s| !s.segments.is_empty()));
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
    assert_eq!(ev("gcd(x**2 - y**2, x**2 + 2*x*y + y**2)"), "x + y");
    assert_eq!(ev("lcm(x**2 - 1, x**2 + x)"), "x**3 - x");
    assert_eq!(ev("gcd(6*x*y, 4*x*z)"), "2*x");
    assert_eq!(ev("gcd(12, 18)"), "6");
    assert_eq!(ev("cancel((x**2 - y**2)/(x**2 + 2*x*y + y**2))"), "(x - y)/(x + y)");
    assert_eq!(ev("cancel((x**3 - y**3)/(x - y))"), "x**2 + x*y + y**2");
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

#[test]
fn electron_positron_to_muons_cross_section() {
    // the textbook tree-level result sigma = 4 pi alpha^2 / (3 s)
    let src = "\
LH = contract(gamma_trace(slash(p2), gamma(mu), slash(p1), gamma(nu)) * gamma_trace(slash(p3), gamma(mu), slash(p4), gamma(nu)))
dots = [(pdot(p1, p2), s/2), (pdot(p3, p4), s/2), (pdot(p1, p3), -t/2), (pdot(p2, p4), -t/2), (pdot(p1, p4), -u/2), (pdot(p2, p3), -u/2)]
M2 = g**4 / s**2 * subs(LH, dots) / 4
M2c = simplify(subs(M2, [(t, -s*(1 - c)/2), (u, -s*(1 + c)/2)]))
simplify(subs(2*pi*integrate(M2c/(64*pi**2*s), (c, -1, 1)), g, sqrt(4*pi*a)))";
    assert_eq!(ev(src), "4*a**2*pi/(3*s)");
}

#[test]
fn slices_follow_python() {
    assert_eq!(ev("[0,1,2,3,4][1:3]"), "[1, 2]");
    assert_eq!(ev("[0,1,2,3,4][:2]"), "[0, 1]");
    assert_eq!(ev("[0,1,2,3,4][-2:]"), "[3, 4]");
    assert_eq!(ev("[0,1,2,3,4][::2]"), "[0, 2, 4]");
    assert_eq!(ev("[0,1,2,3,4][::-1]"), "[4, 3, 2, 1, 0]");
    assert_eq!(ev("[0,1,2,3,4][3:0:-2]"), "[3, 1]");
    assert_eq!(ev("[0,1,2][1:99]"), "[1, 2]");
    assert_eq!(ev("[0,1,2][5:9]"), "[]");
    assert_eq!(ev("(1,2,3)[1:]"), "(2, 3)");
    assert_eq!(ev("'hello'[1:4]"), "'ell'");
    assert_eq!(ev("'hello'[::-1]"), "'olleh'");
    assert_eq!(ev("range(10)[2:5]"), "[2, 3, 4]");
    assert!(ev("[1,2][::0]").contains("cannot be zero"));
    assert!(ev("5[1:2]").contains("cannot be sliced"));
    assert!(ev("[1,2][1.5:]").contains("integers"));
    assert_eq!(ev("n = 2\n[0,1,2,3][n:n+1]"), "[2]");
}

#[test]
fn f_strings_format_values() {
    assert_eq!(ev("x = 3\nf'x is {x}'"), "'x is 3'");
    assert_eq!(ev("f'{1/3}'"), "'1/3'");
    assert_eq!(ev("f'{1/3:.3f}'"), "'0.333'");
    assert_eq!(ev("f'{pi:.5f}'"), "'3.14159'");
    assert_eq!(ev("f'{12345.6789:,.2f}'"), "'12,345.68'");
    assert_eq!(ev("f'{1234567:,d}'"), "'1,234,567'");
    assert_eq!(ev("f'{5:03d}|{5:>4}|{5:<4}|{5:^4}|{5:*^5}'"), "'005|   5|5   | 5  |**5**'");
    assert_eq!(ev("f'{-2.5:+.1f} {2:+d} {1500:.2e}'"), "'-2.5 +2 1.50e+03'");
    assert_eq!(ev("f'{0.25:.1%}'"), "'25.0%'");
    assert_eq!(ev("f'{'abc':>5}|{'abcdef':.3}'"), "'  abc|abc'");
    assert_eq!(ev("w = 6\nf'{7:{w}}'"), "'     7'");
    assert_eq!(ev("f'{'a'!r}'"), "''a''");
    assert_eq!(ev("f'a' 'b' f'{1+1}'"), "'ab2'");
    assert_eq!(ev("f''"), "''");
    assert_eq!(ev("f'{{x}}'"), "'{x}'");
    assert_eq!(ev("f'{x**2 + 1}'"), "'x**2 + 1'");
    assert!(ev("f'{x=}'").contains("not supported"));
    assert!(ev("f'{1.5:d}'").contains("integer"));
    assert!(ev("f'{1:q}'").contains("unsupported format spec"));
    assert!(ev("f'{[1]:.2f}'").contains("cannot format"));
    assert!(ev("f'{1:99999}'").contains("unsupported format spec"));
}

#[test]
fn limits_with_variable_exponents_and_radicals() {
    assert_eq!(ev("limit((1+x)**(1/x), x, 0)"), "E");
    assert_eq!(ev("limit((1+1/x)**x, x, oo)"), "E");
    assert_eq!(ev("limit(x*sin(1/x), x, oo)"), "1");
    assert_eq!(ev("limit(exp(-1/x**2)/x**2, x, 0)"), "0");
    assert_eq!(ev("limit(sqrt(x**2+x)-x, x, oo)"), "1/2");
    // 1 ** (1/0) is not a number, so it must not quietly become 1
    assert_ne!(ev("subs((1+x)**(1/x), x, 0)"), "1");
}

#[test]
fn improper_integrals_take_limits_at_the_ends() {
    assert_eq!(ev("integrate(exp(-x), (x, 0, oo))"), "1");
    assert_eq!(ev("integrate(x**3*exp(-x), (x, 0, oo))"), "6");
    assert_eq!(ev("integrate(1/(1+x**2), (x, -oo, oo))"), "pi");
    assert_eq!(ev("integrate(1/x**2, (x, 1, oo))"), "1");
    assert_eq!(ev("integrate(1/sqrt(x), (x, 0, 4))"), "4");
}

#[test]
fn nonlinear_systems_by_elimination() {
    assert_eq!(ev("solve([x**2+y**2-25, x-y-1])"), "[[x == -3, y == -4], [x == 4, y == 3]]");
    assert_eq!(ev("solve([x*y-6, x+y-5], [x, y])"), "[[x == 2, y == 3], [x == 3, y == 2]]");
    assert_eq!(ev("solve([x**2-1, y**2-4], [x, y])"), "[[x == -1, y == -2], [x == -1, y == 2], [x == 1, y == -2], [x == 1, y == 2]]");
    assert_eq!(ev("solve([x**2+y**2-1, x**2+y**2-2], [x, y])"), "[]");
    assert_eq!(
        ev("solve([x**2+y**2-1, x+y-5], [x, y])"),
        "[[x == 5/2 - I*sqrt(23)/2, y == 5/2 + I*sqrt(23)/2], [x == 5/2 + I*sqrt(23)/2, y == 5/2 - I*sqrt(23)/2]]"
    );
    assert!(ev("solve([x**2+y**2-1], [x, y])").contains("not determined"));
    assert!(ev("solve([sin(x)-y, y-1/2], [x, y])").contains("polynomial"));
    // linear systems keep their flat form
    assert_eq!(ev("solve([x+y-3, x-y-1], [x, y])"), "[x == 2, y == 1]");
}

#[test]
fn integral_families() {
    assert_eq!(ev("simplify(diff(integrate(cos(x)**3, x), x) - cos(x)**3)"), "0");
    assert_eq!(ev("abs(round(float(subs(diff(integrate(sin(x)**2*cos(x)**2, x), x) - sin(x)**2*cos(x)**2, x, 0.7)), 9))"), "0.0");
    assert_eq!(ev("abs(round(float(subs(diff(integrate(sin(x)**4, x), x) - sin(x)**4, x, 0.7)), 9))"), "0.0");
    assert_eq!(ev("abs(round(float(subs(diff(integrate(exp(2*x)*cos(3*x), x), x) - exp(2*x)*cos(3*x), x, 0.7)), 9))"), "0.0");
    assert_eq!(ev("integrate(exp(x)*sin(x), x)"), "exp(x)*(-cos(x) + sin(x))/2");
    assert_eq!(ev("integrate(1/sqrt(1-x**2), x)"), "asin(x)");
    assert_eq!(ev("integrate(1/sqrt(x**2+1), x)"), "asinh(x)");
    assert_eq!(ev("integrate(exp(-x**2), x)"), "sqrt(pi)*erf(x)/2");
    assert_eq!(ev("integrate(exp(-x**2), (x, -oo, oo))"), "sqrt(pi)");
    assert_eq!(ev("integrate(sec(x)**2, x)"), "tan(x)");
    assert_eq!(ev("integrate(abs(x), x)"), "x*abs(x)/2");
}

#[test]
fn new_functions() {
    assert_eq!(ev("erf(0)"), "0");
    assert_eq!(ev("round(erf(1.0), 10)"), "0.8427007929");
    assert_eq!(ev("round(float(asinh(2)), 6)"), "1.443635");
    assert_eq!(ev("diff(erf(x), x)"), "2*exp(-x**2)/sqrt(pi)");
    assert_eq!(ev("limit(erf(x), x, oo)"), "1");
    assert_eq!(ev("atan2(1, 1)"), "pi/4");
    assert_eq!(ev("atan2(1, -1)"), "3*pi/4");
    assert_eq!(ev("atan2(-1, 0)"), "-pi/2");
    assert_eq!(ev("cot(pi/4)"), "1");
}

#[test]
fn euler_identity_and_logs_of_negatives() {
    assert_eq!(ev("exp(I*pi)"), "-1");
    assert_eq!(ev("exp(I*pi/3)"), "1/2 + I*sqrt(3)/2");
    assert_eq!(ev("re(exp(I*pi/3))"), "1/2");
    assert_eq!(ev("exp(I*pi/7)"), "exp(I*pi/7)");
    assert_eq!(ev("log(-1)"), "I*pi");
    assert_eq!(ev("log(-3)"), "log(3) + I*pi");
    // trigonometric polynomial identities reduce to zero
    assert_eq!(ev("simplify(cos(x)**4 - sin(x)**4 - cos(x)**2 + sin(x)**2)"), "0");
    assert_eq!(ev("simplify(sin(x)**2*cos(x)**2 + sin(x)**4 - sin(x)**2)"), "0");
}

#[test]
fn equations_with_square_roots() {
    assert_eq!(ev("solve(sqrt(x+1)-x, x)"), "[sqrt(5)/2 + 1/2]");
    assert_eq!(ev("solve(sqrt(x)+sqrt(x+5)-5, x)"), "[4]");
    assert_eq!(ev("solve(sqrt(2*x+1)-sqrt(x)-1, x)"), "[0, 4]");
    assert_eq!(ev("solve(1/sqrt(x)-2, x)"), "[1/4]");
    assert_eq!(ev("solve(sqrt(x)+3, x)"), "[]");
    assert_eq!(ev("solve(x**(3/2)-8, x)"), "[4]");
}

#[test]
fn eigen_number_theory_and_sums() {
    assert_eq!(ev("eigenvals(matrix([[2,0],[0,3]]))"), "[2, 3]");
    assert_eq!(ev("eigenvals(matrix([[0,1],[-1,0]]))"), "[-I, I]");
    assert_eq!(ev("eigenvals(eye(3))"), "[1, 1, 1]");
    assert_eq!(ev("eigenvects(matrix([[2,1],[1,2]]))"), "[(1, 1, [vec([-1, 1])]), (3, 1, [vec([1, 1])])]");
    assert_eq!(ev("nullspace(matrix([[1,2],[2,4]]))"), "[vec([-2, 1])]");
    assert_eq!(ev("charpoly(matrix([[1,2],[3,4]]), t)"), "t**2 - 5*t - 2");
    assert!(ev("eigenvals(matrix([[1,2,3],[4,5,6]]))").contains("square"));
    assert_eq!(ev("Gamma(5)"), "24");
    assert_eq!(ev("Gamma(7/2)"), "15*sqrt(pi)/8");
    assert_eq!(ev("Gamma(-1/2)"), "-2*sqrt(pi)");
    assert_eq!(ev("isprime(97)"), "True");
    assert_eq!(ev("factorint(360)"), "[(2, 3), (3, 2), (5, 1)]");
    assert_eq!(ev("factorint(600851475143)"), "[(71, 1), (839, 1), (1471, 1), (6857, 1)]");
    assert_eq!(ev("totient(36)"), "12");
    assert_eq!(ev("nextprime(100)"), "101");
    assert_eq!(ev("summation(k**2, (k, 1, n))"), "n**3/3 + n**2/2 + n/6");
    assert_eq!(ev("summation(k**3, (k, 1, 10))"), "3025");
    assert_eq!(ev("summation(1/2**k, (k, 0, oo))"), "2");
    assert_eq!(ev("summation(x**k, (k, 0, oo))"), "1/(-x + 1)");
    assert!(ev("summation(2**k, (k, 0, oo))").contains("does not converge"));
}

#[test]
fn assumptions_refine_results() {
    let decl = "x = symbols('x', positive=True)\nn = symbols('n', integer=True)\ny = symbols('y')\n";
    let ev = |s: &str| ev(&format!("{decl}{s}"));
    assert_eq!(ev("simplify(sqrt(x**2))"), "x");
    assert_eq!(ev("simplify(sqrt(y**2))"), "abs(y)");
    assert_eq!(ev("simplify(sqrt(x**2*y**2))"), "x*abs(y)");
    assert_eq!(ev("abs(x)"), "x");
    assert_eq!(ev("abs(y)"), "abs(y)");
    assert_eq!(ev("log(x**3)"), "3*log(x)");
    assert_eq!(ev("log(y**2)"), "log(y**2)");
    assert_eq!(ev("solve(x**2 - 4, x)"), "[2]");
    assert_eq!(ev("solve(y**2 - 4, y)"), "[-2, 2]");
    assert_eq!(ev("solve(x + 3, x)"), "[]");
    assert_eq!(ev("sin(n*pi)"), "0");
    assert_eq!(ev("cos(n*pi)"), "(-1)**n");
    assert_eq!(ev("simplify(cos(2*n*pi))"), "1");
    assert_eq!(ev("sin(y*pi)"), "sin(pi*y)");
}

#[test]
fn assumption_queries() {
    let decl = "x = symbols('x', positive=True)\nn = symbols('n', integer=True)\ny = symbols('y')\n";
    let ev = |s: &str| ev(&format!("{decl}{s}"));
    assert_eq!(ev("is_positive(x + 1)"), "True");
    assert_eq!(ev("print(is_positive(y))"), "None");
    assert_eq!(ev("is_negative(-x)"), "True");
    assert_eq!(ev("is_positive(-x)"), "False");
    assert_eq!(ev("is_integer(n + 1)"), "True");
    assert_eq!(ev("is_integer(1/2)"), "False");
    assert_eq!(ev("is_nonzero(x)"), "True");
    assert!(ev("symbols('z', blah=True)").contains("unknown assumption"));
}

#[test]
fn redeclaring_a_symbol_clears_its_assumptions() {
    assert_eq!(ev("x = symbols('x', positive=True)\nx = symbols('x')\nabs(x)"), "abs(x)");
}

#[test]
fn pattern_matching_and_rewrite_rules() {
    assert_eq!(ev("t = symbols('t')\nreplace(sin(t)**2 + cos(t)**2 + 5, sin(a_)**2 + cos(a_)**2, 1)"), "6");
    assert_eq!(ev("replace(sin(2*t) + sin(x), sin(a_), a_**2)"), "4*t**2 + x**2");
    assert_eq!(ev("replace(exp(a)*exp(b), exp(u_)*exp(v_), exp(u_ + v_))"), "exp(a + b)");
    assert_eq!(ev("replace(log(x*y), log(a_*b_), log(a_) + log(b_))"), "log(x) + log(y)");
    assert_eq!(ev("replace(x**2 + y**2, [(x, 1), (y, 2)])"), "5");
    assert_eq!(ev("f = symbols('f')\nreplace(f(x) + f(y), f(a_), a_**2)"), "x**2 + y**2");
    assert_eq!(ev("pattern_match(3*x*y, a_*x)"), "[a_ == 3*y]");
    assert_eq!(ev("pattern_match(x**3, x**n_)"), "[n_ == 3]");
    assert_eq!(ev("print(pattern_match(sin(x), cos(a_)))"), "None");
    assert_eq!(ev("findall(sin(x) + sin(y)*cos(x), sin(a_))"), "[sin(y), sin(x)]");
    assert!(ev("replace(x, [x])").contains("pair"));
}

#[test]
fn factoring_over_the_integers() {
    assert_eq!(ev("factor(x**4 + 4)"), "(x**2 - 2*x + 2)*(x**2 + 2*x + 2)");
    assert_eq!(ev("factor(x**8 - 1)"), "(x - 1)*(x + 1)*(x**2 + 1)*(x**4 + 1)");
    assert_eq!(ev("factor(x**4 + x**2 + 1)"), "(x**2 + x + 1)*(x**2 - x + 1)");
    assert_eq!(ev("factor(x**4 - 10*x**2 + 1)"), "x**4 - 10*x**2 + 1");
    assert_eq!(ev("factor(x**2*y - y**3)"), "y*(x + y)*(x - y)");
    assert_eq!(ev("factor(x**3 + y**3 + z**3 - 3*x*y*z)"), "(x + y + z)*(x**2 - x*y - x*z + y**2 - y*z + z**2)");
    assert_eq!(ev("factor(a*x**2 - a)"), "a*(x - 1)*(x + 1)");
    assert_eq!(ev("factor(expand((x**3+x+1)*(x**4-3*x+7)*(2*x**5+x**2-9)))"), "(2*x**5 + x**2 - 9)*(x**3 + x + 1)*(x**4 - 3*x + 7)");
}

#[test]
fn solve_splits_into_irreducible_factors() {
    assert_eq!(ev("solve((x**2 - 2)*(x**3 - 3), x)"), "[-sqrt(2), -3**(1/3)/2 - I*243**(1/6)/2, -3**(1/3)/2 + I*243**(1/6)/2, sqrt(2), 3**(1/3)]");
    assert_eq!(ev("solve(x**6 - 1, x)"), "[-1, -1/2 - I*sqrt(3)/2, -1/2 + I*sqrt(3)/2, 1/2 - I*sqrt(3)/2, 1/2 + I*sqrt(3)/2, 1]");
}

#[test]
fn lambert_w_solves_and_evaluates() {
    assert_eq!(ev("solve(x*exp(x) - 1, x)"), "[LambertW(1)]");
    assert_eq!(ev("solve(2*x*exp(3*x) - 5, x)"), "[LambertW(15/2)/3]");
    assert_eq!(ev("solve(x*exp(x) + 1, x)"), "[]");
    assert_eq!(ev("diff(LambertW(x), x)"), "LambertW(x)/(x*(LambertW(x) + 1))");
    assert!(ev("N(LambertW(1))").starts_with("0.56714329040978"));
}

#[test]
fn sine_integral_and_laurent_series() {
    assert_eq!(ev("integrate(sin(x)/x, (x, 0, oo))"), "pi/2");
    assert_eq!(ev("integrate(sin(3*x)/x, x)"), "Si(3*x)");
    assert!(ev("N(Si(10))").starts_with("1.65834759421887"));
    assert_eq!(ev("series(1/sin(x), x, 0, 4)"), "7*x**3/360 + x/6 + 1/x");
}

#[test]
fn integrates_quartics_that_split_over_sqrt() {
    let a = ev("integrate(1/(x**4 + 1), x)");
    assert!(a.contains("atan") && a.contains("log"), "{a}");
}

#[test]
fn limits_of_polynomial_differences_and_log_pairs() {
    assert_eq!(ev("limit((x**2 + 3*x)/(x**2 - 3*x), x, oo)"), "1");
    assert_eq!(ev("limit(x**3 - 5*x, x, -oo)"), "-oo");
    assert_eq!(ev("limit(log(x**2 + x) - log(x**2), x, oo)"), "0");
    assert_eq!(ev("integrate(1/(x**4 + 1), (x, 0, oo))"), "pi*sqrt(2)/4");
    assert_eq!(ev("integrate(1/(x**4 + 1), (x, -oo, oo))"), "pi*sqrt(2)/2");
}

#[test]
fn factor_cancels_fractions_and_together_combines() {
    assert_eq!(ev("factor((x**2 - 1)/(x**2 + 2*x + 1))"), "(x - 1)/(x + 1)");
    assert_eq!(ev("factor((x**2 - y**2)/(x**2 + 2*x*y + y**2))"), "(x - y)/(x + y)");
    assert_eq!(ev("together(1/x + 1/y)"), "(x + y)/(x*y)");
}

#[test]
fn more_special_functions() {
    assert!(ev("N(Ci(1))").starts_with("0.3374039229009"));
    assert!(ev("N(Ei(1))").starts_with("1.89511781635593"));
    assert!(ev("N(Ei(-1))").starts_with("-0.21938393439552"));
    assert!(ev("N(erfc(0.5))").starts_with("0.47950012218695"));
    assert_eq!(ev("erfc(x)"), "-erf(x) + 1");
    assert_eq!(ev("LambertW(exp(1))"), "1");
    assert_eq!(ev("integrate(cos(2*x)/x, x)"), "Ci(2*x)");
    assert_eq!(ev("integrate(exp(x)/x, x)"), "Ei(x)");
    assert_eq!(ev("limit(Ci(x), x, oo)"), "0");
}

#[test]
fn factors_over_algebraic_extensions() {
    assert_eq!(ev("factor(x**4 + 1, extension=sqrt(2))"), "(x**2 - x*sqrt(2) + 1)*(x**2 + x*sqrt(2) + 1)");
    assert_eq!(ev("factor(x**2 - 2, sqrt(2))"), "(x + sqrt(2))*(x - sqrt(2))");
    assert_eq!(ev("factor(x**2 + 1, extension=I)"), "(x + I)*(x - I)");
    assert_eq!(ev("factor(x**3 - 2, extension=2**(1/3))"), "(x - 2**(1/3))*(x**2 + x*2**(1/3) + 4**(1/3))");
    assert_eq!(ev("factor((x**2 - 2)**2*(x + 1), sqrt(2))"), "(x + sqrt(2))**2*(x - sqrt(2))**2*(x + 1)");
}

#[test]
fn integration_by_parts_substitution_and_repeated_quadratics() {
    assert_eq!(ev("integrate(log(x)**2, x)"), "x*log(x)**2 - 2*(x*log(x) - x)");
    assert_eq!(ev("integrate(x*atan(x), x)"), "x**2*atan(x)/2 - x/2 + atan(x)/2");
    assert_eq!(ev("integrate(log(x)/x, x)"), "log(x)**2/2");
    assert_eq!(ev("integrate(1/(1 + exp(x)), x)"), "x - log(exp(x) + 1)");
    assert_eq!(ev("integrate(x*sqrt(x + 1), x)"), "-2*(x + 1)**(3/2)/3 + 2*(x + 1)**(5/2)/5");
    assert_eq!(ev("integrate(1/(x**2 + 1)**2, (x, -oo, oo))"), "pi/2");
    assert_eq!(ev("integrate(x**2*exp(-x**2), (x, -oo, oo))"), "sqrt(pi)/2");
}

#[test]
fn limits_with_oscillation_and_square_roots_and_equal_power_equations() {
    assert_eq!(ev("limit(exp(-x)*sin(x), x, oo)"), "0");
    assert_eq!(ev("limit(sin(x)/x, x, oo)"), "0");
    assert_eq!(ev("limit(sqrt(x + 1) - sqrt(x), x, oo)"), "0");
    assert_eq!(ev("limit(sqrt(x**2 + x) - sqrt(x**2 - x), x, oo)"), "1");
    assert_eq!(ev("solve(3**x - 2**x, x)"), "[0]");
}

#[test]
fn slider_is_its_value() {
    assert_eq!(ev("a = slider(0, 10, 2)\na * 3"), "6");
    assert_eq!(ev("slider(0, 1, 0.25, 0.05) * 4"), "1.0");
    assert_eq!(ev("slider(1, 5)"), "1");
    assert!(ev("slider(0, 10, 12)").contains("outside the range"));
    assert!(ev("slider(3, 3, 3)").contains("lo < hi"));
    assert!(ev("slider(0, 10, 2, 0)").contains("step must be positive"));
    assert!(ev("slider(0, x, 2)").contains("plain numbers"));
}

#[test]
fn complex_functions() {
    // parts of functions of a complex argument; symbols are real
    assert_eq!(ev("re(exp(I*x))"), "cos(x)");
    assert_eq!(ev("im(exp(2*I*x + 1))"), "E*sin(2*x)");
    assert_eq!(ev("conjugate(exp(I*x))"), "cos(x) - I*sin(x)");
    assert_eq!(ev("abs(exp(I*x))"), "1");
    // integer powers of exact complex numbers are evaluated
    assert_eq!(ev("(1 + I)**10"), "32*I");
    assert_eq!(ev("(2 + 3*I)**5"), "122 - 597*I");
    assert_eq!(ev("1/(1 + I)**3"), "-1/4 - I/4");
    // circular and hyperbolic functions of an imaginary argument
    assert_eq!(ev("sin(I)"), "I*sinh(1)");
    assert_eq!(ev("cos(I*x)"), "cosh(x)");
    assert_eq!(ev("sinh(I*pi/2)"), "I");
    assert_eq!(ev("simplify(cosh(x)**2 - sinh(x)**2)"), "1");
    // argument, numeric evaluation, and rewriting
    assert_eq!(ev("arg(1 + I)"), "pi/4");
    assert_eq!(ev("arg(-1 - I)"), "-3*pi/4");
    assert_eq!(ev("arg(x + I)"), "atan2(1, x)");
    assert_eq!(ev("N(atan2(1, 2))"), "0.46364760900080615");
    assert_eq!(ev("sqrt(3 + 4*I)"), "2 + I");
    assert_eq!(ev("sqrt(-3 - 4*I)"), "1 - 2*I");
    assert_eq!(ev("sqrt(I)"), "sqrt(2)/2 + I*sqrt(2)/2");
    assert_eq!(ev("re(sqrt(1 + I))"), "sqrt(sqrt(2)/2 + 1/2)");
    assert_eq!(ev("N(sqrt(1 + I))"), "1.09868411346781 + 0.4550898605622274*I");
    assert_eq!(ev("N(exp(1 + 2*I))"), "-1.1312043837568135 + 2.4717266720048188*I");
    assert_eq!(ev("simplify((exp(I*x) + exp(-I*x))/2)"), "cos(x)");
    assert_eq!(ev("simplify((exp(I*x) - exp(-I*x))/(2*I))"), "sin(x)");
    assert_eq!(ev("rewrite_exp(cos(x))"), "(exp(-I*x) + exp(I*x))/2");
    assert_eq!(ev("rewrite_trig(exp(I*x)*exp(2))"), "exp(2)*(cos(x) + I*sin(x))");
    assert_eq!(ev("simplify(rewrite_exp(sin(x)**2 + cos(x)**2))"), "1");
}

#[test]
fn trigonometric_polynomials_in_sin_and_cos() {
    // only even powers of one function: Pythagoras turns it into a polynomial in the other
    // a quadratic form is linear in the double angle
    assert_eq!(ev("solve(sin(x)*cos(x) - 1/4, x)"), "[pi/12, 5*pi/12]");
    assert_eq!(ev("solve(sin(x)**2 - cos(x)**2, x)"), "[-pi/4, pi/4, 3*pi/4, 5*pi/4]");
    assert_eq!(ev("solve(cos(x)**2 + sin(x) - 1, x)"), "[0, pi/2, pi]");
    // homogeneous in sin and cos: a polynomial in tan
    assert_eq!(ev("solve(sin(x)**2 - 3*sin(x)*cos(x) + 2*cos(x)**2, x)"), "[pi/4, atan(2)]");
    // a sin + b cos == c by the half-angle substitution
    assert_eq!(ev("solve(sin(x) + cos(x) - 1, x)"), "[0, pi/2]");
    assert_eq!(ev("solve(3*sin(x) + 4*cos(x) - 5, x)"), "[2*atan(1/3)]");
    // exact angles for asin and acos at sqrt(2)/2 and sqrt(3)/2
    assert_eq!(ev("asin(sqrt(2)/2)"), "pi/4");
    assert_eq!(ev("acos(sqrt(3)/2)"), "pi/6");
    assert_eq!(ev("acos(-sqrt(2)/2)"), "3*pi/4");
}

#[test]
fn unevaluated_integrals_sums_and_limits() {
    assert_eq!(ev("Integral(x**2, (x, 0, 1))"), "Integral(x**2, x, 0, 1)");
    assert_eq!(ev("doit(Integral(x**2, (x, 0, 1)))"), "1/3");
    assert_eq!(ev("doit(Sum(k**2, (k, 1, 10)))"), "385");
    assert_eq!(ev("doit(Limit(sin(x)/x, x, 0))"), "1");
    assert_eq!(ev("doit(Integral(Sum(x**k, (k, 0, 3)), x))"), "x**4/4 + x**3/3 + x**2/2 + x");
    assert_eq!(ev("latex(Integral(exp(-x**2), (x, 0, oo)))"), "'\\int_{0}^{\\infty} e^{-x^{2}}\\,dx'");
    assert_eq!(ev("latex(Sum(1/k**2, (k, 1, oo)))"), "'\\sum_{k=1}^{\\infty} \\frac{1}{k^{2}}'");
    assert_eq!(ev("latex(Limit(sin(x)/x, x, 0))"), "'\\lim_{x \\to 0} \\frac{\\sin\\left(x\\right)}{x}'");
}

#[test]
fn limits_at_poles_of_tan_and_across_absolute_values() {
    assert_eq!(ev("limit(tan(x), x, pi/2, '-')"), "oo");
    assert_eq!(ev("limit(tan(x), x, pi/2, '+')"), "-oo");
    assert!(ev("limit(tan(x), x, pi/2)").starts_with("ERROR"));
    assert_eq!(ev("limit(tan(3*x)/tan(2*x), x, 0)"), "3/2");
    assert_eq!(ev("limit(abs(x)/x, x, 0, '+')"), "1");
    assert_eq!(ev("limit(abs(x)/x, x, 0, '-')"), "-1");
    assert!(ev("limit(abs(x)/x, x, 0)").starts_with("ERROR"));
    assert_eq!(ev("limit(abs(x**2 - 4)/(x - 2), x, 2, '-')"), "-4");
}

#[test]
fn trigonometric_expansion_and_inverse_compositions() {
    assert_eq!(ev("expand_trig(sin(x + y))"), "cos(x)*sin(y) + cos(y)*sin(x)");
    assert_eq!(ev("expand_trig(cos(2*x))"), "cos(x)**2 - sin(x)**2");
    assert_eq!(ev("expand_trig(sin(3*x))"), "-sin(x)**3 + 3*cos(x)**2*sin(x)");
    assert_eq!(ev("expand_trig(cos(x + pi/2))"), "-sin(x)");
    assert_eq!(ev("simplify(sin(2*x)/sin(x))"), "2*cos(x)");
    assert_eq!(ev("simplify(sin(3*x) - 3*sin(x) + 4*sin(x)**3)"), "0");
    assert_eq!(ev("simplify(cos(2*x) + 2*sin(x)**2)"), "1");
    assert_eq!(ev("solve(cos(2*x) - cos(x), x)"), "[-2*pi/3, 0, 2*pi/3]");
    assert_eq!(ev("solve(sin(2*x) - sin(x), x)"), "[-pi/3, 0, pi/3, pi]");
    assert_eq!(ev("atan(sqrt(3))"), "pi/3");
    assert_eq!(ev("atan(-1/sqrt(3))"), "-pi/6");
    assert_eq!(ev("cos(acos(x))"), "x");
    assert_eq!(ev("sin(acos(x))"), "sqrt(-x**2 + 1)");
    assert_eq!(ev("cos(atan(2))"), "sqrt(5)/5");
}

#[test]
fn nested_fractions_simple_roots_and_atan2_derivative() {
    assert_eq!(ev("simplify(1/(1 + 1/x))"), "x/(x + 1)");
    assert_eq!(ev("simplify(1/(1 + 1/(1 + 1/x)))"), "(x + 1)/(2*x + 1)");
    assert_eq!(ev("simplify(cos(x)/sin(x)*tan(x))"), "1");
    assert_eq!(ev("solve(x**2 - 4*y, x)"), "[-2*sqrt(y), 2*sqrt(y)]");
    assert_eq!(ev("solve(x**2 - a**2, x)"), "[a, -a]");
    assert_eq!(ev("diff(atan2(y, x), x)"), "-y/(x**2 + y**2)");
    assert_eq!(ev("simplify(diff(atan2(sin(x), cos(x)), x))"), "1");
}

#[test]
fn infinite_sums_zeta_telescoping_and_arithmetic_geometric() {
    assert_eq!(ev("summation(1/k**2, (k, 1, oo))"), "pi**2/6");
    assert_eq!(ev("summation(1/k**4, (k, 1, oo))"), "pi**4/90");
    assert_eq!(ev("summation(1/k**2, (k, 3, oo))"), "pi**2/6 - 5/4");
    assert_eq!(ev("summation(1/(k*(k + 1)), (k, 1, oo))"), "1");
    assert_eq!(ev("summation(1/(k*(k + 2)*(k + 3)), (k, 1, oo))"), "5/36");
    assert_eq!(ev("summation(1/(k**2 - 1), (k, 2, oo))"), "3/4");
    assert_eq!(ev("summation(k*2**(-k), (k, 1, oo))"), "2");
    assert_eq!(ev("summation(k**2/3**k, (k, 0, oo))"), "3/2");
    assert_eq!(ev("summation((k + 1)/2**k, (k, 0, oo))"), "4");
    assert!(ev("summation(1/k, (k, 1, oo))").starts_with("ERROR"));
    assert!(ev("summation(k*2**k, (k, 1, oo))").starts_with("ERROR"));
}

#[test]
fn secant_cosecant_trig_products_and_fractional_powers_with_log() {
    assert_eq!(ev("integrate(sec(x), x)"), "log(sin(x) + 1) - log(cos(x))");
    assert_eq!(ev("integrate(1/sin(x), x)"), "log(-cos(x) + 1) - log(sin(x))");
    assert_eq!(ev("integrate(sqrt(x)*log(x), x)"), "2*x**(3/2)*log(x)/3 - 4*x**(3/2)/9");
    assert_eq!(ev("integrate(sin(x)*cos(2*x), x)"), "(cos(x) - cos(3*x)/3)/2");
    assert_eq!(ev("integrate(sin(3*x)*sin(2*x), x)"), "(sin(x) - sin(5*x)/5)/2");
    assert_eq!(ev("integrate(sin(2*x)*cos(3*x), (x, 0, pi))"), "-4/5");
    assert_eq!(ev("N(integrate(sec(x), (x, 0, pi/4)))"), "0.8813735870195429");
}
