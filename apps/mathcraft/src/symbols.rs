//! The symbol palette: buttons that insert a formula template (`integrate(f, x)`) at the cursor of the code cell
//! being edited, with the first placeholder selected so typing replaces it. Notebooks stay plain text; the
//! buttons only save typing and show what the code looks like as mathematics.

use crate::fonts;
use crate::theme::{self, Theme};
use eframe::egui::{self, RichText, vec2};

/// One button: what it shows, the code it inserts, and the placeholders in that code, space-separated in the order
/// they appear: the first is selected after inserting, Tab moves on to the next.
pub struct Template {
    pub label: &'static str,
    pub code: &'static str,
    pub select: &'static str,
    pub hint: &'static str,
}

const fn t(label: &'static str, code: &'static str, select: &'static str, hint: &'static str) -> Template {
    Template { label, code, select, hint }
}

pub const GROUPS: &[(&str, &[Template])] = &[
    (
        "Calculus",
        &[
            t("∫", "integrate(f, x)", "f", "Indefinite integral"),
            t("∫ a→b", "integrate(f, (x, a, b))", "f x a b", "Definite integral"),
            t("d/dx", "diff(f, x)", "f x", "Derivative"),
            t("dⁿ/dxⁿ", "diff(f, x, 2)", "f x 2", "Higher derivative"),
            t("Σ", "summation(f, (k, 1, n))", "f k 1 n", "Sum over k"),
            t("lim", "limit(f, x, 0)", "f x 0", "Limit"),
            t("series", "series(f, x, 0, 6)", "f x 0 6", "Taylor series"),
            t("Integral", "Integral(f, x)", "f x", "Integral that is shown but not computed"),
            t("Derivative", "Derivative(f, x)", "f x", "Derivative that is shown but not computed"),
        ],
    ),
    (
        "Algebra",
        &[
            t("√", "sqrt(x)", "x", "Square root"),
            t("xⁿ", "x**n", "n", "Power"),
            t("a/b", "a/b", "a", "Fraction"),
            t("|x|", "abs(x)", "x", "Absolute value"),
            t("eˣ", "exp(x)", "x", "Exponential"),
            t("ln", "log(x)", "x", "Natural logarithm"),
            t("n!", "factorial(n)", "n", "Factorial"),
            t("solve", "solve(eq, x)", "eq x", "Solve an equation or inequality"),
            t("dsolve", "dsolve(eq, y(x))", "eq", "Solve a differential equation"),
            t("expand", "expand(f)", "f", "Multiply out"),
            t("factor", "factor(f)", "f", "Factor"),
            t("simplify", "simplify(f)", "f", "Simplify"),
        ],
    ),
    (
        "Trigonometry",
        &[
            t("sin", "sin(x)", "x", "Sine"),
            t("cos", "cos(x)", "x", "Cosine"),
            t("tan", "tan(x)", "x", "Tangent"),
            t("sin⁻¹", "asin(x)", "x", "Inverse sine"),
            t("cos⁻¹", "acos(x)", "x", "Inverse cosine"),
            t("tan⁻¹", "atan(x)", "x", "Inverse tangent"),
        ],
    ),
    ("Constants", &[t("π", "pi", "pi", "Pi"), t("e", "E", "E", "Euler's number"), t("i", "I", "I", "Imaginary unit"), t("∞", "oo", "oo", "Infinity")]),
    (
        "Relations",
        &[
            t("=", " == ", "", "Equation"),
            t("<", " < ", "", "Less than"),
            t(">", " > ", "", "Greater than"),
            t("≤", " <= ", "", "Less than or equal"),
            t("≥", " >= ", "", "Greater than or equal"),
        ],
    ),
    (
        "Matrices",
        &[
            t("[ ]", "[[a, b], [c, d]]", "a b c d", "A 2×2 matrix"),
            t("det", "det(A)", "A", "Determinant"),
            t("A⁻¹", "inv(A)", "A", "Inverse"),
            t("A^T", "transpose(A)", "A", "Transpose"),
            t("eig", "eigenvals(A)", "A", "Eigenvalues"),
        ],
    ),
];

/// The char offsets `(start, end)` of each placeholder named in `select` (space-separated, in the order they appear in
/// `code`). A name matches a whole word only, so `f` is not found inside `integrate`.
pub fn stops(code: &str, select: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = code.chars().collect();
    let word = |c: &char| c.is_alphanumeric() || *c == '_';
    let mut out = Vec::new();
    let mut from = 0;
    for name in select.split_whitespace() {
        let name: Vec<char> = name.chars().collect();
        let found = (from..chars.len()).find(|&i| {
            chars.get(i..i + name.len()) == Some(name.as_slice())
                && !i.checked_sub(1).and_then(|j| chars.get(j)).is_some_and(word)
                && !chars.get(i + name.len()).is_some_and(word)
        });
        let Some(i) = found else { break };
        out.push((i, i + name.len()));
        from = i + name.len();
    }
    out
}

/// The char offsets of the first placeholder, or `None` when there is nothing to select.
pub fn selection(code: &str, select: &str) -> Option<(usize, usize)> {
    stops(code, select).first().copied()
}

/// What is left to visit after a template was inserted: the placeholder being filled in comes first, the point after
/// the template last. Tab walks through them.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Stops {
    pub at: Vec<(usize, usize)>,
    /// The cell's length in chars when `at` was last right, to see how much typing moved the rest.
    pub len: usize,
}

impl Stops {
    /// The stops of `code` inserted at char `start` into a cell now `len` chars long.
    pub fn new(code: &str, select: &str, start: usize, len: usize) -> Stops {
        let mut at: Vec<(usize, usize)> = stops(code, select).into_iter().map(|(a, b)| (start + a, start + b)).collect();
        let end = start + code.chars().count();
        at.push((end, end));
        Stops { at, len }
    }

    /// Tab: the next stop, moved by what was typed since. `None` (and nothing left) when the cursor has left the stop
    /// it was filling in, or there is no next one.
    pub fn advance(&mut self, len: usize, cursor: usize) -> Option<(usize, usize)> {
        let delta = len as i64 - self.len as i64;
        let shift = |n: usize| usize::try_from(n as i64 + delta).unwrap_or(0);
        let (lo, hi) = *self.at.first()?;
        if cursor < lo || cursor > shift(hi) || self.at.len() < 2 {
            self.at.clear();
            return None;
        }
        self.at = self.at.iter().skip(1).map(|(a, b)| (shift(*a), shift(*b))).collect();
        self.len = len;
        self.at.first().copied()
    }
}

/// What an action button adds after the expression it wraps.
#[derive(Clone, Copy, PartialEq)]
pub enum Extra {
    None,
    /// `, x`: the variable the expression is about.
    Var,
    /// `, (x, -5, 5)`: a plotting range.
    Range,
}

/// A button of the action row: it wraps the selected expression in a call, `solve(<selection>, x)`.
pub struct Act {
    pub label: &'static str,
    pub func: &'static str,
    pub extra: Extra,
    pub hint: &'static str,
}

pub const ACTIONS: &[Act] = &[
    Act { label: "Simplify", func: "simplify", extra: Extra::None, hint: "Simplify the selected expression" },
    Act { label: "Expand", func: "expand", extra: Extra::None, hint: "Multiply out" },
    Act { label: "Factor", func: "factor", extra: Extra::None, hint: "Factor" },
    Act { label: "Solve", func: "solve", extra: Extra::Var, hint: "Solve the equation or inequality" },
    Act { label: "Differentiate", func: "diff", extra: Extra::Var, hint: "Derivative" },
    Act { label: "Integrate", func: "integrate", extra: Extra::Var, hint: "Indefinite integral" },
    Act { label: "Numeric", func: "N", extra: Extra::None, hint: "Decimal value" },
    Act { label: "Plot", func: "plot", extra: Extra::Range, hint: "Draw it" },
];

/// The variable an expression is about: the one name in it that is neither a function nor a constant, else `x`.
pub fn guess_variable(expr: &str, is_known: &dyn Fn(&str) -> bool) -> String {
    let mut names: Vec<String> = Vec::new();
    let mut word = String::new();
    let chars: Vec<char> = expr.chars().chain(std::iter::once(' ')).collect();
    for (i, c) in chars.iter().enumerate() {
        if c.is_alphanumeric() || *c == '_' {
            word.push(*c);
            continue;
        }
        let starts_with_letter = word.chars().next().is_some_and(char::is_alphabetic);
        let is_call = chars.get(i) == Some(&'(');
        if starts_with_letter && !is_call && !is_known(&word) && !names.contains(&word) {
            names.push(word.clone());
        }
        word.clear();
    }
    match names.as_slice() {
        [only] => only.clone(),
        _ if names.iter().any(|n| n == "x") => "x".to_owned(),
        [first, ..] if names.len() == 2 => first.clone(),
        _ => "x".to_owned(),
    }
}

/// The char range of the expression on `line` that an action acts on: the whole line, minus indentation, a trailing
/// comment, and the `name =` of an assignment.
pub fn expression_span(line: &str) -> (usize, usize) {
    let chars: Vec<char> = line.chars().collect();
    let mut start = chars.iter().take_while(|c| c.is_whitespace()).count();
    // `name = ` (a single equals sign after one identifier)
    let ident_end = start + chars.iter().skip(start).take_while(|c| c.is_alphanumeric() || **c == '_').count();
    if ident_end > start {
        let mut k = ident_end;
        while chars.get(k) == Some(&' ') {
            k += 1;
        }
        if chars.get(k) == Some(&'=') && chars.get(k + 1) != Some(&'=') {
            start = k + 1;
            while chars.get(start) == Some(&' ') {
                start += 1;
            }
        }
    }
    let mut end = chars.len();
    if let Some(hash) = chars.iter().position(|c| *c == '#') {
        end = hash.max(start);
    }
    while end > start && chars.get(end - 1).is_some_and(|c| c.is_whitespace()) {
        end -= 1;
    }
    (start, end.max(start))
}

/// The call that wraps `expr` for `act`, and the char range inside it that should be selected afterwards (the variable
/// or range, so it is easy to change).
pub fn wrap(act: &Act, expr: &str, var: &str) -> (String, Option<(usize, usize)>) {
    let head = format!("{}({expr}", act.func);
    let base = head.chars().count();
    match act.extra {
        Extra::None => (format!("{head})"), None),
        Extra::Var => (format!("{head}, {var})"), Some((base + 2, base + 2 + var.chars().count()))),
        Extra::Range => (format!("{head}, ({var}, -5, 5))"), Some((base + 3, base + 3 + var.chars().count()))),
    }
}

/// The contents of the symbols window; returns the template that was clicked.
pub fn contents(ui: &mut egui::Ui, theme: &Theme) -> Option<&'static Template> {
    let mut clicked = None;
    for (name, items) in GROUPS {
        ui.add_space(8.0);
        ui.label(RichText::new(*name).size(theme::TEXT_CAPTION).color(theme.text_faint));
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for template in *items {
                let label = RichText::new(template.label).font(fonts::math_font(16.0)).color(theme.text);
                let response = ui.add(egui::Button::new(label).min_size(vec2(38.0, 30.0)));
                if response.on_hover_text(format!("{}\n{}", template.hint, template.code)).clicked() {
                    clicked = Some(template);
                }
            }
        });
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_the_first_placeholder() {
        assert_eq!(selection("integrate(f, x)", "f"), Some((10, 11)));
        assert_eq!(selection("pi", "pi"), Some((0, 2)));
        assert_eq!(selection(" == ", ""), None);
    }

    #[test]
    fn every_placeholder_is_found_in_its_template() {
        for (_, items) in GROUPS {
            for template in *items {
                assert_eq!(stops(template.code, template.select).len(), template.select.split_whitespace().count(), "{}", template.label);
            }
        }
    }

    #[test]
    fn actions_wrap_an_expression_and_select_the_variable() {
        let act = |name: &str| ACTIONS.iter().find(|a| a.func == name).unwrap();
        assert_eq!(wrap(act("solve"), "x**2 - 4", "x"), ("solve(x**2 - 4, x)".to_owned(), Some((16, 17))));
        assert_eq!(wrap(act("expand"), "(a+b)**2", "a"), ("expand((a+b)**2)".to_owned(), None));
        let (code, sel) = wrap(act("plot"), "sin(t)", "t");
        assert_eq!(code, "plot(sin(t), (t, -5, 5))");
        assert_eq!(sel.map(|(a, b)| code.chars().skip(a).take(b - a).collect::<String>()), Some("t".to_owned()));
    }

    #[test]
    fn the_expression_on_a_line_skips_the_assignment_and_comment() {
        let span = |l: &str| {
            let (a, b) = expression_span(l);
            l.chars().skip(a).take(b - a).collect::<String>()
        };
        assert_eq!(span("y = x**2 - 1  # note"), "x**2 - 1");
        assert_eq!(span("  x**2 == 4"), "x**2 == 4");
        assert_eq!(span("eq = x == 3"), "x == 3");
        assert_eq!(span(""), "");
    }

    #[test]
    fn the_variable_is_the_one_unknown_name() {
        let known = |n: &str| matches!(n, "pi" | "E" | "I" | "oo");
        assert_eq!(guess_variable("t**2 - 4", &known), "t");
        assert_eq!(guess_variable("sin(t) + pi", &known), "t");
        assert_eq!(guess_variable("a*x + b", &known), "x", "x wins among several");
        assert_eq!(guess_variable("4 + 4", &known), "x");
        assert_eq!(guess_variable("m*n", &known), "m", "otherwise the first of two");
    }

    #[test]
    fn placeholders_are_whole_words_in_order() {
        assert_eq!(stops("integrate(f, (x, a, b))", "f x a b"), vec![(10, 11), (14, 15), (17, 18), (20, 21)]);
        assert_eq!(stops("limit(f, x, 0)", "f x 0"), vec![(6, 7), (9, 10), (12, 13)]);
    }

    #[test]
    fn tab_walks_through_the_stops_and_follows_the_typing() {
        // "y = " then diff(f, x): f at 9..10, x at 12..13, end at 14
        let mut s = Stops::new("diff(f, x)", "f x", 4, 14);
        assert_eq!(s.at, vec![(9, 10), (12, 13), (14, 14)]);
        // the user typed "sin(x)" over f: 5 chars longer, cursor after it
        assert_eq!(s.advance(19, 15), Some((17, 18)), "x moved right by what was typed");
        assert_eq!(s.advance(19, 18), Some((19, 19)), "then the point after the template");
        assert_eq!(s.advance(19, 19), None, "nothing left");
        // moving away from the placeholder ends the walk
        let mut s = Stops::new("diff(f, x)", "f x", 0, 10);
        assert_eq!(s.advance(10, 0), None);
        assert!(s.at.is_empty());
    }
}
