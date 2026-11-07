//! A small syntax highlighter for code cells.

use crate::theme::Syntax;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};
use mathcraft_eval::DOCS;

const KEYWORDS: &[&str] = &["def", "return", "if", "elif", "else", "for", "while", "in", "and", "or", "not", "lambda", "pass", "break", "continue", "is"];
const CONSTANTS: &[&str] = &["True", "False", "None", "pi", "E"];

pub fn highlight(text: &str, font: FontId, p: &Syntax) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut push = |s: &str, color: Color32| {
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
        if c == '#' {
            let mut j = i;
            while chars.get(j).is_some_and(|c| c.1 != '\n') {
                j += 1;
            }
            push(slice(i, j), p.comment);
            i = j;
        } else if c == '"' || c == '\'' {
            let mut j = i + 1;
            while let Some(&(_, d)) = chars.get(j) {
                j += 1;
                if d == c || d == '\n' {
                    break;
                }
            }
            push(slice(i, j), p.string);
            i = j;
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|n| n.1.is_ascii_digit())) {
            let mut j = i;
            while chars.get(j).is_some_and(|n| n.1.is_ascii_digit() || n.1 == '.' || n.1 == '_' || n.1 == 'e') {
                j += 1;
            }
            push(slice(i, j), p.number);
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
            push(word, color);
            i = j;
        } else {
            let mut j = i + 1;
            while chars.get(j).is_some_and(|n| !(n.1.is_alphanumeric() || "_#'\"".contains(n.1) || n.1 == '.')) {
                j += 1;
            }
            push(slice(i, j), p.text);
            i = j;
        }
    }
    job
}
