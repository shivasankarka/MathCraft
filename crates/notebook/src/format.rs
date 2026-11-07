//! The notebook file format: a plain script with `# %%` cell markers, so the same file runs as a script and opens as
//! a notebook, and diffs cleanly in git. Outputs are not stored.
//!
//! ```text
//! # mathcraft notebook v1
//! # %% [markdown]
//! # # A title
//! # %%
//! x = symbols('x')
//! diff(sin(x), x)
//! ```

use crate::CellKind;

pub const HEADER: &str = "# mathcraft notebook v1";

/// One cell as written in a file.
#[derive(Debug, Clone, PartialEq)]
pub struct CellText {
    pub kind: CellKind,
    pub source: String,
}

fn marker_kind(line: &str) -> Option<CellKind> {
    let t = line.trim_end();
    let rest = t.strip_prefix("# %%")?;
    match rest.trim() {
        "" => Some(CellKind::Code),
        "[markdown]" | "[md]" => Some(CellKind::Markdown),
        _ => Some(CellKind::Code),
    }
}

/// Split file text into cells. Text before the first marker becomes the first cell if it has any content.
pub fn parse(text: &str) -> Vec<CellText> {
    let mut cells: Vec<CellText> = Vec::new();
    let mut current: Option<(CellKind, Vec<&str>)> = None;
    let mut preamble: Vec<&str> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 && line.trim() == HEADER {
            continue;
        }
        if let Some(kind) = marker_kind(line) {
            if let Some((k, lines)) = current.take() {
                cells.push(finish(k, &lines));
            } else if preamble.iter().any(|l| !l.trim().is_empty()) {
                cells.push(finish(CellKind::Code, &preamble));
            }
            current = Some((kind, Vec::new()));
        } else if let Some((_, lines)) = current.as_mut() {
            lines.push(line);
        } else {
            preamble.push(line);
        }
    }
    match current {
        Some((k, lines)) => cells.push(finish(k, &lines)),
        None if preamble.iter().any(|l| !l.trim().is_empty()) => cells.push(finish(CellKind::Code, &preamble)),
        None => {}
    }
    cells
}

fn finish(kind: CellKind, lines: &[&str]) -> CellText {
    let mut body: Vec<String> = match kind {
        CellKind::Code => lines.iter().map(|l| (*l).to_string()).collect(),
        CellKind::Markdown => lines.iter().map(|l| l.strip_prefix("# ").or_else(|| l.strip_prefix('#')).unwrap_or(l).to_string()).collect(),
    };
    while body.last().is_some_and(|l| l.trim().is_empty()) {
        body.pop();
    }
    while body.first().is_some_and(|l| l.trim().is_empty()) {
        body.remove(0);
    }
    CellText { kind, source: body.join("\n") }
}

pub fn write(cells: &[CellText]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    for c in cells {
        out.push('\n');
        match c.kind {
            CellKind::Code => {
                out.push_str("# %%\n");
                out.push_str(&c.source);
            }
            CellKind::Markdown => {
                out.push_str("# %% [markdown]\n");
                out.push_str(&c.source.lines().map(|l| if l.is_empty() { "#".to_string() } else { format!("# {l}") }).collect::<Vec<_>>().join("\n"));
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let cells = vec![
            CellText { kind: CellKind::Markdown, source: "# Title\n\nSome *text*".into() },
            CellText { kind: CellKind::Code, source: "x = 1\n\ny = x + 1".into() },
            CellText { kind: CellKind::Code, source: "y".into() },
        ];
        let text = write(&cells);
        assert!(text.starts_with(HEADER));
        assert_eq!(parse(&text), cells);
    }

    #[test]
    fn plain_scripts_open_as_one_cell() {
        let cells = parse("x = 1\ny = 2\n");
        assert_eq!(cells, vec![CellText { kind: CellKind::Code, source: "x = 1\ny = 2".into() }]);
    }

    #[test]
    fn leading_code_before_a_marker_is_kept() {
        let cells = parse("a = 1\n# %%\nb = 2\n");
        assert_eq!(cells.iter().map(|c| c.source.as_str()).collect::<Vec<_>>(), vec!["a = 1", "b = 2"]);
    }

    #[test]
    fn empty_input_has_no_cells() {
        assert!(parse("").is_empty());
        assert!(parse("# mathcraft notebook v1\n").is_empty());
    }
}
