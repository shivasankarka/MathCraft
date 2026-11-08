//! The command palette (Cmd+P): type a few letters, pick a command. Generic over what a command carries, so the
//! app decides what the items are; this file does the matching and the drawing.

use crate::fonts;
use crate::icons;
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
    /// The last choice was made with Shift+Enter (a function is then inserted instead of looked up).
    pub alt: bool,
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

/// The char positions in `text` that the query's letters land on, by the same left-to-right rule as [`fuzzy`].
/// Empty when the query is empty or does not match.
pub fn matched_chars(query: &str, text: &str) -> Vec<usize> {
    let q: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    let mut out = Vec::new();
    let mut qi = 0;
    for (i, c) in text.chars().enumerate() {
        let Some(want) = q.get(qi) else { break };
        if c.to_lowercase().eq(std::iter::once(*want)) {
            out.push(i);
            qi += 1;
        }
    }
    if qi == q.len() { out } else { Vec::new() }
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
    let (up, down, alt_enter, enter, esc) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowUp) || i.consume_key(Modifiers::CTRL, Key::K),
            i.consume_key(Modifiers::NONE, Key::ArrowDown) || i.consume_key(Modifiers::CTRL, Key::J),
            i.consume_key(Modifiers::SHIFT, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    state.alt = alt_enter;
    let enter = enter || alt_enter;
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
    let width = 600.0_f32.min(ctx.content_rect().width() - 32.0);
    egui::Area::new(egui::Id::unique("palette")).anchor(Align2::CENTER_TOP, vec2(0.0, 64.0)).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::new()
            .fill(theme.surface)
            .stroke(Stroke::new(1.0, if theme.dark { theme.border_strong } else { theme.border }))
            .corner_radius(theme::RADIUS_LARGE + 4)
            .shadow(theme.window_shadow())
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                let field_top = ui.cursor().top();
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut state.query)
                        .hint_text(RichText::new("Search commands, files and functions").color(theme.text_faint))
                        .font(egui::FontId::new(16.0, egui::FontFamily::Proportional))
                        .frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 34, right: 10, top: 10, bottom: 10 }))
                        .desired_width(f32::INFINITY),
                );
                let glass = egui::Rect::from_center_size(egui::pos2(ui.max_rect().left() + 19.0, (field_top + edit.rect.bottom()) / 2.0), vec2(15.0, 15.0));
                icons::paint(ui.painter(), icons::Icon::Search, glass, theme.text_faint);
                if state.fresh {
                    edit.request_focus();
                    state.fresh = false;
                }
                if edit.changed() {
                    state.selected = 0;
                }
                let r = ui.max_rect();
                ui.painter().hline(egui::Rangef::new(r.left() - 6.0, r.right() + 6.0), ui.cursor().top() + 0.5, Stroke::new(1.0, theme.separator_soft()));
                ui.add_space(6.0);
                if hits.is_empty() {
                    ui.add_space(14.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(format!("Nothing matches “{}”", state.query.trim())).size(theme::TEXT_UI).color(theme.text_dim));
                        ui.add_space(4.0);
                        ui.label(RichText::new("Try fewer letters, or the start of each word.").size(theme::TEXT_CAPTION).color(theme.text_faint));
                    });
                    ui.add_space(16.0);
                    return;
                }
                // keep the selected row in view when there are more matches than rows
                let first = state.selected.saturating_sub(MAX_ROWS - 1);
                let shown: Vec<(usize, usize)> = hits.iter().copied().enumerate().skip(first).take(MAX_ROWS).collect();
                // the group names form a column so the titles line up
                let group_font = egui::FontId::new(theme::TEXT_SMALL, egui::FontFamily::Proportional);
                let group_w = shown
                    .iter()
                    .filter_map(|(_, i)| items.get(*i))
                    .map(|it| ui.painter().layout_no_wrap(it.group.to_owned(), group_font.clone(), theme.text_faint).size().x)
                    .fold(0.0_f32, f32::max);
                for (row, idx) in shown {
                    let Some(item) = items.get(idx) else { continue };
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), theme::ROW_LARGE), Sense::click());
                    let selected = row == state.selected;
                    let p = ui.painter();
                    if selected {
                        p.rect_filled(rect, theme::RADIUS, theme.selected_row());
                    } else if resp.hovered() {
                        p.rect_filled(rect, theme::RADIUS, theme.surface_hover);
                    }
                    let mid = rect.center().y;
                    let mut x = rect.left() + 10.0;
                    if group_w > 0.0 {
                        p.text(
                            egui::pos2(x, mid),
                            Align2::LEFT_CENTER,
                            item.group,
                            group_font.clone(),
                            if selected { theme.text_dim } else { theme.text_faint },
                        );
                        x += group_w + 14.0;
                    }
                    let keys_w = if item.detail.is_empty() { 0.0 } else { icons::keycaps(p, theme, egui::pos2(rect.right() - 8.0, mid), &item.detail) + 16.0 };
                    let title = title_job(theme, &item.title, &matched_title(&state.query, item), selected);
                    let galley = ui.fonts_mut(|f| f.layout_job(title));
                    let room = egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(rect.right() - 8.0 - keys_w, rect.bottom()));
                    ui.painter().with_clip_rect(room.intersect(ui.clip_rect())).galley(egui::pos2(x, mid - galley.size().y / 2.0), galley, theme.text);
                    if resp.hovered() && ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                        state.selected = row;
                    }
                    if resp.clicked() {
                        chosen = Some(idx);
                    }
                }
                ui.add_space(4.0);
                if hits.len() > MAX_ROWS {
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        let more = hits.len() - MAX_ROWS;
                        ui.label(
                            RichText::new(format!("{more} more {}. Keep typing to narrow the list.", if more == 1 { "match" } else { "matches" }))
                                .size(theme::TEXT_CAPTION)
                                .color(theme.text_faint),
                        );
                    });
                    ui.add_space(4.0);
                }
            });
    });
    if chosen.is_some() {
        state.open = false;
    }
    chosen
}

/// Where the query's letters fall in the title: in the title alone when they all fit there; otherwise the query
/// matched "group title" (as the search does), with letters that land in the group dropped.
fn matched_title<T>(query: &str, item: &Item<T>) -> Vec<usize> {
    let own = matched_chars(query, &item.title);
    if !own.is_empty() {
        return own;
    }
    let offset = item.group.chars().count() + 1;
    matched_chars(query, &format!("{} {}", item.group, item.title)).into_iter().filter_map(|i| i.checked_sub(offset)).collect()
}

/// The title with matched letters in the strong weight and full text colour; the rest regular.
fn title_job(theme: &Theme, title: &str, matched: &[usize], selected: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let plain = egui::TextFormat::simple(
        egui::FontId::new(theme::TEXT_UI + 0.5, egui::FontFamily::Proportional),
        if selected || matched.is_empty() { theme.text } else { theme.text_dim },
    );
    let hit = egui::TextFormat::simple(egui::FontId::new(theme::TEXT_UI + 0.5, fonts::strong_family()), theme.text);
    let mut buf = String::new();
    let mut buf_hit = false;
    for (i, c) in title.chars().enumerate() {
        let is_hit = matched.contains(&i);
        if is_hit != buf_hit && !buf.is_empty() {
            job.append(&std::mem::take(&mut buf), 0.0, if buf_hit { hit.clone() } else { plain.clone() });
        }
        buf_hit = is_hit;
        buf.push(c);
    }
    if !buf.is_empty() {
        job.append(&buf, 0.0, if buf_hit { hit } else { plain });
    }
    job
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
    fn matched_letters_are_found_in_the_title() {
        assert_eq!(matched_chars("ra", "Run all"), vec![0, 4]);
        assert!(matched_chars("zz", "Run all").is_empty());
        let it = Item { title: "Save".to_owned(), detail: String::new(), group: "File", payload: 0 };
        assert_eq!(matched_title("sa", &it), vec![0, 1]);
        assert_eq!(matched_title("fsa", &it), vec![0, 1], "letters in the group are not marked in the title");
        let it = Item { title: "Strict names".to_owned(), detail: String::new(), group: "Settings", payload: 0 };
        assert_eq!(matched_title("sa", &it), vec![0, 8], "the title alone is preferred");
    }

    #[test]
    fn the_group_is_searchable() {
        let mut it = items(&["Dark"]);
        it[0].group = "Theme";
        assert_eq!(filter(&it, "theme dark").len(), 1);
    }
}
