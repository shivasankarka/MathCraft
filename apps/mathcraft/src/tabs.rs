//! The tab strip above the notebook: one tab per open document.

use crate::fonts;
use crate::icons::{self, Icon};
use crate::theme::{self, Theme};
use eframe::egui::{self, Sense, Stroke, vec2};

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
const MIN_WIDTH: f32 = 116.0;
const MAX_WIDTH: f32 = 232.0;
/// Room at each end of a tab: the close button (or unsaved dot) on the left, the same again on the right so the
/// name sits in the middle.
const SLOT: f32 = 26.0;

/// Draw the strip and report what was clicked. The active tab takes the document's colour and opens into the page
/// below (no hairline under it), the way native document tabs do; the others sit on the chrome.
pub fn tab_strip(ui: &mut egui::Ui, theme: &Theme, tabs: &[TabInfo], active: u64) -> Option<TabAction> {
    let mut action = None;
    let outer = ui.max_rect();
    let outer_painter = ui.painter().clone();
    let mut selected_x: Option<egui::Rangef> = None;
    ui.spacing_mut().item_spacing.x = 0.0;
    egui::ScrollArea::horizontal()
        .id_salt("tab-scroll")
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let active_index = tabs.iter().position(|t| t.id == active);
                for (i, tab) in tabs.iter().enumerate() {
                    let selected = tab.id == active;
                    let label = shorten(&tab.name, 26);
                    let font = egui::FontId::new(theme::TEXT_UI - 0.5, if selected { fonts::strong_family() } else { egui::FontFamily::Proportional });
                    let galley = ui.painter().layout_no_wrap(label, font, if selected { theme.text } else { theme.text_dim });
                    let width = (galley.size().x + 2.0 * SLOT + 8.0).clamp(MIN_WIDTH, MAX_WIDTH);
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, TAB_HEIGHT), Sense::click());
                    let close = egui::Rect::from_center_size(egui::pos2(rect.left() + SLOT / 2.0 + 2.0, rect.center().y), vec2(16.0, 16.0));
                    let close_resp = ui.interact(close, resp.id.with("close"), Sense::click());
                    let hovered = resp.hovered() || close_resp.hovered();
                    let p = ui.painter();
                    if selected {
                        selected_x = Some(rect.x_range());
                        p.rect_filled(rect, 0.0, theme.bg);
                        p.vline(rect.left() + 0.5, rect.y_range(), Stroke::new(1.0, theme.separator));
                        p.vline(rect.right() - 0.5, rect.y_range(), Stroke::new(1.0, theme.separator));
                    } else {
                        if hovered {
                            p.rect_filled(rect, 0.0, theme.surface_hover);
                        }
                        // a short divider between two resting tabs (not next to the active one, which has its own edge)
                        let next_is_active = active_index == Some(i + 1);
                        if !next_is_active {
                            p.vline(rect.right() - 0.5, rect.y_range().shrink(8.0), Stroke::new(1.0, theme.separator_soft()));
                        }
                    }
                    // the name, centred in the space between the two slots (clipped if it is still too wide)
                    let text_room = egui::Rect::from_min_max(egui::pos2(rect.left() + SLOT, rect.top()), egui::pos2(rect.right() - SLOT, rect.bottom()));
                    let x = text_room.center().x - (galley.size().x / 2.0).min(text_room.width() / 2.0);
                    p.with_clip_rect(text_room.intersect(p.clip_rect())).galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, theme.text);
                    if hovered {
                        if close_resp.hovered() {
                            p.rect_filled(
                                close,
                                theme::RADIUS_SMALL,
                                if close_resp.is_pointer_button_down_on() { theme.border_strong } else { theme.surface_pressed },
                            );
                        }
                        icons::close_glyph(p, close.center(), 3.25, if close_resp.hovered() { theme.text } else { theme.text_dim });
                    } else if tab.dirty {
                        p.circle_filled(close.center(), 3.5, if selected { theme.text_dim } else { theme.text_faint });
                    }
                    let close_clicked = close_resp.clicked();
                    close_resp.on_hover_text(if tab.dirty { "Close (unsaved changes)" } else { "Close" });
                    let resp = resp.on_hover_text(&tab.tip);
                    if close_clicked || resp.middle_clicked() {
                        action = Some(TabAction::Close(tab.id));
                    } else if resp.clicked() {
                        action = Some(TabAction::Select(tab.id));
                    }
                }
                ui.add_space(4.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(TAB_HEIGHT, TAB_HEIGHT), Sense::click());
                let button = rect.shrink(5.0);
                if resp.hovered() {
                    ui.painter().rect_filled(button, theme::RADIUS, if resp.is_pointer_button_down_on() { theme.surface_pressed } else { theme.surface_hover });
                }
                icons::paint(
                    ui.painter(),
                    Icon::Plus,
                    egui::Rect::from_center_size(rect.center(), vec2(11.0, 11.0)),
                    if resp.hovered() { theme.text } else { theme.text_dim },
                );
                if resp.on_hover_text("New notebook (⌘N)").clicked() {
                    action = Some(TabAction::New);
                }
            });
        });
    // the hairline under the strip, broken where the active tab opens into the page
    let y = outer.bottom() - 0.5;
    let stroke = Stroke::new(1.0, theme.separator);
    match selected_x {
        Some(sel) => {
            if sel.min > outer.left() {
                outer_painter.hline(egui::Rangef::new(outer.left(), sel.min.min(outer.right())), y, stroke);
            }
            if sel.max < outer.right() {
                outer_painter.hline(egui::Rangef::new(sel.max.max(outer.left()), outer.right()), y, stroke);
            }
        }
        None => {
            outer_painter.hline(outer.x_range(), y, stroke);
        }
    }
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
