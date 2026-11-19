//! Find and replace across the cells of a notebook.

use mathcraft_notebook::{CellId, Notebook};

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Query {
    pub find: String,
    /// Letters must match in case.
    pub case: bool,
    /// The match must not touch a letter, digit or underscore on either side.
    pub word: bool,
}

/// The find window: what is typed in it and whether the search box should take the keyboard.
#[derive(Default)]
pub struct State {
    pub query: Query,
    pub with: String,
    pub focus: bool,
}

/// A line of a cell that holds at least one match.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub cell: CellId,
    /// The cell's place in the notebook, counting from 1.
    pub number: usize,
    /// The line, trimmed.
    pub text: String,
    /// Matches on the line.
    pub count: usize,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The length in bytes of the match at the start of `rest`, if there is one.
fn match_len(rest: &str, find: &str, case: bool) -> Option<usize> {
    let mut len = 0;
    let mut chars = rest.chars();
    for f in find.chars() {
        let c = chars.next()?;
        if c != f && (case || !c.to_lowercase().eq(f.to_lowercase())) {
            return None;
        }
        len += c.len_utf8();
    }
    Some(len)
}

/// Byte ranges of the matches in `text`, left to right and not overlapping.
pub fn ranges(text: &str, q: &Query) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if q.find.is_empty() {
        return out;
    }
    let mut skip_to = 0;
    for (i, _) in text.char_indices() {
        if i < skip_to {
            continue;
        }
        let Some(len) = text.get(i..).and_then(|rest| match_len(rest, &q.find, q.case)) else { continue };
        let end = i + len;
        if q.word {
            let before = text.get(..i).and_then(|t| t.chars().next_back());
            let after = text.get(end..).and_then(|t| t.chars().next());
            if before.is_some_and(is_word) || after.is_some_and(is_word) {
                continue;
            }
        }
        out.push((i, end));
        skip_to = end;
    }
    out
}

/// Every line with a match, in notebook order.
pub fn hits(nb: &Notebook, q: &Query) -> Vec<Hit> {
    let mut out = Vec::new();
    for (n, cell) in nb.cells().iter().enumerate() {
        for line in cell.source.lines() {
            let count = ranges(line, q).len();
            if count > 0 {
                out.push(Hit { cell: cell.id, number: n + 1, text: line.trim().to_owned(), count });
            }
        }
    }
    out
}

/// `text` with every match replaced.
pub fn replace(text: &str, q: &Query, with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (a, b) in ranges(text, q) {
        out.push_str(text.get(last..a).unwrap_or_default());
        out.push_str(with);
        last = b;
    }
    out.push_str(text.get(last..).unwrap_or_default());
    out
}

/// The new source of every cell that has a match.
pub fn replace_all(nb: &Notebook, q: &Query, with: &str) -> Vec<(CellId, String)> {
    nb.cells().iter().filter(|c| !ranges(&c.source, q).is_empty()).map(|c| (c.id, replace(&c.source, q, with))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(find: &str, case: bool, word: bool) -> Query {
        Query { find: find.into(), case, word }
    }

    #[test]
    fn matches_ignore_case_unless_asked() {
        assert_eq!(ranges("Sin sin SIN", &q("sin", false, false)), vec![(0, 3), (4, 7), (8, 11)]);
        assert_eq!(ranges("Sin sin SIN", &q("sin", true, false)), vec![(4, 7)]);
        assert!(ranges("abc", &q("", false, false)).is_empty());
    }

    #[test]
    fn whole_words_do_not_touch_other_letters() {
        assert_eq!(ranges("x xy x_1 x", &q("x", true, true)), vec![(0, 1), (9, 10)]);
        assert_eq!(ranges("x xy x_1 x", &q("x", true, false)).len(), 4);
    }

    #[test]
    fn matches_do_not_overlap_and_unicode_is_safe() {
        assert_eq!(ranges("aaaa", &q("aa", true, false)), vec![(0, 2), (2, 4)]);
        assert_eq!(ranges("α + α", &q("α", true, false)), vec![(0, 2), (5, 7)]);
        assert_eq!(replace("é = é", &q("é", true, false), "e"), "e = e");
    }

    #[test]
    fn replacing_keeps_the_rest() {
        assert_eq!(replace("f(x) + x**2", &q("x", true, true), "t"), "f(t) + t**2");
        assert_eq!(replace("nothing", &q("x", true, true), "t"), "nothing");
    }

    #[test]
    fn a_notebook_reports_lines_and_replaces_by_cell() {
        let nb = Notebook::from_text("# %%\nx = 1\ny = x + x\n\n# %%\nz = 2\n");
        let h = hits(&nb, &q("x", true, true));
        assert_eq!(h.iter().map(|h| (h.number, h.text.as_str(), h.count)).collect::<Vec<_>>(), vec![(1, "x = 1", 1), (1, "y = x + x", 2)]);
        let changed = replace_all(&nb, &q("x", true, true), "u");
        assert_eq!(changed.len(), 1);
        assert_eq!(changed.first().map(|(_, t)| t.as_str()), Some("u = 1\ny = u + u"));
    }
}
