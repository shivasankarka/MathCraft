//! State, session and settings (methods of `App`).

use super::*;

impl App {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let path_given = path.is_some();
        let mut app = Self::with_context(cc.egui_ctx.clone(), path);
        app.unified_title_bar = cfg!(target_os = "macos");
        app.native_menu = native_menu::NativeMenu::install(&cc.egui_ctx);
        app.load_settings(settings::config_dir());
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
        self.saved_session = self.current_session();
    }

    pub(crate) fn restore_session(&mut self, s: &session::Session) {
        if let Some(f) = s.folder.as_ref().filter(|f| f.is_dir()) {
            self.set_folder(f.clone());
            self.show_sidebar = s.sidebar;
            self.status.clear();
        }
        let files: Vec<&PathBuf> = s.files.iter().filter(|p| p.is_file()).collect();
        let Some((first, rest)) = files.split_first() else { return };
        // The welcome notebook gives way to the first restored file.
        match std::fs::read_to_string(first) {
            Ok(t) => self.replace_notebook(Notebook::from_text(&t), Some((*first).clone())),
            Err(_) => return,
        }
        for p in rest {
            self.open_path((*p).clone());
        }
        if let Some(active) = s.files.get(s.active).and_then(|p| self.id_of(p)) {
            self.select_tab(active);
        }
        self.status = format!("Restored {} {}", files.len(), if files.len() == 1 { "tab" } else { "tabs" });
    }

    pub(crate) fn id_of(&self, p: &Path) -> Option<u64> {
        std::iter::once(&self.cur).chain(&self.others).find(|d| d.path.as_deref() == Some(p)).map(|d| d.id)
    }

    pub(crate) fn current_session(&self) -> session::Session {
        let files: Vec<PathBuf> = self.order.iter().filter_map(|id| self.doc(*id)?.path.clone()).collect();
        let active = self.cur.path.as_ref().and_then(|p| files.iter().position(|f| f == p)).unwrap_or(0);
        session::Session { files, active, folder: self.folder.clone(), sidebar: self.show_sidebar, recent: self.recent.clone() }
    }

    /// Save the session when it changed since the last save.
    pub(crate) fn persist_session(&mut self) {
        let now = self.current_session();
        if now != self.saved_session {
            if let Some(dir) = &self.config
                && let Err(e) = now.save(dir)
            {
                self.status = format!("Could not save session: {e}");
            }
            self.saved_session = now;
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
        }
    }
}
