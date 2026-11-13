//! The symbol palette: buttons that insert a formula template (`integrate(f, x)`) at the cursor of the code cell
//! being edited, with the first placeholder selected so typing replaces it. Notebooks stay plain text; the
//! buttons only save typing and show what the code looks like as mathematics.

use crate::fonts;
use crate::theme::{self, Theme};
use eframe::egui::{self, RichText, vec2};

/// One button: what it shows, the code it inserts, and the part of that code to select afterwards.
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
            t("∫ a→b", "integrate(f, (x, a, b))", "f", "Definite integral"),
            t("d/dx", "diff(f, x)", "f", "Derivative"),
            t("dⁿ/dxⁿ", "diff(f, x, 2)", "f", "Higher derivative"),
            t("Σ", "summation(f, (k, 1, n))", "f", "Sum over k"),
            t("lim", "limit(f, x, 0)", "f", "Limit"),
            t("series", "series(f, x, 0, 6)", "f", "Taylor series"),
            t("Integral", "Integral(f, x)", "f", "Integral that is shown but not computed"),
            t("Derivative", "Derivative(f, x)", "f", "Derivative that is shown but not computed"),
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
            t("solve", "solve(eq, x)", "eq", "Solve an equation or inequality"),
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
            t("[ ]", "[[a, b], [c, d]]", "a", "A 2×2 matrix"),
            t("det", "det(A)", "A", "Determinant"),
            t("A⁻¹", "inv(A)", "A", "Inverse"),
            t("A^T", "transpose(A)", "A", "Transpose"),
            t("eig", "eigenvals(A)", "A", "Eigenvalues"),
        ],
    ),
];

/// The char offsets `(start, end)` of `select` inside `code`, or `None` when there is nothing to select.
pub fn selection(code: &str, select: &str) -> Option<(usize, usize)> {
    if select.is_empty() {
        return None;
    }
    let at = code.find(select)?;
    let start = code.get(..at)?.chars().count();
    Some((start, start + select.chars().count()))
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
    fn every_template_selects_something_inside_its_code() {
        for (_, items) in GROUPS {
            for template in *items {
                assert!(template.select.is_empty() || template.code.contains(template.select), "{}", template.label);
            }
        }
    }
}
