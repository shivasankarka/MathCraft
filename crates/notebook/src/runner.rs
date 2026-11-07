//! Running jobs: the runner owns the notebook's global variables and executes one cell at a time under a
//! computation budget (timeout, step limit and an interrupt flag).

use crate::notebook::{CellId, Job};
use mathcraft_core::budget::{self, Budget};
use mathcraft_eval::{EvalError, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct CellResult {
    pub stdout: String,
    pub result: Result<Option<Value>, EvalError>,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub enum JobResult {
    Done(CellResult),
    /// Not run, with the reason (a notebook rule was broken, or an upstream cell failed).
    Skipped(String),
    /// Not started because the user interrupted an earlier cell.
    Cancelled,
}

#[derive(Debug, Clone)]
pub enum Event {
    Started(CellId),
    Finished(CellId, JobResult),
    /// The kernel lost its state (an internal error); every result is out of date.
    Reset,
}

pub struct Runner {
    globals: HashMap<String, Value>,
    /// Names each cell committed, so they can be removed when it re-runs, fails or is deleted.
    owned: HashMap<CellId, Vec<String>>,
    pub timeout: Option<Duration>,
    pub max_ops: u64,
    pub cancel: Arc<AtomicBool>,
}

impl Runner {
    pub fn new(cancel: Arc<AtomicBool>) -> Self {
        Runner { globals: HashMap::new(), owned: HashMap::new(), timeout: Some(Duration::from_secs(60)), max_ops: 400_000_000, cancel }
    }

    pub fn forget_cell(&mut self, id: CellId) {
        if let Some(names) = self.owned.remove(&id) {
            for n in names {
                self.globals.remove(&n);
            }
        }
    }

    pub fn reset(&mut self) {
        self.globals.clear();
        self.owned.clear();
    }

    pub fn globals(&self) -> &HashMap<String, Value> {
        &self.globals
    }

    pub fn run_job(&mut self, job: &Job, failed: &mut HashSet<CellId>) -> JobResult {
        self.forget_cell(job.id);
        if let Some(why) = &job.pre_blocked {
            failed.insert(job.id);
            return JobResult::Skipped(why.clone());
        }
        if self.cancel.load(Ordering::Relaxed) {
            return JobResult::Cancelled;
        }
        if job.deps.iter().any(|d| failed.contains(d)) {
            failed.insert(job.id);
            return JobResult::Skipped("an upstream cell did not finish, so this one cannot run".into());
        }
        let start = Instant::now();
        let outcome = {
            let _guard = budget::install(Budget::new(self.timeout, self.cancel.clone(), self.max_ops));
            mathcraft_eval::run(&mut self.globals, &job.stmts, job.strict)
        };
        if outcome.result.is_ok() {
            self.owned.insert(job.id, outcome.assigned);
        } else {
            failed.insert(job.id);
        }
        JobResult::Done(CellResult { stdout: outcome.stdout, result: outcome.result, elapsed: start.elapsed() })
    }
}

/// Run jobs in order, reporting events as they happen. Clears the interrupt flag when finished.
pub fn execute(runner: &mut Runner, jobs: Vec<Job>, on_event: &mut dyn FnMut(Event)) {
    runner.cancel.store(false, Ordering::Relaxed);
    let mut failed = HashSet::new();
    for job in jobs {
        on_event(Event::Started(job.id));
        let result = runner.run_job(&job, &mut failed);
        on_event(Event::Finished(job.id, result));
    }
    runner.cancel.store(false, Ordering::Relaxed);
}
