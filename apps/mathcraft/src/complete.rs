//! Autocompletion: which names to offer for the word being typed. Pure functions, no UI.

use mathcraft_eval::DOCS;

/// Most entries shown at once.
pub const MAX_SHOWN: usize = 8;
/// Shortest word that opens the list.
const MIN_WORD: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    /// Signature for built-in functions, "defined in this notebook" for names the user made.
    pub detail: String,
    pub is_function: bool,
}

/// The identifier ending at `cursor` (a char index): its start (char index) and text.
pub fn word_before(text: &str, cursor: usize) -> Option<(usize, String)> {
    let before: Vec<char> = text.chars().take(cursor).collect();
    let len = before.iter().rev().take_while(|c| c.is_alphanumeric() || **c == '_').count();
    if len == 0 {
        return None;
    }
    let start = before.len() - len;
    let word: String = before.iter().skip(start).collect();
    if word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((start, word))
}

/// Whether the cursor sits in a comment or string on its line, where completion would only get in the way.
pub fn in_comment_or_string(text: &str, cursor: usize) -> bool {
    let before: String = text.chars().take(cursor).collect();
    let line = before.rsplit('\n').next().unwrap_or("");
    let (mut quote, mut escaped) = (None, false);
    for c in line.chars() {
        match (quote, c) {
            (Some(_), _) if escaped => escaped = false,
            (Some(_), '\\') => escaped = true,
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '#') => return true,
            _ => {}
        }
    }
    quote.is_some()
}

/// Names that start with `word`: the notebook's own names first, then built-ins; each group alphabetical.
/// Nothing is offered when the only match is the word itself.
pub fn candidates(word: &str, defined: &[String]) -> Vec<Candidate> {
    if word.chars().count() < MIN_WORD {
        return Vec::new();
    }
    let mut own: Vec<&String> = defined.iter().filter(|n| n.starts_with(word)).collect();
    own.sort();
    own.dedup();
    let mut out: Vec<Candidate> =
        own.into_iter().map(|n| Candidate { name: n.clone(), detail: "defined in this notebook".to_owned(), is_function: false }).collect();
    let mut builtins: Vec<_> = DOCS.iter().filter(|(n, _, _)| n.starts_with(word)).collect();
    builtins.sort_by_key(|(n, _, _)| *n);
    for (name, sig, _) in builtins {
        if !out.iter().any(|c| c.name == *name) {
            out.push(Candidate { name: (*name).to_owned(), detail: (*sig).to_owned(), is_function: sig.contains('(') });
        }
    }
    if out.len() == 1 && out.first().is_some_and(|c| c.name == word) {
        return Vec::new();
    }
    out.truncate(MAX_SHOWN);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_word_before_the_cursor() {
        assert_eq!(word_before("y = dif", 7), Some((4, "dif".to_owned())));
        assert_eq!(word_before("a\nexp", 5), Some((2, "exp".to_owned())));
        assert_eq!(word_before("f(x) + ", 7), None);
        assert_eq!(word_before("12ab", 4), None);
        assert_eq!(word_before("diff(x", 3), Some((0, "dif".to_owned())));
        assert_eq!(word_before("δx_1", 4), Some((0, "δx_1".to_owned())));
    }

    #[test]
    fn stays_quiet_in_comments_and_strings() {
        assert!(in_comment_or_string("x = 1  # dif", 12));
        assert!(in_comment_or_string("print(\"dif", 10));
        assert!(!in_comment_or_string("print(\"a\") + dif", 16));
        assert!(!in_comment_or_string("# first\ndif", 11));
        assert!(!in_comment_or_string("s = \"#\" + dif", 13));
    }

    #[test]
    fn offers_own_names_before_builtins() {
        let defined = vec!["diffusion".to_owned(), "other".to_owned()];
        let c = candidates("diff", &defined);
        assert_eq!(c.first().map(|c| c.name.as_str()), Some("diffusion"));
        assert!(c.iter().any(|c| c.name == "diff" && c.is_function && c.detail.starts_with("diff(")));
        assert!(candidates("d", &defined).is_empty(), "one letter is too little");
        assert!(candidates("zzzz", &defined).is_empty());
        assert!(c.len() <= MAX_SHOWN);
    }

    #[test]
    fn a_lone_exact_match_closes_the_list() {
        assert!(candidates("other", &["other".to_owned()]).is_empty());
    }
}
