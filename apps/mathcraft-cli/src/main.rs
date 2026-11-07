//! `mathcraft-cli`: evaluate expressions, run notebooks/scripts, check them, or start a tiny REPL.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use mathcraft_core::budget::{self, Budget};
use mathcraft_eval::{Value, run_source};
use mathcraft_notebook::{CellKind, CellState, Notebook, RunRequest, Runner};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const USAGE: &str = "\
mathcraft-cli: symbolic computation from the command line

USAGE:
    mathcraft-cli eval '<code>'      evaluate code and print the result
    mathcraft-cli run <file>         run a notebook or script (cells are marked with `# %%`)
    mathcraft-cli check <file>       look for notebook problems (duplicate names, cycles, syntax errors)
    mathcraft-cli repl               interactive prompt
    mathcraft-cli mcp                serve the engine to AI assistants over MCP (stdio)
    mathcraft-cli help               this text

Inside code, run help() for the list of functions. Examples:
    mathcraft-cli eval 'integrate(sin(x)**2, (x, 0, pi))'
    mathcraft-cli eval 'solve(x**2 - 2, x)'
";

fn main() -> ExitCode {
    // Deep (but bounded) recursion in the algebra needs more than the default main-thread stack.
    let worker = std::thread::Builder::new().stack_size(256 * 1024 * 1024).spawn(real_main);
    match worker.map(|h| h.join()) {
        Ok(Ok(code)) => code,
        _ => {
            eprintln!("internal error: the program crashed");
            ExitCode::from(2)
        }
    }
}

fn real_main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["eval", code] => eval(code),
        ["run", path] => run_file(path),
        ["check", path] => check_file(path),
        ["repl"] => repl(),
        ["mcp"] => mcp(),
        [] | ["help" | "--help" | "-h"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn budget_guard() -> budget::Guard {
    budget::install(Budget::new(Some(Duration::from_secs(60)), Arc::new(AtomicBool::new(false)), 400_000_000))
}

fn eval(code: &str) -> ExitCode {
    let _g = budget_guard();
    let mut globals = HashMap::new();
    let out = run_source(&mut globals, code, false);
    print!("{}", out.stdout);
    match out.result {
        Ok(Some(v)) => {
            println!("{v}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn read(path: &str) -> Result<String, ExitCode> {
    std::fs::read_to_string(path).map_err(|e| {
        eprintln!("cannot read {path}: {e}");
        ExitCode::from(2)
    })
}

fn run_file(path: &str) -> ExitCode {
    let text = match read(path) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let mut nb = Notebook::from_text(&text);
    let mut runner = Runner::new(Arc::new(AtomicBool::new(false)));
    nb.run_blocking(&mut runner, RunRequest::All);
    let mut failed = false;
    let multi = nb.cells().iter().filter(|c| c.kind == CellKind::Code).count() > 1;
    for (i, c) in nb.cells().iter().enumerate() {
        if c.kind != CellKind::Code {
            continue;
        }
        if multi {
            println!("── cell {} ──", i + 1);
        }
        if let Some(o) = &c.output {
            print!("{}", o.stdout);
            match &o.result {
                Ok(Some(v)) => println!("{v}"),
                Ok(None) => {}
                Err(e) => {
                    eprintln!("cell {}: error: {e}", i + 1);
                    failed = true;
                }
            }
        } else if c.state == CellState::Blocked {
            eprintln!("cell {}: not run: {}", i + 1, c.problems.first().map(|p| p.message()).or_else(|| c.note.clone()).unwrap_or_default());
            failed = true;
        }
    }
    if failed { ExitCode::from(1) } else { ExitCode::SUCCESS }
}

fn check_file(path: &str) -> ExitCode {
    let text = match read(path) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let nb = Notebook::from_text(&text);
    let diags = nb.check();
    for d in &diags {
        println!("{path}: cell {}: {} {}", d.cell + 1, d.code, d.message);
    }
    if diags.is_empty() {
        println!("{path}: no problems found in {} cells", nb.cells().len());
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Model Context Protocol server on stdin/stdout. Diagnostics go to stderr; stdout carries only protocol messages.
fn mcp() -> ExitCode {
    let mut engine = mathcraft_engine::Engine::new();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    match mathcraft_mcp::serve(&mut engine, &mut stdin.lock(), &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mcp: {e}");
            ExitCode::from(1)
        }
    }
}

fn repl() -> ExitCode {
    println!("MathCraft REPL. One statement per line; help() lists functions; Ctrl-D exits.");
    let mut globals: HashMap<String, Value> = HashMap::new();
    let stdin = std::io::stdin();
    loop {
        print!(">>> ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let _g = budget_guard();
        let out = run_source(&mut globals, &line, false);
        print!("{}", out.stdout);
        match out.result {
            Ok(Some(v)) => println!("{v}"),
            Ok(None) => {}
            Err(e) => eprintln!("error: {e}"),
        }
    }
    ExitCode::SUCCESS
}
