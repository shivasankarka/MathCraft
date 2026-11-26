//! The menu bar at the top of the screen (macOS).
//!
//! What the menus hold is plain data ([`spec`]), so it is the same on every platform and tests can check it: each
//! entry names a command ([`command`]) that the app already runs from the palette, and each checkable entry reads its
//! state from the app ([`App::menu_checked`]). Only [`NativeMenu`] touches the system menu; elsewhere the toolbar
//! keeps its own menus and `install` returns nothing.
//!
//! A key written on a menu entry is claimed by the system before the window sees it, so entries carry a key only
//! where the window did the same thing with it (open, save, find, ...). Cut, copy, paste and select all are left
//! out: the editors handle those keys themselves, and a system item would take them away.

use super::*;

/// A menu in the bar.
pub(crate) struct Menu {
    pub title: &'static str,
    pub rows: Vec<Row>,
}

/// One row of a menu.
pub(crate) enum Row {
    /// Runs the command named by `id`. `keys` is an accelerator such as `"cmd+shift+enter"`.
    Item {
        id: &'static str,
        title: &'static str,
        keys: Option<&'static str>,
    },
    /// Like `Item`, with a check mark that follows [`App::menu_checked`].
    Check {
        id: &'static str,
        title: &'static str,
        keys: Option<&'static str>,
    },
    Separator,
    Submenu {
        title: &'static str,
        rows: Vec<Row>,
    },
    /// The bundled notebooks, one entry each (`example:N`).
    Examples,
    /// The recent files, one entry each (`recent:N`); the submenu is disabled when there are none.
    Recent,
    /// A standard system entry.
    System(System),
}

/// Standard entries the system supplies (their titles and keys are the system's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum System {
    About,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Minimize,
    Zoom,
    Fullscreen,
    BringAllToFront,
}

fn item(id: &'static str, title: &'static str, keys: Option<&'static str>) -> Row {
    Row::Item { id, title, keys }
}

fn check(id: &'static str, title: &'static str, keys: Option<&'static str>) -> Row {
    Row::Check { id, title, keys }
}

fn submenu(title: &'static str, rows: Vec<Row>) -> Row {
    Row::Submenu { title, rows }
}

/// The ids of the accent colours' entries, in the order of `settings::ACCENTS`.
const ACCENT_IDS: [&str; 6] = ["accent:blue", "accent:purple", "accent:pink", "accent:orange", "accent:green", "accent:graphite"];

/// The whole menu bar.
pub(crate) fn spec() -> Vec<Menu> {
    let accents = settings::ACCENTS.iter().zip(ACCENT_IDS).map(|((name, ..), id)| Row::Check { id, title: name, keys: None }).collect();
    vec![
        Menu {
            title: "MathCraft",
            rows: vec![
                Row::System(System::About),
                Row::Separator,
                Row::System(System::Services),
                Row::Separator,
                Row::System(System::Hide),
                Row::System(System::HideOthers),
                Row::System(System::ShowAll),
                Row::Separator,
                item("quit", "Quit MathCraft", Some("cmd+q")),
            ],
        },
        Menu {
            title: "File",
            rows: vec![
                item("new", "New", Some("cmd+n")),
                item("open", "Open…", Some("cmd+o")),
                item("open-folder", "Open Folder…", None),
                Row::Recent,
                submenu("Open Example", vec![Row::Examples]),
                Row::Separator,
                item("close-tab", "Close Tab", Some("cmd+w")),
                item("save", "Save", Some("cmd+s")),
                item("save-as", "Save As…", None),
                item("export-html", "Export as HTML…", None),
                item("export-pdf", "Export as PDF…", None),
            ],
        },
        Menu {
            title: "Edit",
            rows: vec![
                item("undo-cell", "Undo Cell Change", None),
                item("redo-cell", "Redo Cell Change", None),
                Row::Separator,
                item("find", "Find and Replace…", Some("cmd+f")),
                item("add-code", "Add Code Cell", None),
                item("add-text", "Add Text Cell", None),
            ],
        },
        Menu {
            title: "Run",
            rows: vec![
                item("run-all", "Run All", Some("cmd+shift+enter")),
                item("run-stale", "Run Stale Cells", None),
                item("interrupt", "Interrupt", Some("cmd+.")),
            ],
        },
        Menu {
            title: "View",
            rows: vec![
                check("view:notebook", "Notebook", None),
                check("view:text", "Text", Some("cmd+shift+t")),
                Row::Separator,
                check("sidebar", "Sidebar", Some("cmd+b")),
                check("actions", "Action Row", None),
                Row::Separator,
                check("functions", "Functions", Some("f1")),
                check("symbols", "Symbols", Some("f2")),
                item("changes", "Changes Since Last Commit", None),
                Row::Separator,
                Row::System(System::Fullscreen),
            ],
        },
        Menu {
            title: "Settings",
            rows: vec![
                submenu(
                    "Appearance",
                    vec![check("theme:system", "Match System", None), check("theme:light", "Light", None), check("theme:dark", "Dark", None)],
                ),
                submenu("Accent", accents),
                submenu(
                    "When a Cell Runs",
                    vec![check("mode:auto", "Also Run the Cells That Depend on It", None), check("mode:lazy", "Only Mark Dependent Cells as Stale", None)],
                ),
                Row::Separator,
                check("strict", "Strict Names", None),
                check("vim", "Vim Keys", None),
                check("pretty", "Powers as Superscripts in Cells", None),
                item("reload-theme", "Reload theme.json", None),
            ],
        },
        Menu { title: "Window", rows: vec![Row::System(System::Minimize), Row::System(System::Zoom), Row::Separator, Row::System(System::BringAllToFront)] },
        Menu { title: "Help", rows: vec![item("commands", "Search Commands…", Some("cmd+p"))] },
    ]
}

/// Every id of a checkable entry, in menu order.
pub(crate) fn check_ids(rows: &[Menu]) -> Vec<&'static str> {
    fn walk(rows: &[Row], out: &mut Vec<&'static str>) {
        for row in rows {
            match row {
                Row::Check { id, .. } => out.push(id),
                Row::Submenu { rows, .. } => walk(rows, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for menu in rows {
        walk(&menu.rows, &mut out);
    }
    out
}

/// What the entry `id` does, given the recent files (`recent:N` counts into that list).
pub(crate) fn command(id: &str, recent: &[PathBuf]) -> Option<Cmd> {
    if let Some(n) = id.strip_prefix("example:") {
        return n.parse().ok().filter(|n| *n < examples::EXAMPLES.len()).map(Cmd::Example);
    }
    if let Some(n) = id.strip_prefix("recent:") {
        return n.parse::<usize>().ok().and_then(|n| recent.get(n)).map(|p| Cmd::File(p.clone()));
    }
    if let Some(name) = id.strip_prefix("accent:") {
        let (name, ..) = settings::ACCENTS.iter().find(|(n, ..)| *n == name)?;
        return Some(Cmd::Accent((*name != "blue").then_some(*name)));
    }
    Some(match id {
        "new" => Cmd::New,
        "open" => Cmd::Open,
        "open-folder" => Cmd::OpenFolder,
        "close-tab" => Cmd::CloseTab,
        "save" => Cmd::Save,
        "save-as" => Cmd::SaveAs,
        "export-html" => Cmd::ExportHtml,
        "export-pdf" => Cmd::ExportPdf,
        "quit" => Cmd::Quit,
        "undo-cell" => Cmd::UndoCell,
        "redo-cell" => Cmd::RedoCell,
        "find" => Cmd::Find,
        "add-code" => Cmd::AddCell(CellKind::Code),
        "add-text" => Cmd::AddCell(CellKind::Markdown),
        "run-all" => Cmd::RunAll,
        "run-stale" => Cmd::RunStale,
        "interrupt" => Cmd::Interrupt,
        "view:notebook" => Cmd::View(View::Notebook),
        "view:text" => Cmd::ToggleTextView,
        "sidebar" => Cmd::ToggleSidebar,
        "actions" => Cmd::ToggleActions,
        "functions" => Cmd::ToggleFunctions,
        "symbols" => Cmd::ToggleSymbols,
        "changes" => Cmd::Changes,
        "theme:system" => Cmd::Theme(ThemePreference::System),
        "theme:light" => Cmd::Theme(ThemePreference::Light),
        "theme:dark" => Cmd::Theme(ThemePreference::Dark),
        "mode:auto" => Cmd::Mode(RunMode::Auto),
        "mode:lazy" => Cmd::Mode(RunMode::Lazy),
        "strict" => Cmd::ToggleStrict,
        "vim" => Cmd::ToggleVim,
        "pretty" => Cmd::TogglePretty,
        "reload-theme" => Cmd::ReloadTheme,
        "commands" => Cmd::Commands,
        _ => return None,
    })
}

/// What the menus show, read from the app. Two equal values need no update of the menu.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct State {
    pub checked: Vec<(&'static str, bool)>,
    pub can_undo: bool,
    pub can_redo: bool,
    /// The recent files that still exist, as `(index into the app's list, name)`.
    pub recent: Vec<(usize, String)>,
}

impl App {
    /// Whether the checkable entry `id` is on.
    pub(crate) fn menu_checked(&self, ctx: &egui::Context, id: &str) -> bool {
        let pref = ctx.options(|o| o.theme_preference);
        match id {
            "view:notebook" => self.cur.view == View::Notebook,
            "view:text" => self.cur.view == View::Text,
            "sidebar" => self.show_sidebar,
            "actions" => self.show_actions,
            "functions" => self.show_help,
            "symbols" => self.show_symbols,
            "theme:system" => pref == ThemePreference::System,
            "theme:light" => pref == ThemePreference::Light,
            "theme:dark" => pref == ThemePreference::Dark,
            "mode:auto" => self.cur.nb.mode == RunMode::Auto,
            "mode:lazy" => self.cur.nb.mode == RunMode::Lazy,
            "strict" => self.cur.nb.strict,
            "vim" => self.vim,
            "pretty" => self.pretty,
            _ => id.strip_prefix("accent:").is_some_and(|name| self.accent.as_deref().unwrap_or("blue") == name),
        }
    }

    /// What the menus should show now, for the entries in `checks`.
    pub(crate) fn menu_state(&self, ctx: &egui::Context, checks: &[&'static str]) -> State {
        State {
            checked: checks.iter().map(|id| (*id, self.menu_checked(ctx, id))).collect(),
            can_undo: self.cur.can_undo(),
            can_redo: self.cur.can_redo(),
            recent: self.recent.iter().enumerate().filter(|(_, p)| p.is_file()).map(|(i, p)| (i, file_name(p))).collect(),
        }
    }

    /// Run what was chosen in the menu bar since the last frame, then bring the menus up to date.
    pub(crate) fn poll_native_menu(&mut self, ctx: &egui::Context) {
        let Some(menu) = self.native_menu.take() else { return };
        let chosen = menu.chosen();
        for id in &chosen {
            if let Some(cmd) = command(id, &self.recent) {
                self.run_command(cmd, ctx);
            }
        }
        if !chosen.is_empty() {
            // some commands take effect on the next frame (the theme), and the menus follow it
            ctx.request_repaint();
        }
        let state = self.menu_state(ctx, menu.checks());
        // A click flips the check mark itself, so after one the marks are set again even when nothing changed.
        self.native_menu = Some(menu.show(state, !chosen.is_empty()));
    }
}

#[cfg(target_os = "macos")]
pub(crate) use mac::NativeMenu;

#[cfg(not(target_os = "macos"))]
pub(crate) use other::NativeMenu;

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use muda::accelerator::Accelerator;
    use muda::{AboutMetadata, CheckMenuItem, Menu as SysMenu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
    use std::collections::HashMap;
    use std::sync::mpsc::{Receiver, channel};

    /// The system menu bar, the entries that change, and what they showed last.
    pub(crate) struct NativeMenu {
        _bar: SysMenu,
        events: Receiver<String>,
        ids: Vec<&'static str>,
        checks: HashMap<&'static str, CheckMenuItem>,
        undo: MenuItem,
        redo: MenuItem,
        recent: Submenu,
        shown: Option<State>,
    }

    impl NativeMenu {
        /// Replace the application's menu bar. Must run on the main thread once the event loop has started (the app is
        /// created there). `None` when the system refuses an entry; the toolbar menus stay in that case.
        pub(crate) fn install(ctx: &egui::Context) -> Option<Self> {
            let (tx, events) = channel();
            let repaint = ctx.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if tx.send(e.id().0.clone()).is_ok() {
                    repaint.request_repaint();
                }
            }));
            let menus = spec();
            let bar = SysMenu::new();
            let mut parts = Parts::default();
            for menu in &menus {
                let sub = Submenu::new(menu.title, true);
                fill(&sub, &menu.rows, &mut parts).ok()?;
                bar.append(&sub).ok()?;
                match menu.title {
                    "Window" => sub.set_as_windows_menu_for_nsapp(),
                    "Help" => sub.set_as_help_menu_for_nsapp(),
                    _ => {}
                }
            }
            bar.init_for_nsapp();
            let Parts { checks, mut plain, recent } = parts;
            Some(Self {
                _bar: bar,
                events,
                ids: check_ids(&menus),
                checks,
                undo: plain.remove("undo-cell")?,
                redo: plain.remove("redo-cell")?,
                recent: recent?,
                shown: None,
            })
        }

        pub(crate) fn checks(&self) -> &[&'static str] {
            &self.ids
        }

        /// The ids of the entries chosen since the last call.
        pub(crate) fn chosen(&self) -> Vec<String> {
            self.events.try_iter().collect()
        }

        /// Bring the menus up to date with `state` (all of it again when `force`).
        pub(crate) fn show(mut self, state: State, force: bool) -> Self {
            if force || self.shown.as_ref() != Some(&state) {
                for (id, on) in &state.checked {
                    if let Some(item) = self.checks.get(id) {
                        item.set_checked(*on);
                    }
                }
                self.undo.set_enabled(state.can_undo);
                self.redo.set_enabled(state.can_redo);
                if self.shown.as_ref().is_none_or(|s| s.recent != state.recent) {
                    while self.recent.remove_at(0).is_some() {}
                    for (i, name) in &state.recent {
                        let _ = self.recent.append(&MenuItem::with_id(format!("recent:{i}"), name, true, None));
                    }
                    self.recent.set_enabled(!state.recent.is_empty());
                }
                self.shown = Some(state);
            }
            self
        }
    }

    fn accelerator(keys: Option<&str>) -> Option<Accelerator> {
        keys.and_then(|k| k.parse().ok())
    }

    fn system(kind: System) -> PredefinedMenuItem {
        match kind {
            System::About => PredefinedMenuItem::about(
                Some("About MathCraft"),
                Some(AboutMetadata {
                    name: Some("MathCraft".into()),
                    version: Some(env!("CARGO_PKG_VERSION").into()),
                    comments: Some("A reactive symbolic-math notebook".into()),
                    license: Some(env!("CARGO_PKG_LICENSE").into()),
                    website: Some("https://github.com/shivasankarka/MathCraft".into()),
                    ..Default::default()
                }),
            ),
            System::Services => PredefinedMenuItem::services(None),
            System::Hide => PredefinedMenuItem::hide(None),
            System::HideOthers => PredefinedMenuItem::hide_others(None),
            System::ShowAll => PredefinedMenuItem::show_all(None),
            System::Minimize => PredefinedMenuItem::minimize(None),
            System::Zoom => PredefinedMenuItem::maximize(None),
            System::Fullscreen => PredefinedMenuItem::fullscreen(None),
            System::BringAllToFront => PredefinedMenuItem::bring_all_to_front(None),
        }
    }

    /// The entries that change after the menu is built.
    #[derive(Default)]
    struct Parts {
        checks: HashMap<&'static str, CheckMenuItem>,
        plain: HashMap<&'static str, MenuItem>,
        recent: Option<Submenu>,
    }

    /// Add `rows` to `sub`, keeping the entries that change.
    fn fill(sub: &Submenu, rows: &[Row], parts: &mut Parts) -> muda::Result<()> {
        for row in rows {
            match row {
                Row::Item { id, title, keys } => {
                    let item = MenuItem::with_id(*id, title, true, accelerator(*keys));
                    sub.append(&item)?;
                    parts.plain.insert(id, item);
                }
                Row::Check { id, title, keys } => {
                    let item = CheckMenuItem::with_id(*id, title, true, false, accelerator(*keys));
                    sub.append(&item)?;
                    parts.checks.insert(id, item);
                }
                Row::Separator => sub.append(&PredefinedMenuItem::separator())?,
                Row::Submenu { title, rows } => {
                    let inner = Submenu::new(*title, true);
                    fill(&inner, rows, parts)?;
                    sub.append(&inner)?;
                }
                Row::Examples => {
                    for (i, (name, ..)) in examples::EXAMPLES.iter().enumerate() {
                        sub.append(&MenuItem::with_id(format!("example:{i}"), name, true, None))?;
                    }
                }
                Row::Recent => {
                    let inner = Submenu::new("Open Recent", false);
                    sub.append(&inner)?;
                    parts.recent = Some(inner);
                }
                Row::System(kind) => sub.append(&system(*kind))?,
            }
        }
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
mod other {
    use super::*;

    /// There is no system menu bar here: the toolbar has the menus.
    pub(crate) struct NativeMenu;

    impl NativeMenu {
        pub(crate) fn install(_: &egui::Context) -> Option<Self> {
            None
        }

        pub(crate) fn checks(&self) -> &[&'static str] {
            &[]
        }

        pub(crate) fn chosen(&self) -> Vec<String> {
            Vec::new()
        }

        pub(crate) fn show(self, _: State, _: bool) -> Self {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(rows: &[Row], out: &mut Vec<&'static str>) {
        for row in rows {
            match row {
                Row::Item { id, .. } | Row::Check { id, .. } => out.push(id),
                Row::Submenu { rows, .. } => ids(rows, out),
                _ => {}
            }
        }
    }

    #[test]
    fn every_entry_runs_a_command() {
        let mut all = Vec::new();
        for menu in spec() {
            ids(&menu.rows, &mut all);
        }
        assert!(all.len() > 30);
        for id in &all {
            assert!(command(id, &[]).is_some(), "{id} runs nothing");
        }
        let mut sorted = all.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len(), "an id is used twice");
        assert!(command("nonsense", &[]).is_none());
    }

    #[test]
    fn accelerators_are_unique_and_leave_editing_keys_alone() {
        fn keys(rows: &[Row], out: &mut Vec<&'static str>) {
            for row in rows {
                match row {
                    Row::Item { keys: Some(k), .. } | Row::Check { keys: Some(k), .. } => out.push(k),
                    Row::Submenu { rows, .. } => keys(rows, out),
                    _ => {}
                }
            }
        }
        let mut all = Vec::new();
        for menu in spec() {
            keys(&menu.rows, &mut all);
        }
        let mut sorted = all.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len(), "two entries share a key");
        // the editors handle these themselves
        for taken in ["cmd+c", "cmd+x", "cmd+v", "cmd+a", "cmd+z", "cmd+shift+z"] {
            assert!(!all.contains(&taken), "{taken} belongs to the editors");
        }
    }

    #[test]
    fn examples_recent_files_and_accents_resolve() {
        for i in 0..examples::EXAMPLES.len() {
            assert!(matches!(command(&format!("example:{i}"), &[]), Some(Cmd::Example(n)) if n == i));
        }
        assert!(command(&format!("example:{}", examples::EXAMPLES.len()), &[]).is_none());
        let recent = vec![PathBuf::from("/a.py"), PathBuf::from("/b.py")];
        assert!(matches!(command("recent:1", &recent), Some(Cmd::File(p)) if p == Path::new("/b.py")));
        assert!(command("recent:2", &recent).is_none());
        assert!(matches!(command("accent:blue", &[]), Some(Cmd::Accent(None))));
        assert!(matches!(command("accent:green", &[]), Some(Cmd::Accent(Some("green")))));
        // one entry per accent colour
        let named: Vec<&str> = settings::ACCENTS.iter().map(|(n, ..)| *n).collect();
        for (id, name) in ACCENT_IDS.iter().zip(named) {
            assert_eq!(id.strip_prefix("accent:"), Some(name));
        }
    }
}
