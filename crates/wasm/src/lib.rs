//! The MathCraft engine as a WebAssembly module for the browser.
//!
//! A page keeps one [`Session`] (the variables of a notebook) and sends it code; the answer is a JSON object with the
//! printed text, the value as text and as LaTeX, or an error. The module is built for `wasm32-unknown-unknown` and
//! has a tiny C interface (see `ffi`) instead of wasm-bindgen, so building needs nothing but `cargo` and the target.
//! On other targets only the pure [`Session`] is compiled, which is what the tests exercise.

use mathcraft_core::budget::{self, Budget};
use mathcraft_eval::{Value, run_source};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
mod ffi;

/// The variables a notebook has defined so far.
#[derive(Default)]
pub struct Session {
    globals: HashMap<String, Value>,
}

impl Session {
    pub fn new() -> Session {
        Session::default()
    }

    /// Forget every variable.
    pub fn reset(&mut self) {
        self.globals.clear();
    }

    /// Run `code` with a time limit and return the JSON answer:
    /// `{"stdout": "...", "value": "..."|null, "latex": "..."|null, "error": "..."|null}`.
    pub fn run(&mut self, code: &str, timeout_ms: u32) -> String {
        let _guard = budget::install(Budget::new(Some(Duration::from_millis(u64::from(timeout_ms))), Arc::new(AtomicBool::new(false)), 400_000_000));
        let out = run_source(&mut self.globals, code, false);
        let (value, latex, error) = match &out.result {
            Ok(Some(v)) => (Some(v.to_string()), v.latex(), None),
            Ok(None) => (None, None, None),
            Err(e) => (None, None, Some(e.to_string())),
        };
        serde_json::json!({ "stdout": out.stdout, "value": value, "latex": latex, "error": error }).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &mut Session, code: &str) -> serde_json::Value {
        serde_json::from_str(&s.run(code, 5000)).unwrap_or_default()
    }

    #[test]
    fn values_come_back_as_text_and_latex() {
        let mut s = Session::new();
        let r = run(&mut s, "integrate(x**2, x)");
        assert_eq!(r["value"], "x**3/3");
        assert_eq!(r["latex"], "\\frac{x^{3}}{3}");
        assert!(r["error"].is_null());
    }

    #[test]
    fn variables_persist_until_reset() {
        let mut s = Session::new();
        run(&mut s, "a = 7");
        assert_eq!(run(&mut s, "a * 6")["value"], "42");
        s.reset();
        // an unknown name is just a symbol again
        assert_eq!(run(&mut s, "a * 6")["value"], "6*a");
    }

    #[test]
    fn printed_text_and_errors_are_reported() {
        let mut s = Session::new();
        assert_eq!(run(&mut s, "print('hi')")["stdout"], "hi\n");
        assert!(run(&mut s, "1/0")["error"].as_str().is_some_and(|e| e.contains("division by zero")));
    }

    #[test]
    fn a_runaway_loop_times_out() {
        let mut s = Session::new();
        let started = std::time::Instant::now();
        let r: serde_json::Value = serde_json::from_str(&s.run("for i in range(10**9):\n    pass", 200)).unwrap_or_default();
        assert!(r["error"].is_string());
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
