//! One open document: a notebook with its own kernel, file path and editing state. The app keeps several (tabs).

use crate::math_view::MathCache;
use eframe::egui;
use mathcraft_notebook::{CellId, CellState, Event, Kernel, Notebook, RunRequest};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct Doc {
    /// Stable identity of the tab (paths change on Save As, and untitled documents have none).
    pub id: u64,
    pub nb: Notebook,
    pub kernel: Kernel,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub focus: Option<CellId>,
    pub editing_md: HashSet<CellId>,
    pub math: MathCache,
    /// How many times each cell's output has been replaced; invalidates its cached typeset layout.
    pub generations: HashMap<CellId, u64>,
}

impl Doc {
    /// A document with a fresh kernel; every cell is queued to run.
    pub fn new(id: u64, ctx: &egui::Context, nb: Notebook, path: Option<PathBuf>) -> Self {
        let mut doc = Doc {
            id,
            nb,
            kernel: spawn_kernel(ctx),
            path,
            dirty: false,
            focus: None,
            editing_md: HashSet::new(),
            math: MathCache::default(),
            generations: HashMap::new(),
        };
        doc.run(RunRequest::All);
        doc
    }

    pub fn run(&mut self, req: RunRequest) {
        let jobs = self.nb.plan(req);
        if !jobs.is_empty() {
            self.kernel.run(jobs);
        }
    }

    /// Apply finished work from the kernel. Called for every open document, also those in background tabs.
    pub fn poll(&mut self) {
        while let Ok(ev) = self.kernel.events.try_recv() {
            match ev {
                Event::Started(id) => self.nb.mark_running(id),
                Event::Finished(id, r) => {
                    *self.generations.entry(id).or_default() += 1;
                    self.nb.apply(id, r);
                }
                Event::Reset => self.nb.mark_all_stale(),
            }
        }
    }

    pub fn busy(&self) -> bool {
        self.nb.cells().iter().any(|c| matches!(c.state, CellState::Running | CellState::Queued))
    }

    /// File name for tabs and titles.
    pub fn name(&self) -> String {
        self.path.as_deref().map_or_else(|| "Untitled".to_owned(), file_name)
    }

    /// Nothing typed, nothing saved: opening a file may replace it instead of adding a tab.
    pub fn is_pristine(&self) -> bool {
        self.path.is_none() && !self.dirty && matches!(self.nb.cells(), [c] if c.source.trim().is_empty())
    }
}

/// The last part of a path, for messages (the full path is in tooltips).
pub fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().to_string())
}

pub fn spawn_kernel(ctx: &egui::Context) -> Kernel {
    let c = ctx.clone();
    match Kernel::spawn(move || c.request_repaint()) {
        Ok(k) => k,
        // Without a kernel thread nothing can run; this only happens if the OS refuses a thread.
        Err(e) => {
            eprintln!("cannot start the computation thread: {e}");
            std::process::exit(1);
        }
    }
}
