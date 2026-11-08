//! A small Markdown renderer for text cells: headings, paragraphs, lists, quotes, code blocks, rules, and inline
//! **bold**, *italic* and `code`.

use crate::fonts::strong_family;
use crate::theme::Theme;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId, Margin, Response, RichText, Sense, Stroke, Ui};

#[derive(Debug, PartialEq, Clone, Default)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

/// Split a line of Markdown into styled spans. Unclosed markers are kept as plain text.
pub fn inline(s: &str) -> Vec<Span> {
    let chars: Vec<char> = s.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    let flush = |plain: &mut String, spans: &mut Vec<Span>| {
        if !plain.is_empty() {
            spans.push(Span { text: std::mem::take(plain), ..Span::default() });
        }
    };
    // The index of the closing marker (a run of `len` copies of `marker`) after `from`, if any.
    let close = |from: usize, marker: char, len: usize| -> Option<usize> {
        let mut j = from;
        while j + len <= chars.len() {
            if chars.get(j..j + len).is_some_and(|w| w.iter().all(|c| *c == marker)) && j > from {
                return Some(j);
            }
            j += 1;
        }
        None
    };
    while let Some(&c) = chars.get(i) {
        let next = chars.get(i + 1).copied();
        let styled = match (c, next) {
            ('*', Some('*')) => close(i + 2, '*', 2).map(|j| (j, 2, Span { bold: true, ..Span::default() })),
            ('*', _) | ('_', _)
                if chars.get(i + 1).is_some_and(|n| !n.is_whitespace())
                    && (c == '*' || i == 0 || chars.get(i.wrapping_sub(1)).is_some_and(|p| !p.is_alphanumeric())) =>
            {
                close(i + 1, c, 1).map(|j| (j, 1, Span { italic: true, ..Span::default() }))
            }
            ('`', _) => close(i + 1, '`', 1).map(|j| (j, 1, Span { code: true, ..Span::default() })),
            _ => None,
        };
        match styled {
            Some((j, len, mut span)) => {
                flush(&mut plain, &mut spans);
                span.text = chars.get(i + len..j).map(|w| w.iter().collect()).unwrap_or_default();
                spans.push(span);
                i = j + len;
            }
            None => {
                plain.push(c);
                i += 1;
            }
        }
    }
    flush(&mut plain, &mut spans);
    spans
}

/// A run of a paragraph: text (with `**bold**` etc. still in it) or the source of a `$math$` span.
#[derive(Debug, PartialEq, Clone)]
pub enum Piece {
    Text(String),
    Math(String),
}

/// Split at `$...$` spans. A `$` opens math only when it is followed by a non-space and the closing `$` follows a
/// non-space and is not followed by a digit, so "costs $5 and $6" stays text. `\$` is a literal dollar.
pub fn pieces(s: &str) -> Vec<Piece> {
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<Piece> = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c == '\\' && chars.get(i + 1) == Some(&'$') {
            text.push('$');
            i += 2;
            continue;
        }
        if c == '$' && chars.get(i + 1).is_some_and(|n| !n.is_whitespace() && *n != '$') {
            let close = (i + 1..chars.len()).find(|&j| {
                chars.get(j) == Some(&'$')
                    && chars.get(j - 1).is_some_and(|p| !p.is_whitespace() && *p != '\\')
                    && !chars.get(j + 1).is_some_and(char::is_ascii_digit)
            });
            if let Some(j) = close {
                if !text.is_empty() {
                    out.push(Piece::Text(std::mem::take(&mut text)));
                }
                out.push(Piece::Math(chars.get(i + 1..j).map(|w| w.iter().collect()).unwrap_or_default()));
                i = j + 1;
                continue;
            }
        }
        text.push(c);
        i += 1;
    }
    if !text.is_empty() {
        out.push(Piece::Text(text));
    }
    out
}

/// Body text size and leading of text cells.
const BODY: f32 = 15.0;
const LEADING: f32 = 1.5;

fn job(spans: &[Span], size: f32, theme: &Theme, color: Color32, width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let line_height = Some((size * LEADING).round());
    for s in spans {
        let format = if s.code {
            TextFormat {
                font_id: FontId::new(size * 0.9, FontFamily::Monospace),
                color: theme.syntax.text,
                background: theme.surface_hover,
                valign: egui::Align::Center,
                ..Default::default()
            }
        } else if s.bold {
            TextFormat { font_id: FontId::new(size, strong_family()), color: theme.text, italics: s.italic, line_height, ..Default::default() }
        } else {
            TextFormat { font_id: FontId::new(size, FontFamily::Proportional), color, italics: s.italic, line_height, ..Default::default() }
        };
        job.append(&s.text, 0.0, format);
    }
    job.wrap.max_width = width;
    job
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    line.get(hashes..)?.strip_prefix(' ').map(|t| (hashes, t.trim()))
}

/// One line or paragraph of styled text, with `$math$` typeset in place. Plain text keeps the fast path (one wrapped
/// label); with math it flows left to right and wraps between pieces.
fn rich(ui: &mut Ui, text: &str, color: Color32, theme: &Theme) {
    let parts = pieces(text);
    let ctx = ui.ctx().clone();
    let math = |src: &str| crate::math_view::inline_layout(&ctx, src, BODY * 1.15);
    if !parts.iter().any(|p| matches!(p, Piece::Math(src) if math(src).is_some())) {
        let width = ui.available_width();
        ui.label(job(&inline(text), BODY, theme, color, width));
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for part in &parts {
            match part {
                Piece::Math(src) => match math(src) {
                    Some(layout) => {
                        crate::math_view::show(ui, &layout, theme, BODY * 1.15);
                    }
                    None => {
                        ui.label(job(&inline(&format!("`{src}`")), BODY, theme, color, f32::INFINITY));
                    }
                },
                Piece::Text(t) => {
                    ui.label(job(&inline(t), BODY, theme, color, f32::INFINITY));
                }
            }
        }
    });
}

fn paragraph(ui: &mut Ui, text: &mut String, theme: &Theme) {
    if !text.trim().is_empty() {
        rich(ui, text.trim(), theme.text, theme);
        ui.add_space(8.0);
    }
    text.clear();
}

/// A `$$ ... $$` line: the expression typeset on its own line, centred. Falls back to code-style text.
fn display_math(ui: &mut Ui, src: &str, theme: &Theme) {
    ui.add_space(4.0);
    ui.vertical_centered(|ui| match crate::math_view::inline_layout(ui.ctx(), src, BODY * 1.5) {
        Some(layout) => {
            crate::math_view::show(ui, &layout, theme, BODY * 1.5);
        }
        None => {
            ui.label(RichText::new(src).monospace().color(theme.text_dim));
        }
    });
    ui.add_space(4.0);
}

pub fn show(ui: &mut Ui, source: &str, theme: &Theme) -> Response {
    let r = ui
        .vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            let mut para = String::new();
            let mut code: Option<Vec<&str>> = None;
            for raw in source.lines() {
                let line = raw.trim_end();
                if line.trim_start().starts_with("```") {
                    match code.take() {
                        Some(block) => {
                            egui::Frame::new()
                                .fill(theme.code_bg)
                                .stroke(Stroke::new(1.0, theme.cell_outline))
                                .corner_radius(crate::theme::RADIUS_LARGE)
                                .inner_margin(Margin::symmetric(12, 9))
                                .show(ui, |ui| {
                                    ui.add(egui::Label::new(RichText::new(block.join("\n")).monospace().size(13.5)).selectable(true));
                                });
                            ui.add_space(6.0);
                        }
                        None => {
                            paragraph(ui, &mut para, theme);
                            code = Some(Vec::new());
                        }
                    }
                    continue;
                }
                if let Some(block) = code.as_mut() {
                    block.push(line);
                    continue;
                }
                let trimmed = line.trim_start();
                let indent = (line.len() - trimmed.len()) / 2;
                if line.trim().is_empty() {
                    paragraph(ui, &mut para, theme);
                } else if let Some(src) = line.trim().strip_prefix("$$").and_then(|r| r.strip_suffix("$$")).filter(|r| !r.trim().is_empty()) {
                    paragraph(ui, &mut para, theme);
                    display_math(ui, src.trim(), theme);
                } else if let Some((level, text)) = heading(line) {
                    paragraph(ui, &mut para, theme);
                    let size = [27.0, 20.0, 16.5].get(level - 1).copied().unwrap_or(16.5);
                    ui.add_space(if level == 1 { 2.0 } else { 10.0 });
                    ui.label(RichText::new(text).font(FontId::new(size, strong_family())).color(theme.text));
                    ui.add_space(if level == 1 { 4.0 } else { 2.0 });
                } else if matches!(line.trim(), "---" | "***" | "___") {
                    paragraph(ui, &mut para, theme);
                    ui.separator();
                } else if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
                    paragraph(ui, &mut para, theme);
                    list_item(ui, indent, "•", item, theme);
                } else if let Some((n, item)) = numbered(trimmed) {
                    paragraph(ui, &mut para, theme);
                    list_item(ui, indent, &format!("{n}."), item, theme);
                } else if let Some(q) = trimmed.strip_prefix('>') {
                    paragraph(ui, &mut para, theme);
                    ui.horizontal(|ui| {
                        let (bar, _) = ui.allocate_exact_size(egui::vec2(3.0, 18.0), Sense::hover());
                        ui.painter().rect_filled(bar, 1.5, theme.border_strong);
                        rich(ui, q.trim(), theme.text_dim, theme);
                    });
                } else {
                    if !para.is_empty() {
                        para.push(' ');
                    }
                    para.push_str(line.trim());
                }
            }
            paragraph(ui, &mut para, theme);
        })
        .response;
    r.interact(Sense::click())
}

fn numbered(s: &str) -> Option<(&str, &str)> {
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    let rest = s.get(digits..)?.strip_prefix(". ")?;
    (digits > 0).then(|| (s.get(..digits).unwrap_or("1"), rest))
}

fn list_item(ui: &mut Ui, indent: usize, marker: &str, text: &str, theme: &Theme) {
    ui.horizontal_top(|ui| {
        ui.add_space(8.0 + 16.0 * indent as f32);
        ui.label(RichText::new(marker).size(BODY).color(theme.text_faint));
        ui.add_space(6.0);
        rich(ui, text.trim(), theme.text, theme);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str, bold: bool, italic: bool, code: bool) -> Span {
        Span { text: text.into(), bold, italic, code }
    }

    #[test]
    fn inline_styles() {
        assert_eq!(inline("a **b** c"), vec![s("a ", false, false, false), s("b", true, false, false), s(" c", false, false, false)]);
        assert_eq!(inline("use `x**2` now"), vec![s("use ", false, false, false), s("x**2", false, false, true), s(" now", false, false, false)]);
        assert_eq!(inline("*soft* and _also_"), vec![s("soft", false, true, false), s(" and ", false, false, false), s("also", false, true, false)]);
    }

    #[test]
    fn unclosed_markers_stay_text() {
        assert_eq!(inline("2 * 3 and **open"), vec![s("2 * 3 and **open", false, false, false)]);
        assert_eq!(inline("snake_case_name"), vec![s("snake_case_name", false, false, false)]);
        assert!(inline("").is_empty());
    }

    #[test]
    fn math_spans_are_found_and_prices_are_not() {
        let t = |x: &str| Piece::Text(x.into());
        let m = |x: &str| Piece::Math(x.into());
        assert_eq!(pieces("so $x**2 + 1$ here"), vec![t("so "), m("x**2 + 1"), t(" here")]);
        assert_eq!(pieces("costs $5 and $6 total"), vec![t("costs $5 and $6 total")]);
        assert_eq!(pieces("a \\$ b"), vec![t("a $ b")]);
        assert_eq!(pieces("$a$ and $b$"), vec![m("a"), t(" and "), m("b")]);
        assert_eq!(pieces("open $x"), vec![t("open $x")]);
        assert!(pieces("").is_empty());
    }

    #[test]
    fn headings_and_numbers() {
        assert_eq!(heading("## Title"), Some((2, "Title")));
        assert_eq!(heading("#nospace"), None);
        assert_eq!(numbered("12. item"), Some(("12", "item")));
        assert_eq!(numbered("x. item"), None);
    }
}
