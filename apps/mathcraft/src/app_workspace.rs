//! Folder, git and saving (methods of `App`).

use super::*;

impl App {
    pub(crate) fn open_folder(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.set_folder(dir);
        }
    }

    pub(crate) fn set_folder(&mut self, dir: PathBuf) {
        self.tree = workspace::scan(&dir);
        self.status = format!("Opened folder {}", file_name(&dir));
        self.folder = Some(dir);
        self.collapsed.clear();
        self.show_sidebar = true;
        self.git = None;
        self.refresh_git();
    }

    pub(crate) fn rescan(&mut self) {
        if let Some(dir) = &self.folder {
            self.tree = workspace::scan(dir);
        }
        self.refresh_git();
    }

    /// Ask git about the opened folder on another thread (a big repository can take a moment).
    pub(crate) fn refresh_git(&mut self) {
        let Some(dir) = self.folder.clone() else { return };
        if self.git_pending.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        if std::thread::Builder::new().name("mathcraft-git".into()).spawn(move || drop(tx.send(git::status(&dir)))).is_ok() {
            self.git_pending = Some(rx);
        }
        self.git_checked = Some(std::time::Instant::now());
    }

    /// Take a finished reading, and ask again now and then (every few seconds in a repository, rarely elsewhere).
    pub(crate) fn poll_git(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.git_pending
            && let Ok(result) = rx.try_recv()
        {
            self.git = result;
            self.git_pending = None;
        }
        if self.folder.is_none() {
            return;
        }
        let every = Duration::from_secs(if self.git.is_some() { 4 } else { 30 });
        if self.git_checked.is_none_or(|t| t.elapsed() >= every) {
            self.refresh_git();
        }
        ctx.request_repaint_after(every);
    }

    pub(crate) fn toggle_sidebar(&mut self) {
        if self.folder.is_none() {
            self.open_folder();
        } else {
            self.show_sidebar = !self.show_sidebar;
        }
    }

    /// Create an empty notebook file in the opened folder and open it.
    pub(crate) fn new_in_folder(&mut self) {
        let Some(dir) = self.folder.clone() else { return };
        let path = workspace::unused_name(&dir, "untitled");
        match std::fs::write(&path, "# %%\n") {
            Ok(()) => {
                self.rescan();
                self.open_path(path);
            }
            Err(e) => self.status = format!("Could not create a file: {e}"),
        }
    }

    pub(crate) fn save(&mut self, as_new: bool) {
        let target = match (&self.cur.path, as_new) {
            (Some(p), false) => p.clone(),
            _ => match rfd::FileDialog::new().add_filter("MathCraft notebook", &["py"]).set_file_name("notebook.py").save_file() {
                Some(p) => p,
                None => return,
            },
        };
        // Write beside the target, then rename, so a crash never leaves half a file.
        let tmp = target.with_extension("py.tmp");
        let result = std::fs::write(&tmp, self.cur.nb.to_text()).and_then(|()| std::fs::rename(&tmp, &target));
        match result {
            Ok(()) => {
                self.status = format!("Saved {}", file_name(&target));
                self.remember(&target);
                self.cur.path = Some(target);
                self.cur.dirty = false;
                // its results are filed under the new name
                self.cur.outputs_need_writing();
                self.rescan();
            }
            Err(e) => self.status = format!("Could not save: {e}"),
        }
    }

    /// Write the notebook, with its results, as one HTML page.
    pub(crate) fn export_html(&mut self) {
        let name = self
            .cur
            .path
            .as_ref()
            .map(|p| p.with_extension("html"))
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "notebook.html".into());
        let mut dialog = rfd::FileDialog::new().add_filter("HTML page", &["html"]).set_file_name(&name);
        if let Some(dir) = self.cur.path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else {
            self.status = "Export cancelled".into();
            return;
        };
        let title = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Notebook".into());
        self.status = match std::fs::write(&path, export_html::notebook_html(&self.cur.nb, &title)) {
            Ok(()) => format!("Exported {}", file_name(&path)),
            Err(e) => format!("Could not export: {e}"),
        };
    }
}
