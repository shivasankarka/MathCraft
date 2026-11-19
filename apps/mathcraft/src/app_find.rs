//! The find-and-replace window and jumping to where a name is defined (methods of `App`).

use super::*;

/// Hits listed in the window; the count above the list is exact.
const SHOWN: usize = 200;

impl App {
    /// Open the window (or put the keyboard back in its search box).
    pub(crate) fn open_find(&mut self) {
        self.find.get_or_insert_with(Default::default).focus = true;
    }

    /// Put the keyboard in a cell's editor.
    pub(crate) fn go_to_cell(&mut self, id: CellId) {
        if self.cur.view != View::Notebook {
            self.cur.set_view(View::Notebook);
        }
        self.cur.focus = Some(id);
        self.last_cell = Some(id);
    }

    pub(crate) fn find_window(&mut self, ctx: &egui::Context, theme: &Theme) {
        let Some(mut st) = self.find.take() else { return };
        let hits = find::hits(&self.cur.nb, &st.query);
        let total: usize = hits.iter().map(|h| h.count).sum();
        let (mut open, mut replace, mut jump) = (true, false, None);
        egui::Window::new("Find and replace")
            .title_bar(false)
            .default_rect(egui::Rect::from_min_size(ctx.content_rect().right_top() + vec2(-460.0, 64.0), vec2(420.0, 380.0)))
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Find and replace").font(egui::FontId::new(theme::TEXT_TITLE, fonts::strong_family())).color(theme.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, theme, Icon::Close, "Close (Esc)").clicked() {
                            open = false;
                        }
                    });
                });
                let search = ui.add(egui::TextEdit::singleline(&mut st.query.find).hint_text("Find").desired_width(f32::INFINITY));
                if std::mem::take(&mut st.focus) {
                    search.request_focus();
                }
                if search.has_focus() && ctx.input(|i| i.key_pressed(Key::Escape)) {
                    open = false;
                }
                ui.add(egui::TextEdit::singleline(&mut st.with).hint_text("Replace with").desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut st.query.case, "Match case");
                    ui.checkbox(&mut st.query.word, "Whole word");
                });
                ui.horizontal(|ui| {
                    let cells = hits.iter().map(|h| h.cell).collect::<HashSet<_>>().len();
                    let summary = if st.query.find.is_empty() { String::new() } else { format!("{total} matches in {cells} cells") };
                    ui.label(RichText::new(summary).size(theme::TEXT_SMALL).color(theme.text_dim));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        replace = ui.add_enabled(total > 0, egui::Button::new("Replace all")).clicked();
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(240.0).show(ui, |ui| {
                    for hit in hits.iter().take(SHOWN) {
                        let text: String = hit.text.chars().take(80).collect();
                        let row = ui.selectable_label(false, RichText::new(format!("{:>3}   {text}", hit.number)).monospace().size(12.5));
                        if row.on_hover_text("Go to this cell").clicked() {
                            jump = Some(hit.cell);
                        }
                    }
                    if hits.len() > SHOWN {
                        ui.label(RichText::new(format!("{} more lines", hits.len() - SHOWN)).size(theme::TEXT_SMALL).color(theme.text_faint));
                    }
                });
            });
        if replace {
            let changed = find::replace_all(&self.cur.nb, &st.query, &st.with);
            let cells = changed.len();
            self.apply(changed.into_iter().map(|(id, text)| Action::SetSource(id, text)).collect());
            self.status = format!("Replaced {total} matches in {cells} cells");
        }
        if let Some(id) = jump {
            self.go_to_cell(id);
        }
        if open {
            self.find = Some(st);
        }
    }
}
