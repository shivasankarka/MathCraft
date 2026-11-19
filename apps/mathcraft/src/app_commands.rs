//! Commands, palette and actions (methods of `App`).

use super::*;

impl App {
    /// Everything the command palette offers right now.
    pub(crate) fn commands(&self, ctx: &egui::Context) -> Vec<Item<Cmd>> {
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
            item("View", "Insert symbols and formulas", "F2".to_owned(), Cmd::Symbols),
            item("View", "Changes since the last git commit", String::new(), Cmd::Changes),
            item("View", "Show or hide the action row (Simplify, Solve, …)", String::new(), Cmd::ToggleActions),
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

    pub(crate) fn run_command(&mut self, cmd: Cmd, ctx: &egui::Context) {
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
            Cmd::Symbols => self.show_symbols = true,
            Cmd::Changes => self.open_changes(),
            Cmd::ToggleActions => self.show_actions = !self.show_actions,
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
    pub(crate) fn insert_function(&mut self, name: &str, ctx: &egui::Context) {
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

    /// Put a formula template into the code cell that was being edited, with its first placeholder selected.
    /// Put `what` at the cursor of the last code cell with its first placeholder selected (Tab visits the others).
    pub(crate) fn insert_code(&mut self, what: &symbols::Insert, ctx: &egui::Context) {
        let target = self.last_cell.and_then(|id| self.cur.nb.cell(id)).filter(|c| c.kind == CellKind::Code);
        let Some(cell) = target else {
            self.status = "Click into a code cell first, then insert a symbol".into();
            return;
        };
        let (id, mut text) = (cell.id, cell.source.clone());
        let editor = editor_id(id);
        let start = egui::TextEdit::load_state(ctx, editor)
            .and_then(|s| s.cursor.char_range())
            .map_or_else(|| text.chars().count(), |r| usize::from(r.primary.index).min(usize::from(r.secondary.index)).min(text.chars().count()));
        if !insert_at_cursor(ctx, editor, &mut text, &what.code) {
            text.push_str(&what.code);
        }
        if let Some((from, to)) = symbols::selection(&what.code, &what.select)
            && let Some(mut state) = egui::TextEdit::load_state(ctx, editor)
        {
            let at = |n: usize| egui::text::CCursor::new(start + n);
            state.cursor.set_char_range(Some(CCursorRange::two(at(from), at(to))));
            state.store(ctx, editor);
            // Tab visits the other placeholders
            let stops = symbols::Stops::new(&what.code, &what.select, start, text.chars().count());
            ctx.data_mut(|d| d.insert_temp(stops_key(editor), stops));
        }
        self.apply(vec![Action::SetSource(id, text)]);
        self.cur.focus = Some(id);
        self.status = format!("Inserted {}", what.code);
    }

    /// The row of one-click actions on the expression being edited.
    pub(crate) fn action_row(&self, ui: &mut egui::Ui, theme: &Theme) -> Option<&'static symbols::Act> {
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.spacing_mut().button_padding = vec2(10.0, 3.0);
            ui.label(RichText::new("On the selection").size(theme::TEXT_CAPTION).color(theme.text_faint));
            ui.add_space(4.0);
            for act in symbols::ACTIONS {
                if ui.add(egui::Button::new(RichText::new(act.label).size(13.0)).frame(false)).on_hover_text(act.hint).clicked() {
                    chosen = Some(act);
                }
            }
        });
        chosen
    }

    /// Wrap the selected text (or the expression on the cursor's line) of the last code cell in `act`'s call.
    pub(crate) fn apply_action(&mut self, act: &symbols::Act, ctx: &egui::Context) {
        let target = self.last_cell.and_then(|id| self.cur.nb.cell(id)).filter(|c| c.kind == CellKind::Code);
        let Some(cell) = target else {
            self.status = "Click into a code cell first, then choose an action".into();
            return;
        };
        let (id, mut text) = (cell.id, cell.source.clone());
        let editor = editor_id(id);
        let total = text.chars().count();
        let (mut lo, mut hi) = egui::TextEdit::load_state(ctx, editor).and_then(|s| s.cursor.char_range()).map_or((total, total), |r| {
            (
                usize::from(r.primary.index).min(usize::from(r.secondary.index)).min(total),
                usize::from(r.primary.index).max(usize::from(r.secondary.index)).min(total),
            )
        });
        if lo == hi {
            // no selection: the expression on this line
            let chars: Vec<char> = text.chars().collect();
            let line_start = chars.iter().take(lo).rposition(|c| *c == '\n').map_or(0, |i| i + 1);
            let line_end = chars.iter().skip(lo).position(|c| *c == '\n').map_or(total, |i| lo + i);
            let line: String = chars.iter().skip(line_start).take(line_end - line_start).collect();
            let (a, b) = symbols::expression_span(&line);
            (lo, hi) = (line_start + a, line_start + b);
        }
        let expr: String = text.chars().skip(lo).take(hi - lo).collect();
        let known = |n: &str| matches!(n, "pi" | "E" | "I" | "oo") || mathcraft_eval::DOCS.iter().any(|d| d.0 == n);
        let (code, pick) = if expr.trim().is_empty() {
            // nothing to wrap: an empty call with the argument selected
            let (code, _) = symbols::wrap(act, "f", "x");
            (code, Some((act.func.chars().count() + 1, act.func.chars().count() + 2)))
        } else {
            let var = symbols::guess_variable(&expr, &known);
            symbols::wrap(act, expr.trim(), &var)
        };
        let (lo, hi) = if expr.trim().is_empty() { (lo, lo) } else { (lo, hi) };
        replace_chars(ctx, editor, &mut text, lo, hi, &code);
        if let Some((from, to)) = pick
            && let Some(mut state) = egui::TextEdit::load_state(ctx, editor)
        {
            let at = |n: usize| egui::text::CCursor::new(lo + n);
            state.cursor.set_char_range(Some(CCursorRange::two(at(from), at(to))));
            state.store(ctx, editor);
        }
        self.apply(vec![Action::SetSource(id, text)]);
        self.cur.focus = Some(id);
        self.status = format!("Wrapped in {}(…). Shift+Enter runs it", act.func);
    }

    /// A Vim `:` command.
    pub(crate) fn ex_command(&mut self, line: &str) {
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
}
