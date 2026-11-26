//! MathCraft desktop app: a reactive symbolic-math notebook (egui).
//!
//! The UI is a thin client: the notebook model, dependency analysis and scheduling live in `mathcraft-notebook`,
//! and computation runs on its kernel thread, so the window never blocks.

// Release builds on Windows are GUI programs: without this, double-clicking opens a console window too.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod app_chrome;
mod app_commands;
mod app_find;
mod app_keys;
mod app_state;
mod app_tabs;
mod app_view;
mod app_workspace;
mod cell_view;
mod complete;
mod completion_ui;
mod diff;
mod doc;
mod examples;
mod export_html;
mod find;
mod fonts;
mod git;
mod highlight;
mod icons;
mod markdown;
mod math_view;
mod native_menu;
mod output;
mod palette;
mod plot_export;
mod plot_png;
mod plot_view;
mod session;
mod settings;
mod sidebar;
mod slider;
mod store;
mod symbols;
mod tabs;
mod text_view;
mod theme;
mod vim_edit;
mod widgets;
mod workspace;

#[cfg(test)]
mod tests;

use cell_view::*;
use completion_ui::*;
use doc::{Doc, View, file_name};
use eframe::egui::{self, Color32, Key, Margin, Modifiers, RichText, Stroke, TextStyle, ThemePreference, vec2};
use egui::text_selection::CCursorRange;
use icons::Icon;
use math_view::MathCache;
use mathcraft_eval::{DOCS, Value};
use mathcraft_notebook::{Cell, CellId, CellKind, CellState, Notebook, RunMode, RunRequest};
use output::*;
use palette::{Item, PaletteState};
use plot_view::*;
use sidebar::{OpenFile, SideAction};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tabs::{TabAction, TabInfo};
use theme::Theme;
use widgets::*;

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

/// The notebook at `path` as of the last commit, in the form `Notebook::to_text` writes (so formatting alone is not a change).
fn committed(path: &Path) -> Option<String> {
    git::head_text(path).map(|text| mathcraft_notebook::format::write(&mathcraft_notebook::format::parse(&text)))
}

/// The open "Changes" window: the committed text of the notebook, and the diff of it against the notebook now.
struct Changes {
    /// `None` when the notebook is not in the last commit.
    head: Option<String>,
    current: String,
    lines: Vec<diff::Line>,
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
    Symbols,
    Changes,
    ToggleActions,
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
    Example(usize),
    ExportHtml,
    Find,
    GoTo(CellId),
    Quit,
    /// Show the command palette.
    Commands,
    /// Show or hide the Functions window, the Symbols window, and switch the notebook to or from the text view.
    ToggleFunctions,
    ToggleSymbols,
    ToggleTextView,
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
    show_symbols: bool,
    /// The find-and-replace window, when open.
    find: Option<find::State>,
    /// The system menu bar (macOS), which then takes the place of the toolbar menus.
    native_menu: Option<native_menu::NativeMenu>,
    changes: Option<Changes>,
    show_actions: bool,
    help_filter: String,
    status: String,
    last_title: String,
    /// The opened folder and its notebooks (sidebar).
    folder: Option<PathBuf>,
    tree: Vec<workspace::Node>,
    /// What git says about the opened folder, and the reading in progress (done on another thread).
    git: Option<git::Status>,
    git_pending: Option<std::sync::mpsc::Receiver<Option<git::Status>>>,
    git_checked: Option<std::time::Instant>,
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
    /// When (egui time) the session, drafts and results were last checked for writing.
    stored_at: f64,
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
