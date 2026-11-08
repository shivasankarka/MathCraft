//! The notebook document: cells, their analysis and dependency graph, run states, and run planning.
//!
//! Rules (inspired by marimo): a global name may be defined by one cell only, cells may not depend on each other in a
//! cycle, and running a cell re-runs the cells that read what it defines. A cell that breaks a rule does not run.

use crate::analysis::{CellAnalysis, analyze};
use crate::format::{self, CellText};
use crate::runner::{CellResult, Event, JobResult, Runner, execute};
use mathcraft_eval::{EvalError, Value};
use mathcraft_syntax::ast::Stmt;
use petgraph::algo::tarjan_scc;
use petgraph::graph::DiGraph;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashSet};
use std::sync::Arc;
use std::time::Duration;

pub type CellId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    Code,
    Markdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellState {
    /// Never run.
    New,
    Fresh,
    /// Edited, or an upstream cell changed, since it last ran.
    Stale,
    Queued,
    Running,
    Error,
    /// Not run: it breaks a notebook rule or an upstream cell failed.
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    /// Running a cell also runs the cells that depend on it.
    Auto,
    /// Dependent cells are only marked stale.
    Lazy,
}

/// Something wrong with a cell that stops it from running. Codes are stable so tools can match on them.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    Syntax { line: u32, message: String },
    MultipleDefinition { name: String, other_cells: Vec<usize> },
    Cycle { cells: Vec<usize> },
    AmbiguousReference { name: String },
}

impl Problem {
    pub fn code(&self) -> &'static str {
        match self {
            Problem::Syntax { .. } => "MC001",
            Problem::MultipleDefinition { .. } => "MC002",
            Problem::Cycle { .. } => "MC003",
            Problem::AmbiguousReference { .. } => "MC004",
        }
    }

    pub fn message(&self) -> String {
        let cells = |v: &[usize]| v.iter().map(|i| (i + 1).to_string()).collect::<Vec<_>>().join(", ");
        match self {
            Problem::Syntax { line, message } => format!("line {line}: {message}"),
            Problem::MultipleDefinition { name, other_cells } => {
                format!(
                    "'{name}' is also defined in cell {}. Each name can be defined in only one cell; rename one, or start it with _ to keep it private to the cell",
                    cells(other_cells)
                )
            }
            Problem::Cycle { cells: c } => format!("cells {} depend on each other in a cycle", cells(c)),
            Problem::AmbiguousReference { name } => format!("'{name}' is defined in more than one cell, so it is unclear which one to use"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CellOutput {
    pub stdout: String,
    pub result: Result<Option<Value>, EvalError>,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub id: CellId,
    pub kind: CellKind,
    pub source: String,
    pub state: CellState,
    pub output: Option<CellOutput>,
    /// Why a blocked cell did not run.
    pub note: Option<String>,
    pub problems: Vec<Problem>,
    syntax: Option<(u32, String)>,
    analysis: Option<CellAnalysis>,
    stmts: Option<Arc<Vec<Stmt>>>,
}

impl Cell {
    pub fn defs(&self) -> impl Iterator<Item = &String> {
        self.analysis.iter().flat_map(|a| a.defs.iter())
    }
    pub fn refs(&self) -> impl Iterator<Item = &String> {
        self.analysis.iter().flat_map(|a| a.refs.iter())
    }
}

/// One cell to run, with everything the kernel needs (no access to the notebook).
#[derive(Debug, Clone)]
pub struct Job {
    pub id: CellId,
    pub stmts: Arc<Vec<Stmt>>,
    pub deps: Vec<CellId>,
    /// Set when the cell must not run; the string says why.
    pub pre_blocked: Option<String>,
    pub strict: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunRequest {
    Cell(CellId),
    All,
    /// Every cell that is new or stale.
    Stale,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub cell: usize,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Default)]
struct Graph {
    deps: Vec<Vec<usize>>,
    dependents: Vec<Vec<usize>>,
    order: Vec<usize>,
}

/// A saved cell order (see `Notebook::layout`).
#[derive(Debug, Clone, PartialEq)]
pub struct Layout(Vec<(CellId, CellKind, String)>);

#[derive(Debug)]
pub struct Notebook {
    cells: Vec<Cell>,
    next_id: CellId,
    pub mode: RunMode,
    /// Require `symbols(...)` declarations instead of treating unknown names as symbols.
    pub strict: bool,
    graph: Graph,
}

impl Default for Notebook {
    fn default() -> Self {
        Notebook { cells: Vec::new(), next_id: 1, mode: RunMode::Auto, strict: false, graph: Graph::default() }
    }
}

impl Notebook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_text(text: &str) -> Self {
        let mut nb = Notebook::new();
        for c in format::parse(text) {
            nb.push_cell(c.kind, c.source);
        }
        nb.refresh();
        nb
    }

    pub fn to_text(&self) -> String {
        let cells: Vec<CellText> = self.cells.iter().map(|c| CellText { kind: c.kind, source: c.source.clone() }).collect();
        format::write(&cells)
    }

    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    pub fn cell(&self, id: CellId) -> Option<&Cell> {
        self.cells.iter().find(|c| c.id == id)
    }

    pub fn index_of(&self, id: CellId) -> Option<usize> {
        self.cells.iter().position(|c| c.id == id)
    }

    fn push_cell(&mut self, kind: CellKind, source: String) -> CellId {
        let id = self.next_id;
        self.next_id += 1;
        self.push_with_id(id, kind, source);
        id
    }

    fn push_with_id(&mut self, id: CellId, kind: CellKind, source: String) {
        self.cells.push(Cell {
            id,
            kind,
            source,
            state: CellState::New,
            output: None,
            note: None,
            problems: Vec::new(),
            syntax: None,
            analysis: None,
            stmts: None,
        });
        let last = self.cells.len() - 1;
        self.reanalyze(last);
    }

    fn reanalyze(&mut self, idx: usize) {
        let Some(cell) = self.cells.get_mut(idx) else { return };
        cell.syntax = None;
        cell.analysis = None;
        cell.stmts = None;
        if cell.kind == CellKind::Code {
            match mathcraft_syntax::parse(&cell.source) {
                Ok(stmts) => {
                    cell.analysis = Some(analyze(&stmts));
                    cell.stmts = Some(Arc::new(stmts));
                }
                Err(e) => cell.syntax = Some((e.line, e.message)),
            }
        }
    }

    pub fn add_cell(&mut self, after: Option<CellId>, kind: CellKind, source: &str) -> CellId {
        let id = self.push_cell(kind, source.to_string());
        if let Some(at) = after.and_then(|a| self.index_of(a)) {
            let last = self.cells.len() - 1;
            let cell = self.cells.remove(last);
            self.cells.insert(at + 1, cell);
        }
        self.refresh();
        id
    }

    /// Remove a cell. Cells that read what it defined become stale. The caller should also tell the runner
    /// (`Runner::forget_cell`) so its variables disappear.
    pub fn delete_cell(&mut self, id: CellId) -> bool {
        let Some(idx) = self.index_of(id) else { return false };
        for d in self.descendants_of(idx) {
            if let Some(c) = self.cells.get_mut(d)
                && matches!(c.state, CellState::Fresh | CellState::Error | CellState::Blocked)
            {
                c.state = CellState::Stale;
            }
        }
        self.cells.remove(idx);
        self.refresh();
        true
    }

    /// The cells' order, kinds and text, for structural undo.
    pub fn layout(&self) -> Layout {
        Layout(self.cells.iter().map(|c| (c.id, c.kind, c.source.clone())).collect())
    }

    /// Bring back the cell order of `layout`. Cells that still exist keep their current text and output (so typing
    /// is never undone); cells that were deleted come back with the saved text, and are new. Returns the ids of
    /// the cells that were removed.
    pub fn restore(&mut self, layout: &Layout) -> Vec<CellId> {
        let mut old: Vec<Cell> = std::mem::take(&mut self.cells);
        for (id, kind, source) in &layout.0 {
            match old.iter().position(|c| c.id == *id) {
                Some(i) => self.cells.push(old.remove(i)),
                None => self.push_with_id(*id, *kind, source.clone()),
            }
            self.next_id = self.next_id.max(id + 1);
        }
        let removed: Vec<CellId> = old.iter().map(|c| c.id).collect();
        self.mark_all_stale();
        self.refresh();
        removed
    }

    pub fn move_cell(&mut self, id: CellId, delta: isize) {
        let Some(idx) = self.index_of(id) else { return };
        let Some(to) = idx.checked_add_signed(delta).filter(|t| *t < self.cells.len()) else { return };
        let cell = self.cells.remove(idx);
        self.cells.insert(to, cell);
        self.refresh();
    }

    pub fn set_source(&mut self, id: CellId, source: &str) {
        let Some(idx) = self.index_of(id) else { return };
        let Some(cell) = self.cells.get_mut(idx) else { return };
        if cell.source == source {
            return;
        }
        cell.source = source.to_string();
        if matches!(cell.state, CellState::Fresh | CellState::Error | CellState::Blocked) {
            cell.state = CellState::Stale;
        }
        self.reanalyze(idx);
        self.refresh();
    }

    // ---- dependency graph --------------------------------------------------------------------------------------

    fn refresh(&mut self) {
        let n = self.cells.len();
        let mut problems: Vec<Vec<Problem>> = vec![Vec::new(); n];
        let mut definers: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (i, c) in self.cells.iter().enumerate() {
            if let Some(a) = &c.analysis {
                for d in &a.defs {
                    definers.entry(d.as_str()).or_default().push(i);
                }
            }
            if let Some((line, message)) = &c.syntax
                && let Some(p) = problems.get_mut(i)
            {
                p.push(Problem::Syntax { line: *line, message: message.clone() });
            }
        }
        let mut edges: BTreeSet<(usize, usize)> = BTreeSet::new();
        for (name, ds) in &definers {
            if ds.len() > 1 {
                for d in ds {
                    if let Some(p) = problems.get_mut(*d) {
                        p.push(Problem::MultipleDefinition { name: (*name).to_string(), other_cells: ds.iter().copied().filter(|o| o != d).collect() });
                    }
                }
            }
        }
        for (i, c) in self.cells.iter().enumerate() {
            let Some(a) = &c.analysis else { continue };
            for r in &a.refs {
                match definers.get(r.as_str()).map(Vec::as_slice) {
                    Some([d]) if *d == i => {
                        if let Some(p) = problems.get_mut(i) {
                            p.push(Problem::Cycle { cells: vec![i] });
                        }
                    }
                    Some([d]) => {
                        edges.insert((*d, i));
                    }
                    Some(_) => {
                        if let Some(p) = problems.get_mut(i) {
                            p.push(Problem::AmbiguousReference { name: r.clone() });
                        }
                    }
                    None => {}
                }
            }
        }
        let mut g: DiGraph<usize, ()> = DiGraph::new();
        let nodes: Vec<_> = (0..n).map(|i| g.add_node(i)).collect();
        for (a, b) in &edges {
            if let (Some(x), Some(y)) = (nodes.get(*a), nodes.get(*b)) {
                g.add_edge(*x, *y, ());
            }
        }
        for scc in tarjan_scc(&g) {
            if scc.len() > 1 {
                let mut cells: Vec<usize> = scc.iter().filter_map(|nx| g.node_weight(*nx).copied()).collect();
                cells.sort_unstable();
                for c in &cells {
                    if let Some(p) = problems.get_mut(*c) {
                        p.push(Problem::Cycle { cells: cells.clone() });
                    }
                }
            }
        }
        let mut deps = vec![Vec::new(); n];
        let mut dependents = vec![Vec::new(); n];
        let mut indegree = vec![0usize; n];
        for (a, b) in &edges {
            if let (Some(d), Some(dd), Some(x)) = (deps.get_mut(*b), dependents.get_mut(*a), indegree.get_mut(*b)) {
                d.push(*a);
                dd.push(*b);
                *x += 1;
            }
        }
        // Topological order (Kahn), document order breaking ties; cells stuck in cycles go last.
        let mut ready: BinaryHeap<Reverse<usize>> = (0..n).filter(|i| indegree.get(*i) == Some(&0)).map(Reverse).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(Reverse(i)) = ready.pop() {
            order.push(i);
            for d in dependents.get(i).cloned().unwrap_or_default() {
                if let Some(x) = indegree.get_mut(d) {
                    *x -= 1;
                    if *x == 0 {
                        ready.push(Reverse(d));
                    }
                }
            }
        }
        let placed: HashSet<usize> = order.iter().copied().collect();
        order.extend((0..n).filter(|i| !placed.contains(i)));
        self.graph = Graph { deps, dependents, order };
        for (c, p) in self.cells.iter_mut().zip(problems) {
            c.problems = p;
        }
    }

    fn descendants_of(&self, idx: usize) -> Vec<usize> {
        let mut seen = HashSet::new();
        let mut stack = vec![idx];
        let mut out = Vec::new();
        while let Some(i) = stack.pop() {
            for d in self.graph.dependents.get(i).into_iter().flatten() {
                if seen.insert(*d) {
                    out.push(*d);
                    stack.push(*d);
                }
            }
        }
        out
    }

    fn ancestors_of(&self, idx: usize) -> Vec<usize> {
        let mut seen = HashSet::new();
        let mut stack = vec![idx];
        let mut out = Vec::new();
        while let Some(i) = stack.pop() {
            for d in self.graph.deps.get(i).into_iter().flatten() {
                if seen.insert(*d) {
                    out.push(*d);
                    stack.push(*d);
                }
            }
        }
        out
    }

    pub fn dependents(&self, id: CellId) -> Vec<CellId> {
        let Some(idx) = self.index_of(id) else { return Vec::new() };
        self.descendants_of(idx).into_iter().filter_map(|i| self.cells.get(i).map(|c| c.id)).collect()
    }

    pub fn dependencies(&self, id: CellId) -> Vec<CellId> {
        let Some(idx) = self.index_of(id) else { return Vec::new() };
        self.graph.deps.get(idx).into_iter().flatten().filter_map(|i| self.cells.get(*i).map(|c| c.id)).collect()
    }

    /// Problems across the whole notebook, for `mathcraft check`.
    pub fn check(&self) -> Vec<Diagnostic> {
        self.cells.iter().enumerate().flat_map(|(i, c)| c.problems.iter().map(move |p| Diagnostic { cell: i, code: p.code(), message: p.message() })).collect()
    }

    // ---- running -----------------------------------------------------------------------------------------------

    /// Decide what to run for `req` and mark those cells queued. Jobs come in dependency order.
    pub fn plan(&mut self, req: RunRequest) -> Vec<Job> {
        let mut set: BTreeSet<usize> = BTreeSet::new();
        let is_code = |nb: &Notebook, i: usize| nb.cells.get(i).is_some_and(|c| c.kind == CellKind::Code);
        match req {
            RunRequest::Cell(id) => {
                if let Some(i) = self.index_of(id) {
                    set.insert(i);
                    if self.mode == RunMode::Auto {
                        set.extend(self.descendants_of(i));
                    }
                }
            }
            RunRequest::All => set.extend(0..self.cells.len()),
            RunRequest::Stale => {
                for i in 0..self.cells.len() {
                    if self.cells.get(i).is_some_and(|c| matches!(c.state, CellState::New | CellState::Stale)) {
                        set.insert(i);
                        if self.mode == RunMode::Auto {
                            set.extend(self.descendants_of(i));
                        }
                    }
                }
            }
        }
        // Upstream cells that never ran or are stale must run first, or the names this cell reads would be missing.
        for i in set.clone() {
            for a in self.ancestors_of(i) {
                if self.cells.get(a).is_some_and(|c| matches!(c.state, CellState::New | CellState::Stale)) {
                    set.insert(a);
                }
            }
        }
        set.retain(|i| is_code(self, *i));
        let in_plan: HashSet<usize> = set.iter().copied().collect();
        let order: Vec<usize> = self.graph.order.iter().copied().filter(|i| in_plan.contains(i)).collect();
        let mut jobs = Vec::with_capacity(order.len());
        for i in order {
            let deps_idx = self.graph.deps.get(i).cloned().unwrap_or_default();
            let dep_ids: Vec<CellId> = deps_idx.iter().filter_map(|d| self.cells.get(*d).map(|c| c.id)).collect();
            let failed_dep =
                deps_idx.iter().find(|d| !in_plan.contains(d) && self.cells.get(**d).is_some_and(|c| matches!(c.state, CellState::Error | CellState::Blocked)));
            let strict = self.strict;
            let Some(cell) = self.cells.get_mut(i) else { continue };
            let pre_blocked = match (cell.problems.first(), failed_dep) {
                (Some(p), _) => Some(p.message()),
                (None, Some(d)) => Some(format!("cell {} did not finish, so this one cannot run", d + 1)),
                (None, None) => None,
            };
            cell.state = CellState::Queued;
            let Some(stmts) = cell.stmts.clone() else {
                // A syntax error: nothing to run, but still report it as blocked.
                jobs.push(Job {
                    id: cell.id,
                    stmts: Arc::new(Vec::new()),
                    deps: dep_ids,
                    pre_blocked: pre_blocked.or(Some("the code could not be read".into())),
                    strict,
                });
                continue;
            };
            jobs.push(Job { id: cell.id, stmts, deps: dep_ids, pre_blocked, strict });
        }
        jobs
    }

    pub fn mark_running(&mut self, id: CellId) {
        if let Some(i) = self.index_of(id)
            && let Some(c) = self.cells.get_mut(i)
        {
            c.state = CellState::Running;
        }
    }

    /// Record the outcome of a job.
    pub fn apply(&mut self, id: CellId, result: JobResult) {
        let Some(idx) = self.index_of(id) else { return };
        let finished = matches!(result, JobResult::Done(_));
        if let Some(c) = self.cells.get_mut(idx) {
            match result {
                JobResult::Done(CellResult { stdout, result, elapsed }) => {
                    c.state = if result.is_ok() { CellState::Fresh } else { CellState::Error };
                    c.note = None;
                    c.output = Some(CellOutput { stdout, result, elapsed });
                }
                JobResult::Skipped(why) => {
                    c.state = CellState::Blocked;
                    c.note = Some(why);
                    c.output = None;
                }
                JobResult::Cancelled => c.state = CellState::Stale,
            }
        }
        if finished {
            // Cells that were not re-run (lazy mode) now hold results from older inputs.
            for d in self.descendants_of(idx).into_iter().filter(|d| *d != idx) {
                if let Some(c) = self.cells.get_mut(d)
                    && matches!(c.state, CellState::Fresh | CellState::Error | CellState::Blocked)
                {
                    c.state = CellState::Stale;
                }
            }
        }
    }

    /// The kernel died or restarted: nothing it computed can be trusted.
    pub fn mark_all_stale(&mut self) {
        for c in &mut self.cells {
            if matches!(c.state, CellState::Fresh | CellState::Error | CellState::Blocked | CellState::Queued | CellState::Running) {
                c.state = CellState::Stale;
            }
        }
    }

    /// Run `req` to completion on this thread (used by the CLI and tests).
    pub fn run_blocking(&mut self, runner: &mut Runner, req: RunRequest) {
        let jobs = self.plan(req);
        let mut events: Vec<Event> = Vec::new();
        execute(runner, jobs, &mut |ev| events.push(ev));
        for ev in events {
            match ev {
                Event::Started(id) => self.mark_running(id),
                Event::Finished(id, r) => self.apply(id, r),
                Event::Reset => self.mark_all_stale(),
            }
        }
    }
}
