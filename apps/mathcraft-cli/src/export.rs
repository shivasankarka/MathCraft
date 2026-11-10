//! `export`: run a notebook and write it out as Markdown or a standalone HTML page.

use mathcraft_notebook::{Cell, CellKind, Notebook};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Markdown,
    Html,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        match s {
            "md" | "markdown" => Some(Format::Markdown),
            "html" => Some(Format::Html),
            _ => None,
        }
    }

    pub fn from_path(path: &str) -> Format {
        if path.ends_with(".html") || path.ends_with(".htm") { Format::Html } else { Format::Markdown }
    }
}

/// What a code cell printed, followed by its value or its error.
fn output_text(cell: &Cell) -> String {
    let Some(o) = &cell.output else { return String::new() };
    let mut s = o.stdout.clone();
    match &o.result {
        Ok(Some(v)) => {
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(&format!("{v}\n"));
        }
        Ok(None) => {}
        Err(e) => s.push_str(&format!("error: {e}\n")),
    }
    s
}

pub fn render(nb: &Notebook, format: Format, title: &str) -> String {
    match format {
        Format::Markdown => markdown(nb),
        Format::Html => html(nb, title),
    }
}

fn markdown(nb: &Notebook) -> String {
    let mut out = String::new();
    for c in nb.cells() {
        match c.kind {
            CellKind::Markdown => out.push_str(c.source.trim_end()),
            CellKind::Code => {
                out.push_str("```python\n");
                out.push_str(c.source.trim_end());
                out.push_str("\n```");
                let o = output_text(c);
                if !o.is_empty() {
                    out.push_str("\n\n```text\n");
                    out.push_str(o.trim_end());
                    out.push_str("\n```");
                }
            }
        }
        out.push_str("\n\n");
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn html(nb: &Notebook, title: &str) -> String {
    let mut body = String::new();
    for c in nb.cells() {
        match c.kind {
            // Text cells are shown as written (preformatted); there is no Markdown renderer in the CLI.
            CellKind::Markdown => body.push_str(&format!("<div class=\"text\">{}</div>\n", escape(c.source.trim_end()))),
            CellKind::Code => {
                body.push_str(&format!("<pre class=\"code\">{}</pre>\n", escape(c.source.trim_end())));
                let o = output_text(c);
                if !o.is_empty() {
                    body.push_str(&format!("<pre class=\"out\">{}</pre>\n", escape(o.trim_end())));
                }
            }
        }
    }
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>{t}</title>\n\
         <style>body{{max-width:46rem;margin:2rem auto;padding:0 1rem;font-family:system-ui,sans-serif;line-height:1.5}}\
         pre{{margin:.4rem 0;padding:.6rem .8rem;border-radius:6px;overflow-x:auto;font-size:.92em}}\
         .code{{background:#f3f4f6}}.out{{background:#fff;border:1px solid #e5e7eb;border-left:3px solid #6366f1}}\
         .text{{white-space:pre-wrap;margin:1rem 0}}</style></head>\n<body>\n{body}</body></html>\n",
        t = escape(title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathcraft_notebook::{RunRequest, Runner};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn ran(text: &str) -> Notebook {
        let mut nb = Notebook::from_text(text);
        let mut runner = Runner::new(Arc::new(AtomicBool::new(false)));
        nb.run_blocking(&mut runner, RunRequest::All);
        nb
    }

    #[test]
    fn markdown_has_source_and_output() {
        let nb = ran("# %% [md]\n# Title\n# %%\n1 + 2\n");
        let md = render(&nb, Format::Markdown, "t");
        assert!(md.contains("```python\n1 + 2\n```"), "{md}");
        assert!(md.contains("```text\n3\n```"), "{md}");
    }

    #[test]
    fn html_escapes_and_shows_errors() {
        let nb = ran("# %%\nx < 1 and 1/0\n");
        let h = render(&nb, Format::Html, "a<b");
        assert!(h.contains("<title>a&lt;b</title>"), "{h}");
        assert!(h.contains("x &lt; 1"), "{h}");
        assert!(h.contains("class=\"out\""), "{h}");
    }

    #[test]
    fn format_from_extension() {
        assert_eq!(Format::from_path("a.html"), Format::Html);
        assert_eq!(Format::from_path("a.md"), Format::Markdown);
        assert_eq!(Format::parse("html"), Some(Format::Html));
        assert_eq!(Format::parse("pdf"), None);
    }
}
