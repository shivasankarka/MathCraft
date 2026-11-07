//! Text view: the whole `# %%` file in one editor (where Vim keys shine), with the cells' results in a panel beside it.
//! The notebook is kept in step with the text, so switching back to cells loses nothing.

use crate::doc::Doc;
use crate::icons::Icon;
use crate::theme::Theme;
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
        let mut job = highlight::highlight(buf.as_str(), font.clone(), &syntax);
        job.wrap.max_width = wrap_width;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let keys = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: vim };
    let row_height = ui.text_style_height(&TextStyle::Monospace);
    let rows = ((ui.available_height() - 40.0) / row_height).max(8.0) as usize;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
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
        edited |= r.response.changed();
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

/// The results beside the editor: one block per code cell, the one under the cursor marked.
pub fn results(ui: &mut egui::Ui, env: &mut Env, nb: &Notebook, active: Option<CellId>) {
    let theme = env.theme;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(14.0);
        let mut any = false;
        for (i, cell) in nb.cells().iter().enumerate() {
            if cell.kind != CellKind::Code || cell.source.trim().is_empty() {
                continue;
            }
            any = true;
            let here = active == Some(cell.id);
            let top = ui.cursor().top();
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                let (label, color) = match cell.state {
                    CellState::Fresh => ("", theme.text_faint),
                    CellState::Stale | CellState::New => ("not run", theme.warn),
                    CellState::Running | CellState::Queued => ("running", theme.accent),
                    CellState::Error | CellState::Blocked => ("problem", theme.danger),
                };
                ui.label(RichText::new(format!("Cell {}", i + 1)).size(11.5).color(if here { theme.text } else { theme.text_faint }));
                if !label.is_empty() {
                    ui.label(RichText::new(label).size(11.5).color(color));
                }
            });
            ui.indent(("res", cell.id), |ui| {
                ui.set_max_width((ui.available_width() - 12.0).max(120.0));
                for p in &cell.problems {
                    ui.add_space(4.0);
                    banner(ui, theme, Icon::Warning, theme.warn, Some(p.code()), &p.message());
                }
                output_ui(ui, env, cell);
            });
            ui.add_space(12.0);
            if here {
                let bottom = ui.cursor().top() - 8.0;
                let x = ui.max_rect().left() + 5.0;
                ui.painter().line_segment([egui::pos2(x, top + 2.0), egui::pos2(x, bottom)], Stroke::new(2.0, theme.accent));
            }
        }
        if !any {
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(RichText::new("Results appear here. Put the cursor in a cell and press Shift+Enter.").size(12.5).color(theme.text_faint));
            });
        }
    });
}
