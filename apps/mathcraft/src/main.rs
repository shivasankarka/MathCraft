//! MathCraft desktop app: a reactive symbolic-math notebook (egui).
//!
//! The UI is a thin client: the notebook model, dependency analysis and scheduling live in `mathcraft-notebook`,
//! and computation runs on its kernel thread, so the window never blocks.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod complete;
mod doc;
mod fonts;
mod highlight;
mod icons;
mod markdown;
mod math_view;
mod palette;
mod plot_export;
mod session;
mod settings;
mod sidebar;
mod slider;
mod tabs;
mod text_view;
mod theme;
mod vim_edit;
mod workspace;

use doc::{Doc, View, file_name};
use eframe::egui::{self, Color32, Key, Margin, Modifiers, RichText, Stroke, TextStyle, ThemePreference, vec2};
use egui::text_selection::CCursorRange;
use icons::Icon;
use math_view::MathCache;
use mathcraft_eval::{DOCS, Value};
use mathcraft_notebook::{Cell, CellId, CellKind, CellState, Notebook, RunMode, RunRequest};
use palette::{Item, PaletteState};
use sidebar::{OpenFile, SideAction};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tabs::{TabAction, TabInfo};
use theme::Theme;

const WELCOME: &str = include_str!("welcome.py");
/// The shimaenaga mark (`assets/icon/mathcraft.svg`) rendered at 256 px, for the window and Dock icon.
const APP_ICON: &[u8] = include_bytes!("../../../assets/icon/mathcraft-256.png");

fn main() -> eframe::Result {
    let path = std::env::args().nth(1).map(PathBuf::from);
    let mut window = egui::ViewportBuilder::default().with_inner_size([1060.0, 840.0]).with_min_inner_size([520.0, 380.0]).with_title("MathCraft");
    if let Ok(icon) = eframe::icon_data::from_png_bytes(APP_ICON) {
        window = window.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport: unified_title_bar(window), ..Default::default() };
    eframe::run_native("MathCraft", options, Box::new(move |cc| Ok(Box::new(App::new(cc, path)))))
}

/// On macOS the toolbar doubles as the title bar: the content runs under a transparent title bar, the window buttons
/// sit inside the toolbar, and the toolbar's empty space drags the window (see [`App::toolbar`]).
fn unified_title_bar(builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
    if cfg!(target_os = "macos") { builder.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false) } else { builder }
}

/// What a command-palette entry does.
enum Cmd {
    New,
    Open,
    OpenFolder,
    Save,
    SaveAs,
    CloseTab,
    RunAll,
    RunStale,
    Interrupt,
    UndoCell,
    RedoCell,
    Mode(RunMode),
    View(View),
    ToggleSidebar,
    Functions,
    Theme(ThemePreference),
    ToggleVim,
    TogglePretty,
    Accent(Option<&'static str>),
    ReloadTheme,
    ToggleStrict,
    AddCell(CellKind),
    NewInFolder,
    Tab(u64),
    File(PathBuf),
    Help(String),
}

enum Action {
    SetSource(CellId, String),
    Run(CellId),
    /// Run, then move to the next cell (adding one at the end).
    RunAndAdvance(CellId),
    AddAfter(Option<CellId>, CellKind),
    Delete(CellId),
    Move(CellId, isize),
    /// One step of a cell being dragged; the first one saves the undo point.
    Shift(CellId, isize, bool),
    /// Say something in the status bar.
    Status(String),
    /// A Vim `:` command line (`w`, `q`, `wq`, ...).
    Command(String),
}

struct App {
    /// The document being shown; the others wait in `others` and swap in when their tab is selected.
    cur: Doc,
    others: Vec<Doc>,
    /// Tab order, by document id (includes `cur`).
    order: Vec<u64>,
    next_id: u64,
    ctx: egui::Context,
    show_help: bool,
    help_filter: String,
    status: String,
    last_title: String,
    /// The opened folder and its notebooks (sidebar).
    folder: Option<PathBuf>,
    tree: Vec<workspace::Node>,
    collapsed: HashSet<PathBuf>,
    show_sidebar: bool,
    /// Vim keys in editors (Settings), and the status text of the focused editor ("NORMAL", ":wq", ...).
    vim: bool,
    pretty: bool,
    vim_status: String,
    palette: PaletteState,
    /// The code cell whose editor had the keyboard last (where the palette inserts a function).
    last_cell: Option<CellId>,
    /// The toolbar is also the window's title bar (macOS, real window only): leave room for the window buttons.
    unified_title_bar: bool,
    /// Height of the toolbar last frame, to centre the window buttons in it.
    toolbar_height: f32,
    dark: Theme,
    light: Theme,
    /// Where settings are saved (`None` in tests), what was last saved, and the accent colour in use.
    config: Option<PathBuf>,
    saved: settings::Settings,
    accent: Option<String>,
    /// Recently opened files (newest first) and the session as last written to disk.
    recent: Vec<PathBuf>,
    saved_session: session::Session,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let path_given = path.is_some();
        let mut app = Self::with_context(cc.egui_ctx.clone(), path);
        app.unified_title_bar = cfg!(target_os = "macos");
        app.load_settings(settings::config_dir());
        app.load_session(path_given);
        app
    }

    /// Bring back the last session's tabs and folder (unless a file or folder was named on the command line), and
    /// the recent files.
    fn load_session(&mut self, path_given: bool) {
        let Some(dir) = self.config.clone() else { return };
        let s = session::Session::load(&dir);
        self.recent = s.recent.clone();
        if !path_given {
            self.restore_session(&s);
        }
        self.saved_session = self.current_session();
    }

    fn restore_session(&mut self, s: &session::Session) {
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

    fn id_of(&self, p: &Path) -> Option<u64> {
        std::iter::once(&self.cur).chain(&self.others).find(|d| d.path.as_deref() == Some(p)).map(|d| d.id)
    }

    fn current_session(&self) -> session::Session {
        let files: Vec<PathBuf> = self.order.iter().filter_map(|id| self.doc(*id)?.path.clone()).collect();
        let active = self.cur.path.as_ref().and_then(|p| files.iter().position(|f| f == p)).unwrap_or(0);
        session::Session { files, active, folder: self.folder.clone(), sidebar: self.show_sidebar, recent: self.recent.clone() }
    }

    /// Save the session when it changed since the last save.
    fn persist_session(&mut self) {
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

    fn remember(&mut self, p: &Path) {
        let mut s = session::Session { recent: std::mem::take(&mut self.recent), ..Default::default() };
        s.remember(p);
        self.recent = s.recent;
    }

    /// Adopt the saved settings and start saving changes to `dir`.
    fn load_settings(&mut self, dir: Option<PathBuf>) {
        let s = dir.as_deref().map(settings::Settings::load).unwrap_or_default();
        self.vim = self.vim || s.vim;
        self.pretty = s.pretty_powers;
        self.ctx.set_theme(s.appearance);
        self.accent = s.accent.clone();
        self.config = dir;
        self.rebuild_themes();
        self.saved = self.current_settings();
    }

    /// The themes again from the config folder's `theme.json` and the chosen accent.
    fn rebuild_themes(&mut self) {
        (self.dark, self.light) = settings::themes(self.config.as_deref(), self.accent.as_deref());
        self.dark.apply(&self.ctx);
        self.light.apply(&self.ctx);
        configure_style(&self.ctx);
    }

    fn current_settings(&self) -> settings::Settings {
        settings::Settings { vim: self.vim, pretty_powers: self.pretty, appearance: self.ctx.options(|o| o.theme_preference), accent: self.accent.clone() }
    }

    /// Save the settings when one changed since the last save.
    fn persist_settings(&mut self) {
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

    fn with_context(ctx: egui::Context, path: Option<PathBuf>) -> Self {
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
            help_filter: String::new(),
            status,
            last_title: String::new(),
            folder,
            tree,
            collapsed: HashSet::new(),
            show_sidebar,
            vim: std::env::var_os("MATHCRAFT_VIM").is_some_and(|v| v == "1"),
            pretty: true,
            vim_status: String::new(),
            palette: PaletteState::default(),
            last_cell: None,
            unified_title_bar: false,
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

    fn run(&mut self, req: RunRequest) {
        self.cur.run(req);
    }

    fn poll_kernel(&mut self) {
        self.cur.poll();
        for d in &mut self.others {
            d.poll();
        }
    }

    fn busy(&self) -> bool {
        self.cur.busy() || self.others.iter().any(Doc::busy)
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Replace the current document's content (a new kernel: the old one's variables belong to the old notebook).
    fn replace_notebook(&mut self, nb: Notebook, path: Option<PathBuf>) {
        self.cur = Doc::new(self.cur.id, &self.ctx, nb, path);
    }

    /// Show a notebook in a new tab, or in the current one when that is still untouched and untitled.
    fn open_in_tab(&mut self, nb: Notebook, path: Option<PathBuf>) {
        if self.cur.is_pristine() {
            self.replace_notebook(nb, path);
            return;
        }
        let id = self.fresh_id();
        let old = std::mem::replace(&mut self.cur, Doc::new(id, &self.ctx, nb, path));
        self.others.push(old);
        self.order.push(id);
    }

    fn select_tab(&mut self, id: u64) {
        if id == self.cur.id {
            return;
        }
        if let Some(slot) = self.others.iter_mut().find(|d| d.id == id) {
            std::mem::swap(&mut self.cur, slot);
        }
    }

    fn step_tab(&mut self, delta: isize) {
        let n = self.order.len() as isize;
        let here = self.order.iter().position(|id| *id == self.cur.id).unwrap_or(0) as isize;
        if let Some(id) = self.order.get((here + delta).rem_euclid(n.max(1)) as usize).copied() {
            self.select_tab(id);
        }
    }

    /// Put a tab at index `to` in the strip.
    fn move_tab(&mut self, id: u64, to: usize) {
        if let Some(from) = self.order.iter().position(|o| *o == id) {
            let tab = self.order.remove(from);
            self.order.insert(to.min(self.order.len()), tab);
        }
    }

    fn doc(&self, id: u64) -> Option<&Doc> {
        if self.cur.id == id { Some(&self.cur) } else { self.others.iter().find(|d| d.id == id) }
    }

    fn any_dirty(&self) -> bool {
        self.cur.dirty || self.others.iter().any(|d| d.dirty)
    }

    fn confirm(title: &str, description: &str) -> bool {
        rfd::MessageDialog::new().set_title(title).set_description(description).set_buttons(rfd::MessageButtons::YesNo).show() == rfd::MessageDialogResult::Yes
    }

    fn close_tab(&mut self, id: u64) {
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

    fn new_file(&mut self) {
        let mut nb = Notebook::new();
        nb.add_cell(None, CellKind::Code, "");
        self.open_in_tab(nb, None);
        self.status = "New notebook".into();
    }

    fn open_file(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("MathCraft notebook or script", &["py", "txt"]).pick_file() else { return };
        self.open_path(p);
    }

    /// Open a file in a tab, or switch to its tab when it is already open.
    fn open_path(&mut self, p: PathBuf) {
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

    /// Everything the command palette offers right now.
    fn commands(&self, ctx: &egui::Context) -> Vec<Item<Cmd>> {
        let key = |m: Modifiers, k: Key| Self::shortcut(ctx, m, k);
        let item = |group: &'static str, title: &str, detail: String, cmd: Cmd| Item { title: title.to_owned(), detail, group, payload: cmd };
        let on = |b: bool| if b { "on" } else { "off" };
        let mut v = vec![
            item("File", "New notebook", key(Modifiers::COMMAND, Key::N), Cmd::New),
            item("File", "Open file…", key(Modifiers::COMMAND, Key::O), Cmd::Open),
            item("File", "Open folder…", String::new(), Cmd::OpenFolder),
            item("File", "Save", key(Modifiers::COMMAND, Key::S), Cmd::Save),
            item("File", "Save as…", String::new(), Cmd::SaveAs),
            item("File", "Close tab", key(Modifiers::COMMAND, Key::W), Cmd::CloseTab),
            item("Run", "Run all cells", key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter), Cmd::RunAll),
            item("Edit", "Undo cell change (add, delete, move)", key(Modifiers::COMMAND, Key::Z), Cmd::UndoCell),
            item("Edit", "Redo cell change", key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z), Cmd::RedoCell),
            item("Run", "Run stale cells", String::new(), Cmd::RunStale),
            item("Run", "Interrupt the computation", key(Modifiers::COMMAND, Key::Period), Cmd::Interrupt),
            item("Run", "Dependents run automatically", String::new(), Cmd::Mode(RunMode::Auto)),
            item("Run", "Dependents only turn stale (lazy)", String::new(), Cmd::Mode(RunMode::Lazy)),
            item("View", "Notebook view", String::new(), Cmd::View(View::Notebook)),
            item("View", "Text view", key(Modifiers::COMMAND | Modifiers::SHIFT, Key::T), Cmd::View(View::Text)),
            item("View", "Show or hide the sidebar", key(Modifiers::COMMAND, Key::B), Cmd::ToggleSidebar),
            item("View", "Browse all functions", "F1".to_owned(), Cmd::Functions),
            item("Theme", "Match the system", String::new(), Cmd::Theme(ThemePreference::System)),
            item("Theme", "Light", String::new(), Cmd::Theme(ThemePreference::Light)),
            item("Theme", "Dark", String::new(), Cmd::Theme(ThemePreference::Dark)),
            item("Accent colour", "Blue (default)", String::new(), Cmd::Accent(None)),
            item("Settings", &format!("Powers as superscripts in cells ({})", on(self.pretty)), String::new(), Cmd::TogglePretty),
            item("Settings", &format!("Vim keys ({})", on(self.vim)), String::new(), Cmd::ToggleVim),
            item("Settings", &format!("Strict names ({})", on(self.cur.nb.strict)), String::new(), Cmd::ToggleStrict),
            item("Cell", "Add a code cell", String::new(), Cmd::AddCell(CellKind::Code)),
            item("Cell", "Add a text cell", String::new(), Cmd::AddCell(CellKind::Markdown)),
        ];
        for (name, ..) in settings::ACCENTS.iter().skip(1) {
            v.push(item("Accent colour", &format!("Accent: {name}"), String::new(), Cmd::Accent(Some(name))));
        }
        v.push(item("Theme", "Reload theme.json", String::new(), Cmd::ReloadTheme));
        for p in self.recent.iter().filter(|p| p.is_file()) {
            v.push(item("Recent", &file_name(p), p.parent().map(|d| d.display().to_string()).unwrap_or_default(), Cmd::File(p.clone())));
        }
        if self.folder.is_some() {
            v.push(item("File", "New notebook in the folder", String::new(), Cmd::NewInFolder));
        }
        for id in &self.order {
            if let Some(d) = self.doc(*id).filter(|d| d.id != self.cur.id) {
                v.push(item("Tab", &format!("Go to {}", d.name()), String::new(), Cmd::Tab(d.id)));
            }
        }
        fn files(nodes: &[workspace::Node], out: &mut Vec<PathBuf>) {
            for n in nodes {
                match n {
                    workspace::Node::File { path, .. } => out.push(path.clone()),
                    workspace::Node::Dir { children, .. } => files(children, out),
                }
            }
        }
        let mut paths = Vec::new();
        files(&self.tree, &mut paths);
        for p in paths {
            let rel = self.folder.as_deref().and_then(|f| p.strip_prefix(f).ok()).unwrap_or(&p).display().to_string();
            v.push(item("Open", &rel, String::new(), Cmd::File(p)));
        }
        for (name, sig, _) in DOCS {
            v.push(item("Function", sig, "⇧↵ insert".to_owned(), Cmd::Help((*name).to_owned())));
        }
        v
    }

    fn run_command(&mut self, cmd: Cmd, ctx: &egui::Context) {
        match cmd {
            Cmd::New => self.new_file(),
            Cmd::Open => self.open_file(),
            Cmd::OpenFolder => self.open_folder(),
            Cmd::Save => self.save(false),
            Cmd::SaveAs => self.save(true),
            Cmd::CloseTab => self.close_tab(self.cur.id),
            Cmd::RunAll => self.run(RunRequest::All),
            Cmd::RunStale => self.run(RunRequest::Stale),
            Cmd::Interrupt => self.cur.kernel.interrupt(),
            Cmd::UndoCell => self.undo_cells(),
            Cmd::RedoCell => self.redo_cells(),
            Cmd::Mode(m) => self.cur.nb.mode = m,
            Cmd::View(v) => self.cur.set_view(v),
            Cmd::ToggleSidebar => self.toggle_sidebar(),
            Cmd::Functions => self.show_help = true,
            Cmd::Theme(t) => ctx.set_theme(t),
            Cmd::ToggleVim => self.vim = !self.vim,
            Cmd::TogglePretty => self.pretty = !self.pretty,
            Cmd::Accent(name) => {
                self.accent = name.map(str::to_owned);
                self.rebuild_themes();
            }
            Cmd::ReloadTheme => {
                self.rebuild_themes();
                self.status = "Reloaded theme.json".into();
            }
            Cmd::ToggleStrict => self.cur.nb.strict = !self.cur.nb.strict,
            Cmd::AddCell(kind) => {
                let last = self.cur.nb.cells().last().map(|c| c.id);
                self.apply(vec![Action::AddAfter(last, kind)]);
            }
            Cmd::NewInFolder => self.new_in_folder(),
            Cmd::Tab(id) => self.select_tab(id),
            Cmd::File(p) => self.open_path(p),
            Cmd::Help(name) => {
                self.help_filter = name;
                self.show_help = true;
            }
        }
    }

    /// Put `name(` into the code cell that was being edited, at its cursor.
    fn insert_function(&mut self, name: &str, ctx: &egui::Context) {
        let target = self.last_cell.and_then(|id| self.cur.nb.cell(id)).filter(|c| c.kind == CellKind::Code);
        let Some(cell) = target else {
            self.status = "Click into a code cell first, then insert a function".into();
            return;
        };
        let (id, mut text) = (cell.id, cell.source.clone());
        if !insert_at_cursor(ctx, editor_id(id), &mut text, &format!("{name}(")) {
            text.push_str(&format!("{name}("));
        }
        self.apply(vec![Action::SetSource(id, text)]);
        self.cur.focus = Some(id);
        self.status = format!("Inserted {name}(");
    }

    /// A Vim `:` command.
    fn ex_command(&mut self, line: &str) {
        match line {
            "w" => self.save(false),
            "wq" | "x" => {
                self.save(false);
                if !self.cur.dirty {
                    self.close_tab(self.cur.id);
                }
            }
            "q" => self.close_tab(self.cur.id),
            "q!" => {
                self.cur.dirty = false;
                self.close_tab(self.cur.id);
            }
            "run" | "r" => self.run(RunRequest::All),
            "" => {}
            other => self.status = format!("Not a command: {other}  (try :w, :q, :wq, :q!, :run)"),
        }
    }

    fn open_folder(&mut self) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.set_folder(dir);
        }
    }

    fn set_folder(&mut self, dir: PathBuf) {
        self.tree = workspace::scan(&dir);
        self.status = format!("Opened folder {}", file_name(&dir));
        self.folder = Some(dir);
        self.collapsed.clear();
        self.show_sidebar = true;
    }

    fn rescan(&mut self) {
        if let Some(dir) = &self.folder {
            self.tree = workspace::scan(dir);
        }
    }

    fn toggle_sidebar(&mut self) {
        if self.folder.is_none() {
            self.open_folder();
        } else {
            self.show_sidebar = !self.show_sidebar;
        }
    }

    /// Create an empty notebook file in the opened folder and open it.
    fn new_in_folder(&mut self) {
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

    fn save(&mut self, as_new: bool) {
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
                self.rescan();
            }
            Err(e) => self.status = format!("Could not save: {e}"),
        }
    }

    fn undo_cells(&mut self) {
        self.status = if self.cur.undo_structure() { "Undid the last cell change".into() } else { "Nothing to undo".into() };
    }

    fn redo_cells(&mut self) {
        self.status = if self.cur.redo_structure() { "Redid the cell change".into() } else { "Nothing to redo".into() };
    }

    fn apply(&mut self, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::SetSource(id, text) => {
                    self.cur.nb.set_source(id, &text);
                    self.cur.dirty = true;
                }
                Action::Run(id) => self.run(RunRequest::Cell(id)),
                Action::RunAndAdvance(id) => {
                    self.run(RunRequest::Cell(id));
                    let next = self.cur.nb.index_of(id).and_then(|i| self.cur.nb.cells().get(i + 1)).map(|c| c.id);
                    let target = match next {
                        Some(n) => n,
                        None => {
                            self.cur.checkpoint();
                            self.cur.dirty = true;
                            self.cur.nb.add_cell(Some(id), CellKind::Code, "")
                        }
                    };
                    self.cur.focus = Some(target);
                }
                Action::AddAfter(after, kind) => {
                    self.cur.checkpoint();
                    let id = self.cur.nb.add_cell(after, kind, "");
                    if kind == CellKind::Markdown {
                        self.cur.editing_md.insert(id);
                    }
                    self.cur.focus = Some(id);
                    self.cur.dirty = true;
                }
                Action::Delete(id) => {
                    self.cur.checkpoint();
                    self.status = "Deleted a cell. Cmd+Z undoes it".into();
                    self.cur.nb.delete_cell(id);
                    self.cur.kernel.forget_cell(id);
                    self.cur.dirty = true;
                    if self.cur.nb.cells().is_empty() {
                        self.cur.nb.add_cell(None, CellKind::Code, "");
                    }
                }
                Action::Move(id, delta) => {
                    self.cur.checkpoint();
                    self.cur.nb.move_cell(id, delta);
                    self.cur.dirty = true;
                }
                Action::Shift(id, delta, first) => {
                    if first {
                        self.cur.checkpoint();
                    }
                    self.cur.nb.move_cell(id, delta);
                    self.cur.dirty = true;
                }
                Action::Status(text) => self.status = text,
                Action::Command(line) => self.ex_command(line.trim()),
            }
        }
    }

    /// Move the keyboard to the editor of the previous (-1) or next (1) code cell.
    fn focus_neighbor(&mut self, ctx: &egui::Context, step: isize) {
        let cells = self.cur.nb.cells();
        let here = cells.iter().position(|c| ctx.memory(|m| m.has_focus(editor_id(c.id)))).or_else(|| self.last_cell.and_then(|id| self.cur.nb.index_of(id)));
        let order: Vec<usize> = match (here, step > 0) {
            (Some(i), true) => (i + 1..cells.len()).collect(),
            (Some(i), false) => (0..i).rev().collect(),
            (None, true) => (0..cells.len()).collect(),
            (None, false) => (0..cells.len()).rev().collect(),
        };
        if let Some(c) = order.into_iter().filter_map(|i| cells.get(i)).find(|c| c.kind == CellKind::Code) {
            self.cur.focus = Some(c.id);
            self.last_cell = Some(c.id);
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let (save, open, new, run_all, interrupt, help, close, next, prev, side, text_view, palette) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::S),
                i.consume_key(Modifiers::COMMAND, Key::O),
                i.consume_key(Modifiers::COMMAND, Key::N),
                i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter),
                i.consume_key(Modifiers::COMMAND, Key::Period),
                i.consume_key(Modifiers::NONE, Key::F1),
                i.consume_key(Modifiers::COMMAND, Key::W),
                i.consume_key(Modifiers::CTRL, Key::Tab),
                i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab),
                i.consume_key(Modifiers::COMMAND, Key::B),
                i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::T),
                i.consume_key(Modifiers::COMMAND, Key::P) || i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::P),
            )
        });
        // Cell-level undo only when no text editor has the keyboard (the editor's own undo handles typing).
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)) {
                self.redo_cells();
            } else if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Z)) {
                self.undo_cells();
            }
        }
        let (prev_cell, next_cell) = ctx.input_mut(|i| {
            (i.consume_key(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowUp), i.consume_key(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowDown))
        });
        if prev_cell || next_cell {
            self.focus_neighbor(ctx, if next_cell { 1 } else { -1 });
        }
        if save {
            self.save(false);
        }
        if open {
            self.open_file();
        }
        if new {
            self.new_file();
        }
        if run_all {
            self.run(RunRequest::All);
        }
        if interrupt {
            self.cur.kernel.interrupt();
        }
        if help {
            self.show_help = !self.show_help;
        }
        if palette {
            if self.palette.open {
                self.palette.open = false;
            } else {
                self.palette.show();
            }
        }
        if side {
            self.toggle_sidebar();
        }
        if text_view {
            let to = if self.cur.view == View::Text { View::Notebook } else { View::Text };
            self.cur.set_view(to);
        }
        if close {
            self.close_tab(self.cur.id);
        }
        if next {
            self.step_tab(1);
        }
        if prev {
            self.step_tab(-1);
        }
    }

    fn shortcut(ctx: &egui::Context, mods: Modifiers, key: Key) -> String {
        ctx.format_shortcut(&egui::KeyboardShortcut::new(mods, key))
    }

    fn menu_item(ui: &mut egui::Ui, theme: &Theme, ctx: &egui::Context, label: &str, keys: Option<(Modifiers, Key)>) -> bool {
        let shortcut = keys.map(|(m, k)| Self::shortcut(ctx, m, k));
        let clicked = menu_row(ui, theme, label, shortcut.as_deref()).clicked();
        if clicked {
            ui.close();
        }
        clicked
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let ctx = ui.ctx().clone();
        if self.unified_title_bar {
            // The empty part of the toolbar behaves like a title bar. Registered first, so every control on top wins.
            let bar = ui.interact(ui.max_rect().expand2(vec2(12.0, 7.0)), egui::Id::unique("toolbar-drag"), egui::Sense::click_and_drag());
            if bar.double_clicked() {
                let zoomed = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!zoomed));
            } else if bar.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }
        // roomier, rounder menu titles than egui's default
        ui.spacing_mut().button_padding = vec2(9.0, 4.0);
        ui.spacing_mut().item_spacing.x = 6.0;
        egui::MenuBar::new().ui(ui, |ui| {
            if self.unified_title_bar {
                ui.add_space(theme::TRAFFIC_LIGHTS_INSET);
            }
            ui.menu_button("File", |ui| {
                ui.spacing_mut().button_padding = vec2(12.0, 5.0);
                if Self::menu_item(ui, theme, &ctx, "New", Some((Modifiers::COMMAND, Key::N))) {
                    self.new_file();
                }
                if Self::menu_item(ui, theme, &ctx, "Open…", Some((Modifiers::COMMAND, Key::O))) {
                    self.open_file();
                }
                if Self::menu_item(ui, theme, &ctx, "Open Folder…", None) {
                    self.open_folder();
                }
                if Self::menu_item(ui, theme, &ctx, "Show or hide sidebar", Some((Modifiers::COMMAND, Key::B))) {
                    self.toggle_sidebar();
                }
                ui.separator();
                if Self::menu_item(ui, theme, &ctx, "Close tab", Some((Modifiers::COMMAND, Key::W))) {
                    self.close_tab(self.cur.id);
                }
                ui.separator();
                if Self::menu_item(ui, theme, &ctx, "Save", Some((Modifiers::COMMAND, Key::S))) {
                    self.save(false);
                }
                if Self::menu_item(ui, theme, &ctx, "Save As…", None) {
                    self.save(true);
                }
                let recent: Vec<PathBuf> = self.recent.iter().filter(|p| p.is_file()).cloned().collect();
                ui.add_enabled_ui(!recent.is_empty(), |ui| {
                    ui.menu_button("Open recent", |ui| {
                        for p in &recent {
                            if menu_row(ui, theme, &file_name(p), None).on_hover_text(p.display().to_string()).clicked() {
                                self.open_path(p.clone());
                                ui.close();
                            }
                        }
                    });
                });
            });
            ui.menu_button("Edit", |ui| {
                ui.add_enabled_ui(self.cur.can_undo(), |ui| {
                    if Self::menu_item(ui, theme, &ctx, "Undo cell change", Some((Modifiers::COMMAND, Key::Z))) {
                        self.undo_cells();
                    }
                });
                ui.add_enabled_ui(self.cur.can_redo(), |ui| {
                    if Self::menu_item(ui, theme, &ctx, "Redo cell change", Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::Z))) {
                        self.redo_cells();
                    }
                });
            });
            ui.menu_button("Run", |ui| {
                if Self::menu_item(ui, theme, &ctx, "Run all", Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter))) {
                    self.run(RunRequest::All);
                }
                if Self::menu_item(ui, theme, &ctx, "Run stale cells", None) {
                    self.run(RunRequest::Stale);
                }
                if Self::menu_item(ui, theme, &ctx, "Interrupt", Some((Modifiers::COMMAND, Key::Period))) {
                    self.cur.kernel.interrupt();
                }
            });
            ui.menu_button("View", |ui| {
                let mut view = self.cur.view;
                for (v, label) in [(View::Notebook, "Notebook: one block per cell"), (View::Text, "Text: the whole file in one editor")] {
                    if choice(ui, theme, label, view == v) {
                        view = v;
                    }
                }
                if view != self.cur.view {
                    self.cur.set_view(view);
                    ui.close();
                }
            });
            if ui.button("Commands").on_hover_text("Search every command (⌘P)").clicked() {
                self.palette.show();
            }
            ui.menu_button("Settings", |ui| {
                menu_section(ui, theme, &format!("Appearance (now {})", theme.name));
                let mut pref = ctx.options(|o| o.theme_preference);
                for (v, label) in [(ThemePreference::System, "Match system"), (ThemePreference::Light, "Light"), (ThemePreference::Dark, "Dark")] {
                    if choice(ui, theme, label, pref == v) {
                        pref = v;
                    }
                }
                ctx.set_theme(pref);
                menu_section(ui, theme, "Accent");
                if let Some(name) = accent_row(ui, theme, self.accent.as_deref()) {
                    self.accent = (name != "blue").then(|| name.to_owned());
                    self.rebuild_themes();
                }
                ui.separator();
                menu_section(ui, theme, "When a cell runs");
                for (m, label) in [(RunMode::Auto, "Also run the cells that depend on it"), (RunMode::Lazy, "Only mark dependent cells as stale")] {
                    if choice(ui, theme, label, self.cur.nb.mode == m) {
                        self.cur.nb.mode = m;
                    }
                }
                ui.separator();
                menu_section(ui, theme, "Names");
                if choice(ui, theme, "Strict: declare symbols with symbols()", self.cur.nb.strict) {
                    self.cur.nb.strict = !self.cur.nb.strict;
                }
                ui.separator();
                menu_section(ui, theme, "Editing");
                if choice(ui, theme, "Vim keys", self.vim) {
                    self.vim = !self.vim;
                }
                if choice(ui, theme, "Show powers as superscripts in cells", self.pretty) {
                    self.pretty = !self.pretty;
                }
            });
            if ui.button("Functions").on_hover_text("List every function (F1)").clicked() {
                self.show_help = !self.show_help;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if self.busy() {
                    if icons::primary_button(ui, theme, Icon::Stop, "Stop", "Stop the running computation (⌘.)").clicked() {
                        self.cur.kernel.interrupt();
                    }
                    ui.add(egui::Spinner::new().size(13.0).color(theme.text_faint));
                } else if icons::primary_button(ui, theme, Icon::PlayAll, "Run all", "Run every cell (⌘⇧↵)").clicked() {
                    self.run(RunRequest::All);
                }
                let pref = ctx.options(|o| o.theme_preference);
                let (next, label) = match pref {
                    ThemePreference::System => (ThemePreference::Light, "Appearance: match system. Click for Light"),
                    ThemePreference::Light => (ThemePreference::Dark, "Appearance: Light. Click for Dark"),
                    ThemePreference::Dark => (ThemePreference::System, "Appearance: Dark. Click to match the system"),
                };
                if icons::button(ui, theme, Icon::Contrast, label).clicked() {
                    ctx.set_theme(next);
                }
                // the file name gives way when the window is too narrow to hold it beside the menus
                if ui.available_width() > 190.0 {
                    vertical_rule(ui, theme);
                    document_title(ui, theme, self.cur.path.as_ref(), self.cur.dirty);
                }
            });
        });
    }

    fn help_window(&mut self, ctx: &egui::Context, theme: &Theme) {
        if !self.show_help {
            return;
        }
        let mut open = true;
        // Our own header instead of egui's title bar, so the whole window is one surface (no second grey band).
        egui::Window::new("Functions")
            .title_bar(false)
            .default_rect(egui::Rect::from_min_size(ctx.content_rect().center_top() + vec2(-320.0, 64.0), vec2(640.0, 500.0)))
            .max_width(720.0)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Functions").font(egui::FontId::new(theme::TEXT_TITLE, fonts::strong_family())).color(theme.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, theme, Icon::Close, "Close (F1)").clicked() {
                            open = false;
                        }
                    });
                });
                ui.add_space(6.0);
                let search = ui.add(
                    egui::TextEdit::singleline(&mut self.help_filter)
                        .hint_text(RichText::new("Search by name or description").color(theme.text_faint))
                        .margin(Margin { left: 28, right: 8, top: 5, bottom: 5 })
                        .desired_width(f32::INFINITY),
                );
                let glass = egui::Rect::from_center_size(egui::pos2(search.rect.left() + 15.0, search.rect.center().y), vec2(13.0, 13.0));
                icons::paint(ui.painter(), Icon::Search, glass, theme.text_faint);
                ui.add_space(4.0);
                ui.label(
                    RichText::new("⇧↵ runs a cell and moves on   ⌘↵ runs it in place   ⇧⌘↵ runs all   ⌘. stops")
                        .size(theme::TEXT_CAPTION)
                        .color(theme.text_faint),
                );
                ui.add_space(4.0);
                ui.separator();
                let needle = self.help_filter.trim().to_lowercase();
                let matches: Vec<_> = DOCS
                    .iter()
                    .filter(|(name, _, summary)| needle.is_empty() || name.to_lowercase().contains(&needle) || summary.to_lowercase().contains(&needle))
                    .collect();
                egui::ScrollArea::vertical().auto_shrink([true, false]).show(ui, |ui| {
                    if matches.is_empty() {
                        ui.add_space(24.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(format!("No function matches “{}”", self.help_filter.trim())).color(theme.text_dim));
                            ui.label(RichText::new("Try a shorter word, or part of a description such as “matrix”.").size(12.0).color(theme.text_faint));
                        });
                        return;
                    }
                    // One entry per function: the signature, and what it does underneath.
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (_, sig, summary) in matches {
                        ui.add_space(6.0);
                        ui.add(egui::Label::new(RichText::new(*sig).monospace().size(13.0).color(theme.syntax.function)).wrap());
                        ui.add(egui::Label::new(RichText::new(*summary).size(13.0).color(theme.text_dim)).wrap());
                    }
                    ui.add_space(6.0);
                });
            });
        self.show_help = open;
    }

    fn status_bar(&self, ui: &mut egui::Ui, theme: &Theme) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            let (fresh, stale, errors) = self.cur.nb.cells().iter().filter(|c| c.kind == CellKind::Code).fold((0, 0, 0), |(f, s, e), c| match c.state {
                CellState::Fresh => (f + 1, s, e),
                CellState::Stale | CellState::New => (f, s + 1, e),
                CellState::Error | CellState::Blocked => (f, s, e + 1),
                _ => (f, s, e),
            });
            let dim = |t: String| RichText::new(t).size(theme::TEXT_CAPTION).color(theme.text_dim);
            if self.vim && !self.vim_status.is_empty() {
                vim_segment(ui, theme, &self.vim_status);
                ui.add_space(8.0);
            }
            let cells = self.cur.nb.cells().len();
            ui.label(dim(format!("{cells} {}", if cells == 1 { "cell" } else { "cells" })));
            for (n, label, color) in [(fresh, "up to date", theme.ok), (stale, "stale", theme.warn), (errors, "with problems", theme.danger)] {
                if n > 0 {
                    ui.add_space(6.0);
                    dot(ui, color);
                    ui.label(dim(format!("{n} {label}")));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mode = if self.cur.nb.mode == RunMode::Auto { "Dependents run automatically" } else { "Dependents wait (lazy)" };
                // what is left of the bar after the counts on the left: drop the wordy parts first
                if ui.available_width() > 300.0 {
                    ui.label(dim(mode.into())).on_hover_text("Change in Settings");
                }
                if !self.status.is_empty() && ui.available_width() > 120.0 {
                    ui.add_space(10.0);
                    ui.add(egui::Label::new(RichText::new(self.status.clone()).size(11.5).color(theme.text_faint)).truncate());
                }
            });
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.show(root);
        if self.unified_title_bar {
            // AppKit moves the buttons back on every layout pass, so this runs each frame.
            frame.set_traffic_lights_position(root.ctx(), egui::Rangef::new(0.0, self.toolbar_height), 14.0);
        }
    }
}

/// Width of the notebook column.
const COLUMN: f32 = 940.0;

impl App {
    fn theme(&self, ctx: &egui::Context) -> Theme {
        if ctx.theme() == egui::Theme::Dark { self.dark.clone() } else { self.light.clone() }
    }

    /// One frame of the whole window.
    fn show(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        self.poll_kernel();
        self.persist_settings();
        self.persist_session();
        if let Some(c) = self.cur.nb.cells().iter().find(|c| ctx.memory(|m| m.has_focus(editor_id(c.id)))) {
            self.last_cell = Some(c.id);
        }
        self.shortcuts(ctx);
        let theme = self.theme(ctx);

        if ctx.input(|i| i.viewport().close_requested())
            && self.any_dirty()
            && !Self::confirm("Unsaved changes", "Some notebooks have unsaved changes. Quit and discard them?")
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

        let name = self.cur.path.as_ref().and_then(|p| p.file_name()).map_or("Untitled".to_string(), |n| n.to_string_lossy().to_string());
        let title = format!("{}{} — MathCraft", if self.cur.dirty { "• " } else { "" }, name);
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }

        let top = egui::Panel::top("toolbar")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(theme.chrome).inner_margin(Margin { left: 10, right: 12, top: 7, bottom: 7 }))
            .show(root, |ui| self.toolbar(ui, &theme));
        self.toolbar_height = top.response.rect.height();
        hairline(root, &theme, top.response.rect.x_range(), top.response.rect.bottom() - 0.5);
        if self.order.len() > 1 {
            let tabs: Vec<TabInfo> = self
                .order
                .iter()
                .filter_map(|id| self.doc(*id))
                .map(|d| TabInfo {
                    id: d.id,
                    name: d.name(),
                    dirty: d.dirty,
                    tip: d.path.as_ref().map_or_else(|| "Not saved yet".to_owned(), |p| p.display().to_string()),
                })
                .collect();
            let strip = egui::Panel::top("tabs")
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(theme.chrome))
                .show(root, |ui| tabs::tab_strip(ui, &theme, &tabs, self.cur.id));
            match strip.inner {
                Some(TabAction::Select(id)) => self.select_tab(id),
                Some(TabAction::Close(id)) => self.close_tab(id),
                Some(TabAction::New) => self.new_file(),
                Some(TabAction::Move(id, to)) => self.move_tab(id, to),
                None => {}
            }
        }
        let bottom = egui::Panel::bottom("status")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(theme.chrome).inner_margin(Margin::symmetric(14, 4)))
            .show(root, |ui| self.status_bar(ui, &theme));
        hairline(root, &theme, bottom.response.rect.x_range(), bottom.response.rect.top() + 0.5);

        if self.show_sidebar
            && let Some(folder) = self.folder.clone()
        {
            let tree = std::mem::take(&mut self.tree);
            let open: Vec<(PathBuf, bool, bool)> =
                std::iter::once(&self.cur).chain(&self.others).filter_map(|d| d.path.clone().map(|p| (p, d.dirty, d.id == self.cur.id))).collect();
            let open_files: Vec<OpenFile<'_>> = open.iter().map(|(p, dirty, active)| OpenFile { path: p, dirty: *dirty, active: *active }).collect();
            let side = egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(232.0)
                .size_range(160.0..=420.0)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(theme.chrome).inner_margin(Margin::symmetric(4, 0)))
                .show(root, |ui| sidebar::sidebar(ui, &theme, &folder, &tree, &mut self.collapsed, &open_files));
            let r = side.response.rect;
            root.painter().vline(r.right() - 0.5, r.y_range(), Stroke::new(1.0, theme.separator));
            self.tree = tree;
            match side.inner {
                Some(SideAction::Open(p)) => self.open_path(p),
                Some(SideAction::NewFile) => self.new_in_folder(),
                Some(SideAction::Refresh) => self.rescan(),
                Some(SideAction::OpenFolder) => self.open_folder(),
                None => {}
            }
        }

        let mut actions: Vec<Action> = Vec::new();
        let focus = self.cur.focus.take();
        if self.cur.view == View::Text {
            let active = text_view::active_cell(ctx, &self.cur);
            let names: Vec<String> = self.cur.nb.cells().iter().flat_map(|c| c.defs().cloned()).collect();
            // the results never squeeze the editor out: it keeps at least 240 points
            let widest = (root.available_width() - 240.0).clamp(120.0, 800.0);
            let panel = egui::Panel::right("results")
                .resizable(true)
                .default_size(400.0_f32.min(widest))
                .size_range(120.0_f32.min(widest)..=widest)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(theme.bg))
                .show(root, |ui| {
                    let busy = self.cur.busy();
                    let mut env = Env {
                        busy,
                        theme: &theme,
                        math: &mut self.cur.math,
                        generations: &self.cur.generations,
                        focus: None,
                        editing_md: &mut self.cur.editing_md,
                        names: &names,
                        actions: &mut actions,
                        vim: self.vim,
                        pretty: self.pretty,
                        vim_status: &mut self.vim_status,
                    };
                    text_view::results(ui, &mut env, &self.cur.nb, active);
                });
            let r = panel.response.rect;
            root.painter().vline(r.left() + 0.5, r.y_range(), Stroke::new(1.0, theme.separator));
            let want_focus = ctx.memory(|m| m.focused().is_none());
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme.bg)).show(root, |ui| {
                let out = text_view::editor(ui, &theme, &mut self.cur, self.vim, &mut self.vim_status, want_focus);
                if let Some(id) = out.run_cell {
                    actions.push(Action::Run(id));
                }
                actions.extend(out.commands.into_iter().map(Action::Command));
            });
        } else {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme.bg)).show(root, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let side = ((ui.available_width() - COLUMN) / 2.0).max(16.0);
                    ui.add_space(14.0);
                    ui.horizontal_top(|ui| {
                        ui.add_space(side);
                        let width = (ui.available_width() - side).clamp(240.0, COLUMN);
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let names: Vec<String> = self.cur.nb.cells().iter().flat_map(|c| c.defs().cloned()).collect();
                            let busy = self.cur.busy();
                            let mut env = Env {
                                busy,
                                theme: &theme,
                                math: &mut self.cur.math,
                                generations: &self.cur.generations,
                                focus,
                                editing_md: &mut self.cur.editing_md,
                                names: &names,
                                actions: &mut actions,
                                vim: self.vim,
                                pretty: self.pretty,
                                vim_status: &mut self.vim_status,
                            };
                            let mut rows = Vec::new();
                            for (i, cell) in self.cur.nb.cells().iter().enumerate() {
                                let r = cell_ui(ui, &mut env, cell, i, &self.cur.nb);
                                rows.push((r.top(), r.bottom()));
                                add_row(ui, &mut env, cell.id);
                            }
                            drag_cells(ui, &mut env, &rows, &self.cur.nb);
                            if let [only] = self.cur.nb.cells()
                                && only.source.trim().is_empty()
                            {
                                empty_state(ui, &theme);
                            }
                            ui.add_space(140.0);
                        });
                    });
                });
            });
        }
        self.apply(actions);
        self.help_window(ctx, &theme);
        if self.palette.open {
            let items = self.commands(ctx);
            let mut state = std::mem::take(&mut self.palette);
            let chosen = palette::show(ctx, &theme, &mut state, &items);
            self.palette = state;
            if let Some(i) = chosen
                && let Some(item) = items.into_iter().nth(i)
            {
                match item.payload {
                    Cmd::Help(name) if self.palette.alt => self.insert_function(&name, ctx),
                    other => self.run_command(other, ctx),
                }
            }
        }

        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }
}

fn configure_style(ctx: &egui::Context) {
    use egui::{FontFamily, FontId};
    ctx.all_styles_mut(|s| {
        // One type scale: 11.5 captions, 13 chrome, 15 document text, 13.5 code; headings in the markdown renderer.
        s.text_styles.insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(13.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(11.5, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Heading, FontId::new(17.0, fonts::strong_family()));
        s.text_styles.insert(TextStyle::Monospace, FontId::new(13.5, FontFamily::Monospace));
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(8.0, 4.0);
        s.spacing.interact_size.y = 24.0;
        s.spacing.menu_margin = Margin::same(5);
        s.spacing.menu_spacing = 4.0;
        s.spacing.window_margin = Margin::same(14);
        s.spacing.scroll = egui::style::ScrollStyle { bar_width: 7.0, floating_width: 3.0, ..egui::style::ScrollStyle::floating() };
    });
}

/// Everything a cell needs from the app while it is drawn.
struct Env<'a> {
    /// The kernel is working (a dragged slider then waits instead of queueing more runs).
    busy: bool,
    theme: &'a Theme,
    math: &'a mut MathCache,
    generations: &'a HashMap<CellId, u64>,
    focus: Option<CellId>,
    editing_md: &'a mut HashSet<CellId>,
    /// Names defined by the notebook's cells, offered by autocompletion.
    names: &'a [String],
    actions: &'a mut Vec<Action>,
    /// Vim keys are on; the focused editor writes its mode here for the status bar.
    vim: bool,
    /// Powers are drawn as superscripts while editing (notebook view).
    pretty: bool,
    vim_status: &'a mut String,
}

fn state_color(theme: &Theme, state: CellState) -> Color32 {
    match state {
        CellState::New => theme.text_faint,
        CellState::Fresh => theme.ok,
        CellState::Stale | CellState::Blocked => theme.warn,
        CellState::Queued | CellState::Running => theme.accent,
        CellState::Error => theme.danger,
    }
}

fn state_label(state: CellState) -> &'static str {
    match state {
        CellState::New => "not run",
        CellState::Fresh => "",
        CellState::Stale => "stale",
        CellState::Queued => "queued",
        CellState::Running => "running…",
        CellState::Error => "error",
        CellState::Blocked => "blocked",
    }
}

/// The Vim mode as a status-bar segment: a coloured pill with the mode name, then any keys typed so far
/// (`2d`, `:wq`, `/foo`) in monospace. Insert is the accent, visual the warning colour, normal and command quiet.
fn vim_segment(ui: &mut egui::Ui, theme: &Theme, status: &str) {
    let (mode, pending) = if status.starts_with(':') || status.starts_with('/') {
        ("COMMAND", status)
    } else {
        let mut parts = status.splitn(2, char::is_whitespace);
        (parts.next().unwrap_or(""), parts.next().unwrap_or("").trim())
    };
    let color = match mode {
        "INSERT" => theme.accent,
        "VISUAL" | "V-LINE" => theme.warn,
        _ => theme.text_dim,
    };
    let font = egui::FontId::new(10.5, fonts::strong_family());
    let galley = ui.painter().layout_no_wrap(mode.to_owned(), font, color);
    let (r, resp) = ui.allocate_exact_size(vec2(galley.size().x + 14.0, 17.0), egui::Sense::hover());
    ui.painter().rect_filled(r, theme::RADIUS_SMALL, theme.pill(color));
    ui.painter().galley(r.center() - galley.size() / 2.0, galley, color);
    resp.on_hover_text("Vim keys are on (Settings > Editing)");
    if !pending.is_empty() {
        ui.label(RichText::new(pending).monospace().size(theme::TEXT_CAPTION).color(theme.text));
    }
}

fn dot(ui: &mut egui::Ui, color: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), 3.5, color);
}

/// Shown under the single empty cell of a new notebook: what to type and the keys that matter.
fn empty_state(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(28.0);
    ui.horizontal(|ui| {
        ui.add_space(GUTTER + EDITOR_PAD_X);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(RichText::new("A new notebook").font(egui::FontId::new(15.0, fonts::strong_family())).color(theme.text));
            ui.label(RichText::new("Type an expression in the cell above, such as integrate(x*sin(x), x), and run it.").size(13.5).color(theme.text_dim));
            ui.add_space(4.0);
            egui::Grid::new("empty-keys").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                for (keys, what) in [
                    ("⇧↵", "Run the cell and move to the next one"),
                    ("⌘↵", "Run the cell and stay"),
                    ("⇧⌘↵", "Run every cell"),
                    ("F1", "Browse all functions"),
                ] {
                    keycap(ui, theme, keys);
                    ui.label(RichText::new(what).size(13.0).color(theme.text_dim));
                    ui.end_row();
                }
            });
        });
    });
}

/// A key or key combination drawn as a small cap.
fn keycap(ui: &mut egui::Ui, theme: &Theme, keys: &str) {
    // measure on a throwaway painter first, then paint into the allocated space, left-aligned in the column
    let width = icons::keycaps(&ui.painter().with_clip_rect(egui::Rect::NOTHING), theme, egui::Pos2::ZERO, keys);
    let (r, _) = ui.allocate_exact_size(vec2(width.max(54.0), 20.0), egui::Sense::hover());
    icons::keycaps(ui.painter(), theme, egui::pos2(r.left() + width, r.center().y), keys);
}

/// A one-pixel line across the window at `y`, between chrome and document.
fn hairline(ui: &egui::Ui, theme: &Theme, x: egui::Rangef, y: f32) {
    ui.painter().hline(x, y, Stroke::new(1.0, theme.separator));
}

/// A short vertical divider between groups of toolbar items.
fn vertical_rule(ui: &mut egui::Ui, theme: &Theme) {
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 16.0), egui::Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, theme.border_strong));
}

/// The file name in the toolbar, with "Edited" beside it while there are unsaved changes.
fn document_title(ui: &mut egui::Ui, theme: &Theme, path: Option<&PathBuf>, dirty: bool) {
    let name = path.and_then(|p| p.file_name()).map_or("Untitled".to_string(), |n| n.to_string_lossy().to_string());
    if dirty {
        ui.label(RichText::new("Edited").size(12.0).color(theme.text_faint));
    }
    let tip = path.map_or_else(|| "Not saved yet".to_string(), |p| p.display().to_string());
    ui.label(RichText::new(name).size(13.0).font(egui::FontId::new(13.0, fonts::strong_family())).color(theme.text_dim)).on_hover_text(tip);
}

/// One row of a menu: the label on the left, the shortcut as keycaps on the right, tinted with the accent under the
/// pointer and dimmed when disabled. All rows in a menu are as wide as the widest, so the highlight is a clean bar.
fn menu_row(ui: &mut egui::Ui, theme: &Theme, label: &str, shortcut: Option<&str>) -> egui::Response {
    const PAD: f32 = 12.0;
    let enabled = ui.is_enabled();
    let font = TextStyle::Button.resolve(ui.style());
    let color = if enabled { theme.text } else { theme.text_faint };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, color);
    let keys_w = shortcut.map_or(0.0, |s| icons::keycaps(&ui.painter().with_clip_rect(egui::Rect::NOTHING), theme, egui::Pos2::ZERO, s) + 28.0);
    let need = PAD + galley.size().x + keys_w + PAD;
    let size = vec2(ui.available_width().min(MENU_WIDTH).max(need), 26.0);
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        if enabled && (resp.hovered() || resp.has_focus()) {
            ui.painter().rect_filled(rect.shrink2(vec2(0.0, 1.0)), 5.0, theme.selected_row());
        }
        ui.painter().galley(egui::pos2(rect.left() + PAD, rect.center().y - galley.size().y / 2.0), galley, color);
        if let (Some(s), true) = (shortcut, enabled) {
            icons::keycaps(ui.painter(), theme, egui::pos2(rect.right() - PAD + 2.0, rect.center().y), s);
        }
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// How wide a menu is drawn (wider only when a label and its shortcut need it).
const MENU_WIDTH: f32 = 250.0;

/// A quiet heading inside a menu.
/// A menu row for a setting that is on or off, or one of several: a check mark in front of the chosen one.
/// Returns true when clicked. Does not close the menu, so several settings can be changed in one visit.
fn choice(ui: &mut egui::Ui, theme: &Theme, label: &str, selected: bool) -> bool {
    let font = TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, theme.text);
    // a popup offers the whole window as its width, so rows ask for a sensible one
    let size = vec2(ui.available_width().min(330.0).max(galley.size().x + 38.0), 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        if resp.hovered() {
            ui.painter().rect_filled(rect.shrink2(vec2(0.0, 1.0)), 5.0, theme.selected_row());
        }
        if selected {
            let c = egui::pos2(rect.left() + 14.0, rect.center().y);
            let stroke = Stroke::new(1.7, theme.accent);
            ui.painter().line_segment([c + vec2(-4.0, 0.5), c + vec2(-1.5, 3.5)], stroke);
            ui.painter().line_segment([c + vec2(-1.5, 3.5), c + vec2(4.5, -3.5)], stroke);
        }
        let at = egui::pos2(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, theme.text);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// The accent colours as a row of dots; the chosen one has a ring. Returns the name clicked.
fn accent_row(ui: &mut egui::Ui, theme: &Theme, current: Option<&str>) -> Option<&'static str> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.spacing_mut().item_spacing.x = 6.0;
        for (name, dark, light) in settings::ACCENTS {
            let [r, g, b] = if theme.dark { dark } else { light };
            let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
            let selected = current.unwrap_or("blue") == name;
            ui.painter().circle_filled(rect.center(), 7.0, Color32::from_rgb(r, g, b));
            if selected || resp.hovered() {
                ui.painter().circle_stroke(rect.center(), 10.0, Stroke::new(1.5, if selected { theme.text } else { theme.border_strong }));
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(name).clicked() {
                picked = Some(name);
            }
        }
    });
    picked
}

fn menu_section(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(title).font(egui::FontId::new(11.5, fonts::strong_family())).color(theme.text_faint));
}

/// A note under a cell: problems found before running (warning) or the error from running (error). A tinted strip
/// with a coloured rule and icon on the left; the message itself is in the normal text colour so it stays readable.
fn banner(ui: &mut egui::Ui, theme: &Theme, icon: Icon, color: Color32, code: Option<&str>, message: &str) {
    let inner = egui::Frame::new().fill(theme.tint(color)).corner_radius(theme::RADIUS).inner_margin(Margin { left: 34, right: 12, top: 7, bottom: 7 }).show(
        ui,
        |ui| {
            ui.set_min_width(ui.available_width());
            // One paragraph (code, then message) so long messages wrap under themselves instead of leaving the code
            // stranded on a line of its own.
            let mut job = egui::text::LayoutJob::default();
            if let Some(code) = code {
                job.append(
                    code,
                    0.0,
                    egui::TextFormat { valign: egui::Align::Center, ..egui::TextFormat::simple(egui::FontId::monospace(theme::TEXT_CAPTION), color) },
                );
                job.append("  ", 0.0, egui::TextFormat::simple(egui::FontId::proportional(13.5), theme.text));
            }
            job.append(message, 0.0, egui::TextFormat { line_height: Some(20.0), ..egui::TextFormat::simple(egui::FontId::proportional(13.5), theme.text) });
            ui.add(egui::Label::new(job).wrap().selectable(true));
        },
    );
    let r = inner.response.rect;
    let rule = egui::Rect::from_min_size(r.min, vec2(3.0, r.height()));
    ui.painter().rect_filled(rule, egui::CornerRadius { nw: theme::RADIUS, sw: theme::RADIUS, ne: 0, se: 0 }, color);
    icons::paint(ui.painter(), icon, egui::Rect::from_center_size(egui::pos2(r.left() + 19.0, r.top() + 16.5), vec2(14.0, 14.0)), color);
}

fn editor_id(id: CellId) -> egui::Id {
    egui::Id::unique(("cell-source", id))
}

/// The thin "add a cell" strip between cells; its buttons appear while the pointer is over it.
fn add_row(ui: &mut egui::Ui, env: &mut Env, id: CellId) {
    let theme = env.theme;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), egui::Sense::hover());
    if !ui.rect_contains_pointer(rect) {
        return;
    }
    let mid = rect.center() + vec2(GUTTER / 2.0, 0.0);
    let line = egui::Rangef::new(rect.left() + GUTTER, rect.right());
    ui.painter().hline(line, mid.y, Stroke::new(1.0, theme.border));
    for (dx, label, kind) in [(-40.0, "Code", CellKind::Code), (40.0, "Text", CellKind::Markdown)] {
        let r = egui::Rect::from_center_size(mid + vec2(dx, 0.0), vec2(72.0, 22.0));
        let resp = ui.interact(r, ui.scope_id().with(("add", id, dx as i32)), egui::Sense::click());
        let hot = resp.hovered();
        ui.painter().rect(r, 11.0, if hot { theme.surface_hover } else { theme.bg }, Stroke::new(1.0, theme.border_strong), egui::StrokeKind::Inside);
        let ink = if hot { theme.text } else { theme.text_dim };
        icons::paint(
            ui.painter(),
            Icon::Plus,
            egui::Rect::from_center_size(r.left_center() + vec2(17.0, 0.0), vec2(9.0, 9.0)),
            if hot { theme.accent } else { ink },
        );
        ui.painter().text(r.left_center() + vec2(26.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.0), ink);
        let tip = if kind == CellKind::Code { "Add a code cell here" } else { "Add a text cell here" };
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked() {
            env.actions.push(Action::AddAfter(Some(id), kind));
        }
    }
}

/// Width reserved left of every cell for its number and run button.
const GUTTER: f32 = 46.0;
/// Horizontal padding inside the editor; outputs are indented by the same amount so they line up with the code.
const EDITOR_PAD_X: f32 = 12.0;

fn cell_ui(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, index: usize, nb: &Notebook) -> egui::Rect {
    let theme = env.theme;
    let is_code = cell.kind == CellKind::Code;
    let editing_text = !is_code && (env.editing_md.contains(&cell.id) || cell.source.trim().is_empty());
    let row = ui
        .horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ui.add_space(GUTTER);
            ui.vertical(|ui| {
                ui.set_min_width(ui.available_width());
                match cell.kind {
                    CellKind::Code => {
                        editor(ui, env, cell, true);
                        slider_row(ui, env, cell);
                        for p in &cell.problems {
                            ui.add_space(6.0);
                            banner(ui, theme, Icon::Warning, theme.warn, Some(p.code()), &p.message());
                        }
                        if let (CellState::Blocked, Some(note), true) = (cell.state, &cell.note, cell.problems.is_empty()) {
                            ui.add_space(6.0);
                            banner(ui, theme, Icon::Warning, theme.warn, None, note);
                        }
                        output_ui(ui, env, cell);
                    }
                    CellKind::Markdown => {
                        if editing_text {
                            if editor(ui, env, cell, false) {
                                env.editing_md.remove(&cell.id);
                            }
                        } else {
                            let r = markdown::show(ui, &cell.source, theme);
                            if r.double_clicked() {
                                env.editing_md.insert(cell.id);
                            }
                            r.on_hover_text("Double-click to edit");
                        }
                    }
                }
            });
        })
        .response
        .rect;

    let focused = ui.ctx().memory(|m| m.has_focus(editor_id(cell.id)));
    let dragging = dragged_cell(ui.ctx()).is_some_and(|d| d.id == cell.id);
    let active = focused || dragging || ui.rect_contains_pointer(row);
    if dragging {
        ui.painter().rect_stroke(row.expand2(vec2(2.0, 0.0)), theme::RADIUS_LARGE, Stroke::new(1.5, theme.accent), egui::StrokeKind::Inside);
    }

    // A rule beside the cell says when it needs attention; a cell that is up to date has none.
    if is_code && !matches!(cell.state, CellState::Fresh) {
        let x = row.left() + GUTTER - 9.0;
        let bar = egui::Rect::from_min_max(egui::pos2(x, row.top() + 4.0), egui::pos2(x + 2.0, row.bottom() - 4.0));
        let alpha = if cell.state == CellState::New { 0.4 } else { 1.0 };
        ui.painter().rect_filled(bar, 1.0, state_color(theme, cell.state).gamma_multiply(alpha));
    }

    // Gutter: the cell number, which becomes the run button while the cell is active.
    let slot = egui::Rect::from_min_size(row.min + vec2(4.0, 3.0), vec2(26.0, 26.0));
    if is_code {
        let resp = ui.interact(slot, ui.scope_id().with(("run", cell.id)), egui::Sense::click());
        if active || resp.hovered() {
            let hot = resp.hovered();
            if hot {
                ui.painter().rect_filled(slot, theme::RADIUS, if resp.is_pointer_button_down_on() { theme.surface_pressed } else { theme.surface_hover });
            }
            icons::paint(ui.painter(), Icon::Play, slot.shrink(8.0), if hot { theme.accent } else { theme.text_dim });
        } else {
            ui.painter().text(slot.right_center(), egui::Align2::RIGHT_CENTER, format!("{}", index + 1), egui::FontId::monospace(11.5), theme.text_faint);
        }
        let mut tip = String::from("Run this cell (⌘↵)");
        let defs: Vec<&str> = cell.defs().map(String::as_str).collect();
        if !defs.is_empty() {
            tip.push_str(&format!("\nDefines {}", defs.join(", ")));
        }
        let deps: Vec<String> = nb.dependencies(cell.id).iter().filter_map(|d| nb.index_of(*d)).map(|i| (i + 1).to_string()).collect();
        if !deps.is_empty() {
            tip.push_str(&format!("\nUses cell {}", deps.join(", ")));
        }
        let label = state_label(cell.state);
        if !label.is_empty() {
            tip.push_str(&format!("\nState: {label}"));
        }
        if let Some(o) = &cell.output {
            tip.push_str(&format!("\nRan in {}", format_elapsed(o.elapsed)));
        }
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked() {
            env.actions.push(Action::Run(cell.id));
        }
    }

    // Move and delete appear only while the cell is active.
    if active {
        let size = vec2(26.0 * 4.0 + 6.0, 26.0);
        let at = egui::Rect::from_min_size(egui::pos2(row.right() - size.x - 5.0, row.top() + 3.0), size);
        let mut tools = ui.new_child(egui::UiBuilder::new().max_rect(at).layout(egui::Layout::left_to_right(egui::Align::Center)));
        tools.spacing_mut().item_spacing.x = 2.0;
        let strip = at.expand(2.0);
        tools.painter().add(theme.popup_shadow_small().as_shape(strip, theme::RADIUS_LARGE));
        tools.painter().rect(strip, theme::RADIUS_LARGE, theme.surface, Stroke::new(1.0, theme.border_strong), egui::StrokeKind::Inside);
        let grip = icons::drag_handle(&mut tools, theme, "Drag to move this cell");
        if grip.drag_started() {
            set_dragged_cell(ui.ctx(), Some(Drag { id: cell.id, moved: false }));
        }
        if icons::button(&mut tools, theme, Icon::Up, "Move cell up").clicked() {
            env.actions.push(Action::Move(cell.id, -1));
        }
        if icons::button(&mut tools, theme, Icon::Down, "Move cell down").clicked() {
            env.actions.push(Action::Move(cell.id, 1));
        }
        if icons::button(&mut tools, theme, Icon::Close, "Delete this cell").clicked() {
            env.actions.push(Action::Delete(cell.id));
        }
    }
    row
}

/// The cell being dragged by its handle, and whether it has moved yet (the first move saves the undo point).
#[derive(Clone, Copy)]
struct Drag {
    id: CellId,
    moved: bool,
}

fn dragged_cell(ctx: &egui::Context) -> Option<Drag> {
    ctx.data(|d| d.get_temp::<Option<Drag>>(egui::Id::NULL)).flatten()
}

fn set_dragged_cell(ctx: &egui::Context, drag: Option<Drag>) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::NULL, drag));
}

/// While a cell's handle is held, move the cell past each neighbour the pointer crosses. `rows` are this frame's
/// `(top, bottom)` of every cell, in order.
fn drag_cells(ui: &egui::Ui, env: &mut Env, rows: &[(f32, f32)], nb: &Notebook) {
    let ctx = ui.ctx();
    let Some(mut drag) = dragged_cell(ctx) else { return };
    if !ctx.input(|i| i.pointer.any_down()) {
        set_dragged_cell(ctx, None);
        return;
    }
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let (Some(at), Some(cur)) = (ctx.pointer_latest_pos(), nb.index_of(drag.id)) else { return };
    if let Some(to) = tabs::drop_index(rows, cur, at.y) {
        env.actions.push(Action::Shift(drag.id, to as isize - cur as isize, !drag.moved));
        drag.moved = true;
        set_dragged_cell(ctx, Some(drag));
    }
}

/// The slider under a `name = slider(lo, hi, value)` cell. It rewrites the cell's value as it moves and re-runs the
/// cell (and its dependents), but while the kernel is still busy with the last value it waits instead of queueing
/// more runs; releasing the handle always runs the final value.
fn slider_row(ui: &mut egui::Ui, env: &mut Env, cell: &Cell) {
    let Some(spec) = slider::parse(&cell.source) else { return };
    ui.add_space(6.0);
    let mut value = spec.value;
    let r = ui
        .horizontal(|ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 150.0).clamp(120.0, 420.0);
            let mut s =
                egui::Slider::new(&mut value, spec.lo..=spec.hi).smart_aim(false).text(RichText::new(&spec.name).monospace().color(env.theme.text_faint));
            if let Some(step) = spec.step {
                s = s.step_by(step);
            }
            if spec.whole {
                s = s.integer();
            }
            ui.add(s)
        })
        .inner;
    if r.changed() {
        let text = spec.with_value(value);
        if text != cell.source {
            env.actions.push(Action::SetSource(cell.id, text));
            if !r.dragged() || !env.busy {
                env.actions.push(Action::Run(cell.id));
            }
        }
    }
    if r.drag_stopped() {
        env.actions.push(Action::Run(cell.id));
    }
}

fn format_elapsed(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 1.0 {
        "<1 ms".into()
    } else if ms < 1000.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", d.as_secs_f64())
    }
}

/// The text editor of a cell. Returns true when the user pressed Shift+Enter in a text cell.
fn editor(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, code: bool) -> bool {
    let theme = env.theme;
    let id = editor_id(cell.id);
    let ctx = ui.ctx().clone();
    let mut text = cell.source.clone();
    let mut edited = false;
    let mut run_advance = false;
    let mut run_stay = false;

    let mut insertish = true;
    if ctx.memory(|m| m.has_focus(id)) {
        if env.vim {
            let r = vim_edit::drive(&ctx, id, &mut text);
            edited |= r.edited;
            env.actions.extend(r.commands.into_iter().map(Action::Command));
            *env.vim_status = r.status;
            insertish = r.mode == mathcraft_vim::Mode::Insert;
        }
        if code && insertish {
            edited |= handle_completion_keys(&ctx, id, &mut text, env.names);
        }
        let (shift_enter, cmd_enter, tab, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::SHIFT, Key::Enter),
                i.consume_key(Modifiers::COMMAND, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Tab),
                code && i.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        run_advance = shift_enter;
        run_stay = cmd_enter;
        if tab && code {
            edited |= insert_at_cursor(&ctx, id, &mut text, "    ");
        } else if tab {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        if enter {
            let (before, _) = split_at_cursor(&ctx, id, &text);
            let line = before.rsplit('\n').next().unwrap_or("");
            let mut indent: String = line.chars().take_while(|c| *c == ' ').collect();
            if line.trim_end().ends_with(':') {
                indent.push_str("    ");
            }
            edited |= insert_at_cursor(&ctx, id, &mut text, &format!("\n{indent}"));
        }
    }

    // While the completion list is open, Escape closes it instead of leaving the editor.
    let list_open = code && insertish && ctx.memory(|m| m.has_focus(id)) && suggest(&ctx, id, &text, env.names).is_some();
    let keys = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: list_open || env.vim };
    let font = TextStyle::Monospace.resolve(ui.style());
    let syntax = theme.syntax.clone();
    let plain = theme.text;
    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let mut job = if code {
            highlight::highlight(buf.as_str(), font.clone(), &syntax, env.pretty)
        } else {
            egui::text::LayoutJob::simple(buf.as_str().to_owned(), font.clone(), plain, wrap_width)
        };
        job.wrap.max_width = wrap_width;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let hint = if code { "Type code, e.g. diff(x**2, x), then press Shift+Enter" } else { "Write text; # makes a heading" };
    let has_focus = ctx.memory(|m| m.has_focus(id));
    // Code sits on a soft panel; a text cell being edited gets the same panel so it reads as a field.
    let frame = egui::Frame::new()
        .fill(theme.code_bg)
        .stroke(Stroke::new(1.0, if has_focus { theme.accent } else { theme.cell_outline }))
        .corner_radius(theme::RADIUS_LARGE)
        .inner_margin(Margin::symmetric(EDITOR_PAD_X as i8, 9));
    let shown = frame.show(ui, |ui| {
        egui::TextEdit::multiline(&mut text)
            .id(id)
            .frame(egui::Frame::NONE)
            .desired_width(f32::INFINITY)
            .desired_rows(1)
            .lock_focus(true)
            .event_filter(keys)
            .hint_text(RichText::new(hint).color(theme.text_faint))
            .layouter(&mut layouter)
            .show(ui)
    });
    let out = shown.inner;
    if has_focus {
        // The macOS focus halo, outside the outline.
        let r = shown.response.rect.expand(2.5);
        ui.painter().rect_stroke(r, f32::from(theme::RADIUS_LARGE) + 2.5, Stroke::new(3.5, theme.focus_ring()), egui::StrokeKind::Middle);
    }
    edited |= out.response.changed();
    if env.focus == Some(cell.id) {
        out.response.request_focus();
    }
    if code
        && insertish
        && out.response.has_focus()
        && let Some(range) = out.cursor_range
    {
        let at = out.galley_pos + out.galley.pos_from_cursor(range.primary).left_bottom().to_vec2();
        edited |= completion_popup(&ctx, theme, id, &mut text, env.names, at);
    }
    if code && !edited {
        hover_help(&out, &text, theme);
    }
    if edited {
        env.actions.push(Action::SetSource(cell.id, text));
    }
    if run_advance {
        env.actions.push(if code { Action::RunAndAdvance(cell.id) } else { Action::Run(cell.id) });
    } else if run_stay && code {
        env.actions.push(Action::Run(cell.id));
    }
    run_advance && !code
}

/// A tooltip with the signature and summary of the built-in function under the pointer.
fn hover_help(out: &egui::text_edit::TextEditOutput, text: &str, theme: &Theme) {
    let Some(pos) = out.response.hover_pos() else { return };
    let at = usize::from(out.galley.cursor_from_pos(pos - out.galley_pos).index);
    if complete::in_comment_or_string(text, at) {
        return;
    }
    let Some((name, (sig, summary))) = complete::word_at(text, at).and_then(|w| complete::doc_for(&w).map(|d| (w, d))) else { return };
    egui::Response::clone(&out.response).on_hover_ui(|ui| {
        ui.set_max_width(380.0);
        ui.label(RichText::new(sig).monospace().color(theme.accent));
        ui.label(RichText::new(summary).color(theme.text_dim));
        ui.label(RichText::new(format!("help('{name}') shows more")).size(11.5).color(theme.text_faint));
    });
}

/// What the completion list offers right now.
struct Suggestion {
    /// Char index where the word being completed starts, and the cursor's char index.
    start: usize,
    cursor: usize,
    word: String,
    candidates: Vec<complete::Candidate>,
}

fn cursor_index(ctx: &egui::Context, id: egui::Id, text: &str) -> usize {
    let total = text.chars().count();
    egui::TextEdit::load_state(ctx, id).and_then(|s| s.cursor.char_range()).map_or(total, |r| usize::from(r.primary.index).min(total))
}

fn suggest(ctx: &egui::Context, id: egui::Id, text: &str, names: &[String]) -> Option<Suggestion> {
    let dismissed = id.with("dismissed");
    let cursor = cursor_index(ctx, id, text);
    let found = complete::word_before(text, cursor).filter(|_| !complete::in_comment_or_string(text, cursor));
    let Some((start, word)) = found else {
        ctx.data_mut(|d| d.remove::<usize>(dismissed));
        return None;
    };
    if ctx.data(|d| d.get_temp::<usize>(dismissed)) == Some(start) {
        return None;
    }
    let candidates = complete::candidates(&word, names);
    (!candidates.is_empty()).then_some(Suggestion { start, cursor, word, candidates })
}

/// The highlighted row, kept per editor and reset when the word changes.
fn selected_row(ctx: &egui::Context, id: egui::Id, s: &Suggestion) -> usize {
    let stored = ctx.data(|d| d.get_temp::<(String, usize)>(id.with("selected")));
    match stored {
        Some((word, row)) if word == s.word => row.min(s.candidates.len().saturating_sub(1)),
        _ => 0,
    }
}

/// Up/Down move in the list, Tab or Enter accept, Escape closes it. Runs before the text field sees the keys.
fn handle_completion_keys(ctx: &egui::Context, id: egui::Id, text: &mut String, names: &[String]) -> bool {
    let Some(s) = suggest(ctx, id, text, names) else { return false };
    let row = selected_row(ctx, id, &s);
    let (up, down, accept, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::Tab) || i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    let last = s.candidates.len().saturating_sub(1);
    let moved = if up {
        if row == 0 { last } else { row - 1 }
    } else if down {
        if row >= last { 0 } else { row + 1 }
    } else {
        row
    };
    ctx.data_mut(|d| d.insert_temp(id.with("selected"), (s.word.clone(), moved)));
    if escape {
        ctx.data_mut(|d| d.insert_temp(id.with("dismissed"), s.start));
    }
    match s.candidates.get(row) {
        Some(c) if accept => accept_candidate(ctx, id, text, &s, c),
        _ => false,
    }
}

fn accept_candidate(ctx: &egui::Context, id: egui::Id, text: &mut String, s: &Suggestion, c: &complete::Candidate) -> bool {
    let ins = if c.is_function { format!("{}(", c.name) } else { c.name.clone() };
    replace_chars(ctx, id, text, s.start, s.cursor, &ins);
    true
}

/// The list itself, drawn under the cursor. Returns whether a click accepted an entry (which edits `text`).
fn completion_popup(ctx: &egui::Context, theme: &Theme, id: egui::Id, text: &mut String, names: &[String], at: egui::Pos2) -> bool {
    let Some(s) = suggest(ctx, id, text, names) else { return false };
    let row = selected_row(ctx, id, &s);
    let mut clicked = None;
    egui::Area::new(id.with("popup")).order(egui::Order::Foreground).fixed_pos(at + vec2(-EDITOR_PAD_X - 4.0, 6.0)).show(ctx, |ui| {
        egui::Frame::new()
            .fill(theme.surface)
            .stroke(Stroke::new(1.0, if theme.dark { theme.border_strong } else { theme.border }))
            .corner_radius(theme::RADIUS_LARGE)
            .shadow(theme.popup_shadow())
            .inner_margin(Margin::same(4))
            .show(ui, |ui| {
                ui.set_min_width(280.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let mono = TextStyle::Monospace.resolve(ui.style());
                for (i, c) in s.candidates.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), egui::Sense::click());
                    let selected = i == row;
                    if selected {
                        ui.painter().rect_filled(rect, theme::RADIUS, theme.selected_row());
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, theme::RADIUS, theme.surface_hover);
                    }
                    // f for a function, x for a name defined in the notebook.
                    let (letter, kind) = if c.is_function { ("f", theme.syntax.function) } else { ("x", theme.syntax.keyword) };
                    icons::badge(ui.painter(), egui::Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(16.0, 16.0)), letter, kind);
                    let name_end =
                        ui.painter().text(rect.left_center() + vec2(30.0, 0.0), egui::Align2::LEFT_CENTER, &c.name, mono.clone(), theme.text).right();
                    let detail = c.detail.strip_prefix(c.name.as_str()).filter(|_| c.is_function).unwrap_or(&c.detail);
                    ui.painter().text(
                        egui::pos2(name_end + 2.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        detail,
                        egui::FontId::monospace(12.0),
                        if selected { theme.text_dim } else { theme.text_faint },
                    );
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        clicked = Some(c.clone());
                    }
                }
            });
    });
    match clicked {
        Some(c) => {
            accept_candidate(ctx, id, text, &s, &c);
            ctx.memory_mut(|m| m.request_focus(id));
            true
        }
        None => false,
    }
}

/// Text before and after the cursor.
fn split_at_cursor(ctx: &egui::Context, id: egui::Id, text: &str) -> (String, String) {
    let idx = egui::TextEdit::load_state(ctx, id).and_then(|s| s.cursor.char_range()).map_or_else(|| text.chars().count(), |r| usize::from(r.primary.index));
    (text.chars().take(idx).collect(), text.chars().skip(idx).collect())
}

/// Insert text at the cursor (replacing any selection) and put the cursor after it. Returns whether text changed.
fn insert_at_cursor(ctx: &egui::Context, id: egui::Id, text: &mut String, ins: &str) -> bool {
    let Some(state) = egui::TextEdit::load_state(ctx, id) else { return false };
    let total = text.chars().count();
    let (lo, hi) = match state.cursor.char_range() {
        Some(r) => {
            let (a, b) = (usize::from(r.primary.index), usize::from(r.secondary.index));
            (a.min(b).min(total), a.max(b).min(total))
        }
        None => (total, total),
    };
    replace_chars(ctx, id, text, lo, hi, ins);
    true
}

/// Replace the chars `lo..hi` with `ins` and put the cursor after it.
fn replace_chars(ctx: &egui::Context, id: egui::Id, text: &mut String, lo: usize, hi: usize, ins: &str) {
    let new: String = text.chars().take(lo).chain(ins.chars()).chain(text.chars().skip(hi)).collect();
    *text = new;
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        let at = egui::text::CCursor::new(lo + ins.chars().count());
        state.cursor.set_char_range(Some(CCursorRange::one(at)));
        state.store(ctx, id);
    }
}

fn output_ui(ui: &mut egui::Ui, env: &mut Env, cell: &Cell) {
    let Some(o) = &cell.output else { return };
    let theme = env.theme;
    if o.stdout.is_empty() && matches!(o.result, Ok(None)) {
        return;
    }
    let stale = cell.state == CellState::Stale;
    let has_value = matches!(o.result, Ok(Some(_)));
    if !o.stdout.is_empty() || has_value {
        ui.add_space(10.0);
        // Outputs line up with the code above them, not with the editor's outline.
        egui::Frame::new().inner_margin(Margin { left: EDITOR_PAD_X as i8, right: 0, top: 0, bottom: 2 }).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if stale {
                ui.set_opacity(0.45);
            }
            if !o.stdout.is_empty() {
                stdout_ui(ui, theme, cell.id, o.stdout.trim_end());
                if has_value {
                    ui.add_space(8.0);
                }
            }
            match &o.result {
                Ok(Some(Value::Plot(p))) => plot_ui(ui, theme, cell.id, p, env.actions),
                Ok(Some(v)) => value_ui(ui, env, cell, v),
                _ => {}
            }
        });
    }
    if let Err(e) = &o.result {
        ui.add_space(6.0);
        ui.scope(|ui| {
            if stale {
                ui.set_opacity(0.45);
            }
            banner(ui, theme, Icon::Error, theme.danger, None, &e.to_string());
        });
    }
}

/// Printed output longer than this many lines is cut, with a button to show the rest.
const STDOUT_LINES: usize = 20;

/// The text a cell printed. Long output shows its first lines and "Show N more lines".
fn stdout_ui(ui: &mut egui::Ui, theme: &Theme, id: CellId, text: &str) {
    let total = text.lines().count();
    let key = egui::Id::unique(("stdout-open", id));
    let open = ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let shown: String = if open || total <= STDOUT_LINES + 4 { text.to_owned() } else { text.lines().take(STDOUT_LINES).collect::<Vec<_>>().join("\n") };
    ui.add(egui::Label::new(RichText::new(shown).monospace().size(13.0).color(theme.text_dim)).selectable(true));
    if total > STDOUT_LINES + 4 {
        let label = if open { "Show less".to_owned() } else { format!("Show {} more lines", total - STDOUT_LINES) };
        if ui.add(egui::Button::new(RichText::new(label).size(12.0).color(theme.accent)).frame(false)).clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(key, !open));
        }
    }
}

fn value_ui(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, v: &Value) {
    if let Value::Array(a) = v {
        array_ui(ui, env, cell, v, &a.describe());
        return;
    }
    value_math(ui, env, cell, v);
    if let Some(note) = unevaluated_note(v) {
        ui.add_space(4.0);
        ui.label(RichText::new(note).size(11.5).color(env.theme.text_faint));
    }
}

/// A fade from the background colour into transparent over the edge of `rect`: the right edge if `right`, else the left.
fn edge_fade(ui: &egui::Ui, rect: egui::Rect, right: bool, bg: Color32) {
    const WIDTH: f32 = 36.0;
    let (x0, x1) = if right { (rect.right() - WIDTH, rect.right()) } else { (rect.left(), rect.left() + WIDTH) };
    let (c0, c1) = if right { (Color32::TRANSPARENT, bg) } else { (bg, Color32::TRANSPARENT) };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(egui::pos2(x0, rect.top()), c0);
    mesh.colored_vertex(egui::pos2(x1, rect.top()), c1);
    mesh.colored_vertex(egui::pos2(x1, rect.bottom()), c1);
    mesh.colored_vertex(egui::pos2(x0, rect.bottom()), c0);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// A result that is still an unevaluated call means no closed form was found; say so rather than leave it unexplained.
fn unevaluated_note(v: &Value) -> Option<&'static str> {
    let Value::Expr(e) = v else { return None };
    match e.kind() {
        mathcraft_core::Kind::Call(name, _) if &**name == "integrate" => {
            Some("No closed form found. A definite integral, integrate(f, (x, a, b)), is computed numerically.")
        }
        _ => None,
    }
}

/// A vector or matrix: typeset, with its kind, shape and element type underneath.
fn array_ui(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, v: &Value, caption: &str) {
    value_math(ui, env, cell, v);
    ui.add_space(4.0);
    ui.label(RichText::new(caption).size(11.5).color(env.theme.text_faint));
}

fn value_math(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, v: &Value) {
    let generation = env.generations.get(&cell.id).copied().unwrap_or(0);
    match env.math.get(ui.ctx(), cell.id, generation, v, (ui.available_width() - 8.0).max(120.0)) {
        Some(layout) => {
            let theme = env.theme;
            let resp = if layout.width + 8.0 > ui.available_width() {
                let shown = egui::ScrollArea::horizontal().id_salt(("out", cell.id)).show(ui, |ui| math_view::show(ui, &layout, theme, math_view::SIZE));
                // a soft edge on the side that has more, so a cut-off result reads as scrollable
                let view = shown.inner_rect;
                if shown.state.clamped_offset().x > 1.0 {
                    edge_fade(ui, view, false, theme.bg);
                }
                if shown.state.clamped_offset().x + view.width() < shown.content_size.x - 1.0 {
                    edge_fade(ui, view, true, theme.bg);
                }
                shown.inner
            } else {
                math_view::show(ui, &layout, theme, math_view::SIZE)
            };
            resp.context_menu(|ui| copy_as_menu(ui, theme, v));
        }
        None => match v {
            Value::Str(s) => {
                ui.add(egui::Label::new(RichText::new(s.as_ref()).size(15.0).color(env.theme.text)).selectable(true));
            }
            other => {
                ui.add(egui::Label::new(RichText::new(other.to_string()).monospace().size(14.0).color(env.theme.text)).selectable(true));
            }
        },
    }
}

/// The ways a result can leave MathCraft, each with the text to copy (`None` when this value has no such form).
fn copy_formats(v: &Value) -> Vec<(&'static str, Option<String>)> {
    use mathcraft_core::codegen::{Target, expression};
    let code = |target| match v {
        Value::Expr(e) => expression(e, target).ok(),
        _ => None,
    };
    vec![("Text", Some(v.to_string())), ("LaTeX", v.latex()), ("Python", code(Target::Python)), ("NumPy", code(Target::NumPy)), ("Rust", code(Target::Rust))]
}

/// The output's context menu: copy the value as text, LaTeX or source code in another language.
fn copy_as_menu(ui: &mut egui::Ui, theme: &Theme, v: &Value) {
    ui.menu_button("Copy as", |ui| {
        for (label, text) in copy_formats(v) {
            let enabled = text.is_some();
            if ui.add_enabled_ui(enabled, |ui| menu_row(ui, theme, label, None)).inner.clicked()
                && let Some(text) = text
            {
                ui.ctx().copy_text(text);
                ui.close();
            }
        }
    });
}

/// Remembers, per plot, that "Reset view" was chosen (applied on the next frame).
#[derive(Clone, Copy, Default)]
struct ResetPlot(bool);

/// A plot you can look around in: drag to pan, pinch or Ctrl+scroll to zoom, right-drag a box to zoom into it,
/// double-click to reset. The coordinates under the pointer show in the corner; the context menu resets the view,
/// copies the points as CSV or saves the view as an SVG picture.
fn plot_ui(ui: &mut egui::Ui, theme: &Theme, id: CellId, p: &mathcraft_eval::PlotSpec, actions: &mut Vec<Action>) {
    let plot_id = egui::Id::unique(("plot", id));
    let reset_key = plot_id.with("reset");
    ui.scope(|ui| {
        // The plot draws its frame, labels and legend from the visuals; keep them quiet and on the page colour.
        let v = ui.visuals_mut();
        v.override_text_color = Some(theme.text_dim);
        v.extreme_bg_color = theme.bg;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, theme.border);
        let mut plot = egui_plot::Plot::new(plot_id)
            .height(280.0)
            .grid_spacing(80.0..=400.0)
            .x_grid_spacer(nice_grid(1.0))
            .y_grid_spacer(nice_grid(0.25))
            .x_axis_formatter(|m, _| tick_label(m.value))
            .y_axis_formatter(|m, _| tick_label(m.value))
            .coordinates_formatter(egui_plot::Corner::LeftBottom, egui_plot::CoordinatesFormatter::new(|pt, b| coordinate_text(pt.x, pt.y, b)))
            .allow_scroll(false)
            .allow_boxed_zoom(true)
            .show_grid(false)
            .x_axis_label(p.x_label.clone());
        if ui.ctx().data_mut(|d| d.get_temp::<ResetPlot>(reset_key).is_some_and(|r| r.0)) {
            ui.ctx().data_mut(|d| d.insert_temp(reset_key, ResetPlot(false)));
            plot = plot.reset();
        }
        if p.series.len() > 1 {
            plot = plot.legend(egui_plot::Legend::default().position(egui_plot::Corner::RightTop));
        }
        let shown = plot.show(ui, |plot| {
            for (i, s) in p.series.iter().enumerate() {
                let color = theme.plot.get(i % theme.plot.len()).copied().unwrap_or(theme.accent);
                for seg in &s.segments {
                    plot.line(egui_plot::Line::new(s.label.clone(), egui_plot::PlotPoints::from(seg.clone())).color(color).width(1.75));
                }
            }
        });
        let b = shown.transform.bounds();
        let view = [b.min()[0], b.max()[0], b.min()[1], b.max()[1]];
        shown.response.context_menu(|ui| {
            if menu_row(ui, theme, "Reset view", None).clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(reset_key, ResetPlot(true)));
                ui.close();
            }
            if menu_row(ui, theme, "Copy points as CSV", None).clicked() {
                ui.ctx().copy_text(plot_export::csv(p));
                actions.push(Action::Status("Copied the plot's points as CSV".into()));
                ui.close();
            }
            if menu_row(ui, theme, "Save as SVG…", None).clicked() {
                ui.close();
                actions.push(Action::Status(save_svg(p, view)));
            }
        });
    });
}

/// "x = 1.25   y = -0.5", with as many decimals as the visible range needs.
fn coordinate_text(x: f64, y: f64, b: &egui_plot::PlotBounds) -> String {
    let decimals = |span: f64| (3.0 - span.abs().max(f64::MIN_POSITIVE).log10().floor()).clamp(0.0, 10.0) as usize;
    let fmt = |v: f64, span: f64| {
        let s = format!("{v:.*}", decimals(span));
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
    };
    format!("x = {}   y = {}", fmt(x, b.width()), fmt(y, b.height()))
}

/// Ask where to save, write the picture, and say what happened.
fn save_svg(p: &mathcraft_eval::PlotSpec, view: [f64; 4]) -> String {
    let Some(path) = rfd::FileDialog::new().add_filter("SVG picture", &["svg"]).set_file_name("plot.svg").save_file() else { return "Save cancelled".into() };
    let sane = view.iter().all(|v| v.is_finite()) && view[1] > view[0] && view[3] > view[2];
    let view = if sane { view } else { plot_export::data_view(p) };
    let hex = |c: Color32| format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
    let series: Vec<String> = Theme::light().plot.iter().map(|c| hex(*c)).collect();
    let colors = plot_export::SvgColors { series: &series, text: "#333333", grid: "#e4e4e8" };
    match std::fs::write(&path, plot_export::svg(p, view, &colors, &tick_label)) {
        Ok(()) => format!("Saved {}", file_name(&path)),
        Err(e) => format!("Could not save the plot: {e}"),
    }
}

/// Axis ticks at 1, 2 or 5 times a power of ten, about `fraction` of the plot's minimum grid spacing apart, so an axis
/// carries several labels (the default base-10 spacer often leaves a short y axis with a single `0`).
fn nice_grid(fraction: f64) -> Box<dyn Fn(egui_plot::GridInput) -> Vec<egui_plot::GridMark>> {
    egui_plot::uniform_grid_spacer(move |input| {
        let want = (input.base_step_size * fraction).abs().max(f64::MIN_POSITIVE);
        let power = 10f64.powf(want.log10().floor());
        let step = [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * power).find(|s| *s >= want).unwrap_or(power * 10.0);
        [step, step * 10.0, step * 100.0]
    })
}

/// An axis tick: the shortest decimal for the value (ticks are 1, 2 or 5 times a power of ten), `0` not `-0`.
fn tick_label(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s.as_str() };
    if s == "-0" { "0".to_owned() } else { s.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use std::time::Instant;

    #[test]
    fn results_can_be_copied_in_several_formats() {
        use mathcraft_core::Expr;
        let e = Value::from_expr(Expr::pow(&Expr::sym("x"), &Expr::int(2)));
        let formats: Vec<_> = copy_formats(&e).into_iter().collect();
        let get = |name: &str| formats.iter().find(|(n, _)| *n == name).and_then(|(_, t)| t.clone());
        assert_eq!(get("Text").as_deref(), Some("x**2"));
        assert_eq!(get("LaTeX").as_deref(), Some("x^{2}"));
        assert_eq!(get("Python").as_deref(), Some("x**2"));
        assert_eq!(get("Rust").as_deref(), Some("x.powi(2)"));
        // a string has no LaTeX or code form
        assert!(copy_formats(&Value::Str("hi".into())).iter().filter(|(n, _)| *n != "Text").all(|(_, t)| t.is_none()));
    }

    #[test]
    fn settings_are_saved_and_restored() {
        let dir = std::env::temp_dir().join(format!("mathcraft-app-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), None);
        app.load_settings(Some(dir.clone()));
        app.run_command(Cmd::ToggleVim, &ctx);
        app.run_command(Cmd::Theme(ThemePreference::Dark), &ctx);
        app.run_command(Cmd::Accent(Some("purple")), &ctx);
        frame(&ctx, &mut app);
        let saved = settings::Settings::load(&dir);
        assert!(saved.vim);
        assert_eq!(saved.appearance, ThemePreference::Dark);
        assert_eq!(saved.accent.as_deref(), Some("purple"));
        // a fresh app picks them up
        let ctx2 = egui::Context::default();
        let mut again = App::with_context(ctx2.clone(), None);
        again.load_settings(Some(dir.clone()));
        assert!(again.vim);
        assert_eq!(ctx2.options(|o| o.theme_preference), ThemePreference::Dark);
        assert_eq!(again.dark.accent, egui::Color32::from_rgb(168, 128, 255));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_session_comes_back_with_tabs_folder_and_recent_files() {
        let base = std::env::temp_dir().join(format!("mathcraft-app-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (cfg, work) = (base.join("cfg"), base.join("work"));
        std::fs::create_dir_all(&work).unwrap();
        for f in ["a.py", "b.py", "c.py"] {
            std::fs::write(work.join(f), "# %%\nx = 1\n").unwrap();
        }
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), None);
        app.load_settings(Some(cfg.clone()));
        app.set_folder(work.clone());
        app.open_path(work.join("a.py"));
        app.open_path(work.join("b.py"));
        app.open_path(work.join("c.py"));
        app.open_path(work.join("b.py"));
        frame(&ctx, &mut app);
        let saved = session::Session::load(&cfg);
        assert_eq!(saved.files.len(), 3);
        assert_eq!(saved.folder.as_deref(), Some(work.as_path()));
        assert_eq!(saved.recent.first(), Some(&work.join("c.py")));
        // a file deleted since is skipped; the active tab (b) is selected again
        std::fs::remove_file(work.join("a.py")).unwrap();
        let ctx2 = egui::Context::default();
        let mut again = App::with_context(ctx2.clone(), None);
        again.load_settings(Some(cfg.clone()));
        again.load_session(false);
        assert_eq!(again.order.len(), 2);
        assert_eq!(again.cur.path.as_deref(), Some(work.join("b.py").as_path()));
        assert_eq!(again.folder.as_deref(), Some(work.as_path()));
        assert!(again.recent.contains(&work.join("a.py")));
        // a file named on the command line wins: no tabs are restored
        let mut named = App::with_context(egui::Context::default(), None);
        named.load_settings(Some(cfg.clone()));
        named.load_session(true);
        assert_eq!(named.order.len(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn coordinates_use_the_precision_the_view_needs() {
        let b = |w: f64| egui_plot::PlotBounds::from_min_max([0.0, 0.0], [w, w]);
        assert_eq!(coordinate_text(1.23456, -0.5, &b(10.0)), "x = 1.23   y = -0.5");
        assert_eq!(coordinate_text(1.23456, 2.0, &b(0.01)), "x = 1.23456   y = 2");
        assert_eq!(coordinate_text(1234.4, 7.0, &b(5000.0)), "x = 1234   y = 7");
    }

    #[test]
    fn a_plot_cell_draws_and_resets() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), None);
        app.replace_notebook(Notebook::from_text("# %%\nplot(sin(x), (x, 0, 6))\n"), None);
        let start = Instant::now();
        while app.busy() || start.elapsed() < Duration::from_millis(200) {
            frame(&ctx, &mut app);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(app.cur.nb.cells().first().and_then(|c| c.output.as_ref()).map(|o| &o.result), Some(Ok(Some(Value::Plot(_))))));
        let id = app.cur.nb.cells().first().map(|c| c.id).unwrap_or(0);
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("plot", id)).with("reset"), ResetPlot(true)));
        frame(&ctx, &mut app);
        assert!(ctx.data(|d| d.get_temp::<ResetPlot>(egui::Id::unique(("plot", id)).with("reset"))).is_some_and(|r| !r.0), "the request is consumed");
    }

    #[test]
    fn tick_labels_are_short() {
        assert_eq!(tick_label(0.0), "0");
        assert_eq!(tick_label(-0.0), "0");
        assert_eq!(tick_label(0.2), "0.2");
        assert_eq!(tick_label(2.5), "2.5");
        assert_eq!(tick_label(10.0), "10");
        assert_eq!(tick_label(-0.30000000000000004), "-0.3");
    }

    fn frame(ctx: &egui::Context, app: &mut App) {
        ctx.run_ui(egui::RawInput::default(), |ui| app.show(ui)).drop_without_applying_deltas();
    }

    /// Render real frames of the welcome notebook until the kernel has finished every cell.
    #[test]
    fn welcome_notebook_renders_and_runs() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), None);
        let start = Instant::now();
        while app.busy() || start.elapsed() < Duration::from_millis(300) {
            frame(&ctx, &mut app);
            std::thread::sleep(Duration::from_millis(20));
            assert!(start.elapsed() < Duration::from_secs(60), "the welcome notebook did not finish running");
        }
        frame(&ctx, &mut app);
        let code: Vec<_> = app.cur.nb.cells().iter().filter(|c| c.kind == CellKind::Code).collect();
        assert!(code.len() >= 8);
        for c in &code {
            assert_eq!(c.state, CellState::Fresh, "cell `{}` is {:?}: {:?}", c.source, c.state, c.output);
        }
        assert!(app.cur.nb.check().is_empty());
        let show = |i: usize| match app.cur.nb.cells().get(i).and_then(|c| c.output.as_ref()).map(|o| &o.result) {
            Some(Ok(Some(v))) => v.to_string().chars().take(200).collect::<String>(),
            other => format!("{other:?}").chars().take(200).collect(),
        };
        assert!(show(2).contains("cos(x)"), "{}", show(2));
        assert!(show(3).starts_with("<plot"), "{}", show(3));
    }

    #[test]
    fn editing_and_running_through_actions_updates_dependents() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), None);
        app.replace_notebook(Notebook::from_text("# %%\na = 2\n# %%\nb = a * 10\n# %%\nb + 1\n"), None);
        let wait = |ctx: &egui::Context, app: &mut App| {
            let start = Instant::now();
            while app.busy() || start.elapsed() < Duration::from_millis(200) {
                frame(ctx, app);
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        wait(&ctx, &mut app);
        let ids: Vec<CellId> = app.cur.nb.cells().iter().map(|c| c.id).collect();
        let last = |app: &App| app.cur.nb.cells().last().and_then(|c| c.output.as_ref()).map(|o| format!("{:?}", o.result)).unwrap_or_default();
        assert!(last(&app).contains("21"), "{}", last(&app));
        app.apply(vec![Action::SetSource(ids[0], "a = 5".into()), Action::Run(ids[0])]);
        wait(&ctx, &mut app);
        assert!(last(&app).contains("51"), "{}", last(&app));
        assert!(app.cur.dirty);
    }

    #[test]
    fn moving_a_slider_reruns_its_dependents() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), None);
        app.replace_notebook(Notebook::from_text("# %%\na = slider(0, 10, 2)\n# %%\nb = a * 10\n# %%\nb + 1\n"), None);
        let wait = |ctx: &egui::Context, app: &mut App| {
            let start = Instant::now();
            while app.busy() || start.elapsed() < Duration::from_millis(200) {
                frame(ctx, app);
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        wait(&ctx, &mut app);
        let last = |app: &App| app.cur.nb.cells().last().and_then(|c| c.output.as_ref()).map(|o| format!("{:?}", o.result)).unwrap_or_default();
        assert!(last(&app).contains("21"), "{}", last(&app));
        let first = app.cur.nb.cells().first().map(|c| (c.id, c.source.clone())).unwrap();
        let spec = slider::parse(&first.1).expect("a slider cell");
        app.apply(vec![Action::SetSource(first.0, spec.with_value(7.2)), Action::Run(first.0)]);
        wait(&ctx, &mut app);
        assert_eq!(app.cur.nb.cells().first().map(|c| c.source.as_str()), Some("a = slider(0, 10, 7)"));
        assert!(last(&app).contains("71"), "{}", last(&app));
    }

    #[test]
    fn tabs_can_be_reordered_and_the_order_is_remembered() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), None);
        app.open_in_tab(Notebook::from_text("# %%\na = 1\n"), Some(PathBuf::from("/tmp/mc_r1.py")));
        app.open_in_tab(Notebook::from_text("# %%\nb = 2\n"), Some(PathBuf::from("/tmp/mc_r2.py")));
        let before = app.order.clone();
        app.move_tab(before[2], 0);
        assert_eq!(app.order, vec![before[2], before[0], before[1]]);
        app.move_tab(before[2], 99);
        assert_eq!(app.order, vec![before[0], before[1], before[2]]);
        assert_eq!(app.order.len(), 3);
        frame(&ctx, &mut app);
    }

    #[test]
    fn tabs_open_switch_and_close() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), None);
        let first = app.cur.id;
        app.open_in_tab(Notebook::from_text("# %%\na = 1\n"), Some(PathBuf::from("/tmp/mc_a.py")));
        assert_eq!(app.order.len(), 2, "the welcome notebook is not pristine, so a tab is added");
        let second = app.cur.id;
        app.open_in_tab(Notebook::from_text("# %%\nb = 2\n"), Some(PathBuf::from("/tmp/mc_b.py")));
        assert_eq!(app.order.len(), 3);
        app.select_tab(first);
        assert_eq!(app.cur.id, first);
        assert!(app.cur.nb.cells().len() > 3, "the welcome notebook came back intact");
        app.step_tab(1);
        assert_eq!(app.cur.id, second);
        app.step_tab(-1);
        app.step_tab(-1);
        assert_ne!(app.cur.id, first, "wraps around");
        frame(&ctx, &mut app);
        app.close_tab(second);
        assert_eq!(app.order.len(), 2);
        assert!(app.doc(second).is_none());
        let remaining: Vec<u64> = app.order.clone();
        for id in remaining {
            app.close_tab(id);
        }
        assert_eq!(app.order.len(), 1, "closing the last tab leaves a blank one");
        assert!(app.cur.is_pristine());
        frame(&ctx, &mut app);
    }

    fn sample_folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mathcraft-app-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("lectures")).unwrap();
        std::fs::write(dir.join("derivatives.py"), "# %%\nf = sin(x)**2\n# %%\ndiff(f, x)\n").unwrap();
        std::fs::write(dir.join("solving.py"), "# %%\nsolve(x**3 - 3*x + 1, x)\n").unwrap();
        std::fs::write(dir.join("lectures/week1.py"), "# %%\n1/3 + 1/6\n").unwrap();
        dir
    }

    #[test]
    fn a_folder_fills_the_sidebar_and_files_open_in_tabs() {
        let dir = sample_folder("open");
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), Some(dir.clone()));
        assert!(app.show_sidebar);
        let names: Vec<&str> = app.tree.iter().map(workspace::Node::name).collect();
        assert_eq!(names, vec!["lectures", "derivatives.py", "solving.py"]);
        frame(&ctx, &mut app);
        app.open_path(dir.join("solving.py"));
        app.open_path(dir.join("derivatives.py"));
        app.open_path(dir.join("solving.py"));
        assert_eq!(app.cur.name(), "solving.py");
        assert_eq!(app.order.len(), 2, "the blank tab was reused and the repeat open only switched");
        app.new_in_folder();
        assert_eq!(app.cur.name(), "untitled.py");
        assert!(app.tree.iter().any(|n| n.name() == "untitled.py"), "the list shows the new file");
        frame(&ctx, &mut app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn rich_cells_screenshot() {
        let Some(out) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&out).unwrap();
        let src = "# %% [markdown]\n# # Math in text\n# The roots of $x**2 - 2$ are $sqrt(2)$ and $-sqrt(2)$, and it costs $5 or $6.\n# $$ integrate(1/(x**2+1), x) $$\n# - a list item with $a/b + c$ inside\n# > a quote about $exp(I*pi) + 1$\n# %%\na = slider(0, 10, 3)\n# %%\nplot([sin(x), cos(x)], (x, 0, 6))\n";
        for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
            let mut harness = egui_kittest::Harness::builder().wgpu().with_size(vec2(900.0, 780.0)).build_ui_state(
                move |ui, app: &mut Option<App>| {
                    app.get_or_insert_with(|| {
                        let mut app = App::with_context(ui.ctx().clone(), None);
                        app.replace_notebook(Notebook::from_text(src), None);
                        app
                    })
                    .show(ui)
                },
                None,
            );
            harness.ctx.set_theme(pref);
            let start = Instant::now();
            while harness.state().as_ref().is_none_or(App::busy) && start.elapsed() < Duration::from_secs(30) {
                harness.run_steps(3);
                std::thread::sleep(Duration::from_millis(30));
            }
            harness.run_steps(6);
            harness.render().unwrap().save(out.join(format!("rich-{name}.png"))).unwrap();
        }
    }

    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn sidebar_screenshot() {
        let Some(out) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&out).unwrap();
        let dir = sample_folder("shot");
        for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
            let d = dir.clone();
            let mut harness = egui_kittest::Harness::builder().wgpu().with_size(vec2(1000.0, 520.0)).build_ui_state(
                move |ui, app: &mut Option<App>| {
                    app.get_or_insert_with(|| {
                        let mut app = App::with_context(ui.ctx().clone(), Some(d.clone()));
                        app.open_path(d.join("derivatives.py"));
                        app.open_path(d.join("solving.py"));
                        app
                    })
                    .show(ui)
                },
                None,
            );
            harness.ctx.set_theme(pref);
            let start = Instant::now();
            while harness.state().as_ref().is_none_or(App::busy) && start.elapsed() < Duration::from_secs(30) {
                harness.run_steps(3);
                std::thread::sleep(Duration::from_millis(30));
            }
            harness.run_steps(6);
            harness.render().unwrap().save(out.join(format!("sidebar-{name}.png"))).unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn narrow_views_screenshot() {
        let Some(out) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&out).unwrap();
        let dir = sample_folder("narrow");
        for (name, pref, text) in [("text-dark", ThemePreference::Dark, true), ("sidebar-light", ThemePreference::Light, false)] {
            let d = dir.clone();
            let mut harness = egui_kittest::Harness::builder().wgpu().with_size(vec2(560.0, 560.0)).build_ui_state(
                move |ui, app: &mut Option<App>| {
                    app.get_or_insert_with(|| {
                        let mut app = App::with_context(ui.ctx().clone(), Some(d.clone()));
                        app.open_path(d.join("derivatives.py"));
                        app.open_path(d.join("solving.py"));
                        if text {
                            app.cur.set_view(View::Text);
                        }
                        app
                    })
                    .show(ui)
                },
                None,
            );
            harness.ctx.set_theme(pref);
            let start = Instant::now();
            while harness.state().as_ref().is_none_or(App::busy) && start.elapsed() < Duration::from_secs(30) {
                harness.run_steps(3);
                std::thread::sleep(Duration::from_millis(30));
            }
            harness.run_steps(6);
            harness.render().unwrap().save(out.join(format!("narrow-{name}.png"))).unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_view_keeps_the_notebook_in_step() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), None);
        app.replace_notebook(Notebook::from_text("# %%\na = 1\n# %%\nb = a + 1\n"), None);
        let ids: Vec<CellId> = app.cur.nb.cells().iter().map(|c| c.id).collect();
        app.cur.set_view(View::Text);
        assert!(app.cur.text.contains("b = a + 1"));
        frame(&ctx, &mut app);
        // edit the first cell
        app.cur.text = app.cur.text.replace("a = 1", "a = 5");
        app.cur.sync_from_text();
        let now: Vec<CellId> = app.cur.nb.cells().iter().map(|c| c.id).collect();
        assert_eq!(now, ids, "editing keeps every cell's identity");
        assert_eq!(app.cur.nb.cells().first().map(|c| c.source.trim()), Some("a = 5"));
        // insert a cell in the middle: the last cell keeps its identity
        app.cur.text = app.cur.text.replace("# %%\nb = a + 1", "# %%\nc = 3\n# %%\nb = a + 1");
        app.cur.sync_from_text();
        let now: Vec<CellId> = app.cur.nb.cells().iter().map(|c| c.id).collect();
        assert_eq!(now.len(), 3);
        assert_eq!((now.first(), now.last()), (ids.first(), ids.last()));
        // the cursor decides which cell Shift+Enter runs
        let at = app.cur.text.find("c = 3").unwrap_or(0);
        assert_eq!(app.cur.cell_at(at), now.get(1).copied());
        // deleting text down to one cell
        app.cur.text = "# %%\nz = 1\n".into();
        app.cur.sync_from_text();
        assert_eq!(app.cur.nb.cells().len(), 1);
        frame(&ctx, &mut app);
        app.cur.set_view(View::Notebook);
        frame(&ctx, &mut app);
    }

    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn text_view_screenshot() {
        let Some(out) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&out).unwrap();
        for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
            let mut harness = egui_kittest::Harness::builder().wgpu().with_size(vec2(1100.0, 600.0)).build_ui_state(
                |ui, app: &mut Option<App>| {
                    app.get_or_insert_with(|| {
                        let mut app = App::with_context(ui.ctx().clone(), None);
                        app.replace_notebook(
                            Notebook::from_text("# %% [markdown]\n# # Derivatives\n# %%\nf = sin(x)**2\n# %%\ndiff(f, x)\n# %%\nsolve(x**3 - 3*x + 1, x)\n# %%\nf = 1/0\n# %%\nplot(sin(x)**2, (x, 0, 6))\n"),
                            None,
                        );
                        app.cur.set_view(View::Text);
                        // the Vim mode segment shows in the status bar
                        app.vim = true;
                        app
                    })
                    .show(ui)
                },
                None,
            );
            harness.ctx.set_theme(pref);
            let start = Instant::now();
            while harness.state().as_ref().is_none_or(App::busy) && start.elapsed() < Duration::from_secs(30) {
                harness.run_steps(3);
                std::thread::sleep(Duration::from_millis(30));
            }
            // put the cursor in the `solve` cell so the results panel marks it
            if let Some(app) = harness.state() {
                let id = text_view::editor_id(app.cur.id);
                let at = app.cur.text.find("solve").unwrap_or(0) + 2;
                let mut st = egui::TextEdit::load_state(&harness.ctx, id).unwrap_or_default();
                st.cursor.set_char_range(Some(CCursorRange::one(egui::text::CCursor::new(at))));
                st.store(&harness.ctx, id);
            }
            harness.run_steps(6);
            harness.render().unwrap().save(out.join(format!("text-{name}.png"))).unwrap();
        }
    }

    #[test]
    fn an_untouched_blank_tab_is_reused_when_opening() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), None);
        app.replace_notebook(Notebook::from_text("# %%\n"), None);
        app.open_in_tab(Notebook::from_text("# %%\nx = 1\n"), Some(PathBuf::from("/tmp/mc_c.py")));
        assert_eq!(app.order.len(), 1);
        assert_eq!(app.cur.name(), "mc_c.py");
    }

    #[test]
    fn delete_last_cell_leaves_an_empty_one() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), None);
        app.replace_notebook(Notebook::from_text("# %%\nx = 1\n"), None);
        let id = app.cur.nb.cells().first().map(|c| c.id).unwrap_or(0);
        app.apply(vec![Action::Delete(id)]);
        assert_eq!(app.cur.nb.cells().len(), 1);
        frame(&ctx, &mut app);
    }

    #[test]
    fn cell_changes_undo_and_redo_without_losing_typing() {
        let ctx = egui::Context::default();
        let mut app = App::with_context(ctx.clone(), None);
        app.replace_notebook(Notebook::from_text("# %%\nx = 1\n# %%\ny = x + 1\n# %%\nz = 3\n"), None);
        let ids: Vec<_> = app.cur.nb.cells().iter().map(|c| c.id).collect();
        app.apply(vec![Action::Delete(ids[1])]);
        assert_eq!(app.cur.nb.cells().len(), 2);
        // typing in a remaining cell after the delete must survive the undo
        app.apply(vec![Action::SetSource(ids[2], "z = 33".into())]);
        app.undo_cells();
        let cells = app.cur.nb.cells();
        assert_eq!(cells.iter().map(|c| c.id).collect::<Vec<_>>(), ids);
        assert_eq!(cells[1].source, "y = x + 1");
        assert_eq!(cells[2].source, "z = 33");
        app.redo_cells();
        assert_eq!(app.cur.nb.cells().len(), 2);
        app.undo_cells();
        app.apply(vec![Action::Move(ids[0], 1)]);
        assert_eq!(app.cur.nb.cells().first().map(|c| c.id), Some(ids[1]));
        app.undo_cells();
        assert_eq!(app.cur.nb.cells().first().map(|c| c.id), Some(ids[0]));
        // a new change drops the redo history
        app.apply(vec![Action::Delete(ids[0])]);
        app.undo_cells();
        assert!(app.cur.can_redo());
        app.apply(vec![Action::AddAfter(None, CellKind::Code)]);
        assert!(!app.cur.can_redo());
        frame(&ctx, &mut app);
    }

    /// Renders the notebook with the GPU, in both themes, to PNG files for a visual check. Skipped unless
    /// Step the harness until no cell is running (at most about ten seconds).
    fn settle(harness: &mut egui_kittest::Harness<'_, Option<App>>) {
        for _ in 0..500 {
            harness.run_steps(2);
            if !harness.state().as_ref().is_some_and(App::busy) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// A headless harness around one empty code cell, with the editor focused.
    fn typing_harness() -> egui_kittest::Harness<'static, Option<App>> {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(900.0, 600.0))
            .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
        // the welcome notebook is still running (a spinner repaints), so step until the kernel is idle instead of
        // waiting for the UI to settle; a slow machine needs more than a few frames
        settle(&mut harness);
        if let Some(app) = harness.state_mut() {
            app.replace_notebook(Notebook::from_text("# %%\n"), None);
        }
        settle(&mut harness);
        harness.run();
        harness.get_by_role(egui::accesskit::Role::MultilineTextInput).focus();
        harness.run();
        harness
    }

    fn code_cell(harness: &egui_kittest::Harness<'_, Option<App>>) -> Option<Cell> {
        harness.state().as_ref()?.cur.nb.cells().iter().find(|c| c.kind == CellKind::Code).cloned()
    }

    fn source(harness: &egui_kittest::Harness<'_, Option<App>>) -> String {
        code_cell(harness).map(|c| c.source).unwrap_or_default()
    }

    #[test]
    fn tab_accepts_a_completion() {
        let mut h = typing_harness();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("y = dif");
        h.run();
        assert_eq!(source(&h), "y = dif");
        h.key_press(Key::Tab);
        h.run();
        assert_eq!(source(&h), "y = diff(");
    }

    #[test]
    fn arrows_choose_and_escape_closes_the_list() {
        let mut h = typing_harness();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("fact");
        h.run();
        let first = complete::candidates("fact", &[]);
        assert!(first.len() >= 2, "the test needs two `fact` completions: {first:?}");
        h.key_press(Key::ArrowDown);
        h.run();
        h.key_press(Key::Enter);
        h.run();
        let want = first.get(1).map(|c| format!("{}(", c.name)).unwrap_or_default();
        assert_eq!(source(&h), want);

        let mut h = typing_harness();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("fact");
        h.run();
        h.key_press(Key::Escape);
        h.run();
        h.key_press(Key::Tab);
        h.run();
        assert_eq!(source(&h), "fact    ", "after Escape, Tab indents instead of completing");
    }

    #[test]
    fn enter_keeps_indentation_and_shift_enter_runs() {
        let mut h = typing_harness();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("if True:");
        h.run();
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(source(&h), "if True:\n    ");
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("1 + 1");
        h.run();
        h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
        let start = Instant::now();
        while code_cell(&h).map(|c| c.state) != Some(CellState::Fresh) && start.elapsed() < Duration::from_secs(20) {
            h.run_steps(2);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(code_cell(&h).map(|c| c.state), Some(CellState::Fresh));
    }

    #[test]
    fn the_palette_runs_a_command() {
        let mut harness = typing_harness();
        harness.key_press_modifiers(Modifiers::COMMAND, Key::P);
        harness.run();
        assert!(harness.state().as_ref().is_some_and(|a| a.palette.open), "Cmd+P opens the palette");
        harness.get_by_role(egui::accesskit::Role::TextInput).type_text("text vi");
        harness.run();
        harness.key_press(Key::Enter);
        harness.run();
        harness.run();
        let app = harness.state().as_ref();
        assert!(app.is_some_and(|a| !a.palette.open), "Enter closes it");
        assert!(app.is_some_and(|a| a.cur.view == View::Text), "and ran Text view");
        harness.key_press_modifiers(Modifiers::COMMAND, Key::P);
        harness.run();
        harness.key_press(Key::Escape);
        harness.run();
        assert!(harness.state().as_ref().is_some_and(|a| !a.palette.open), "Esc closes it");
    }

    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn palette_screenshot() {
        let Some(out) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&out).unwrap();
        for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
            let mut harness = egui_kittest::Harness::builder()
                .wgpu()
                .with_size(vec2(1000.0, 560.0))
                .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
            harness.ctx.set_theme(pref);
            harness.run_steps(4);
            harness.key_press_modifiers(Modifiers::COMMAND, Key::P);
            harness.run_steps(3);
            harness.get_by_role(egui::accesskit::Role::TextInput).type_text("sa");
            harness.run_steps(4);
            harness.render().unwrap().save(out.join(format!("palette-{name}.png"))).unwrap();
        }
    }

    #[test]
    fn vim_keys_edit_a_cell() {
        let mut harness = typing_harness();
        if let Some(app) = harness.state_mut() {
            app.vim = true;
        }
        let mode = |h: &egui_kittest::Harness<'_, Option<App>>| h.state().as_ref().map(|a| a.vim_status.clone()).unwrap_or_default();
        harness.run();
        harness.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("i");
        harness.run();
        assert_eq!(mode(&harness), "INSERT");
        harness.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("abc def");
        harness.run();
        assert_eq!(source(&harness), "abc def", "insert mode types normally");
        harness.key_press(Key::Escape);
        harness.run();
        assert_eq!(mode(&harness), "NORMAL");
        assert!(code_cell(&harness).is_some());
        harness.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("0dw");
        harness.run();
        assert_eq!(source(&harness), "def", "0 then dw removes the first word");
        harness.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("xu");
        harness.run();
        assert_eq!(source(&harness), "def", "x then u restores it");
        harness.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("Ax");
        harness.run();
        harness.key_press(Key::Escape);
        harness.run();
        assert_eq!(source(&harness), "defx", "A appends");
        harness.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text(":q");
        harness.run();
        assert_eq!(mode(&harness), ":q");
        harness.key_press(Key::Escape);
        harness.run();
        assert_eq!(mode(&harness), "NORMAL");
    }

    #[test]
    fn vim_j_scrolls_the_text_view_to_follow_the_cursor() {
        let mut h = egui_kittest::Harness::builder()
            .with_size(vec2(900.0, 400.0))
            .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
        h.run_steps(6);
        if let Some(app) = h.state_mut() {
            app.vim = true;
            let text: String = (0..150).map(|i| format!("# %%\nx{i} = {i}\n")).collect();
            app.replace_notebook(Notebook::from_text(&text), None);
            app.cur.set_view(View::Text);
        }
        h.run_steps(6);
        let editor = |h: &egui_kittest::Harness<'_, Option<App>>| h.get_by_role(egui::accesskit::Role::MultilineTextInput).rect().top();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).focus();
        h.run_steps(6);
        let before = editor(&h);
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text(&"j".repeat(120));
        h.run_steps(4);
        assert!(editor(&h) < before - 200.0, "the view followed the cursor down: {before} -> {}", editor(&h));
    }

    #[test]
    fn undo_and_redo_work_in_a_cell() {
        let mut h = typing_harness();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("abc");
        h.run();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text(" def");
        h.run();
        assert_eq!(source(&h), "abc def");
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        h.run();
        assert!(source(&h).len() < 7, "undo removed something: {:?}", source(&h));
        h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        h.run();
        assert_eq!(source(&h), "abc def");
    }

    #[test]
    fn the_palette_inserts_a_function_into_the_last_cell() {
        let mut h = typing_harness();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("y = ");
        h.run();
        h.key_press_modifiers(Modifiers::COMMAND, Key::P);
        h.run();
        h.get_by_role(egui::accesskit::Role::TextInput).type_text("diff(f");
        h.run();
        h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
        h.run();
        h.run();
        assert_eq!(source(&h), "y = diff(");
        assert!(!h.state().as_ref().is_some_and(|a| a.palette.open), "the palette closed");
        // Enter alone only opens the function list
        h.key_press_modifiers(Modifiers::COMMAND, Key::P);
        h.run();
        h.get_by_role(egui::accesskit::Role::TextInput).type_text("integrate(f");
        h.run();
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(source(&h), "y = diff(");
        assert!(h.state().as_ref().is_some_and(|a| a.show_help));
    }

    #[test]
    fn cmd_alt_arrows_move_between_cells() {
        let mut h = typing_harness();
        if let Some(app) = h.state_mut() {
            app.replace_notebook(Notebook::from_text("# %%\na = 1\n# %% [markdown]\n# text\n# %%\nb = 2\n"), None);
        }
        settle(&mut h);
        h.run();
        let first = h.state().as_ref().and_then(|a| a.cur.nb.cells().first().map(|c| c.id)).unwrap();
        let last = h.state().as_ref().and_then(|a| a.cur.nb.cells().last().map(|c| c.id)).unwrap();
        if let Some(app) = h.state_mut() {
            app.cur.focus = Some(first);
        }
        h.run();
        let focused = |h: &egui_kittest::Harness<'_, Option<App>>| {
            h.state().as_ref().and_then(|a| a.cur.nb.cells().iter().find(|c| h.ctx.memory(|m| m.has_focus(editor_id(c.id)))).map(|c| c.id))
        };
        assert_eq!(focused(&h), Some(first));
        h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowDown);
        h.run();
        assert_eq!(focused(&h), Some(last), "the text cell in between is skipped");
        h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowUp);
        h.run();
        assert_eq!(focused(&h), Some(first));
    }

    #[test]
    fn long_printed_output_is_folded() {
        let mut h = typing_harness();
        h.set_size(vec2(900.0, 1400.0));
        if let Some(app) = h.state_mut() {
            app.replace_notebook(Notebook::from_text("# %%\nfor i in range(26):\n    print(i)\n# %%\nprint(1)\n"), None);
        }
        settle(&mut h);
        h.run();
        assert!(h.query_by_label("Show 6 more lines").is_some(), "26 lines fold after 20");
        assert_eq!(h.query_all_by_label_contains("Show ").count(), 1, "the one-line output is not folded");
        h.get_by_label("Show 6 more lines").click();
        h.run_steps(4);
        assert!(h.query_by_label("Show less").is_some());
        h.get_by_label("Show less").click();
        h.run_steps(4);
        assert!(h.query_by_label("Show 6 more lines").is_some());
    }

    #[test]
    fn dragging_a_cell_by_its_handle_reorders_it() {
        let mut h = typing_harness();
        h.set_size(vec2(900.0, 900.0));
        let ids = {
            let app = h.state_mut().as_mut().unwrap();
            app.replace_notebook(Notebook::from_text("# %%\na = 1\n# %%\nb = 2\n# %%\nc = 3\n"), None);
            app.cur.nb.cells().iter().map(|c| c.id).collect::<Vec<_>>()
        };
        settle(&mut h);
        h.run();
        let editors: Vec<_> = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).map(|n| n.rect()).collect();
        assert_eq!(editors.len(), 3);
        // point at the first cell so its tools show, then drag its handle down past the second cell's middle
        h.hover_at(editors[0].center());
        h.run_steps(3);
        let grip = h.get_by_label("Drag to move this cell").rect().center();
        h.hover_at(grip);
        h.run_steps(2);
        h.drag_at(grip);
        h.run_steps(2);
        let mut y = grip.y;
        while y < editors[1].center().y + 20.0 {
            y += 10.0;
            h.hover_at(egui::pos2(grip.x, y));
            h.run_steps(2);
        }
        h.drop_at(egui::pos2(grip.x, y));
        h.run_steps(3);
        let order: Vec<_> = h.state().as_ref().unwrap().cur.nb.cells().iter().map(|c| c.id).collect();
        assert_eq!(order, vec![ids[1], ids[0], ids[2]], "the first cell moved below the second");
        // one drag is one undo step
        let app = h.state_mut().as_mut().unwrap();
        app.undo_cells();
        let order: Vec<_> = app.cur.nb.cells().iter().map(|c| c.id).collect();
        assert_eq!(order, ids);
    }

    #[test]
    fn pointing_at_a_function_shows_its_signature() {
        let mut h = typing_harness();
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).type_text("y = diff(x, x)");
        h.run();
        let rect = h.get_by_role(egui::accesskit::Role::MultilineTextInput).rect();
        // two characters into the function name, vertically in the first row
        let font = 8.0;
        h.hover_at(egui::pos2(rect.left() + EDITOR_PAD_X + font * 6.5, rect.top() + 14.0));
        for _ in 0..12 {
            h.run_steps(2);
        }
        assert!(h.query_by_label_contains("diff(f, x[, n])").is_some(), "the tooltip names the signature");
        // away from any name: nothing
        h.hover_at(egui::pos2(rect.right() - 20.0, rect.top() + 14.0));
        for _ in 0..4 {
            h.run_steps(2);
        }
        assert!(h.query_by_label_contains("diff(f, x[, n])").is_none());
    }

    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn completion_screenshot() {
        let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
            let mut h = egui_kittest::Harness::builder()
                .wgpu()
                .with_size(vec2(900.0, 420.0))
                .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
            h.ctx.set_theme(pref);
            h.run();
            if let Some(app) = h.state_mut() {
                app.replace_notebook(Notebook::from_text("# %%\nvelocity = 3\n# %%\n"), None);
            }
            h.run();
            let editors = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).count();
            assert!(editors >= 2);
            h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).nth(1).unwrap().focus();
            h.run();
            h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).nth(1).unwrap().type_text("y = ex");
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("completion-{name}.png"))).unwrap();
        }
    }

    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn narrow_screenshot() {
        let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = egui_kittest::Harness::builder()
            .wgpu()
            .with_size(vec2(480.0, 600.0))
            .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
        h.ctx.set_theme(ThemePreference::Light);
        h.run_steps(4);
        if let Some(app) = h.state_mut() {
            app.show_help = true;
        }
        h.run_steps(6);
        h.render().unwrap().save(dir.join("narrow-help.png")).unwrap();
    }

    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn menu_screenshot() {
        let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        for (name, pref) in [("light", ThemePreference::Light), ("dark", ThemePreference::Dark)] {
            for menu in ["File", "Edit", "Settings"] {
                let mut h = egui_kittest::Harness::builder()
                    .wgpu()
                    .with_size(vec2(760.0, 640.0))
                    .with_pixels_per_point(2.0)
                    .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
                h.ctx.set_theme(pref);
                h.run_steps(4);
                h.get_by_label(menu).click();
                h.run_steps(6);
                h.render().unwrap().save(dir.join(format!("menu-{menu}-{name}.png"))).unwrap();
            }
        }
    }

    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn powers_screenshot() {
        let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = egui_kittest::Harness::builder()
            .wgpu()
            .with_size(vec2(760.0, 300.0))
            .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
        h.ctx.set_theme(ThemePreference::Light);
        h.run_steps(4);
        if let Some(app) = h.state_mut() {
            app.replace_notebook(Notebook::from_text("# %%\nf = x**2 + 3*y^3 - (a+b)**(n+1)\ndiff(f, x) / e^-x + 2**10\n"), None);
        }
        h.run_steps(6);
        h.render().unwrap().save(dir.join("powers.png")).unwrap();
    }

    /// `MATHCRAFT_SHOTS=<dir>` is set: `MATHCRAFT_SHOTS=/tmp/shots cargo test -p mathcraft screenshots -- --ignored`.
    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn screenshots() {
        let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        // `MATHCRAFT_NOTEBOOK=<file>` renders that notebook instead of the welcome one.
        let file = std::env::var_os("MATHCRAFT_NOTEBOOK").map(PathBuf::from);
        for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
            for (suffix, size, ppp) in [
                ("top", vec2(1060.0, 900.0), 2.0),
                (
                    "full",
                    vec2(
                        std::env::var("MATHCRAFT_WIDTH").ok().and_then(|w| w.parse().ok()).unwrap_or(1060.0),
                        std::env::var("MATHCRAFT_HEIGHT").ok().and_then(|h| h.parse().ok()).unwrap_or(if file.is_some() { 1100.0 } else { 3000.0 }),
                    ),
                    1.0,
                ),
            ] {
                let mut harness = egui_kittest::Harness::builder()
                    .wgpu()
                    .with_size(size)
                    .with_pixels_per_point(ppp)
                    .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), file.clone())).show(ui), None);
                harness.ctx.set_theme(pref);
                let start = Instant::now();
                loop {
                    harness.run_steps(3);
                    std::thread::sleep(Duration::from_millis(30));
                    if harness.state().as_ref().is_some_and(|a| !a.busy()) || start.elapsed() > Duration::from_secs(60) {
                        break;
                    }
                }
                harness.run_steps(6);
                let img = harness.render().unwrap();
                img.save(dir.join(format!("{name}-{suffix}.png"))).unwrap();
            }
        }
    }

    /// Three open documents, so the tab strip shows.
    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn tabs_screenshot() {
        let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
            let mut harness = egui_kittest::Harness::builder().wgpu().with_size(vec2(1000.0, 520.0)).build_ui_state(
                |ui, app: &mut Option<App>| {
                    app.get_or_insert_with(|| {
                        let mut app = App::with_context(ui.ctx().clone(), None);
                        app.open_in_tab(Notebook::from_text("# %%\nf = sin(x)**2\n# %%\ndiff(f, x)\n"), Some(PathBuf::from("/tmp/derivatives.py")));
                        app.open_in_tab(Notebook::from_text("# %%\nsolve(x**2 - 2, x)\n"), Some(PathBuf::from("/tmp/a_rather_long_notebook_name_here.py")));
                        app.cur.dirty = true;
                        // lay the toolbar out as the macOS title bar (the window buttons themselves are native, not drawn)
                        app.unified_title_bar = true;
                        app
                    })
                    .show(ui)
                },
                None,
            );
            harness.ctx.set_theme(pref);
            let start = Instant::now();
            while harness.state().as_ref().is_none_or(App::busy) && start.elapsed() < Duration::from_secs(30) {
                harness.run_steps(3);
                std::thread::sleep(Duration::from_millis(30));
            }
            harness.run_steps(6);
            harness.render().unwrap().save(dir.join(format!("tabs-{name}.png"))).unwrap();
        }
    }

    /// A new notebook (the empty state) with the function list open, in both themes.
    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn help_screenshot() {
        let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
            let mut h = egui_kittest::Harness::builder()
                .wgpu()
                .with_size(vec2(1000.0, 720.0))
                .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
            h.ctx.set_theme(pref);
            h.run();
            if let Some(app) = h.state_mut() {
                app.replace_notebook(Notebook::from_text("# %%\n"), None);
            }
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("empty-{name}.png"))).unwrap();
            if let Some(app) = h.state_mut() {
                app.show_help = true;
            }
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("help-{name}.png"))).unwrap();
        }
    }
}
