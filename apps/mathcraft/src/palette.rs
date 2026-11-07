//! The command palette (Cmd+P): type a few letters, pick a command. Generic over what a command carries, so the
//! app decides what the items are; this file does the matching and the drawing.

use crate::fonts;
use crate::theme::{self, Theme};
use eframe::egui::{self, Align2, Key, Modifiers, RichText, Sense, Stroke, vec2};

pub struct Item<T> {
    pub title: String,
    /// Shortcut or other quiet text on the right.
    pub detail: String,
    /// Group label shown in front of the title in search results (`File`, `View`, ...).
    pub group: &'static str,
    pub payload: T,
}

#[derive(Default)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    fresh: bool,
}

impl PaletteState {
    pub fn show(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.fresh = true;
    }
}

/// Fuzzy match: every character of `query` must appear in `text` in order. Higher is better; `None` is no match.
/// Rewards runs of consecutive characters, matches at word starts and early matches.
pub fn fuzzy(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut qi, mut score, mut last) = (0usize, 0i32, None::<usize>);
    for (i, c) in t.iter().enumerate() {
        if q.get(qi) != Some(c) {
            continue;
        }
        let word_start = i == 0 || t.get(i - 1).is_some_and(|p| !p.is_alphanumeric());
        score += 10;
        if word_start {
            score += 8;
        }
        if last.is_some_and(|l| l + 1 == i) {
            score += 6;
        }
        if qi == 0 {
            score -= i.min(20) as i32;
        }
        last = Some(i);
        qi += 1;
        if qi == q.len() {
            // shorter titles rank above long ones for the same letters
            return Some(score - (t.len() as i32) / 4);
        }
    }
    None
}

/// Indices into `items` that match the query, best first (original order when the query is empty).
pub fn filter<T>(items: &[Item<T>], query: &str) -> Vec<usize> {
    let mut hits: Vec<(i32, usize)> =
        items.iter().enumerate().filter_map(|(i, it)| fuzzy(query, &format!("{} {}", it.group, it.title)).map(|s| (s, i))).collect();
    if !query.trim().is_empty() {
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    hits.into_iter().map(|(_, i)| i).collect()
}

const MAX_ROWS: usize = 10;

/// Draw the palette and return the index (into `items`) of the command the user chose, if any.
pub fn show<T>(ctx: &egui::Context, theme: &Theme, state: &mut PaletteState, items: &[Item<T>]) -> Option<usize> {
    if !state.open {
        return None;
    }
    let hits = filter(items, &state.query);
    state.selected = state.selected.min(hits.len().saturating_sub(1));
    let (up, down, enter, esc) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowUp) || i.consume_key(Modifiers::CTRL, Key::K),
            i.consume_key(Modifiers::NONE, Key::ArrowDown) || i.consume_key(Modifiers::CTRL, Key::J),
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    if esc {
        state.open = false;
        return None;
    }
    if up {
        state.selected = state.selected.saturating_sub(1);
    }
    if down && state.selected + 1 < hits.len() {
        state.selected += 1;
    }
    let mut chosen = None;
    if enter {
        chosen = hits.get(state.selected).copied();
    }
    let width = 560.0_f32.min(ctx.content_rect().width() - 32.0);
    egui::Area::new(egui::Id::unique("palette")).anchor(Align2::CENTER_TOP, vec2(0.0, 70.0)).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::new()
            .fill(theme.surface)
            .stroke(Stroke::new(1.0, theme.border_strong))
            .corner_radius(theme::RADIUS_LARGE + 2)
            .shadow(theme.popup_shadow())
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_width(width);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut state.query)
                        .hint_text(RichText::new("Type a command or a file name").color(theme.text_faint))
                        .font(egui::FontId::new(15.0, egui::FontFamily::Proportional))
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin::symmetric(8, 6))
                        .desired_width(f32::INFINITY),
                );
                if state.fresh {
                    edit.request_focus();
                    state.fresh = false;
                }
                if edit.changed() {
                    state.selected = 0;
                }
                let r = ui.max_rect();
                ui.painter().hline(egui::Rangef::new(r.left() - 8.0, r.right() + 8.0), ui.cursor().top() + 2.0, Stroke::new(1.0, theme.separator));
                ui.add_space(6.0);
                if hits.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new("No command matches").size(13.0).color(theme.text_dim));
                    ui.add_space(4.0);
                    return;
                }
                // keep the selected row in view when there are more matches than rows
                let first = state.selected.saturating_sub(MAX_ROWS - 1);
                for (row, idx) in hits.iter().enumerate().skip(first).take(MAX_ROWS) {
                    let Some(item) = items.get(*idx) else { continue };
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                    let selected = row == state.selected;
                    if selected {
                        ui.painter().rect_filled(rect, theme::RADIUS, theme.surface_pressed);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, theme::RADIUS, theme.surface_hover);
                    }
                    let mid = rect.center().y;
                    let mut x = rect.left() + 10.0;
                    if !item.group.is_empty() {
                        let g = ui.painter().layout_no_wrap(item.group.to_owned(), egui::FontId::new(12.0, egui::FontFamily::Proportional), theme.text_faint);
                        let w = g.size().x;
                        ui.painter().galley(egui::pos2(x, mid - g.size().y / 2.0), g, theme.text_faint);
                        x += w + 8.0;
                    }
                    let font = egui::FontId::new(13.5, if selected { fonts::strong_family() } else { egui::FontFamily::Proportional });
                    ui.painter().text(egui::pos2(x, mid), Align2::LEFT_CENTER, &item.title, font, theme.text);
                    if !item.detail.is_empty() {
                        ui.painter().text(
                            egui::pos2(rect.right() - 10.0, mid),
                            Align2::RIGHT_CENTER,
                            &item.detail,
                            egui::FontId::new(12.0, egui::FontFamily::Proportional),
                            theme.text_faint,
                        );
                    }
                    if resp.hovered() && ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                        state.selected = row;
                    }
                    if resp.clicked() {
                        chosen = Some(*idx);
                    }
                }
                if hits.len() > MAX_ROWS {
                    ui.add_space(2.0);
                    ui.label(RichText::new(format!("{} more: keep typing to narrow down", hits.len() - MAX_ROWS)).size(11.5).color(theme.text_faint));
                }
            });
    });
    if chosen.is_some() {
        state.open = false;
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(titles: &[&str]) -> Vec<Item<usize>> {
        titles.iter().enumerate().map(|(i, t)| Item { title: (*t).to_owned(), detail: String::new(), group: "", payload: i }).collect()
    }

    #[test]
    fn matches_letters_in_order() {
        assert!(fuzzy("rua", "Run all").is_some());
        assert!(fuzzy("ral", "Run all").is_some());
        assert!(fuzzy("xyz", "Run all").is_none());
        assert!(fuzzy("", "anything").is_some());
        assert!(fuzzy("lar", "Run all").is_none(), "order matters");
    }

    #[test]
    fn better_matches_rank_first() {
        let it = items(&["Toggle sidebar", "Save", "Save as…", "Run all", "Open folder…"]);
        let order = |q: &str| filter(&it, q).iter().map(|i| it[*i].title.clone()).collect::<Vec<_>>();
        assert_eq!(order("save").first().map(String::as_str), Some("Save"), "shorter title wins on the same letters");
        assert_eq!(order("ra").first().map(String::as_str), Some("Run all"));
        assert_eq!(order("ts").first().map(String::as_str), Some("Toggle sidebar"), "word starts count");
        assert_eq!(order("").len(), 5, "an empty query lists everything in order");
        assert!(order("zzz").is_empty());
    }

    #[test]
    fn the_group_is_searchable() {
        let mut it = items(&["Dark"]);
        it[0].group = "Theme";
        assert_eq!(filter(&it, "theme dark").len(), 1);
    }
}
