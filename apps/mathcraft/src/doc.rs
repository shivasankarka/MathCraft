//! One open document: a notebook with its own kernel, file path and editing state. The app keeps several (tabs).

use crate::math_view::MathCache;
use eframe::egui;
use mathcraft_notebook::format::{self, CellText};
use mathcraft_notebook::{CellId, CellKind, CellState, Event, Kernel, Layout, Notebook, RunRequest};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// How a document is shown: as cells, or as the whole `# %%` file in one text editor.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    #[default]
    Notebook,
    Text,
}

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
    pub view: View,
    /// The file text while in Text view (the notebook is kept in step with it as it changes).
    pub text: String,
    /// Cell orders from before each add, delete or move, newest last (text edits have their own undo in the editor).
    undo: Vec<Layout>,
    redo: Vec<Layout>,
}

const UNDO_DEPTH: usize = 200;

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
            view: View::Notebook,
            text: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
        };
        doc.run(RunRequest::All);
        doc
    }

    /// Remember the cell order before a structural change.
    pub fn checkpoint(&mut self) {
        self.undo.push(self.nb.layout());
        self.redo.clear();
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undo the last add, delete or move. Returns false when there is nothing to undo.
    pub fn undo_structure(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(self.nb.layout());
        self.apply_layout(&prev);
        true
    }

    pub fn redo_structure(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(self.nb.layout());
        self.apply_layout(&next);
        true
    }

    fn apply_layout(&mut self, layout: &Layout) {
        for gone in self.nb.restore(layout) {
            self.kernel.forget_cell(gone);
            self.editing_md.remove(&gone);
        }
        self.dirty = true;
        self.run(RunRequest::Stale);
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

    pub fn set_view(&mut self, view: View) {
        if view == View::Text && self.view != View::Text {
            self.text = self.nb.to_text();
        }
        self.view = view;
    }

    /// Bring the notebook in line with `self.text`. Cells that did not change keep their identity and outputs; a
    /// change in the middle only touches the cells between the unchanged start and the unchanged end.
    pub fn sync_from_text(&mut self) {
        let new = format::parse(&self.text);
        let old: Vec<(CellId, CellText)> = self.nb.cells().iter().map(|c| (c.id, CellText { kind: c.kind, source: c.source.clone() })).collect();
        let same = |a: &(CellId, CellText), b: &CellText| a.1.kind == b.kind && a.1.source == b.source;
        let prefix = old.iter().zip(&new).take_while(|(a, b)| same(a, b)).count();
        let suffix = old.iter().rev().zip(new.iter().rev()).take(old.len().min(new.len()) - prefix).take_while(|(a, b)| same(a, b)).count();
        let (old_mid, new_mid) = (old.get(prefix..old.len() - suffix).unwrap_or(&[]), new.get(prefix..new.len() - suffix).unwrap_or(&[]));
        let mut last: Option<CellId> = prefix.checked_sub(1).and_then(|i| old.get(i)).map(|c| c.0);
        for (i, n) in new_mid.iter().enumerate() {
            match old_mid.get(i) {
                Some((id, o)) if o.kind == n.kind => {
                    self.nb.set_source(*id, &n.source);
                    last = Some(*id);
                }
                Some((id, _)) => {
                    // the kind changed (code <-> text): a new cell takes its place
                    let fresh = self.nb.add_cell(Some(*id), n.kind, &n.source);
                    self.nb.delete_cell(*id);
                    self.kernel.forget_cell(*id);
                    last = Some(fresh);
                }
                None => last = Some(self.nb.add_cell(last, n.kind, &n.source)),
            }
        }
        for (id, _) in old_mid.iter().skip(new_mid.len()) {
            self.nb.delete_cell(*id);
            self.kernel.forget_cell(*id);
        }
        if self.nb.cells().is_empty() {
            self.nb.add_cell(None, CellKind::Code, "");
        }
        self.dirty = true;
    }

    /// The id of the cell the text cursor (a char index into `self.text`) is in.
    pub fn cell_at(&self, cursor: usize) -> Option<CellId> {
        self.nb.cells().get(Self::cell_index_in(&self.text, cursor)).map(|c| c.id)
    }

    /// The index of the cell that the char index `cursor` of `text` falls in.
    pub fn cell_index_in(text: &str, cursor: usize) -> usize {
        let upto: String = text.chars().take(cursor).collect();
        // include the rest of the cursor's line so a cursor on a `# %%` marker belongs to that cell
        let rest: String = text.chars().skip(cursor).take_while(|c| *c != '\n').collect();
        format::parse(&format!("{upto}{rest}")).len().saturating_sub(1)
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
