//! `format(value, spec)`: the part of Python's format-spec mini-language that f-strings like `f"{x:.3f}"` need:
//! `[[fill]align][sign][0][width][,][.precision][type]` with type `f e d s %` (or none).

use crate::interp::numeric;
use crate::value::{EvalError, R, Value, err};
use mathcraft_core::numeric::eval_f64;

#[derive(Default)]
struct Spec {
    fill: Option<char>,
    align: Option<char>,
    sign: Option<char>,
    zero: bool,
    width: usize,
    comma: bool,
    precision: Option<usize>,
    kind: Option<char>,
}

fn parse(spec: &str) -> R<Spec> {
    let bad = || EvalError::new(format!("unsupported format spec '{spec}' (supported: [[fill]align][sign][0][width][,][.precision][f|e|d|s|%])"));
    let cs: Vec<char> = spec.chars().collect();
    let mut s = Spec::default();
    let mut i = 0;
    let is_align = |c: char| matches!(c, '<' | '>' | '^');
    if cs.get(1).is_some_and(|c| is_align(*c)) {
        s.fill = cs.first().copied();
        s.align = cs.get(1).copied();
        i = 2;
    } else if cs.first().is_some_and(|c| is_align(*c)) {
        s.align = cs.first().copied();
        i = 1;
    }
    if let Some(c) = cs.get(i).filter(|c| matches!(c, '+' | '-' | ' ')) {
        s.sign = Some(*c);
        i += 1;
    }
    if cs.get(i) == Some(&'0') {
        s.zero = true;
        i += 1;
    }
    let digits = |i: &mut usize| -> Option<usize> {
        let start = *i;
        while cs.get(*i).is_some_and(char::is_ascii_digit) {
            *i += 1;
        }
        cs.get(start..*i).filter(|d| !d.is_empty()).and_then(|d| d.iter().collect::<String>().parse().ok())
    };
    s.width = digits(&mut i).unwrap_or(0);
    if cs.get(i) == Some(&',') {
        s.comma = true;
        i += 1;
    }
    if cs.get(i) == Some(&'.') {
        i += 1;
        s.precision = Some(digits(&mut i).ok_or_else(bad)?);
    }
    if let Some(c) = cs.get(i) {
        if !matches!(c, 'f' | 'e' | 'd' | 's' | '%') {
            return Err(bad());
        }
        s.kind = Some(*c);
        i += 1;
    }
    if i < cs.len() || s.width > 10_000 || s.precision.is_some_and(|p| p > 1000) {
        return Err(bad());
    }
    Ok(s)
}

fn group(int_part: &str) -> String {
    let n = int_part.chars().count();
    let mut out = String::new();
    for (k, c) in int_part.chars().enumerate() {
        if k > 0 && (n - k).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Python's `%e`-style exponent: `1.5e3` becomes `1.500000e+03`.
fn sci(x: f64, prec: usize) -> String {
    let s = format!("{x:.prec$e}");
    match s.split_once('e') {
        Some((m, e)) => {
            let (sign, d) = e.strip_prefix('-').map_or(('+', e), |d| ('-', d));
            format!("{m}e{sign}{d:0>2}")
        }
        None => s,
    }
}

pub fn format_value(v: &Value, spec: &str) -> R<String> {
    if spec.is_empty() {
        return Ok(v.to_text());
    }
    let s = parse(spec)?;
    let number = numeric(v);
    let body = match (s.kind, &number) {
        (Some('s'), _) | (None, None) => {
            let t = v.to_text();
            match s.precision {
                Some(p) => t.chars().take(p).collect(),
                None => t,
            }
        }
        (None, Some(_)) => v.to_text(),
        (Some(_), None) => return err(format!("cannot format a {} with '{spec}'", v.type_name())),
        (Some(k), Some(e)) => {
            if k == 'd' {
                if !e.as_num().is_some_and(|n| n.is_integer() && n.is_exact()) {
                    return err("the 'd' format needs an integer");
                }
                e.to_string()
            } else {
                let x = eval_f64(e, &|_| None)?;
                let p = s.precision.unwrap_or(6);
                match k {
                    'e' => sci(x, p),
                    '%' => format!("{:.p$}%", x * 100.0),
                    _ => format!("{x:.p$}"),
                }
            }
        }
    };
    let (neg, digits) = body.strip_prefix('-').map_or((false, body.as_str()), |d| (true, d));
    let numeric_body = number.is_some() && s.kind != Some('s');
    let digits = if s.comma && numeric_body {
        let (int, rest) = digits.split_at(digits.find(['.', 'e', '%']).unwrap_or(digits.len()));
        format!("{}{rest}", group(int))
    } else {
        digits.to_string()
    };
    let sign = match (neg, s.sign) {
        (true, _) => "-",
        (false, Some('+')) => "+",
        (false, Some(' ')) => " ",
        _ => "",
    };
    let len = sign.chars().count() + digits.chars().count();
    let pad = s.width.saturating_sub(len);
    let align = s.align.unwrap_or(if numeric_body { '>' } else { '<' });
    Ok(if s.zero && s.align.is_none() && numeric_body {
        format!("{sign}{}{digits}", "0".repeat(pad))
    } else {
        let fill = s.fill.unwrap_or(' ');
        let (left, right) = match align {
            '<' => (0, pad),
            '^' => (pad / 2, pad - pad / 2),
            _ => (pad, 0),
        };
        format!("{}{sign}{digits}{}", fill.to_string().repeat(left), fill.to_string().repeat(right))
    })
}
