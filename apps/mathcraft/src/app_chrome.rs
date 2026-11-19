//! Toolbar, windows and status bar (methods of `App`).

use super::*;

impl App {
    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui, theme: &Theme) {
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
                if Self::menu_item(ui, theme, &ctx, "Export as HTML…", None) {
                    self.export_html();
                }
                ui.menu_button("Open example", |ui| {
                    for (i, (name, about, _)) in examples::EXAMPLES.iter().enumerate() {
                        if menu_row(ui, theme, name, None).on_hover_text(*about).clicked() {
                            self.open_example(i);
                            ui.close();
                        }
                    }
                });
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
            if ui.button("Symbols").on_hover_text("Insert integrals, sums, roots and more (F2)").clicked() {
                self.show_symbols = !self.show_symbols;
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

    pub(crate) fn help_window(&mut self, ctx: &egui::Context, theme: &Theme) {
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

    /// Open the changes window for the current notebook, reading the committed text from git.
    pub(crate) fn open_changes(&mut self) {
        let Some(path) = self.cur.path.clone() else {
            self.status = "Save the notebook first, then its changes can be compared with git".into();
            return;
        };
        self.changes = Some(Changes { head: committed(&path), current: String::new(), lines: Vec::new() });
    }

    pub(crate) fn changes_window(&mut self, ctx: &egui::Context, theme: &Theme) {
        let Some(mut changes) = self.changes.take() else { return };
        let current = self.cur.nb.to_text();
        if let Some(head) = &changes.head
            && (changes.current != current || changes.lines.is_empty())
        {
            changes.lines = diff::diff(head, &current);
            changes.current = current;
        }
        let (mut open, mut refresh) = (true, false);
        egui::Window::new("Changes")
            .title_bar(false)
            .default_rect(egui::Rect::from_min_size(ctx.content_rect().right_top() + vec2(-560.0, 64.0), vec2(520.0, 420.0)))
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Changes").font(egui::FontId::new(theme::TEXT_TITLE, fonts::strong_family())).color(theme.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, theme, Icon::Close, "Close").clicked() {
                            open = false;
                        }
                        refresh = ui.button("Refresh").on_hover_text("Read the last commit again").clicked();
                    });
                });
                if changes.head.is_none() {
                    ui.label(
                        RichText::new("This notebook is not in the last commit (new file, no repository, or git is missing).")
                            .size(theme::TEXT_SMALL)
                            .color(theme.text_dim),
                    );
                    return;
                }
                let (added, removed) = diff::counts(&changes.lines);
                if added + removed == 0 {
                    ui.label(RichText::new("No changes since the last commit.").size(theme::TEXT_SMALL).color(theme.text_dim));
                    return;
                }
                ui.label(RichText::new(format!("{added} added, {removed} removed since the last commit")).size(theme::TEXT_CAPTION).color(theme.text_faint));
                egui::ScrollArea::both().auto_shrink([false, true]).max_height(360.0).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for row in diff::rows(&changes.lines, 2) {
                        match row {
                            diff::Row::Skipped(n) => {
                                ui.label(
                                    RichText::new(format!("   … {n} unchanged line{}", if n == 1 { "" } else { "s" }))
                                        .monospace()
                                        .size(theme::TEXT_CAPTION)
                                        .color(theme.text_faint),
                                );
                            }
                            diff::Row::Line(line) => {
                                let (sign, color) = match line.kind {
                                    diff::Kind::Same => (' ', theme.text_dim),
                                    diff::Kind::Added => ('+', theme.ok),
                                    diff::Kind::Removed => ('-', theme.danger),
                                };
                                let text = RichText::new(format!("{sign} {}", line.text)).monospace().size(13.0).color(color);
                                let label = if line.kind == diff::Kind::Same {
                                    egui::Label::new(text)
                                } else {
                                    egui::Label::new(text.background_color(color.gamma_multiply(0.12)))
                                };
                                ui.add(label.extend());
                            }
                        }
                    }
                });
            });
        if refresh && let Some(path) = &self.cur.path {
            changes.head = committed(path);
            changes.lines.clear();
        }
        if open {
            self.changes = Some(changes);
        }
    }

    pub(crate) fn symbols_window(&mut self, ctx: &egui::Context, theme: &Theme) {
        if !self.show_symbols {
            return;
        }
        let mut open = true;
        let mut picked = None;
        egui::Window::new("Symbols")
            .title_bar(false)
            .default_rect(egui::Rect::from_min_size(ctx.content_rect().right_top() + vec2(-360.0, 64.0), vec2(330.0, 540.0)))
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Symbols").font(egui::FontId::new(theme::TEXT_TITLE, fonts::strong_family())).color(theme.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, theme, Icon::Close, "Close (F2)").clicked() {
                            open = false;
                        }
                    });
                });
                ui.label(RichText::new("Click into a code cell, then pick a symbol.").size(theme::TEXT_CAPTION).color(theme.text_faint));
                egui::ScrollArea::vertical().auto_shrink([true, false]).show(ui, |ui| picked = symbols::contents(ui, theme));
            });
        if let Some(what) = picked {
            self.insert_code(&what, ctx);
        }
        self.show_symbols = open;
    }

    pub(crate) fn status_bar(&self, ui: &mut egui::Ui, theme: &Theme) {
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
            if let Some(git) = &self.git {
                ui.add_space(10.0);
                ui.label(dim(git.summary())).on_hover_text("Git branch of the opened folder, and how many notebooks changed since the last commit");
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
