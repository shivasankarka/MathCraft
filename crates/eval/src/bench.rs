//! Engine benchmark: a battery of standard symbolic problems with known answers, grouped by area.
//! Run `cargo test -p mathcraft-eval --release benchmark -- --ignored --nocapture` for a pass table;
//! it never fails, it reports. Cases live in `bench/cases.txt` as `area | input | expected`, where `expected` is
//! compared to the result by symbolic difference, then by sampling numerically (lists match in any order).

use crate::{Value, run_source};
use mathcraft_core::budget::{self, Budget};
use mathcraft_core::{Expr, Kind};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const CASES: &str = include_str!("../bench/cases.txt");
const LIMIT: Duration = Duration::from_secs(20);

enum Verdict {
    Pass,
    Wrong(String),
    Error(String),
    Timeout,
}

impl Verdict {
    fn describe(&self) -> String {
        match self {
            Verdict::Pass => "pass".into(),
            Verdict::Wrong(got) => format!("got {got}"),
            Verdict::Error(e) => format!("error: {e}"),
            Verdict::Timeout => "timeout".into(),
        }
    }
}

fn eval(src: &str) -> Result<Value, String> {
    let mut g = HashMap::new();
    run_source(&mut g, src, false).result.map_err(|e| e.to_string())?.ok_or_else(|| "no value".to_string())
}

fn sample(e: &Expr, k: usize) -> Option<f64> {
    let env = |name: &str| Some(0.37 + 0.211 * (k as f64) + 0.093 * (name.bytes().map(f64::from).sum::<f64>() % 7.0));
    mathcraft_core::numeric::eval_f64(e, &env).ok()
}

fn same(a: &Expr, b: &Expr) -> bool {
    if a == b {
        return true;
    }
    match (a.kind(), b.kind()) {
        (Kind::List(x), Kind::List(y)) => {
            if x.len() != y.len() {
                return false;
            }
            let mut used = vec![false; y.len()];
            x.iter().all(|p| {
                let hit = y.iter().enumerate().position(|(i, q)| !used[i] && same(p, q));
                hit.is_some_and(|i| {
                    used[i] = true;
                    true
                })
            })
        }
        (Kind::Eq(l1, r1), Kind::Eq(l2, r2)) => same(
            &Expr::add(vec![l1.clone(), Expr::mul(vec![Expr::int(-1), r1.clone()])]),
            &Expr::add(vec![l2.clone(), Expr::mul(vec![Expr::int(-1), r2.clone()])]),
        ),
        _ => {
            let diff = Expr::add(vec![a.clone(), Expr::mul(vec![Expr::int(-1), b.clone()])]);
            if mathcraft_algebra::simplify(&diff).is_ok_and(|d| d.as_i64() == Some(0)) {
                return true;
            }
            let ok = (0..3).filter_map(|k| Some((sample(a, k)?, sample(b, k)?))).collect::<Vec<_>>();
            ok.len() >= 2 && ok.iter().all(|(p, q)| (p - q).abs() <= 1e-8 * (1.0 + p.abs().max(q.abs())))
        }
    }
}

fn judge(input: &str, expected: &str) -> Verdict {
    let got = match eval(input) {
        Ok(v) => v,
        Err(e) => return Verdict::Error(e),
    };
    let want = match eval(expected) {
        Ok(v) => v,
        Err(e) => return Verdict::Error(format!("bad expectation: {e}")),
    };
    match (got.to_expr(), want.to_expr()) {
        (Ok(a), Ok(b)) if same(&a, &b) => Verdict::Pass,
        _ => Verdict::Wrong(got.to_string()),
    }
}

fn run_case(input: &str, expected: &str) -> Verdict {
    let (tx, rx) = mpsc::channel();
    let (i, e) = (input.to_string(), expected.to_string());
    let spawned = std::thread::Builder::new().stack_size(256 << 20).spawn(move || {
        let _g = budget::install(Budget::new(Some(LIMIT), Arc::new(AtomicBool::new(false)), u64::MAX));
        let _ = tx.send(judge(&i, &e));
    });
    if spawned.is_err() {
        return Verdict::Error("could not start a thread".into());
    }
    rx.recv_timeout(LIMIT + Duration::from_secs(10)).unwrap_or(Verdict::Timeout)
}

#[test]
#[ignore = "benchmark report, run on demand"]
fn benchmark() {
    let filter = std::env::var("BENCH").unwrap_or_default();
    let mut areas: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut failures = Vec::new();
    for line in CASES.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let mut parts = line.splitn(3, '|').map(str::trim);
        let (Some(area), Some(input), Some(expected)) = (parts.next(), parts.next(), parts.next()) else { continue };
        if !filter.is_empty() && !area.contains(&filter) {
            continue;
        }
        let start = Instant::now();
        let v = run_case(input, expected);
        let slot = areas.entry(area.to_string()).or_default();
        slot.1 += 1;
        if matches!(v, Verdict::Pass) {
            slot.0 += 1;
        } else {
            failures.push(format!("[{area}] {input}   (want {expected}; {:.2}s)\n      {}", start.elapsed().as_secs_f64(), v.describe()));
        }
    }
    println!();
    for f in &failures {
        println!("FAIL {f}");
    }
    println!("\n{:<14}{:>6}{:>7}", "area", "pass", "total");
    let (mut p, mut t) = (0, 0);
    for (a, (pass, total)) in &areas {
        println!("{a:<14}{pass:>6}{total:>7}");
        p += pass;
        t += total;
    }
    println!("{:<14}{p:>6}{t:>7}", "ALL");
}
