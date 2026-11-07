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
mod sidebar;
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
use std::path::PathBuf;
use std::time::Duration;
use tabs::{TabAction, TabInfo};
use theme::Theme;

const WELCOME: &str = include_str!("welcome.py");

fn main() -> eframe::Result {
    let path = std::env::args().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1060.0, 840.0]).with_min_inner_size([520.0, 380.0]).with_title("MathCraft"),
        ..Default::default()
    };
    eframe::run_native("MathCraft", options, Box::new(move |cc| Ok(Box::new(App::new(cc, path)))))
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
    Mode(RunMode),
    View(View),
    ToggleSidebar,
    Functions,
    Theme(ThemePreference),
    ToggleVim,
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
    vim_status: String,
    palette: PaletteState,
    dark: Theme,
    light: Theme,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        Self::with_context(cc.egui_ctx.clone(), path)
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
            vim_status: String::new(),
            palette: PaletteState::default(),
            dark,
            light,
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
            item("Settings", &format!("Vim keys ({})", on(self.vim)), String::new(), Cmd::ToggleVim),
            item("Settings", &format!("Strict names ({})", on(self.cur.nb.strict)), String::new(), Cmd::ToggleStrict),
            item("Cell", "Add a code cell", String::new(), Cmd::AddCell(CellKind::Code)),
            item("Cell", "Add a text cell", String::new(), Cmd::AddCell(CellKind::Markdown)),
        ];
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
            v.push(item("Function", sig, String::new(), Cmd::Help((*name).to_owned())));
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
            Cmd::Mode(m) => self.cur.nb.mode = m,
            Cmd::View(v) => self.cur.set_view(v),
            Cmd::ToggleSidebar => self.toggle_sidebar(),
            Cmd::Functions => self.show_help = true,
            Cmd::Theme(t) => ctx.set_theme(t),
            Cmd::ToggleVim => self.vim = !self.vim,
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
                self.cur.path = Some(target);
                self.cur.dirty = false;
                self.rescan();
            }
            Err(e) => self.status = format!("Could not save: {e}"),
        }
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
                            self.cur.dirty = true;
                            self.cur.nb.add_cell(Some(id), CellKind::Code, "")
                        }
                    };
                    self.cur.focus = Some(target);
                }
                Action::AddAfter(after, kind) => {
                    let id = self.cur.nb.add_cell(after, kind, "");
                    if kind == CellKind::Markdown {
                        self.cur.editing_md.insert(id);
                    }
                    self.cur.focus = Some(id);
                    self.cur.dirty = true;
                }
                Action::Delete(id) => {
                    self.cur.nb.delete_cell(id);
                    self.cur.kernel.forget_cell(id);
                    self.cur.dirty = true;
                    if self.cur.nb.cells().is_empty() {
                        self.cur.nb.add_cell(None, CellKind::Code, "");
                    }
                }
                Action::Move(id, delta) => {
                    self.cur.nb.move_cell(id, delta);
                    self.cur.dirty = true;
                }
                Action::Command(line) => self.ex_command(line.trim()),
            }
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

    fn menu_item(ui: &mut egui::Ui, ctx: &egui::Context, label: &str, keys: Option<(Modifiers, Key)>) -> bool {
        let mut b = egui::Button::new(label);
        if let Some((m, k)) = keys {
            b = b.shortcut_text(Self::shortcut(ctx, m, k));
        }
        let clicked = ui.add(b).clicked();
        if clicked {
            ui.close();
        }
        clicked
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if Self::menu_item(ui, &ctx, "New", Some((Modifiers::COMMAND, Key::N))) {
                    self.new_file();
                }
                if Self::menu_item(ui, &ctx, "Open…", Some((Modifiers::COMMAND, Key::O))) {
                    self.open_file();
                }
                if Self::menu_item(ui, &ctx, "Open Folder…", None) {
                    self.open_folder();
                }
                if Self::menu_item(ui, &ctx, "Show or hide sidebar", Some((Modifiers::COMMAND, Key::B))) {
                    self.toggle_sidebar();
                }
                ui.separator();
                if Self::menu_item(ui, &ctx, "Close tab", Some((Modifiers::COMMAND, Key::W))) {
                    self.close_tab(self.cur.id);
                }
                ui.separator();
                if Self::menu_item(ui, &ctx, "Save", Some((Modifiers::COMMAND, Key::S))) {
                    self.save(false);
                }
                if Self::menu_item(ui, &ctx, "Save As…", None) {
                    self.save(true);
                }
            });
            ui.menu_button("Run", |ui| {
                if Self::menu_item(ui, &ctx, "Run all", Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter))) {
                    self.run(RunRequest::All);
                }
                if Self::menu_item(ui, &ctx, "Run stale cells", None) {
                    self.run(RunRequest::Stale);
                }
                if Self::menu_item(ui, &ctx, "Interrupt", Some((Modifiers::COMMAND, Key::Period))) {
                    self.cur.kernel.interrupt();
                }
            });
            ui.menu_button("View", |ui| {
                let mut view = self.cur.view;
                ui.radio_value(&mut view, View::Notebook, "Notebook: one block per cell");
                ui.radio_value(&mut view, View::Text, "Text: the whole file in one editor");
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
                ui.radio_value(&mut pref, ThemePreference::System, "Match system");
                ui.radio_value(&mut pref, ThemePreference::Light, "Light");
                ui.radio_value(&mut pref, ThemePreference::Dark, "Dark");
                ctx.set_theme(pref);
                ui.separator();
                menu_section(ui, theme, "When a cell runs");
                ui.radio_value(&mut self.cur.nb.mode, RunMode::Auto, "Also run the cells that depend on it");
                ui.radio_value(&mut self.cur.nb.mode, RunMode::Lazy, "Only mark dependent cells as stale");
                ui.separator();
                menu_section(ui, theme, "Names");
                ui.checkbox(&mut self.cur.nb.strict, "Strict: declare symbols with symbols()");
                ui.separator();
                menu_section(ui, theme, "Editing");
                ui.checkbox(&mut self.vim, "Vim keys");
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
                vertical_rule(ui, theme);
                document_title(ui, theme, self.cur.path.as_ref(), self.cur.dirty);
            });
        });
    }

    fn help_window(&mut self, ctx: &egui::Context, theme: &Theme) {
        let mut open = self.show_help;
        egui::Window::new(RichText::new("Functions").font(egui::FontId::new(14.0, fonts::strong_family())))
            .open(&mut open)
            .default_rect(egui::Rect::from_min_size(ctx.content_rect().center_top() + vec2(-320.0, 64.0), vec2(640.0, 500.0)))
            .max_width(720.0)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.help_filter)
                        .hint_text(RichText::new("Search by name or description").color(theme.text_faint))
                        .margin(Margin::symmetric(8, 5))
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(4.0);
                ui.label(RichText::new("⇧↵ runs a cell and moves on   ⌘↵ runs it in place   ⌘⇧↵ runs all   ⌘. stops").size(11.5).color(theme.text_faint));
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
            let dim = |t: String| RichText::new(t).size(11.5).color(theme.text_dim);
            let cells = self.cur.nb.cells().len();
            ui.label(dim(format!("{cells} {}", if cells == 1 { "cell" } else { "cells" })));
            for (n, label, color) in [(fresh, "up to date", theme.ok), (stale, "stale", theme.warn), (errors, "with problems", theme.danger)] {
                if n > 0 {
                    ui.add_space(6.0);
                    dot(ui, color);
                    ui.label(dim(format!("{n} {label}")));
                }
            }
            if self.vim && !self.vim_status.is_empty() {
                ui.add_space(10.0);
                ui.label(RichText::new(self.vim_status.clone()).monospace().size(11.5).color(theme.accent));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mode = if self.cur.nb.mode == RunMode::Auto { "Dependents run automatically" } else { "Dependents wait (lazy)" };
                ui.label(dim(mode.into())).on_hover_text("Change in Settings");
                if !self.status.is_empty() {
                    ui.add_space(10.0);
                    ui.add(egui::Label::new(RichText::new(self.status.clone()).size(11.5).color(theme.text_faint)).truncate());
                }
            });
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(root);
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
            hairline(root, &theme, strip.response.rect.x_range(), strip.response.rect.bottom() - 0.5);
            match strip.inner {
                Some(TabAction::Select(id)) => self.select_tab(id),
                Some(TabAction::Close(id)) => self.close_tab(id),
                Some(TabAction::New) => self.new_file(),
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
            let panel = egui::Panel::right("results")
                .resizable(true)
                .default_size(400.0)
                .size_range(240.0..=800.0)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(theme.bg))
                .show(root, |ui| {
                    let mut env = Env {
                        theme: &theme,
                        math: &mut self.cur.math,
                        generations: &self.cur.generations,
                        focus: None,
                        editing_md: &mut self.cur.editing_md,
                        names: &names,
                        actions: &mut actions,
                        vim: self.vim,
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
                            let mut env = Env {
                                theme: &theme,
                                math: &mut self.cur.math,
                                generations: &self.cur.generations,
                                focus,
                                editing_md: &mut self.cur.editing_md,
                                names: &names,
                                actions: &mut actions,
                                vim: self.vim,
                                vim_status: &mut self.vim_status,
                            };
                            for (i, cell) in self.cur.nb.cells().iter().enumerate() {
                                cell_ui(ui, &mut env, cell, i, &self.cur.nb);
                                add_row(ui, &mut env, cell.id);
                            }
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
                self.run_command(item.payload, ctx);
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
                    ("⇧ ↵", "Run the cell and move to the next one"),
                    ("⌘ ↵", "Run the cell and stay"),
                    ("⌘ ⇧ ↵", "Run every cell"),
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
    let galley = ui.painter().layout_no_wrap(keys.to_string(), egui::FontId::proportional(12.0), theme.text_dim);
    let (r, _) = ui.allocate_exact_size(vec2(galley.size().x + 12.0, 20.0), egui::Sense::hover());
    ui.painter().rect(r, 5.0, theme.code_bg, Stroke::new(1.0, theme.border_strong), egui::StrokeKind::Inside);
    ui.painter().galley(r.center() - galley.size() / 2.0, galley, theme.text_dim);
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

/// A quiet heading inside a menu.
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
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(code) = code {
                    ui.label(RichText::new(code).monospace().size(11.5).color(color));
                }
                ui.add(egui::Label::new(RichText::new(message).size(13.5).color(theme.text)).selectable(true));
            });
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

fn cell_ui(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, index: usize, nb: &Notebook) {
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
    let active = focused || ui.rect_contains_pointer(row);

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
        let size = vec2(26.0 * 3.0 + 4.0, 26.0);
        let at = egui::Rect::from_min_size(egui::pos2(row.right() - size.x - 5.0, row.top() + 3.0), size);
        let mut tools = ui.new_child(egui::UiBuilder::new().max_rect(at).layout(egui::Layout::left_to_right(egui::Align::Center)));
        tools.spacing_mut().item_spacing.x = 2.0;
        let strip = at.expand(2.0);
        tools.painter().add(theme.popup_shadow_small().as_shape(strip, theme::RADIUS_LARGE));
        tools.painter().rect(strip, theme::RADIUS_LARGE, theme.surface, Stroke::new(1.0, theme.border_strong), egui::StrokeKind::Inside);
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
            highlight::highlight(buf.as_str(), font.clone(), &syntax)
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
                        ui.painter().rect_filled(rect, theme::RADIUS, theme.accent.gamma_multiply(if theme.dark { 0.32 } else { 0.14 }));
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
                ui.add(egui::Label::new(RichText::new(o.stdout.trim_end()).monospace().size(13.0).color(theme.text_dim)).selectable(true));
                if has_value {
                    ui.add_space(8.0);
                }
            }
            match &o.result {
                Ok(Some(Value::Plot(p))) => plot_ui(ui, theme, cell.id, p),
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

fn value_ui(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, v: &Value) {
    if let Value::Array(a) = v {
        array_ui(ui, env, cell, v, &a.describe());
        return;
    }
    value_math(ui, env, cell, v);
}

/// A vector or matrix: typeset, with its kind, shape and element type underneath.
fn array_ui(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, v: &Value, caption: &str) {
    value_math(ui, env, cell, v);
    ui.add_space(4.0);
    ui.label(RichText::new(caption).size(11.5).color(env.theme.text_faint));
}

fn value_math(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, v: &Value) {
    let generation = env.generations.get(&cell.id).copied().unwrap_or(0);
    match env.math.get(ui.ctx(), cell.id, generation, v) {
        Some(layout) => {
            let theme = env.theme;
            let resp = if layout.width + 8.0 > ui.available_width() {
                egui::ScrollArea::horizontal().id_salt(("out", cell.id)).show(ui, |ui| math_view::show(ui, &layout, theme, math_view::SIZE)).inner
            } else {
                math_view::show(ui, &layout, theme, math_view::SIZE)
            };
            resp.context_menu(|ui| {
                if ui.button("Copy as text").clicked() {
                    ui.ctx().copy_text(v.to_string());
                    ui.close();
                }
            });
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

fn plot_ui(ui: &mut egui::Ui, theme: &Theme, id: CellId, p: &mathcraft_eval::PlotSpec) {
    ui.scope(|ui| {
        // The plot draws its frame, labels and legend from the visuals; keep them quiet and on the page colour.
        let v = ui.visuals_mut();
        v.override_text_color = Some(theme.text_dim);
        v.extreme_bg_color = theme.bg;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, theme.border);
        let mut plot = egui_plot::Plot::new(egui::Id::unique(("plot", id)))
            .height(280.0)
            .grid_spacing(80.0..=400.0)
            .allow_scroll(false)
            .show_grid(false)
            .x_axis_label(p.x_label.clone());
        if p.series.len() > 1 {
            plot = plot.legend(egui_plot::Legend::default().position(egui_plot::Corner::RightTop));
        }
        plot.show(ui, |plot| {
            for (i, s) in p.series.iter().enumerate() {
                let color = theme.plot.get(i % theme.plot.len()).copied().unwrap_or(theme.accent);
                for seg in &s.segments {
                    plot.line(egui_plot::Line::new(s.label.clone(), egui_plot::PlotPoints::from(seg.clone())).color(color).width(1.75));
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use std::time::Instant;

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
                            Notebook::from_text("# %% [markdown]\n# # Derivatives\n# %%\nf = sin(x)**2\n# %%\ndiff(f, x)\n# %%\nsolve(x**3 - 3*x + 1, x)\n# %%\nplot(f, (x, 0, 6))\n"),
                            None,
                        );
                        app.cur.set_view(View::Text);
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

    /// Renders the notebook with the GPU, in both themes, to PNG files for a visual check. Skipped unless
    /// A headless harness around one empty code cell, with the editor focused.
    fn typing_harness() -> egui_kittest::Harness<'static, Option<App>> {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(900.0, 600.0))
            .build_ui_state(|ui, app: &mut Option<App>| app.get_or_insert_with(|| App::with_context(ui.ctx().clone(), None)).show(ui), None);
        harness.run();
        if let Some(app) = harness.state_mut() {
            app.replace_notebook(Notebook::from_text("# %%\n"), None);
        }
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
        while h.state().as_ref().is_some_and(App::busy) && start.elapsed() < Duration::from_secs(20) {
            h.run();
            std::thread::sleep(Duration::from_millis(10));
        }
        h.run();
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

    /// `MATHCRAFT_SHOTS=<dir>` is set: `MATHCRAFT_SHOTS=/tmp/shots cargo test -p mathcraft screenshots -- --ignored`.
    #[test]
    #[ignore = "renders with the GPU; needs MATHCRAFT_SHOTS=<dir>"]
    fn screenshots() {
        let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        // `MATHCRAFT_NOTEBOOK=<file>` renders that notebook instead of the welcome one.
        let file = std::env::var_os("MATHCRAFT_NOTEBOOK").map(PathBuf::from);
        for (name, pref) in [("dark", ThemePreference::Dark), ("light", ThemePreference::Light)] {
            for (suffix, size, ppp) in [("top", vec2(1060.0, 900.0), 2.0), ("full", vec2(1060.0, if file.is_some() { 1100.0 } else { 3000.0 }), 1.0)] {
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
