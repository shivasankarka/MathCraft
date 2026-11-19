//! Export a notebook as one HTML page: text cells as HTML, code cells with their results, plots as inline SVG.
//!
//! Maths is written as LaTeX between `\(...\)` and `\[...\]` and typeset by MathJax, which the page loads from a
//! content network when it is online. Offline the LaTeX shows as text; the plain-text value is in each result's tooltip.

use crate::markdown::{self, Piece};
use mathcraft_eval::Value;
use mathcraft_notebook::{Cell, CellKind, Notebook};

const STYLE: &str = "\
:root { color-scheme: light dark; --bg: #ffffff; --fg: #1d1d1f; --dim: #6e6e73; --code: #f5f5f7; --line: #d2d2d7; --accent: #0a64d6; --bad: #c5221f; }
@media (prefers-color-scheme: dark) { :root { --bg: #1c1c1e; --fg: #f2f2f7; --dim: #98989d; --code: #2c2c2e; --line: #3a3a3c; --accent: #6cb0ff; --bad: #ff8a80; } }
body { background: var(--bg); color: var(--fg); font: 16px/1.55 -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif; margin: 0; }
main { max-width: 860px; margin: 0 auto; padding: 32px 24px 80px; }
h1, h2, h3 { line-height: 1.25; }
a { color: var(--accent); }
pre, code { font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace; font-size: 0.9em; }
code { background: var(--code); padding: 1px 5px; border-radius: 4px; }
pre { margin: 0; padding: 10px 14px; background: var(--code); border-radius: 8px; overflow-x: auto; }
pre code { background: none; padding: 0; }
.cell { margin: 18px 0; }
.out { margin: 6px 0 0 14px; }
.out pre { background: none; padding: 2px 0; color: var(--dim); }
.math { overflow-x: auto; padding: 4px 0; }
.error { color: var(--bad); margin: 6px 0 0 14px; }
.plot svg { max-width: 100%; height: auto; background: #fff; border-radius: 8px; }
blockquote { margin: 8px 0; padding-left: 14px; border-left: 3px solid var(--line); color: var(--dim); }
table { border-collapse: collapse; margin: 10px 0; }
th, td { border-bottom: 1px solid var(--line); padding: 5px 14px 5px 0; text-align: left; }
hr { border: 0; border-top: 1px solid var(--line); }
";

const MATHJAX: &str = "https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-chtml.js";

/// The whole notebook as an HTML page.
pub fn notebook_html(nb: &Notebook, title: &str) -> String {
    let mut body = String::new();
    for cell in nb.cells() {
        match cell.kind {
            CellKind::Markdown => {
                body.push_str("<section class=\"cell text\">\n");
                body.push_str(&markdown_html(&cell.source));
                body.push_str("</section>\n");
            }
            CellKind::Code => code_cell(cell, &mut body),
        }
    }
    let title = escape(title);
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{title}</title>\n<style>\n{STYLE}</style>\n<script>window.MathJax = {{ tex: {{ inlineMath: [['\\\\(', '\\\\)']], displayMath: [['\\\\[', '\\\\]']] }} }};</script>\n<script defer src=\"{MATHJAX}\"></script>\n</head>\n<body>\n<main>\n{body}</main>\n</body>\n</html>\n"
    )
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

fn code_cell(cell: &Cell, out: &mut String) {
    out.push_str("<section class=\"cell code\">\n");
    out.push_str(&format!("<pre><code>{}</code></pre>\n", escape(cell.source.trim_end())));
    if let Some(o) = &cell.output {
        if !o.stdout.trim().is_empty() {
            out.push_str(&format!("<div class=\"out\"><pre>{}</pre></div>\n", escape(o.stdout.trim_end())));
        }
        match &o.result {
            Ok(Some(v)) => out.push_str(&value_html(v)),
            Ok(None) => {}
            Err(e) => out.push_str(&format!("<div class=\"error\">{}</div>\n", escape(&e.to_string()))),
        }
    }
    out.push_str("</section>\n");
}

/// A result: a plot as SVG, a typesettable value as display maths, anything else as text.
fn value_html(v: &Value) -> String {
    if let Value::Plot(p) = v {
        let svg = crate::plot_view::svg_text(p, [f64::NAN; 4], crate::plot_export::Look::default());
        return format!("<div class=\"out plot\">{svg}</div>\n");
    }
    let text = v.to_string();
    match v.latex() {
        Some(tex) => format!("<div class=\"out math\" title=\"{}\">\\[{}\\]</div>\n", escape(&text), escape(&tex)),
        None => format!("<div class=\"out\"><pre>{}</pre></div>\n", escape(&text)),
    }
}

/// Block state while converting Markdown.
#[derive(Default)]
struct Blocks {
    out: String,
    para: String,
    list: Option<&'static str>,
    rows: Vec<Vec<String>>,
}

impl Blocks {
    fn paragraph(&mut self) {
        if !self.para.trim().is_empty() {
            self.out.push_str(&format!("<p>{}</p>\n", inline_html(self.para.trim())));
        }
        self.para.clear();
    }

    fn close_list(&mut self) {
        if let Some(tag) = self.list.take() {
            self.out.push_str(&format!("</{tag}>\n"));
        }
    }

    fn table(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let rows = std::mem::take(&mut self.rows);
        let header = rows.get(1).is_some_and(|r| markdown::is_rule_row(r));
        self.out.push_str("<table>\n");
        for (n, row) in rows.iter().enumerate() {
            if header && n == 1 {
                continue;
            }
            let tag = if header && n == 0 { "th" } else { "td" };
            let cells: String = row.iter().map(|c| format!("<{tag}>{}</{tag}>", inline_html(c))).collect();
            self.out.push_str(&format!("<tr>{cells}</tr>\n"));
        }
        self.out.push_str("</table>\n");
    }

    /// Everything open ends: the paragraph, the list and the table.
    fn flush(&mut self) {
        self.paragraph();
        self.close_list();
        self.table();
    }

    fn item(&mut self, tag: &'static str, text: &str) {
        self.paragraph();
        self.table();
        if self.list != Some(tag) {
            self.close_list();
            self.out.push_str(&format!("<{tag}>\n"));
            self.list = Some(tag);
        }
        self.out.push_str(&format!("<li>{}</li>\n", inline_html(text.trim())));
    }
}

/// A text cell as HTML: the same blocks the app draws.
pub fn markdown_html(source: &str) -> String {
    let mut b = Blocks::default();
    let mut code: Option<Vec<&str>> = None;
    for raw in source.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            match code.take() {
                Some(block) => b.out.push_str(&format!("<pre><code>{}</code></pre>\n", escape(&block.join("\n")))),
                None => {
                    b.flush();
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
        if trimmed.starts_with('|') {
            b.paragraph();
            b.close_list();
            b.rows.push(markdown::table_row(trimmed));
            continue;
        }
        if trimmed.is_empty() {
            b.flush();
        } else if let Some(src) = trimmed.strip_prefix("$$").and_then(|r| r.strip_suffix("$$")).filter(|r| !r.trim().is_empty()) {
            b.flush();
            match math_tex(src.trim()) {
                Some(tex) => b.out.push_str(&format!("<div class=\"math\">\\[{}\\]</div>\n", escape(&tex))),
                None => b.out.push_str(&format!("<pre><code>{}</code></pre>\n", escape(src.trim()))),
            }
        } else if let Some((level, text)) = markdown::heading(line) {
            b.flush();
            b.out.push_str(&format!("<h{level}>{}</h{level}>\n", inline_html(text)));
        } else if matches!(trimmed, "---" | "***" | "___") {
            b.flush();
            b.out.push_str("<hr>\n");
        } else if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            b.item("ul", item);
        } else if let Some((_, item)) = markdown::numbered(trimmed) {
            b.item("ol", item);
        } else if let Some(q) = trimmed.strip_prefix('>') {
            b.flush();
            b.out.push_str(&format!("<blockquote>{}</blockquote>\n", inline_html(q.trim())));
        } else {
            b.close_list();
            b.table();
            if !b.para.is_empty() {
                b.para.push(' ');
            }
            b.para.push_str(trimmed);
        }
    }
    if let Some(block) = code {
        b.out.push_str(&format!("<pre><code>{}</code></pre>\n", escape(&block.join("\n"))));
    }
    b.flush();
    b.out
}

/// LaTeX for the MathCraft expression in a `$...$` span, if it is one.
fn math_tex(source: &str) -> Option<String> {
    crate::math_view::inline_expr(source).map(|e| mathcraft_core::latex::latex(&e))
}

/// A line of text with its `**bold**`, `*italic*`, `~~struck~~` and `code` spans, links and maths.
fn inline_html(text: &str) -> String {
    let mut out = String::new();
    for piece in markdown::parts(text) {
        match piece {
            Piece::Text(t) => {
                for s in markdown::inline(&t) {
                    let mut html = escape(&s.text);
                    if s.code {
                        html = format!("<code>{html}</code>");
                    }
                    if s.bold {
                        html = format!("<strong>{html}</strong>");
                    }
                    if s.italic {
                        html = format!("<em>{html}</em>");
                    }
                    if s.strike {
                        html = format!("<s>{html}</s>");
                    }
                    out.push_str(&html);
                }
            }
            Piece::Math(src) => match math_tex(&src) {
                Some(tex) => out.push_str(&format!("\\({}\\)", escape(&tex))),
                None => out.push_str(&format!("<code>{}</code>", escape(&src))),
            },
            Piece::Link(label, url) => out.push_str(&format!("<a href=\"{}\">{}</a>", escape(&url), escape(&label))),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathcraft_notebook::{RunRequest, Runner};

    #[test]
    fn text_is_escaped() {
        assert_eq!(escape("a < b & \"c\""), "a &lt; b &amp; &quot;c&quot;");
    }

    #[test]
    fn markdown_blocks_become_html() {
        let html = markdown_html(
            "# Title\n\nSome **bold** and `code` with $x**2$.\n\n- one\n- two\n\n1. first\n\n> quoted\n\n---\n\n| a | b |\n|---|---|\n| 1 | [s](https://e.org) |\n",
        );
        assert!(html.contains("<h1>Title</h1>"), "{html}");
        assert!(html.contains("<strong>bold</strong>") && html.contains("<code>code</code>"), "{html}");
        assert!(html.contains("\\(x^{2}\\)"), "{html}");
        assert!(html.contains("<ul>\n<li>one</li>\n<li>two</li>\n</ul>"), "{html}");
        assert!(html.contains("<ol>\n<li>first</li>\n</ol>"), "{html}");
        assert!(html.contains("<blockquote>quoted</blockquote>") && html.contains("<hr>"), "{html}");
        assert!(html.contains("<th>a</th><th>b</th>") && html.contains("<a href=\"https://e.org\">s</a>"), "{html}");
    }

    #[test]
    fn a_notebook_exports_sources_results_and_plots() {
        let mut nb = Notebook::from_text(
            "# %% [markdown]\n# # Heading\n\n# %%\nx = symbols('x')\nint_x = diff(x**3, x)\nint_x\n\n# %%\nplot(sin(x), (x, 0, 3))\n\n# %%\nprint('hi <b>')\n",
        );
        nb.run_blocking(&mut Runner::new(Default::default()), RunRequest::All);
        let html = notebook_html(&nb, "demo <1>");
        assert!(html.contains("<title>demo &lt;1&gt;</title>"));
        assert!(html.contains("<h1>Heading</h1>"), "{html}");
        assert!(html.contains("3 x^{2}") || html.contains("3x^{2}"), "{html}");
        assert!(html.contains("<svg"), "plots are inline SVG");
        assert!(html.contains("hi &lt;b&gt;"), "printed text is escaped");
        assert!(html.contains("mathjax"));
    }
}
