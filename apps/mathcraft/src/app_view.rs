//! The egui frame: layout of the window.

use super::*;

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
pub(crate) const COLUMN: f32 = 940.0;

impl App {
    pub(crate) fn theme(&self, ctx: &egui::Context) -> Theme {
        if ctx.theme() == egui::Theme::Dark { self.dark.clone() } else { self.light.clone() }
    }

    /// One frame of the whole window.
    pub(crate) fn show(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        self.poll_kernel();
        self.poll_git(ctx);
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
        if self.show_actions {
            let row = egui::Panel::top("actions")
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(theme.chrome).inner_margin(Margin { left: 12, right: 12, top: 4, bottom: 5 }))
                .show(root, |ui| self.action_row(ui, &theme));
            hairline(root, &theme, row.response.rect.x_range(), row.response.rect.bottom() - 0.5);
            if let Some(act) = row.inner {
                self.apply_action(act, root.ctx());
            }
        }
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
                .show(root, |ui| sidebar::sidebar(ui, &theme, &folder, &tree, &mut self.collapsed, &open_files, self.git.as_ref()));
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
        self.symbols_window(ctx, &theme);
        self.changes_window(ctx, &theme);
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
