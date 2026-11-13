//! A line diff, to show what changed in a notebook since the last commit.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Same,
    Added,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
}

/// Longest middle (in lines on both sides multiplied) that is compared exactly; beyond it the middle is shown as
/// removed then added, which is still correct but longer than it needs to be.
const MAX_CELLS: usize = 4_000_000;

/// The lines of `old` and `new` in order, each marked as kept, added or removed.
pub fn diff(old: &str, new: &str) -> Vec<Line> {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a.iter().rev().zip(b.iter().rev()).take(a.len().min(b.len()) - prefix).take_while(|(x, y)| x == y).count();
    let (am, bm) = (a.get(prefix..a.len() - suffix).unwrap_or(&[]), b.get(prefix..b.len() - suffix).unwrap_or(&[]));
    let line = |kind, text: &str| Line { kind, text: text.to_owned() };
    let mut out: Vec<Line> = a.iter().take(prefix).map(|t| line(Kind::Same, t)).collect();
    out.extend(middle(am, bm));
    out.extend(a.iter().skip(a.len() - suffix).map(|t| line(Kind::Same, t)));
    out
}

fn middle(a: &[&str], b: &[&str]) -> Vec<Line> {
    let line = |kind, text: &str| Line { kind, text: text.to_owned() };
    if a.len().saturating_mul(b.len()) > MAX_CELLS {
        return a.iter().map(|t| line(Kind::Removed, t)).chain(b.iter().map(|t| line(Kind::Added, t))).collect();
    }
    // lcs[i][j]: length of the longest common subsequence of a[i..] and b[j..]
    let w = b.len() + 1;
    let mut lcs = vec![0u32; (a.len() + 1) * w];
    let at = |t: &[u32], i: usize, j: usize| t.get(i * w + j).copied().unwrap_or(0);
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            let v = if a.get(i) == b.get(j) { at(&lcs, i + 1, j + 1) + 1 } else { at(&lcs, i + 1, j).max(at(&lcs, i, j + 1)) };
            if let Some(cell) = lcs.get_mut(i * w + j) {
                *cell = v;
            }
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while let (Some(x), Some(y)) = (a.get(i), b.get(j)) {
        if x == y {
            out.push(line(Kind::Same, x));
            (i, j) = (i + 1, j + 1);
        } else if at(&lcs, i + 1, j) >= at(&lcs, i, j + 1) {
            out.push(line(Kind::Removed, x));
            i += 1;
        } else {
            out.push(line(Kind::Added, y));
            j += 1;
        }
    }
    out.extend(a.iter().skip(i).map(|t| line(Kind::Removed, t)));
    out.extend(b.iter().skip(j).map(|t| line(Kind::Added, t)));
    out
}

/// What the window shows: a changed line with its neighbours, or a count of unchanged lines left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row<'a> {
    Line(&'a Line),
    Skipped(usize),
}

/// Keep only the changed lines and `context` unchanged ones around each; runs of the rest become `Skipped`.
pub fn rows(lines: &[Line], context: usize) -> Vec<Row<'_>> {
    let changed: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.kind != Kind::Same).map(|(i, _)| i).collect();
    let near = |i: usize| changed.iter().any(|&c| c.abs_diff(i) <= context);
    let mut out = Vec::new();
    let mut skipped = 0;
    for (i, l) in lines.iter().enumerate() {
        if near(i) {
            if skipped > 0 {
                out.push(Row::Skipped(skipped));
                skipped = 0;
            }
            out.push(Row::Line(l));
        } else {
            skipped += 1;
        }
    }
    if skipped > 0 && !changed.is_empty() {
        out.push(Row::Skipped(skipped));
    }
    out
}

/// How many lines were added and removed.
pub fn counts(lines: &[Line]) -> (usize, usize) {
    let n = |k| lines.iter().filter(|l| l.kind == k).count();
    (n(Kind::Added), n(Kind::Removed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|l| {
                format!(
                    "{}{}",
                    match l.kind {
                        Kind::Same => ' ',
                        Kind::Added => '+',
                        Kind::Removed => '-',
                    },
                    l.text
                )
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    #[test]
    fn identical_text_has_no_changes() {
        let d = diff("a\nb\n", "a\nb\n");
        assert_eq!(counts(&d), (0, 0));
        assert!(rows(&d, 2).is_empty());
    }

    #[test]
    fn a_changed_line_is_removed_then_added() {
        assert_eq!(show(&diff("a\nb\nc", "a\nB\nc")), " a|-b|+B| c");
    }

    #[test]
    fn insertions_and_deletions_keep_the_rest_aligned() {
        assert_eq!(show(&diff("a\nb\nc\nd", "a\nc\nd\ne")), " a|-b| c| d|+e");
        assert_eq!(show(&diff("", "x\ny")), "+x|+y");
        assert_eq!(show(&diff("x\ny", "")), "-x|-y");
    }

    #[test]
    fn unchanged_stretches_collapse_to_a_count() {
        let old: String = (0..20).map(|i| format!("l{i}\n")).collect();
        let new = old.replace("l10\n", "ten\n");
        let d = diff(&old, &new);
        assert_eq!(counts(&d), (1, 1));
        let r = rows(&d, 2);
        assert_eq!(r.first(), Some(&Row::Skipped(8)));
        assert_eq!(r.last(), Some(&Row::Skipped(7)));
        assert_eq!(r.len(), 1 + 2 + 2 + 2 + 1, "a count, two lines of context, the pair, two of context, a count");
    }

    #[test]
    fn a_huge_middle_still_gives_a_correct_if_long_diff() {
        let old: String = (0..2500).map(|i| format!("a{i}\n")).collect();
        let new: String = (0..2500).map(|i| format!("b{i}\n")).collect();
        assert_eq!(counts(&diff(&old, &new)), (2500, 2500));
    }
}
