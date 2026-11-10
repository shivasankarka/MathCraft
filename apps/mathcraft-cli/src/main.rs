//! `mathcraft-cli`: evaluate expressions, run notebooks/scripts, check them, or start a tiny REPL.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod export;

use mathcraft_core::budget::{self, Budget};
use mathcraft_eval::{Value, run_source};
use mathcraft_notebook::{CellKind, CellState, Notebook, RunRequest, Runner};
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const USAGE: &str = "\
mathcraft-cli: symbolic computation from the command line

USAGE:
    mathcraft-cli eval '<code>'      evaluate code and print the result (`-` reads stdin)
    mathcraft-cli run <file>         run a notebook or script (cells are marked with `# %%`; `-` reads stdin)
    mathcraft-cli check <file>       look for notebook problems (duplicate names, cycles, syntax errors)
    mathcraft-cli export <file>      run a notebook and write it as Markdown or HTML (to stdout, or -o <out>)
    mathcraft-cli repl               interactive prompt
    mathcraft-cli mcp                serve the engine to AI assistants over MCP (stdio)
    mathcraft-cli help               this text

OPTIONS:
    --timeout <seconds>              give up on a computation after this long (default 60)
    -o <path>                        export: write here; the extension picks the format (.html or .md)
    -f <md|html>                     export: choose the format explicitly
    -V, --version                    print the version

Inside code, run help() for the list of functions. Examples:
    mathcraft-cli eval 'integrate(sin(x)**2, (x, 0, pi))'
    mathcraft-cli eval 'solve(x**2 - 2, x)'
    mathcraft-cli export examples/tour.py -o tour.html
";

/// Options that may appear anywhere on the command line.
struct Opts {
    timeout: Duration,
    out: Option<String>,
    format: Option<String>,
    version: bool,
    rest: Vec<String>,
}

fn parse_args(args: Vec<String>) -> Result<Opts, String> {
    let mut o = Opts { timeout: Duration::from_secs(60), out: None, format: None, version: false, rest: Vec::new() };
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--timeout" => {
                let v = it.next().ok_or("--timeout needs a number of seconds")?;
                let secs: f64 = v.parse().map_err(|_| format!("--timeout: `{v}` is not a number"))?;
                if !secs.is_finite() || secs <= 0.0 {
                    return Err("--timeout must be positive".into());
                }
                o.timeout = Duration::from_secs_f64(secs);
            }
            "-o" => o.out = Some(it.next().ok_or("-o needs a path")?),
            "-f" => o.format = Some(it.next().ok_or("-f needs md or html")?),
            "-V" | "--version" => o.version = true,
            _ => o.rest.push(a),
        }
    }
    Ok(o)
}

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
    let opts = match parse_args(std::env::args().skip(1).collect()) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if opts.version {
        println!("mathcraft-cli {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let t = opts.timeout;
    match opts.rest.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["eval", code] => eval(code, t),
        ["run", path] => run_file(path, t),
        ["check", path] => check_file(path),
        ["export", path] => export_file(path, &opts),
        ["repl"] => repl(t),
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

fn budget_guard(timeout: Duration) -> budget::Guard {
    budget::install(Budget::new(Some(timeout), Arc::new(AtomicBool::new(false)), 400_000_000))
}

fn eval(code: &str, timeout: Duration) -> ExitCode {
    let _g = budget_guard(timeout);
    let code = match read_arg(code) {
        Ok(c) => c,
        Err(c) => return c,
    };
    let mut globals = HashMap::new();
    let out = run_source(&mut globals, &code, false);
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
    let text = if path == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).map(|_| s)
    } else {
        std::fs::read_to_string(path)
    };
    text.map_err(|e| {
        eprintln!("cannot read {path}: {e}");
        ExitCode::from(2)
    })
}

/// Code given on the command line, or all of stdin when it is `-`.
fn read_arg(code: &str) -> Result<String, ExitCode> {
    if code == "-" { read(code) } else { Ok(code.to_string()) }
}

/// Run every cell of a notebook, giving each the time limit.
fn run_notebook(text: &str, timeout: Duration) -> Notebook {
    let mut nb = Notebook::from_text(text);
    let mut runner = Runner::new(Arc::new(AtomicBool::new(false)));
    runner.timeout = Some(timeout);
    nb.run_blocking(&mut runner, RunRequest::All);
    nb
}

fn run_file(path: &str, timeout: Duration) -> ExitCode {
    let text = match read(path) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let nb = run_notebook(&text, timeout);
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

fn export_file(path: &str, opts: &Opts) -> ExitCode {
    let format = match (&opts.format, &opts.out) {
        (Some(f), _) => match export::Format::parse(f) {
            Some(f) => f,
            None => {
                eprintln!("error: unknown format `{f}` (use md or html)");
                return ExitCode::from(2);
            }
        },
        (None, Some(o)) => export::Format::from_path(o),
        (None, None) => export::Format::Markdown,
    };
    let text = match read(path) {
        Ok(t) => t,
        Err(c) => return c,
    };
    let nb = run_notebook(&text, opts.timeout);
    let rendered = export::render(&nb, format, path);
    match &opts.out {
        Some(o) => match std::fs::write(o, rendered) {
            Ok(()) => {
                eprintln!("wrote {o}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("cannot write {o}: {e}");
                ExitCode::from(2)
            }
        },
        None => {
            print!("{rendered}");
            ExitCode::SUCCESS
        }
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

fn repl(timeout: Duration) -> ExitCode {
    println!("MathCraft REPL. One statement per line; help() lists functions; Ctrl-D or `exit` quits.");
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
        if matches!(line.trim(), "exit" | "quit" | "exit()" | "quit()") {
            break;
        }
        let _g = budget_guard(timeout);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn options_may_come_before_or_after_the_command() {
        let o = parse_args(args(&["--timeout", "2.5", "export", "a.py", "-o", "a.html"])).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(o.timeout, Duration::from_secs_f64(2.5));
        assert_eq!(o.out.as_deref(), Some("a.html"));
        assert_eq!(o.rest, ["export", "a.py"]);
        let o = parse_args(args(&["eval", "1+1", "--timeout", "3"])).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(o.timeout, Duration::from_secs(3));
        assert_eq!(o.rest, ["eval", "1+1"]);
    }

    #[test]
    fn bad_options_are_rejected() {
        assert!(parse_args(args(&["--timeout"])).is_err());
        assert!(parse_args(args(&["--timeout", "soon"])).is_err());
        assert!(parse_args(args(&["--timeout", "0"])).is_err());
        assert!(parse_args(args(&["-o"])).is_err());
    }

    #[test]
    fn version_flag() {
        assert!(parse_args(args(&["--version"])).is_ok_and(|o| o.version));
        assert!(parse_args(args(&["-V"])).is_ok_and(|o| o.version));
    }
}
