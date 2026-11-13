//! Charts of data rather than of formulas: `scatter`, `bar` and `hist`. They build the same `PlotSpec` as `plot`,
//! with a series kind that says how to draw it.

use crate::builtins::Args;
use crate::value::{EvalError, PlotSeries, PlotSpec, R, SeriesKind, Value, err};
use mathcraft_core::numeric::eval_f64;
use std::sync::Arc;

/// Most points one chart takes; a picture of more is slow to draw and unreadable anyway.
const MAX_POINTS: usize = 100_000;

/// The numbers in a list, tuple, range or array, as floats.
fn floats(v: &Value, what: &str) -> R<Vec<f64>> {
    let items: Vec<Value> = match v {
        Value::List(l) | Value::Tuple(l) => l.as_ref().clone(),
        Value::Range { start, stop, step } => {
            if *step == 0 {
                return err("a range step cannot be 0");
            }
            let mut out = Vec::new();
            let mut i = *start;
            while (*step > 0 && i < *stop) || (*step < 0 && i > *stop) {
                if out.len() >= MAX_POINTS {
                    break;
                }
                out.push(Value::int(i));
                i += step;
            }
            out
        }
        Value::Array(a) => a.exprs().into_iter().map(Value::Expr).collect(),
        other => return err(format!("{what} must be a list of numbers, not a {}", other.type_name())),
    };
    if items.len() > MAX_POINTS {
        return err(format!("{what} has {} values; a chart takes at most {MAX_POINTS}", items.len()));
    }
    items
        .iter()
        .map(|item| {
            let e = item.to_expr().map_err(|_| EvalError::new(format!("{what} must hold only numbers, found a {}", item.type_name())))?;
            let x = eval_f64(&e, &|_| None).map_err(|_| EvalError::new(format!("{what} must hold only numbers, found {e}")))?;
            if x.is_finite() { Ok(x) } else { err(format!("{what} holds a value that is not a finite number")) }
        })
        .collect()
}

fn spec(series: PlotSeries, x_label: &str, x_ticks: Vec<(f64, String)>) -> Value {
    Value::Plot(Arc::new(PlotSpec { series: vec![series], x_label: x_label.to_owned(), x_ticks }))
}

/// `scatter(xs, ys)`, or `scatter(ys)` against 0, 1, 2, …
pub(crate) fn scatter(a: &Args) -> R<Value> {
    if a.pos.is_empty() || a.pos.len() > 2 || !a.kw.is_empty() {
        return err("scatter(xs, ys) draws a dot for each pair; scatter(ys) numbers the points from 0");
    }
    let (xs, ys) = if a.pos.len() == 2 {
        (floats(a.get(0)?, "xs")?, floats(a.get(1)?, "ys")?)
    } else {
        let ys = floats(a.get(0)?, "ys")?;
        ((0..ys.len()).map(|i| i as f64).collect(), ys)
    };
    if xs.len() != ys.len() {
        return err(format!("scatter needs as many x values as y values ({} and {})", xs.len(), ys.len()));
    }
    let points: Vec<[f64; 2]> = xs.into_iter().zip(ys).map(|(x, y)| [x, y]).collect();
    let series = PlotSeries { label: String::new(), segments: vec![points], kind: SeriesKind::Points };
    Ok(spec(series, "", Vec::new()))
}

/// `bar(heights)` or `bar(labels, heights)`.
pub(crate) fn bar(a: &Args) -> R<Value> {
    if a.pos.is_empty() || a.pos.len() > 2 || !a.kw.is_empty() {
        return err("bar(heights) or bar(labels, heights) draws one bar for each value");
    }
    let (labels, heights) = if a.pos.len() == 2 {
        let names = match a.get(0)? {
            Value::List(l) | Value::Tuple(l) => l.iter().map(|v| if let Value::Str(s) = v { s.to_string() } else { v.to_string() }).collect(),
            other => return err(format!("labels must be a list, not a {}", other.type_name())),
        };
        (names, floats(a.get(1)?, "heights")?)
    } else {
        let h = floats(a.get(0)?, "heights")?;
        ((1..=h.len()).map(|i| i.to_string()).collect::<Vec<_>>(), h)
    };
    if labels.len() != heights.len() {
        return err(format!("bar needs one label for each height ({} and {})", labels.len(), heights.len()));
    }
    let points: Vec<[f64; 2]> = heights.iter().enumerate().map(|(i, h)| [i as f64, *h]).collect();
    let ticks = labels.into_iter().enumerate().map(|(i, l)| (i as f64, l)).collect();
    let series = PlotSeries { label: String::new(), segments: vec![points], kind: SeriesKind::Bars { width: 0.7 } };
    Ok(spec(series, "", ticks))
}

/// `hist(data, bins=10)`: how many values fall in each of `bins` equal-width intervals between the smallest and largest.
pub(crate) fn hist(a: &Args) -> R<Value> {
    if let Some((k, _)) = a.kw.iter().find(|(k, _)| k != "bins") {
        return err(format!("unknown keyword '{k}'; the only keyword is bins="));
    }
    if a.pos.is_empty() || a.pos.len() > 2 {
        return err("hist(data, bins=10) counts the values in each of `bins` intervals");
    }
    let data = floats(a.get(0)?, "data")?;
    let bins = match a.opt(1, "bins") {
        None => 10,
        Some(v) => {
            let n = v.to_expr().ok().and_then(|e| e.as_i64()).ok_or_else(|| EvalError::new("bins must be a whole number"))?;
            usize::try_from(n).ok().filter(|n| (1..=1000).contains(n)).ok_or_else(|| EvalError::new("bins must be between 1 and 1000"))?
        }
    };
    let (lo, hi) = data.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| (l.min(*v), h.max(*v)));
    if data.is_empty() {
        return err("hist needs at least one value");
    }
    // all values equal: one interval of width 1 around them
    let (lo, hi) = if hi > lo { (lo, hi) } else { (lo - 0.5, lo + 0.5) };
    let width = (hi - lo) / bins as f64;
    let mut counts = vec![0u32; bins];
    for v in &data {
        let i = (((v - lo) / width) as usize).min(bins - 1);
        if let Some(c) = counts.get_mut(i) {
            *c += 1;
        }
    }
    let points: Vec<[f64; 2]> = counts.iter().enumerate().map(|(i, c)| [lo + (i as f64 + 0.5) * width, f64::from(*c)]).collect();
    let series = PlotSeries { label: String::new(), segments: vec![points], kind: SeriesKind::Bars { width } };
    Ok(spec(series, "", Vec::new()))
}
