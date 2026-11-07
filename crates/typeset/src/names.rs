//! How symbol and function names are shown: Greek letters, math italics, subscripts, constants.

use crate::layout::Role;

pub(crate) struct SymbolParts {
    pub base: String,
    pub role: Role,
    pub descender: bool,
    pub sub: Option<String>,
}

const GREEK_LOWER: [(&str, char); 24] = [
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("epsilon", 'ε'),
    ("zeta", 'ζ'),
    ("eta", 'η'),
    ("theta", 'θ'),
    ("iota", 'ι'),
    ("kappa", 'κ'),
    ("lambda", 'λ'),
    ("mu", 'μ'),
    ("nu", 'ν'),
    ("xi", 'ξ'),
    ("omicron", 'ο'),
    ("pi", 'π'),
    ("rho", 'ρ'),
    ("sigma", 'σ'),
    ("tau", 'τ'),
    ("upsilon", 'υ'),
    ("phi", 'φ'),
    ("chi", 'χ'),
    ("psi", 'ψ'),
    ("omega", 'ω'),
];

const GREEK_UPPER: [(&str, char); 11] = [
    ("Gamma", 'Γ'),
    ("Delta", 'Δ'),
    ("Theta", 'Θ'),
    ("Lambda", 'Λ'),
    ("Xi", 'Ξ'),
    ("Pi", 'Π'),
    ("Sigma", 'Σ'),
    ("Phi", 'Φ'),
    ("Psi", 'Ψ'),
    ("Omega", 'Ω'),
    ("Upsilon", 'Υ'),
];

/// Mathematical italic form of an ASCII letter or lowercase Greek letter (the math font has real italics).
fn italic(c: char) -> char {
    let code = c as u32;
    let mapped = match c {
        'h' => Some(0x210E),
        'a'..='z' => Some(0x1D44E + (code - 'a' as u32)),
        'A'..='Z' => Some(0x1D434 + (code - 'A' as u32)),
        'α'..='ω' if c != 'ς' => Some(0x1D6FC + (code - 'α' as u32)),
        _ => None,
    };
    mapped.and_then(char::from_u32).unwrap_or(c)
}

pub(crate) fn italic_str(s: &str) -> String {
    s.chars().map(italic).collect()
}

fn has_descender(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'g' | 'j' | 'p' | 'q' | 'y' | 'β' | 'γ' | 'ζ' | 'η' | 'μ' | 'ξ' | 'ρ' | 'φ' | 'χ' | 'ψ'))
}

/// Split `x_1`, `x1`, `theta_n` into base and subscript. Digits are only split off a one-letter or Greek base.
fn split_subscript(name: &str) -> (&str, Option<&str>) {
    if let Some((base, sub)) = name.split_once('_') {
        if !base.is_empty() && !sub.is_empty() {
            return (base, Some(sub));
        }
        return (name, None);
    }
    let alpha_end = name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (alpha, digits) = name.split_at(alpha_end);
    let simple = alpha.chars().count() == 1 || is_greek(alpha);
    if !digits.is_empty() && !alpha.is_empty() && simple { (alpha, Some(digits)) } else { (name, None) }
}

fn is_greek(name: &str) -> bool {
    GREEK_LOWER.iter().any(|(n, _)| *n == name) || GREEK_UPPER.iter().any(|(n, _)| *n == name)
}

pub(crate) fn symbol(name: &str) -> SymbolParts {
    let (base, sub) = split_subscript(name);
    let sub = sub.map(|s| if s.chars().all(|c| c.is_ascii_digit()) { s.to_string() } else { italic_str(s) });
    let (text, role, desc_src) = match base {
        "E" => ("e".to_string(), Role::Constant, String::new()),
        "I" => ("i".to_string(), Role::Constant, String::new()),
        "pi" => ("π".to_string(), Role::Constant, String::new()),
        "oo" | "inf" => ("∞".to_string(), Role::Constant, String::new()),
        _ => {
            if let Some((_, g)) = GREEK_LOWER.iter().find(|(n, _)| *n == base) {
                (italic(*g).to_string(), Role::Variable, g.to_string())
            } else if let Some((_, g)) = GREEK_UPPER.iter().find(|(n, _)| *n == base) {
                (g.to_string(), Role::Variable, String::new())
            } else if base.chars().count() == 1 && base.chars().all(|c| c.is_ascii_alphabetic()) {
                (italic_str(base), Role::Variable, base.to_string())
            } else {
                (base.to_string(), Role::Variable, base.to_string())
            }
        }
    };
    SymbolParts { descender: has_descender(&desc_src), base: text, role, sub }
}

/// Display name of a function and whether it is an upright, operator-like name (`sin`) rather than a user function (`f`).
pub(crate) fn function_name(name: &str) -> (String, Role) {
    let shown = match name {
        "asin" => "arcsin",
        "acos" => "arccos",
        "atan" => "arctan",
        "acot" => "arccot",
        "asinh" => "arsinh",
        "acosh" => "arcosh",
        "atanh" => "artanh",
        "gamma" => "Γ",
        other => other,
    };
    if shown.chars().count() == 1 && shown.chars().all(|c| c.is_ascii_alphabetic()) {
        (italic_str(shown), Role::Variable)
    } else {
        (shown.to_string(), Role::Function)
    }
}

/// Functions whose positive integer power is written before the argument: `sin^2(x)`.
pub(crate) fn is_trig(name: &str) -> bool {
    matches!(name, "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "sinh" | "cosh" | "tanh" | "coth")
}
