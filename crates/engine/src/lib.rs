//! The MathCraft engine as a set of tools (layer L2). Each tool has a name, a description and a JSON input schema, and
//! is called with JSON arguments; the CLI, the MCP server and (later) the app all go through this one registry.
//! An [`Engine`] is a session: variables defined by `evaluate` stay for the next call.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use mathcraft_core::budget::{self, Budget};
use mathcraft_eval::{DOCS, Value, run_source};
use mathcraft_notebook::{CellKind, CellState, Notebook, RunRequest, Runner};
use serde_json::{Value as Json, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Longest file a tool will open.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// Longest text a tool returns (the rest is cut with a note), so one result cannot flood a client.
const MAX_OUTPUT_CHARS: usize = 60_000;

/// What a tool needs to be listed.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Json,
}

/// The outcome of a call: text for the caller, and whether it is an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

impl ToolResult {
    fn ok(text: impl Into<String>) -> Self {
        ToolResult { text: truncate(text.into()), is_error: false }
    }
    fn err(text: impl Into<String>) -> Self {
        ToolResult { text: truncate(text.into()), is_error: true }
    }
}

fn truncate(mut s: String) -> String {
    if s.chars().count() > MAX_OUTPUT_CHARS {
        s = s.chars().take(MAX_OUTPUT_CHARS).collect();
        s.push_str("\n… (output cut)");
    }
    s
}

/// A computation session.
#[derive(Default)]
pub struct Engine {
    globals: HashMap<String, Value>,
}

/// The tools, in the order they are listed.
pub fn tools() -> Vec<ToolSpec> {
    let path = json!({"type": "string", "description": "Path of a .py notebook (cells are marked with `# %%`)"});
    vec![
        ToolSpec {
            name: "evaluate",
            description: "Evaluate MathCraft code (Python syntax; 1/3 is exact; undefined names are symbols). Variables stay for later calls in this session. Returns printed output and the value of the last expression. Call `functions` to see what exists, e.g. integrate, solve, diff, limit, series, matrix, plot.",
            input_schema: json!({"type": "object", "properties": {"code": {"type": "string", "description": "One or more statements"}}, "required": ["code"]}),
        },
        ToolSpec { name: "reset", description: "Forget every variable defined in this session.", input_schema: json!({"type": "object", "properties": {}}) },
        ToolSpec {
            name: "functions",
            description: "List the built-in functions with their signatures, optionally filtered by a word in the name or description.",
            input_schema: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        },
        ToolSpec {
            name: "run_notebook",
            description: "Run every cell of a notebook file and return each cell's output or error.",
            input_schema: json!({"type": "object", "properties": {"path": path.clone()}, "required": ["path"]}),
        },
        ToolSpec {
            name: "check_notebook",
            description: "Check a notebook file for problems without running it (syntax errors, a name defined in two cells, dependency cycles).",
            input_schema: json!({"type": "object", "properties": {"path": path}, "required": ["path"]}),
        },
    ]
}

fn string_arg<'a>(args: &'a Json, key: &str) -> Result<&'a str, ToolResult> {
    args.get(key).and_then(Json::as_str).ok_or_else(|| ToolResult::err(format!("missing or invalid argument `{key}` (expected a string)")))
}

fn budget_guard() -> budget::Guard {
    budget::install(Budget::new(Some(Duration::from_secs(60)), Arc::new(AtomicBool::new(false)), 400_000_000))
}

fn read_notebook(path: &str) -> Result<Notebook, ToolResult> {
    let p = Path::new(path);
    if p.extension().and_then(|e| e.to_str()) != Some("py") {
        return Err(ToolResult::err("only .py notebook files can be opened"));
    }
    match std::fs::metadata(p) {
        Ok(m) if m.len() > MAX_FILE_BYTES => return Err(ToolResult::err("the file is larger than 2 MB")),
        Err(e) => return Err(ToolResult::err(format!("cannot read {path}: {e}"))),
        Ok(_) => {}
    }
    std::fs::read_to_string(p).map(|t| Notebook::from_text(&t)).map_err(|e| ToolResult::err(format!("cannot read {path}: {e}")))
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Call a tool by name. Never panics; problems come back as an error result.
    pub fn call(&mut self, name: &str, args: &Json) -> ToolResult {
        match name {
            "evaluate" => match string_arg(args, "code") {
                Ok(code) => self.evaluate(code),
                Err(e) => e,
            },
            "reset" => {
                self.globals.clear();
                ToolResult::ok("session cleared")
            }
            "functions" => functions(args.get("query").and_then(Json::as_str).unwrap_or("")),
            "run_notebook" => match string_arg(args, "path").and_then(read_notebook) {
                Ok(nb) => run_notebook(nb),
                Err(e) => e,
            },
            "check_notebook" => match string_arg(args, "path").and_then(read_notebook) {
                Ok(nb) => check_notebook(&nb),
                Err(e) => e,
            },
            other => ToolResult::err(format!("unknown tool `{other}`")),
        }
    }

    fn evaluate(&mut self, code: &str) -> ToolResult {
        let _g = budget_guard();
        let out = run_source(&mut self.globals, code, false);
        let mut text = out.stdout;
        match out.result {
            Ok(Some(v)) => {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&v.to_string());
                ToolResult::ok(text)
            }
            Ok(None) => ToolResult::ok(if text.is_empty() { "(no value)".to_owned() } else { text }),
            Err(e) => {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&format!("error: {e}"));
                ToolResult::err(text)
            }
        }
    }
}

fn functions(query: &str) -> ToolResult {
    let q = query.trim().to_lowercase();
    let lines: Vec<String> = DOCS
        .iter()
        .filter(|(name, _, summary)| q.is_empty() || name.to_lowercase().contains(&q) || summary.to_lowercase().contains(&q))
        .map(|(_, sig, summary)| format!("{sig}: {summary}"))
        .collect();
    if lines.is_empty() { ToolResult::ok(format!("no function matches `{query}`")) } else { ToolResult::ok(lines.join("\n")) }
}

fn run_notebook(mut nb: Notebook) -> ToolResult {
    let _g = budget_guard();
    let mut runner = Runner::new(Arc::new(AtomicBool::new(false)));
    nb.run_blocking(&mut runner, RunRequest::All);
    let (mut out, mut failed) = (String::new(), false);
    for (i, c) in nb.cells().iter().enumerate().filter(|(_, c)| c.kind == CellKind::Code) {
        out.push_str(&format!("── cell {} ──\n", i + 1));
        if let Some(o) = &c.output {
            out.push_str(&o.stdout);
            match &o.result {
                Ok(Some(v)) => out.push_str(&format!("{v}\n")),
                Ok(None) => {}
                Err(e) => {
                    out.push_str(&format!("error: {e}\n"));
                    failed = true;
                }
            }
        } else if c.state == CellState::Blocked {
            out.push_str(&format!("not run: {}\n", c.problems.first().map(|p| p.message()).or_else(|| c.note.clone()).unwrap_or_default()));
            failed = true;
        }
    }
    if failed { ToolResult::err(out) } else { ToolResult::ok(out) }
}

fn check_notebook(nb: &Notebook) -> ToolResult {
    let diags = nb.check();
    if diags.is_empty() {
        return ToolResult::ok(format!("no problems found in {} cells", nb.cells().len()));
    }
    ToolResult::err(diags.iter().map(|d| format!("cell {}: {} {}", d.cell + 1, d.code, d.message)).collect::<Vec<_>>().join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(r: &ToolResult) -> &str {
        &r.text
    }

    #[test]
    fn evaluate_keeps_variables_between_calls() {
        let mut e = Engine::new();
        let r = e.call("evaluate", &json!({"code": "a = 2\nprint('hi')"}));
        assert_eq!((text(&r), r.is_error), ("hi\n", false));
        let r = e.call("evaluate", &json!({"code": "a * 21"}));
        assert_eq!(text(&r), "42");
        let r = e.call("evaluate", &json!({"code": "integrate(x*exp(x), x)"}));
        assert_eq!(text(&r), "x*exp(x) - exp(x)");
        e.call("reset", &json!({}));
        let r = e.call("evaluate", &json!({"code": "a"}));
        assert_eq!(text(&r), "a", "after reset `a` is a plain symbol again");
    }

    #[test]
    fn errors_are_results_not_panics() {
        let mut e = Engine::new();
        let r = e.call("evaluate", &json!({"code": "1/0"}));
        assert!(r.is_error && r.text.starts_with("error:"), "{r:?}");
        assert!(e.call("evaluate", &json!({})).is_error);
        assert!(e.call("evaluate", &json!({"code": 5})).is_error);
        assert!(e.call("nope", &json!({})).is_error);
        // hostile input comes back (as an error or an unevaluated value), it does not hang or crash
        let r = e.call("evaluate", &json!({"code": "2**(2**1024)"}));
        assert!(!r.text.is_empty());
    }

    #[test]
    fn lists_functions() {
        let mut e = Engine::new();
        let r = e.call("functions", &json!({"query": "integr"}));
        assert!(r.text.contains("integrate("), "{}", r.text);
        assert!(e.call("functions", &json!({"query": "zzzzzz"})).text.starts_with("no function"));
        assert!(tools().iter().all(|t| t.input_schema.get("type").is_some()));
    }

    #[test]
    fn notebook_tools_read_py_files_only() {
        let dir = std::env::temp_dir().join(format!("mathcraft-engine-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("n.py");
        std::fs::write(&good, "# %%\na = 2\n# %%\nb = a * 3\nb\n").unwrap();
        let mut e = Engine::new();
        let r = e.call("run_notebook", &json!({"path": good.to_string_lossy()}));
        assert!(!r.is_error && r.text.contains('6'), "{r:?}");
        let r = e.call("check_notebook", &json!({"path": good.to_string_lossy()}));
        assert!(!r.is_error && r.text.starts_with("no problems"), "{r:?}");
        let bad = dir.join("secret.txt");
        std::fs::write(&bad, "x").unwrap();
        assert!(e.call("run_notebook", &json!({"path": bad.to_string_lossy()})).is_error);
        assert!(e.call("run_notebook", &json!({"path": "/nonexistent/a.py"})).is_error);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
