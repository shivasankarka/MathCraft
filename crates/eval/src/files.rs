//! Reading and writing delimited text files (`read_csv`, `write_csv`).
//!
//! The engine has no file access by default. A host that wants it (the desktop app, the CLI) calls
//! [`set_file_root`] on the thread that runs code, naming one folder; nothing outside that folder can be touched, and
//! the MCP server and the browser build never turn it on. Paths are relative, may not contain `..`, are checked after
//! symbolic links are resolved, and only `.csv`, `.tsv` and `.txt` files are accepted.

use crate::builtins::Args;
use crate::value::{EvalError, R, Value, err};
use dashu::integer::IBig;
use mathcraft_core::expr::Expr;
use std::cell::RefCell;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// The largest file read, and the most cells a read or write handles.
const MAX_BYTES: u64 = 20 * 1024 * 1024;
const MAX_CELLS: usize = 2_000_000;

thread_local! {
    static ROOT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Allow file access to `root` (and below) for code run on this thread; `None` turns it off again.
pub fn set_file_root(root: Option<PathBuf>) {
    ROOT.with(|r| *r.borrow_mut() = root);
}

fn root() -> R<PathBuf> {
    let root = ROOT.with(|r| r.borrow().clone());
    let root =
        root.ok_or_else(|| EvalError::new("file access is off here (it works in the desktop app, in a saved notebook, and in the command line tool)"))?;
    root.canonicalize().map_err(|e| EvalError::new(format!("the notebook folder cannot be used: {e}")))
}

/// The file `name` inside the allowed folder. For a file to be written it need not exist yet, but its folder must.
fn resolve(name: &str, for_write: bool) -> R<PathBuf> {
    let rel = Path::new(name);
    if name.is_empty() || rel.is_absolute() || !rel.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)) {
        return err("use a path inside the notebook's folder, like \"data.csv\" (no absolute paths and no ..)");
    }
    if !rel.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "csv" | "tsv" | "txt")) {
        return err("only .csv, .tsv and .txt files can be read or written");
    }
    let root = root()?;
    let joined = root.join(rel);
    let outside = || EvalError::new("that path leads outside the notebook's folder");
    if for_write {
        let name = joined.file_name().ok_or_else(|| EvalError::new("missing file name"))?.to_owned();
        let parent = joined.parent().ok_or_else(outside)?.canonicalize().map_err(|_| EvalError::new("the folder for that file does not exist"))?;
        if !parent.starts_with(&root) {
            return Err(outside());
        }
        let target = parent.join(name);
        if target.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
            return err("will not write through a symbolic link");
        }
        Ok(target)
    } else {
        let real = joined.canonicalize().map_err(|_| EvalError::new(format!("no file named {name} in the notebook's folder")))?;
        if real.starts_with(&root) { Ok(real) } else { Err(outside()) }
    }
}

/// A field as a number when it is one (whole numbers stay exact), else as text.
fn field(s: &str) -> Value {
    let t = s.trim();
    // only digits and the signs of a number, so `inf` and `nan` stay text
    if !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E')) {
        if let Ok(i) = t.parse::<i64>() {
            return Value::Expr(Expr::int(i));
        }
        if let Ok(b) = t.parse::<IBig>() {
            return Value::Expr(Expr::big(b));
        }
        if let Ok(f) = t.parse::<f64>()
            && f.is_finite()
        {
            return Value::Expr(Expr::float(f));
        }
    }
    Value::Str(Arc::from(s))
}

/// Split delimited text into rows of fields; quotes may hold the delimiter, newlines and doubled quotes.
pub(crate) fn parse(text: &str, delim: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let (mut row, mut cur) = (Vec::new(), String::new());
    let (mut quoted, mut any) = (false, false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        any = true;
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == delim && !quoted => row.push(std::mem::take(&mut cur)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(std::mem::take(&mut cur));
                rows.push(std::mem::take(&mut row));
                any = false;
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        row.push(cur);
        rows.push(row);
    }
    rows
}

fn delimiter(name: &str) -> char {
    if name.to_ascii_lowercase().ends_with(".tsv") { '\t' } else { ',' }
}

fn flag(a: &Args, at: usize, name: &str) -> R<bool> {
    match a.opt(at, name) {
        None => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(other) => err(format!("{name} must be True or False, not a {}", other.type_name())),
    }
}

/// `read_csv("data.csv", header=False, columns=False)`: a list of rows, numbers read as numbers. `header=True` leaves
/// out the first row; `columns=True` gives a list of columns instead, so `x, y = read_csv("d.csv", header=True, columns=True)`.
pub(crate) fn read_csv(a: &Args) -> R<Value> {
    if let Some((k, _)) = a.kw.iter().find(|(k, _)| k != "header" && k != "columns") {
        return err(format!("unknown keyword '{k}'; the keywords are header= and columns="));
    }
    if a.pos.is_empty() || a.pos.len() > 3 {
        return err("read_csv(\"file.csv\", header=False, columns=False) reads a file from the notebook's folder");
    }
    let Value::Str(name) = a.get(0)? else { return err("the file name must be a string, like \"data.csv\"") };
    let (header, columns) = (flag(a, 1, "header")?, flag(a, 2, "columns")?);
    let path = resolve(name, false)?;
    let size = std::fs::metadata(&path).map_err(|e| EvalError::new(format!("cannot read {name}: {e}")))?.len();
    if size > MAX_BYTES {
        return err(format!("{name} is {} MB; the limit is {} MB", size / 1_000_000, MAX_BYTES / 1_000_000));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| EvalError::new(format!("cannot read {name}: {e}")))?;
    let mut rows = parse(text.strip_prefix('\u{feff}').unwrap_or(&text), delimiter(name));
    if header && !rows.is_empty() {
        rows.remove(0);
    }
    if rows.iter().map(Vec::len).sum::<usize>() > MAX_CELLS {
        return err(format!("{name} has more than {MAX_CELLS} cells"));
    }
    let values: Vec<Vec<Value>> = rows
        .iter()
        .map(|r| {
            mathcraft_core::budget::tick()?;
            Ok(r.iter().map(|f| field(f)).collect())
        })
        .collect::<R<_>>()?;
    if !columns {
        return Ok(Value::list(values.into_iter().map(Value::list).collect()));
    }
    let width = values.iter().map(Vec::len).max().unwrap_or(0);
    let cols = (0..width).map(|j| Value::list(values.iter().map(|r| r.get(j).cloned().unwrap_or_else(|| Value::Str(Arc::from("")))).collect())).collect();
    Ok(Value::list(cols))
}

fn cell_text(v: &Value, delim: char) -> R<String> {
    let s = match v {
        Value::Str(s) => s.to_string(),
        Value::Expr(_) | Value::Bool(_) => v.to_string(),
        other => return err(format!("write_csv cannot write a {}", other.type_name())),
    };
    Ok(if s.contains([delim, '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s })
}

/// `write_csv("out.csv", rows, header=None)`: rows is a list of lists, a matrix, or a flat list (one column).
pub(crate) fn write_csv(a: &Args) -> R<Value> {
    if let Some((k, _)) = a.kw.iter().find(|(k, _)| k != "header") {
        return err(format!("unknown keyword '{k}'; the only keyword is header="));
    }
    if a.pos.len() < 2 || a.pos.len() > 3 {
        return err("write_csv(\"file.csv\", rows, header=None) writes a list of rows to the notebook's folder");
    }
    let Value::Str(name) = a.get(0)? else { return err("the file name must be a string, like \"out.csv\"") };
    let delim = delimiter(name);
    let items = |v: &Value| -> Option<Vec<Value>> {
        match v {
            Value::List(l) | Value::Tuple(l) => Some(l.as_ref().clone()),
            Value::Array(arr) => match arr.to_list() {
                Value::List(l) => Some(l.as_ref().clone()),
                _ => None,
            },
            _ => None,
        }
    };
    let rows = items(a.get(1)?).ok_or_else(|| EvalError::new("the rows must be a list (of lists) or a matrix"))?;
    let mut out = String::new();
    let mut cells = 0usize;
    let mut push_row = |fields: Vec<&Value>, out: &mut String| -> R<()> {
        cells += fields.len();
        if cells > MAX_CELLS {
            return err(format!("more than {MAX_CELLS} cells; write in pieces"));
        }
        let line: Vec<String> = fields.into_iter().map(|f| cell_text(f, delim)).collect::<R<_>>()?;
        out.push_str(&line.join(&delim.to_string()));
        out.push('\n');
        Ok(())
    };
    if let Some(h) = a.opt(2, "header").filter(|h| !matches!(h, Value::None)) {
        let names = items(h).ok_or_else(|| EvalError::new("header must be a list of names"))?;
        push_row(names.iter().collect(), &mut out)?;
    }
    for row in &rows {
        mathcraft_core::budget::tick()?;
        match items(row) {
            Some(fields) => push_row(fields.iter().collect(), &mut out)?,
            None => push_row(vec![row], &mut out)?,
        }
    }
    let path = resolve(name, true)?;
    std::fs::write(&path, out).map_err(|e| EvalError::new(format!("cannot write {name}: {e}")))?;
    Ok(Value::None)
}
