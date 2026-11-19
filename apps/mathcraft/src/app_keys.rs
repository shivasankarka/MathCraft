//! Cell edits, shortcuts and menu items (methods of `App`).

use super::*;

impl App {
    pub(crate) fn undo_cells(&mut self) {
        self.status = if self.cur.undo_structure() { "Undid the last cell change".into() } else { "Nothing to undo".into() };
    }

    pub(crate) fn redo_cells(&mut self) {
        self.status = if self.cur.redo_structure() { "Redid the cell change".into() } else { "Nothing to redo".into() };
    }

    pub(crate) fn apply(&mut self, actions: Vec<Action>) {
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
    pub(crate) fn focus_neighbor(&mut self, ctx: &egui::Context, step: isize) {
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

    pub(crate) fn shortcuts(&mut self, ctx: &egui::Context) {
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
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F)) {
            self.open_find();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F2)) {
            self.show_symbols = !self.show_symbols;
        }
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

    pub(crate) fn shortcut(ctx: &egui::Context, mods: Modifiers, key: Key) -> String {
        ctx.format_shortcut(&egui::KeyboardShortcut::new(mods, key))
    }

    pub(crate) fn menu_item(ui: &mut egui::Ui, theme: &Theme, ctx: &egui::Context, label: &str, keys: Option<(Modifiers, Key)>) -> bool {
        let shortcut = keys.map(|(m, k)| Self::shortcut(ctx, m, k));
        let clicked = menu_row(ui, theme, label, shortcut.as_deref()).clicked();
        if clicked {
            ui.close();
        }
        clicked
    }
}
