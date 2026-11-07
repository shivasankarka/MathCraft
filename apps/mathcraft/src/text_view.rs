//! Text view: the whole `# %%` file in one editor (where Vim keys shine), with the cells' results in a panel beside it.
//! The notebook is kept in step with the text, so switching back to cells loses nothing.

use crate::doc::Doc;
use crate::fonts;
use crate::icons::Icon;
use crate::theme::{self, Theme};
use crate::{Env, banner, highlight, insert_at_cursor, output_ui, split_at_cursor, vim_edit};
use eframe::egui::{self, Key, Margin, Modifiers, RichText, Stroke, TextStyle};
use mathcraft_notebook::{CellId, CellKind, CellState, Notebook};

/// What the text editor asks the app to do this frame.
#[derive(Default)]
pub struct TextOut {
    pub run_cell: Option<CellId>,
    pub commands: Vec<String>,
}

pub fn editor_id(doc: u64) -> egui::Id {
    egui::Id::unique(("text-view", doc))
}

/// The cell under the text cursor, from the editor's stored cursor.
pub fn active_cell(ctx: &egui::Context, doc: &Doc) -> Option<CellId> {
    let cursor = egui::TextEdit::load_state(ctx, editor_id(doc.id)).and_then(|s| s.cursor.char_range()).map_or(0, |r| usize::from(r.primary.index));
    doc.cell_at(cursor)
}

/// The editor itself. Edits go to `doc.text` and are synced into the notebook.
pub fn editor(ui: &mut egui::Ui, theme: &Theme, doc: &mut Doc, vim: bool, vim_status: &mut String, want_focus: bool) -> TextOut {
    let ctx = ui.ctx().clone();
    let id = editor_id(doc.id);
    let mut out = TextOut::default();
    let mut text = std::mem::take(&mut doc.text);
    let mut edited = false;

    if ctx.memory(|m| m.has_focus(id)) {
        if vim {
            let r = vim_edit::drive(&ctx, id, &mut text);
            edited |= r.edited;
            out.commands = r.commands;
            *vim_status = r.status;
        }
        let (run, tab, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::SHIFT, Key::Enter) || i.consume_key(Modifiers::COMMAND, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Tab),
                i.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        if run {
            let cursor = egui::TextEdit::load_state(&ctx, id).and_then(|s| s.cursor.char_range()).map_or(0, |r| usize::from(r.primary.index));
            let probe = Doc::cell_index_in(&text, cursor);
            doc.text = text.clone();
            out.run_cell = doc.nb.cells().get(probe).map(|c| c.id);
        }
        if tab {
            edited |= insert_at_cursor(&ctx, id, &mut text, "    ");
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

    let font = TextStyle::Monospace.resolve(ui.style());
    let syntax = theme.syntax.clone();
    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let mut job = highlight::highlight(buf.as_str(), font.clone(), &syntax, false);
        job.wrap.max_width = wrap_width;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let keys = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: vim };
    let row_height = ui.text_style_height(&TextStyle::Monospace);
    let rows = ((ui.available_height() - 40.0) / row_height).max(8.0) as usize;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // a slot under the text for the cell-marker bands, filled in once the text is laid out
        let bands = ui.painter().add(egui::Shape::Noop);
        let shown = egui::Frame::new().inner_margin(Margin { left: 18, right: 14, top: 14, bottom: 14 }).show(ui, |ui| {
            egui::TextEdit::multiline(&mut text)
                .id(id)
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY)
                .desired_rows(rows)
                .lock_focus(true)
                .event_filter(keys)
                .hint_text(RichText::new("Write code; start each cell with a line  # %%").color(theme.text_faint))
                .layouter(&mut layouter)
                .show(ui)
        });
        let r = shown.inner;
        let full = shown.response.rect;
        let mut marks = Vec::new();
        let mut line_start = true;
        let mut lines = text.split('\n');
        let mut line = "";
        for row in &r.galley.rows {
            if line_start {
                line = lines.next().unwrap_or("");
            }
            if line_start && line.trim_start().starts_with("# %%") {
                let rr = row.rect().translate(r.galley_pos.to_vec2());
                let band = egui::Rect::from_x_y_ranges(full.x_range(), egui::Rangef::new(rr.top() - 1.0, rr.bottom() + 1.0));
                marks.push(egui::Shape::rect_filled(band, 0.0, theme.code_bg));
                marks.push(egui::Shape::hline(full.x_range(), band.top() + 0.5, Stroke::new(1.0, theme.border)));
            }
            line_start = row.ends_with_newline;
        }
        ui.painter().set(bands, egui::Shape::Vec(marks));
        edited |= r.response.changed();
        // Vim moves the cursor itself, so egui does not scroll to it: follow it when it changed.
        if vim && let Some(cursor) = r.state.cursor.char_range().map(|c| c.primary) {
            let seen = ctx.data_mut(|d| d.get_temp::<usize>(id).unwrap_or(usize::MAX));
            if seen != usize::from(cursor.index) {
                ctx.data_mut(|d| d.insert_temp(id, usize::from(cursor.index)));
                let at = r.galley.pos_from_cursor(cursor).translate(r.galley_pos.to_vec2());
                ui.scroll_to_rect(at.expand2(egui::vec2(0.0, row_height * 2.0)), None);
            }
        }
        if want_focus {
            r.response.request_focus();
        }
    });
    doc.text = text;
    if edited {
        doc.sync_from_text();
    }
    out
}

/// The results beside the editor: one block per code cell. The cell under the cursor gets a quiet tinted backing
/// and a brighter header; state shows as a small coloured dot and a word only when the cell is not up to date.
pub fn results(ui: &mut egui::Ui, env: &mut Env, nb: &Notebook, active: Option<CellId>) {
    let theme = env.theme;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(10.0);
        let mut any = false;
        for (i, cell) in nb.cells().iter().enumerate() {
            if cell.kind != CellKind::Code || cell.source.trim().is_empty() {
                continue;
            }
            any = true;
            let here = active == Some(cell.id);
            let backing = ui.painter().add(egui::Shape::Noop);
            let top = ui.cursor().top();
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.spacing_mut().item_spacing.x = 6.0;
                let (label, color) = match cell.state {
                    CellState::Fresh => ("", theme.text_faint),
                    CellState::New => ("not run", theme.text_faint),
                    CellState::Stale => ("stale", theme.warn),
                    CellState::Running | CellState::Queued => ("running", theme.accent),
                    CellState::Error | CellState::Blocked => ("problem", theme.danger),
                };
                ui.label(RichText::new(format!("Cell {}", i + 1)).font(egui::FontId::new(theme::TEXT_CAPTION, fonts::strong_family())).color(if here {
                    theme.text_dim
                } else {
                    theme.text_faint
                }));
                if !label.is_empty() {
                    let (dot, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 2.5, color);
                    ui.label(RichText::new(label).size(theme::TEXT_CAPTION).color(color));
                }
            });
            ui.indent(("res", cell.id), |ui| {
                ui.set_max_width((ui.available_width() - 16.0).max(120.0));
                for p in &cell.problems {
                    ui.add_space(4.0);
                    banner(ui, theme, Icon::Warning, theme.warn, Some(p.code()), &p.message());
                }
                output_ui(ui, env, cell);
            });
            ui.add_space(10.0);
            let bottom = ui.cursor().top();
            let block = egui::Rect::from_x_y_ranges(ui.max_rect().x_range().shrink(6.0), egui::Rangef::new(top, bottom));
            if here {
                ui.painter().set(backing, egui::Shape::rect_filled(block, theme::RADIUS_LARGE, theme.tint(theme.accent)));
            }
            ui.add_space(4.0);
        }
        if !any {
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(RichText::new("No results yet").size(theme::TEXT_UI).color(theme.text_dim));
                    ui.add(
                        egui::Label::new(RichText::new("Put the cursor in a cell and press ⇧↵ to run it.").size(theme::TEXT_SMALL).color(theme.text_faint))
                            .wrap(),
                    );
                });
            });
        }
    });
}
