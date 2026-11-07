//! A Model Context Protocol server (layer L2): JSON-RPC 2.0, one message per line, over any reader and writer
//! (stdin/stdout in practice). It lists and calls the tools of a [`mathcraft_engine::Engine`].

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use mathcraft_engine::{Engine, tools};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

/// Protocol version used when the client does not name one.
const DEFAULT_PROTOCOL: &str = "2025-03-26";
/// Longest request line accepted.
const MAX_LINE: usize = 4 * 1024 * 1024;

const INSTRUCTIONS: &str = "MathCraft evaluates symbolic mathematics written in Python syntax: exact arithmetic (1/3 stays exact), undefined names are symbols, and functions such as diff, integrate, solve, limit, series, factor, matrix and plot exist. Use `evaluate` for calculations (variables persist in the session) and `functions` to look up what is available.";

fn response(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// Handle one parsed message. Notifications (no `id`) get no reply.
pub fn handle(engine: &mut Engine, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?;
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL);
            response(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "mathcraft", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => response(id, json!({})),
        "tools/list" => {
            let list: Vec<Value> = tools().into_iter().map(|t| json!({"name": t.name, "description": t.description, "inputSchema": t.input_schema})).collect();
            response(id, json!({"tools": list}))
        }
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else { return Some(error(id, -32602, "tools/call needs a tool name")) };
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let r = engine.call(name, &args);
            response(id, json!({"content": [{"type": "text", "text": r.text}], "isError": r.is_error}))
        }
        other => error(id, -32601, &format!("method not found: {other}")),
    })
}

/// Read messages until the input ends, writing replies as they are produced.
pub fn serve(engine: &mut Engine, input: &mut impl BufRead, output: &mut impl Write) -> std::io::Result<()> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        let n = std::io::Read::take(&mut *input, MAX_LINE as u64 + 1).read_until(b'\n', &mut buf)?;
        if n == 0 {
            return Ok(());
        }
        if buf.len() > MAX_LINE {
            // skip the rest of an oversized line and say so
            while buf.last() != Some(&b'\n') {
                buf.clear();
                if std::io::Read::take(&mut *input, 65_536).read_until(b'\n', &mut buf)? == 0 {
                    break;
                }
            }
            writeln!(output, "{}", error(&Value::Null, -32600, "request too large"))?;
            output.flush()?;
            continue;
        }
        let line = String::from_utf8_lossy(&buf);
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(engine, &msg),
            Err(e) => Some(error(&Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(r) = reply {
            writeln!(output, "{r}")?;
            output.flush()?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &[&str]) -> Vec<Value> {
        let mut engine = Engine::new();
        let input = lines.join("\n") + "\n";
        let mut out = Vec::new();
        serve(&mut engine, &mut input.as_bytes(), &mut out).unwrap();
        String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    #[test]
    fn handshake_list_and_call() {
        let replies = run(&[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"evaluate","arguments":{"code":"diff(sin(x)*x, x)"}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"evaluate","arguments":{"code":"1/0"}}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#,
        ]);
        assert_eq!(replies.len(), 5, "the notification gets no reply");
        assert_eq!(replies[0]["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(replies[0]["result"]["serverInfo"]["name"], "mathcraft");
        let names: Vec<&str> = replies[1]["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"evaluate") && names.contains(&"run_notebook"));
        assert_eq!(replies[2]["result"]["content"][0]["text"], "x*cos(x) + sin(x)");
        assert_eq!(replies[2]["result"]["isError"], false);
        assert_eq!(replies[3]["result"]["isError"], true);
        assert_eq!(replies[4]["result"], json!({}));
    }

    #[test]
    fn bad_input_gets_protocol_errors() {
        let replies = run(&["not json", r#"{"jsonrpc":"2.0","id":7,"method":"nope"}"#, r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{}}"#, ""]);
        assert_eq!(replies[0]["error"]["code"], -32700);
        assert_eq!(replies[1]["error"]["code"], -32601);
        assert_eq!(replies[2]["error"]["code"], -32602);
        assert_eq!(replies.len(), 3, "blank lines are skipped");
    }

    #[test]
    fn variables_persist_across_calls_in_a_session() {
        let replies = run(&[
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"evaluate","arguments":{"code":"k = 6"}}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"evaluate","arguments":{"code":"k * 7"}}}"#,
        ]);
        assert_eq!(replies[1]["result"]["content"][0]["text"], "42");
    }
}
