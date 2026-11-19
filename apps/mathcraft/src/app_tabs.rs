//! Run requests, tabs and files (methods of `App`).

use super::*;

impl App {
    pub(crate) fn run(&mut self, req: RunRequest) {
        self.cur.run(req);
    }

    pub(crate) fn poll_kernel(&mut self) {
        self.cur.poll();
        for d in &mut self.others {
            d.poll();
        }
    }

    pub(crate) fn busy(&self) -> bool {
        self.cur.busy() || self.others.iter().any(Doc::busy)
    }

    pub(crate) fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Replace the current document's content (a new kernel: the old one's variables belong to the old notebook).
    pub(crate) fn replace_notebook(&mut self, nb: Notebook, path: Option<PathBuf>) {
        self.cur = Doc::new(self.cur.id, &self.ctx, nb, path);
    }

    /// Show a notebook in a new tab, or in the current one when that is still untouched and untitled.
    pub(crate) fn open_in_tab(&mut self, nb: Notebook, path: Option<PathBuf>) {
        if self.cur.is_pristine() {
            self.replace_notebook(nb, path);
            return;
        }
        let id = self.fresh_id();
        let old = std::mem::replace(&mut self.cur, Doc::new(id, &self.ctx, nb, path));
        self.others.push(old);
        self.order.push(id);
    }

    pub(crate) fn select_tab(&mut self, id: u64) {
        if id == self.cur.id {
            return;
        }
        if let Some(slot) = self.others.iter_mut().find(|d| d.id == id) {
            std::mem::swap(&mut self.cur, slot);
        }
    }

    pub(crate) fn step_tab(&mut self, delta: isize) {
        let n = self.order.len() as isize;
        let here = self.order.iter().position(|id| *id == self.cur.id).unwrap_or(0) as isize;
        if let Some(id) = self.order.get((here + delta).rem_euclid(n.max(1)) as usize).copied() {
            self.select_tab(id);
        }
    }

    /// Put a tab at index `to` in the strip.
    pub(crate) fn move_tab(&mut self, id: u64, to: usize) {
        if let Some(from) = self.order.iter().position(|o| *o == id) {
            let tab = self.order.remove(from);
            self.order.insert(to.min(self.order.len()), tab);
        }
    }

    pub(crate) fn doc(&self, id: u64) -> Option<&Doc> {
        if self.cur.id == id { Some(&self.cur) } else { self.others.iter().find(|d| d.id == id) }
    }

    pub(crate) fn any_dirty(&self) -> bool {
        self.cur.dirty || self.others.iter().any(|d| d.dirty)
    }

    pub(crate) fn confirm(title: &str, description: &str) -> bool {
        rfd::MessageDialog::new().set_title(title).set_description(description).set_buttons(rfd::MessageButtons::YesNo).show() == rfd::MessageDialogResult::Yes
    }

    pub(crate) fn close_tab(&mut self, id: u64) {
        if self.doc(id).is_some_and(|d| d.dirty) {
            self.select_tab(id);
            if !Self::confirm("Unsaved changes", &format!("{} has unsaved changes. Close it and discard them?", self.cur.name())) {
                return;
            }
        }
        let pos = self.order.iter().position(|o| *o == id).unwrap_or(0);
        self.order.retain(|o| *o != id);
        if id == self.cur.id {
            let next = self.order.get(pos).or_else(|| self.order.get(pos.wrapping_sub(1))).copied();
            match next {
                Some(n) => {
                    self.select_tab(n);
                    self.others.retain(|d| d.id != id);
                }
                None => {
                    let fresh = self.fresh_id();
                    let mut nb = Notebook::new();
                    nb.add_cell(None, CellKind::Code, "");
                    self.cur = Doc::new(fresh, &self.ctx, nb, None);
                    self.order.push(fresh);
                }
            }
        } else {
            self.others.retain(|d| d.id != id);
        }
    }

    pub(crate) fn new_file(&mut self) {
        let mut nb = Notebook::new();
        nb.add_cell(None, CellKind::Code, "");
        self.open_in_tab(nb, None);
        self.status = "New notebook".into();
    }

    /// Open a bundled example as an untitled notebook and run it.
    pub(crate) fn open_example(&mut self, index: usize) {
        let Some((name, _, text)) = examples::EXAMPLES.get(index) else { return };
        self.open_in_tab(Notebook::from_text(text), None);
        self.status = format!("Opened the example \"{name}\"");
    }

    pub(crate) fn open_file(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("MathCraft notebook or script", &["py", "txt"]).pick_file() else { return };
        self.open_path(p);
    }

    /// Open a file in a tab, or switch to its tab when it is already open.
    pub(crate) fn open_path(&mut self, p: PathBuf) {
        let same = |d: &Doc| d.path.as_ref().is_some_and(|q| q == &p);
        if let Some(id) = std::iter::once(&self.cur).chain(&self.others).find(|d| same(d)).map(|d| d.id) {
            self.select_tab(id);
            return;
        }
        match std::fs::read_to_string(&p) {
            Ok(t) => {
                self.status = format!("Opened {}", file_name(&p));
                self.remember(&p);
                self.open_in_tab(Notebook::from_text(&t), Some(p));
            }
            Err(e) => self.status = format!("Could not open {}: {e}", file_name(&p)),
        }
    }
}
