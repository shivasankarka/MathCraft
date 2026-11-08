//! Slider cells. A cell whose code is one line, `name = slider(lo, hi, value[, step])`, shows a slider under the
//! editor. Dragging it rewrites the value in the source text (so the file stays plain Python and a saved notebook
//! keeps its settings) and the notebook re-runs the cells that use `name`.

/// A slider cell read from its source.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub name: String,
    pub lo: f64,
    pub hi: f64,
    pub value: f64,
    pub step: Option<f64>,
    /// Every number was written as an integer, so the slider moves in whole numbers.
    pub whole: bool,
    lo_text: String,
    hi_text: String,
    step_text: Option<String>,
    comment: String,
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_')
}

fn number(s: &str) -> Option<f64> {
    let s = s.trim();
    let digits = s.strip_prefix('-').or_else(|| s.strip_prefix('+')).unwrap_or(s);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '-' | '+')) {
        return None;
    }
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn is_whole(s: &str) -> bool {
    let d = s.trim().trim_start_matches(['-', '+']);
    !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())
}

/// Read `name = slider(lo, hi, value[, step])`; `None` for anything else.
pub fn parse(source: &str) -> Option<Spec> {
    let (code, comment) = match source.find('#') {
        Some(i) => (source.get(..i)?, source.get(i..)?.trim_end()),
        None => (source, ""),
    };
    let code = code.trim();
    if code.contains('\n') {
        return None;
    }
    let (name, call) = code.split_once('=')?;
    let name = name.trim();
    let args = call.trim().strip_prefix("slider(")?.strip_suffix(')')?;
    if !is_ident(name) || args.contains(['(', ')']) {
        return None;
    }
    let mut parts: Vec<(Option<&str>, &str)> = Vec::new();
    for p in args.split(',') {
        match p.split_once('=') {
            Some((k, v)) => parts.push((Some(k.trim()), v.trim())),
            None => parts.push((None, p.trim())),
        }
    }
    let pos: Vec<&str> = parts.iter().filter(|(k, _)| k.is_none()).map(|(_, v)| *v).collect();
    let kw = |name: &str| parts.iter().find(|(k, _)| *k == Some(name)).map(|(_, v)| *v);
    let lo_text = *pos.first()?;
    let hi_text = *pos.get(1)?;
    let value_text = pos.get(2).copied().or_else(|| kw("value")).unwrap_or(lo_text);
    let step_text = pos.get(3).copied().or_else(|| kw("step"));
    let (lo, hi, value) = (number(lo_text)?, number(hi_text)?, number(value_text)?);
    let step = match step_text {
        Some(t) => Some(number(t).filter(|s| *s > 0.0)?),
        None => None,
    };
    if lo >= hi || value < lo || value > hi {
        return None;
    }
    let whole = [lo_text, hi_text, value_text].iter().chain(step_text.iter()).all(|t| is_whole(t));
    Some(Spec {
        name: name.to_owned(),
        lo,
        hi,
        value,
        step: if whole { Some(step.unwrap_or(1.0)) } else { step },
        whole,
        lo_text: lo_text.to_owned(),
        hi_text: hi_text.to_owned(),
        step_text: step_text.map(str::to_owned),
        comment: comment.to_owned(),
    })
}

/// A number as short source text: whole numbers without a point, others with up to six decimals.
fn show(v: f64, whole: bool) -> String {
    if whole {
        return format!("{}", v.round() as i64);
    }
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') { format!("{s}0") } else { s.to_owned() }
}

impl Spec {
    /// The value snapped to the step (counted from `lo`) and kept inside the range.
    pub fn snap(&self, v: f64) -> f64 {
        let v = match self.step {
            Some(s) => self.lo + ((v - self.lo) / s).round() * s,
            None => v,
        };
        v.clamp(self.lo, self.hi)
    }

    /// The cell's source with a new value.
    pub fn with_value(&self, v: f64) -> String {
        let mut args = format!("{}, {}, {}", self.lo_text, self.hi_text, show(self.snap(v), self.whole));
        if let Some(s) = &self.step_text {
            args.push_str(", ");
            args.push_str(s);
        }
        let tail = if self.comment.is_empty() { String::new() } else { format!("  {}", self.comment) };
        format!("{} = slider({args}){tail}", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_common_forms() {
        let s = parse("a = slider(0, 10, 2)").unwrap();
        assert_eq!((s.name.as_str(), s.lo, s.hi, s.value, s.whole, s.step), ("a", 0.0, 10.0, 2.0, true, Some(1.0)));
        let s = parse("w = slider(-1.5, 1.5, 0.25, step=0.05)  # width").unwrap();
        assert!(!s.whole && s.step == Some(0.05) && s.value == 0.25);
        assert_eq!(parse("t = slider(0, 5)").unwrap().value, 0.0);
        assert_eq!(parse("t = slider(0, 5, value=3)").unwrap().value, 3.0);
    }

    #[test]
    fn rejects_everything_else() {
        for bad in [
            "a = slider(0, 10, 12)",
            "a = slider(3, 3, 3)",
            "slider(0, 1, 0)",
            "a = slider(x, 1, 0)",
            "a = 1\nb = slider(0, 1)",
            "1a = slider(0, 1, 0)",
            "a = slider(0, 1, 0, step=0)",
            "a = slider(0, f(2), 1)",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn rewrites_the_value_and_keeps_the_rest() {
        let s = parse("a = slider(0, 10, 2)").unwrap();
        assert_eq!(s.with_value(7.4), "a = slider(0, 10, 7)");
        assert_eq!(s.with_value(99.0), "a = slider(0, 10, 10)");
        let s = parse("w = slider(-1.5, 1.5, 0, step=0.05)  # width").unwrap();
        assert_eq!(s.with_value(0.3333), "w = slider(-1.5, 1.5, 0.35, 0.05)  # width");
        assert_eq!(parse(&s.with_value(0.5)).unwrap().value, 0.5);
        let s = parse("k = slider(0, 1, 0.5)").unwrap();
        assert_eq!(s.with_value(0.123456789), "k = slider(0, 1, 0.123457)");
    }
}
