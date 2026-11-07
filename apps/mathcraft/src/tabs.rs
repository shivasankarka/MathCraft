//! The tab strip above the notebook: one tab per open document.

use crate::fonts;
use crate::theme::Theme;
use eframe::egui::{self, RichText, Sense, Stroke, vec2};

pub struct TabInfo {
    pub id: u64,
    pub name: String,
    pub dirty: bool,
    pub tip: String,
}

pub enum TabAction {
    Select(u64),
    Close(u64),
    New,
}

const TAB_HEIGHT: f32 = 30.0;

/// Draw the strip and report what was clicked. `active` is highlighted.
pub fn tab_strip(ui: &mut egui::Ui, theme: &Theme, tabs: &[TabInfo], active: u64) -> Option<TabAction> {
    let mut action = None;
    ui.spacing_mut().item_spacing.x = 0.0;
    egui::ScrollArea::horizontal()
        .id_salt("tab-scroll")
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for tab in tabs {
                    let selected = tab.id == active;
                    let label = shorten(&tab.name, 22);
                    let font = egui::FontId::new(12.5, if selected { fonts::strong_family() } else { egui::FontFamily::Proportional });
                    let galley = ui.painter().layout_no_wrap(label, font, if selected { theme.text } else { theme.text_dim });
                    let width = galley.size().x + 52.0;
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, TAB_HEIGHT), Sense::click());
                    let hovered = resp.hovered();
                    let p = ui.painter();
                    if selected {
                        p.rect_filled(rect, 0.0, theme.bg);
                        p.hline(rect.x_range(), rect.top() + 1.0, Stroke::new(2.0, theme.accent));
                    } else if hovered {
                        p.rect_filled(rect, 0.0, theme.surface_hover);
                    }
                    if !selected {
                        p.vline(rect.right() - 0.5, rect.y_range().shrink(8.0), Stroke::new(1.0, theme.separator));
                    }
                    p.galley(egui::pos2(rect.left() + 14.0, rect.center().y - galley.size().y / 2.0), galley, theme.text);
                    // the close button (or the unsaved dot when not hovering)
                    let close = egui::Rect::from_center_size(egui::pos2(rect.right() - 16.0, rect.center().y), vec2(18.0, 18.0));
                    let close_resp = ui.interact(close, resp.id.with("close"), Sense::click());
                    if hovered || selected || close_resp.hovered() {
                        if close_resp.hovered() {
                            p.rect_filled(close, 4.0, theme.surface_pressed);
                        }
                        let c = close.center();
                        let s = Stroke::new(1.3, if close_resp.hovered() { theme.text } else { theme.text_faint });
                        p.line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], s);
                        p.line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], s);
                    }
                    if tab.dirty && !(hovered || close_resp.hovered()) {
                        p.circle_filled(close.center(), 3.0, theme.text_faint);
                    }
                    resp.clone().on_hover_text(&tab.tip);
                    if close_resp.clicked() || resp.middle_clicked() {
                        action = Some(TabAction::Close(tab.id));
                    } else if resp.clicked() {
                        action = Some(TabAction::Select(tab.id));
                    }
                }
                let (rect, resp) = ui.allocate_exact_size(vec2(32.0, TAB_HEIGHT), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 0.0, theme.surface_hover);
                }
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, RichText::new("+").text(), egui::FontId::proportional(16.0), theme.text_dim);
                if resp.on_hover_text("New notebook (⌘N)").clicked() {
                    action = Some(TabAction::New);
                }
            });
        });
    action
}

/// Shorten a name in the middle so the extension stays visible.
fn shorten(name: &str, max: usize) -> String {
    let n = name.chars().count();
    if n <= max {
        return name.to_owned();
    }
    let head: String = name.chars().take(max.saturating_sub(7)).collect();
    let tail: String = name.chars().skip(n.saturating_sub(5)).collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::shorten;

    #[test]
    fn long_names_keep_their_ending() {
        assert_eq!(shorten("a.py", 22), "a.py");
        let s = shorten("a_very_long_notebook_name_for_testing.py", 22);
        assert!(s.ends_with("ng.py") && s.chars().count() <= 22, "{s}");
    }
}
