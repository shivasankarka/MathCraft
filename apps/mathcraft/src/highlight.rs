//! A small syntax highlighter for code cells.

use crate::theme::Syntax;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};
use mathcraft_eval::DOCS;

const KEYWORDS: &[&str] = &["def", "return", "if", "elif", "else", "for", "while", "in", "and", "or", "not", "lambda", "pass", "break", "continue", "is"];
const CONSTANTS: &[&str] = &["True", "False", "None", "pi", "E"];

/// Where a power starts at char `i`: the length of the operator (`**` or `^`) and the end of the exponent that
/// follows it directly (a number, a name, or a parenthesised group on the same line, with an optional sign).
fn power_at(chars: &[(usize, char)], i: usize) -> Option<(usize, usize)> {
    let c = |k: usize| chars.get(k).map(|x| x.1);
    let op = match (c(i)?, c(i + 1)) {
        ('*', Some('*')) if i == 0 || c(i - 1) != Some('*') => 2,
        ('^', _) => 1,
        _ => return None,
    };
    let mut j = i + op;
    if matches!(c(j), Some('-' | '+')) {
        j += 1;
    }
    let start = j;
    if c(j) == Some('(') {
        let mut depth = 0usize;
        while let Some(d) = c(j) {
            match d {
                '(' => depth += 1,
                ')' => depth -= 1,
                '\n' => return None,
                _ => {}
            }
            j += 1;
            if depth == 0 {
                return Some((op, j));
            }
        }
        return None;
    }
    while c(j).is_some_and(|d| d.is_alphanumeric() || d == '_' || d == '.') {
        j += 1;
    }
    (j > start).then_some((op, j))
}

/// Highlight `text`. With `pretty`, a power such as `x**2` or `x^(n+1)` is drawn as a superscript: the operator
/// shrinks to nothing and the exponent is set small and raised. The text itself is unchanged, so editing works as usual.
pub fn highlight(text: &str, font: FontId, p: &Syntax, pretty: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let push = |job: &mut LayoutJob, s: &str, color: Color32| {
        if !s.is_empty() {
            job.append(s, 0.0, TextFormat { font_id: font.clone(), color, ..Default::default() });
        }
    };
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    let slice = |from: usize, to: usize| -> &str {
        let a = chars.get(from).map_or(text.len(), |c| c.0);
        let b = chars.get(to).map_or(text.len(), |c| c.0);
        text.get(a..b).unwrap_or("")
    };
    while i < chars.len() {
        let c = chars.get(i).map_or(' ', |c| c.1);
        if pretty && let Some((op, end)) = power_at(&chars, i) {
            let tiny = FontId::new(1.0, font.family.clone());
            job.append(slice(i, i + op), 0.0, TextFormat { font_id: tiny, color: Color32::TRANSPARENT, ..Default::default() });
            let small = FontId::new(font.size * 0.72, font.family.clone());
            let exp = slice(i + op, end);
            let color = if exp.trim_start_matches(['-', '+']).starts_with(|d: char| d.is_ascii_digit()) { p.number } else { p.text };
            job.append(exp, 0.0, TextFormat { font_id: small, color, valign: egui::Align::TOP, ..Default::default() });
            i = end;
        } else if c == '#' {
            let mut j = i;
            while chars.get(j).is_some_and(|c| c.1 != '\n') {
                j += 1;
            }
            push(&mut job, slice(i, j), p.comment);
            i = j;
        } else if c == '"' || c == '\'' {
            let mut j = i + 1;
            while let Some(&(_, d)) = chars.get(j) {
                j += 1;
                if d == c || d == '\n' {
                    break;
                }
            }
            push(&mut job, slice(i, j), p.string);
            i = j;
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|n| n.1.is_ascii_digit())) {
            let mut j = i;
            while chars.get(j).is_some_and(|n| n.1.is_ascii_digit() || n.1 == '.' || n.1 == '_' || n.1 == 'e') {
                j += 1;
            }
            push(&mut job, slice(i, j), p.number);
            i = j;
        } else if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while chars.get(j).is_some_and(|n| n.1.is_alphanumeric() || n.1 == '_') {
                j += 1;
            }
            let word = slice(i, j);
            let color = if KEYWORDS.contains(&word) {
                p.keyword
            } else if CONSTANTS.contains(&word) {
                p.constant
            } else if DOCS.iter().any(|d| d.0 == word) && chars.get(j).is_some_and(|n| n.1 == '(') {
                p.function
            } else {
                p.text
            };
            push(&mut job, word, color);
            i = j;
        } else {
            let mut j = i + 1;
            while chars.get(j).is_some_and(|n| !(n.1.is_alphanumeric() || "_#'\"".contains(n.1) || n.1 == '.')) && !(pretty && power_at(&chars, j).is_some()) {
                j += 1;
            }
            push(&mut job, slice(i, j), p.text);
            i = j;
        }
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    fn job(text: &str, pretty: bool) -> LayoutJob {
        highlight(text, FontId::monospace(14.0), &Theme::dark().syntax, pretty)
    }

    #[test]
    fn powers_become_superscripts_without_changing_the_text() {
        for src in ["x**2", "x ** 2 + y^3", "(a+b)**(n+1) - e^-x", "2**10", "a**b**c"] {
            assert_eq!(job(src, true).text, src, "the buffer must stay exactly what was typed");
        }
        let j = job("x**2 + 1", true);
        let raised: Vec<&str> =
            j.sections.iter().filter(|s| s.format.valign == egui::Align::TOP).map(|s| &j.text[s.byte_range.start.0..s.byte_range.end.0]).collect();
        assert_eq!(raised, ["2"]);
        let j = job("x^(n+1)", true);
        assert!(j.sections.iter().any(|s| s.format.valign == egui::Align::TOP && &j.text[s.byte_range.start.0..s.byte_range.end.0] == "(n+1)"));
        // off: nothing is raised
        assert!(job("x**2", false).sections.iter().all(|s| s.format.valign != egui::Align::TOP));
        // not a power: a lone `*`, a trailing operator, a space before the exponent
        assert!(job("2 * 3", true).sections.iter().all(|s| s.format.valign != egui::Align::TOP));
        assert!(job("x**", true).sections.iter().all(|s| s.format.valign != egui::Align::TOP));
        assert!(job("x ** 2", true).sections.iter().all(|s| s.format.valign != egui::Align::TOP));
    }
}
