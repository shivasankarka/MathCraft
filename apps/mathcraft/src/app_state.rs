//! State, session and settings (methods of `App`).

use super::*;

impl App {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let path_given = path.is_some();
        let mut app = Self::with_context(cc.egui_ctx.clone(), path);
        app.unified_title_bar = cfg!(target_os = "macos");
        app.native_menu = native_menu::NativeMenu::install(&cc.egui_ctx);
        app.load_settings(settings::config_dir());
        if let Some(p) = app.cur.path.clone() {
            // the file named on the command line was started before the config folder was known: show its saved results
            let nb = Notebook::from_text(&app.cur.nb.to_text());
            app.replace_notebook(nb, Some(p));
        }
        app.load_session(path_given);
        app
    }

    /// Bring back the last session's tabs and folder (unless a file or folder was named on the command line), and
    /// the recent files.
    pub(crate) fn load_session(&mut self, path_given: bool) {
        let Some(dir) = self.config.clone() else { return };
        let s = session::Session::load(&dir);
        self.recent = s.recent.clone();
        if !path_given {
            self.restore_session(&s);
        }
        // drafts that no tab refers to any more, and results nobody has opened for a long time
        store::prune(&dir, &self.live_drafts());
        self.saved_session = self.current_session();
    }

    pub(crate) fn restore_session(&mut self, s: &session::Session) {
        if let Some(f) = s.folder.as_ref().filter(|f| f.is_dir()) {
            self.set_folder(f.clone());
            self.show_sidebar = s.sidebar;
            self.status.clear();
        }
        let mut restored: Vec<(session::Tab, Doc)> = Vec::new();
        for tab in &s.tabs {
            let id = if restored.is_empty() { self.cur.id } else { self.fresh_id() };
            let doc = match tab {
                session::Tab::File(p) if p.is_file() => {
                    std::fs::read_to_string(p).ok().map(|t| self.make_doc(id, Notebook::from_text(&t), Some(p.clone()), None))
                }
                session::Tab::File(_) => None,
                session::Tab::Draft(name) => self.config.as_deref().and_then(|dir| store::read_draft(dir, name)).map(|t| {
                    let mut d = self.make_doc(id, Notebook::from_text(&t), None, Some(name.clone()));
                    d.dirty = true;
                    d
                }),
            };
            if let Some(mut doc) = doc {
                // until it is shown, the tab keeps the view it had
                doc.restore_view = s.views.get(&tab.key()).copied();
                doc.memo = doc.restore_view.unwrap_or_default();
                restored.push((tab.clone(), doc));
            }
        }
        if restored.is_empty() {
            return;
        }
        let count = restored.len();
        let active_tab = s.tabs.get(s.active);
        let active_id = restored.iter().find(|(t, _)| Some(t) == active_tab).map(|(_, d)| d.id);
        // the welcome notebook gives way to the first restored tab
        self.order = restored.iter().map(|(_, d)| d.id).collect();
        let mut docs = restored.into_iter().map(|(_, d)| d);
        if let Some(first) = docs.next() {
            self.cur = first;
        }
        self.others = docs.collect();
        if let Some(id) = active_id {
            self.select_tab(id);
        }
        self.status = format!("Restored {count} {}", if count == 1 { "tab" } else { "tabs" });
    }

    /// A document that shows the results saved for it (from its path, or its draft name) instead of running them.
    pub(crate) fn make_doc(&self, id: u64, nb: Notebook, path: Option<PathBuf>, draft: Option<String>) -> Doc {
        let key = match (&path, &draft) {
            (Some(p), _) => Some(store::key_for(p)),
            (None, d) => d.clone(),
        };
        let saved = self.config.as_deref().zip(key).and_then(|(dir, k)| store::read_outputs(dir, &k));
        let mut doc = Doc::with_saved(id, &self.ctx, nb, path, saved.as_deref());
        doc.draft = draft;
        doc
    }

    /// Note which cell has the cursor, and where in it, to come back to next launch.
    pub(crate) fn track_cursor(&mut self, ctx: &egui::Context) {
        for (i, c) in self.cur.nb.cells().iter().enumerate() {
            let id = editor_id(c.id);
            if ctx.memory(|m| m.has_focus(id)) {
                let caret = egui::TextEdit::load_state(ctx, id).and_then(|s| s.cursor.char_range()).map_or(0, |r| usize::from(r.primary.index));
                self.cur.memo.cursor = Some((i, caret));
                return;
            }
        }
    }

    /// Put the cursor back where `restore_view` says (the cell takes the keyboard on the next draw); returns the scroll
    /// offset to restore.
    pub(crate) fn take_restored_view(&mut self, ctx: &egui::Context) -> Option<f32> {
        let m = self.cur.restore_view.take()?;
        if let Some((cell, caret)) = m.cursor
            && let Some(c) = self.cur.nb.cells().get(cell)
        {
            let id = editor_id(c.id);
            let at = egui::text::CCursor::new(caret.min(c.source.chars().count()));
            let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::one(at)));
            state.store(ctx, id);
            self.cur.focus = Some(c.id);
        }
        self.cur.memo = m;
        Some(m.scroll as f32)
    }

    fn live_drafts(&self) -> Vec<String> {
        std::iter::once(&self.cur).chain(&self.others).filter_map(|d| d.draft.clone()).collect()
    }

    pub(crate) fn current_session(&self) -> session::Session {
        let tab = |d: &Doc| match (&d.path, &d.draft) {
            (Some(p), _) => Some(session::Tab::File(p.clone())),
            (None, Some(n)) => Some(session::Tab::Draft(n.clone())),
            (None, None) => None,
        };
        let mut tabs = Vec::new();
        let mut views = std::collections::BTreeMap::new();
        let mut active = 0;
        for d in self.order.iter().filter_map(|id| self.doc(*id)) {
            if let Some(t) = tab(d) {
                if d.id == self.cur.id {
                    active = tabs.len();
                }
                views.insert(t.key(), d.memo);
                tabs.push(t);
            }
        }
        session::Session { tabs, active, folder: self.folder.clone(), sidebar: self.show_sidebar, recent: self.recent.clone(), views }
    }

    /// Keep the session, untitled drafts and results on disk up to date. Where a tab was scrolled changes all the time,
    /// so that alone is written at most every couple of seconds; `now` forces everything out (when quitting).
    pub(crate) fn persist_session(&mut self, now: bool) {
        let Some(dir) = self.config.clone() else { return };
        let time = self.ctx.input(|i| i.time);
        let due = now || time - self.stored_at >= 2.0;
        if due {
            self.stored_at = time;
        }
        self.keep_drafts();
        let mut failure = None;
        for d in std::iter::once(&mut self.cur).chain(self.others.iter_mut()) {
            if let Err(e) = d.write_store(&dir, due) {
                failure = Some(e);
            }
        }
        let session = self.current_session();
        if session != self.saved_session {
            let structural = session::Session { views: Default::default(), ..session.clone() }
                != session::Session { views: Default::default(), ..self.saved_session.clone() };
            if structural || due {
                if let Err(e) = session.save(&dir) {
                    failure = Some(e);
                }
                self.saved_session = session;
            } else {
                // only the scroll position or cursor moved: write it a little later
                self.ctx.request_repaint_after(Duration::from_millis(2100));
            }
        }
        if let Some(e) = failure {
            self.status = format!("Could not save session: {e}");
        }
    }

    /// Give a draft name to each untitled notebook that has been typed into, and take it from one that was saved.
    fn keep_drafts(&mut self) {
        let dir = self.config.clone();
        let mut taken = self.live_drafts();
        for d in std::iter::once(&mut self.cur).chain(self.others.iter_mut()) {
            if d.path.is_some()
                && let Some(name) = d.draft.take()
            {
                // saved to a file: the draft is no longer needed, and its results move under the file's name
                if let Some(dir) = &dir {
                    store::forget_draft(dir, &name);
                }
                d.outputs_need_writing();
            } else if d.draft.is_none() && d.worth_a_draft() {
                let name = store::new_draft_name(dir.as_deref(), &taken);
                taken.push(name.clone());
                d.draft = Some(name);
                d.outputs_need_writing();
            }
        }
    }

    pub(crate) fn remember(&mut self, p: &Path) {
        let mut s = session::Session { recent: std::mem::take(&mut self.recent), ..Default::default() };
        s.remember(p);
        self.recent = s.recent;
    }

    /// Adopt the saved settings and start saving changes to `dir`.
    pub(crate) fn load_settings(&mut self, dir: Option<PathBuf>) {
        let s = dir.as_deref().map(settings::Settings::load).unwrap_or_default();
        self.vim = self.vim || s.vim;
        self.pretty = s.pretty_powers;
        self.ctx.set_theme(s.appearance);
        self.accent = s.accent.clone();
        self.ctx.set_zoom_factor(s.zoom);
        self.config = dir;
        self.rebuild_themes();
        self.saved = self.current_settings();
    }

    /// The themes again from the config folder's `theme.json` and the chosen accent.
    pub(crate) fn rebuild_themes(&mut self) {
        (self.dark, self.light) = settings::themes(self.config.as_deref(), self.accent.as_deref());
        self.dark.apply(&self.ctx);
        self.light.apply(&self.ctx);
        configure_style(&self.ctx);
    }

    pub(crate) fn current_settings(&self) -> settings::Settings {
        settings::Settings {
            vim: self.vim,
            pretty_powers: self.pretty,
            appearance: self.ctx.options(|o| o.theme_preference),
            accent: self.accent.clone(),
            zoom: self.ctx.zoom_factor(),
        }
    }

    /// Save the settings when one changed since the last save.
    pub(crate) fn persist_settings(&mut self) {
        let now = self.current_settings();
        if now != self.saved {
            if let Some(dir) = &self.config
                && let Err(e) = now.save(dir)
            {
                self.status = format!("Could not save settings: {e}");
            }
            self.saved = now;
        }
    }

    pub(crate) fn with_context(ctx: egui::Context, path: Option<PathBuf>) -> Self {
        // a folder on the command line opens the sidebar instead of a file
        let (path, folder) = match path {
            Some(p) if p.is_dir() => (None, Some(p)),
            other => (other, None),
        };
        fonts::install(&ctx);
        let (dark, light) = (Theme::dark(), Theme::light());
        dark.apply(&ctx);
        light.apply(&ctx);
        configure_style(&ctx);
        let (nb, status) = match &path {
            Some(p) => match std::fs::read_to_string(p) {
                Ok(t) => (Notebook::from_text(&t), format!("Opened {}", file_name(p))),
                Err(e) => (Notebook::from_text(WELCOME), format!("Could not read {}: {e}", file_name(p))),
            },
            None if folder.is_some() => (Notebook::from_text("# %%\n"), String::new()),
            None => (Notebook::from_text(WELCOME), String::new()),
        };
        let cur = Doc::new(0, &ctx, nb, path);
        let tree = folder.as_deref().map(workspace::scan).unwrap_or_default();
        let show_sidebar = folder.is_some();
        App {
            order: vec![cur.id],
            cur,
            others: Vec::new(),
            next_id: 1,
            ctx,
            show_help: false,
            show_symbols: false,
            find: None,
            changes: None,
            show_actions: true,
            help_filter: String::new(),
            status,
            last_title: String::new(),
            folder,
            tree,
            git: None,
            git_pending: None,
            git_checked: None,
            collapsed: HashSet::new(),
            show_sidebar,
            vim: std::env::var_os("MATHCRAFT_VIM").is_some_and(|v| v == "1"),
            pretty: true,
            vim_status: String::new(),
            palette: PaletteState::default(),
            last_cell: None,
            unified_title_bar: false,
            native_menu: None,
            toolbar_height: 40.0,
            dark,
            light,
            config: None,
            saved: settings::Settings::default(),
            accent: None,
            recent: Vec::new(),
            saved_session: session::Session::default(),
            stored_at: 0.0,
        }
    }
}
