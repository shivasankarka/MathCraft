//! The completion popup and cursor insertion helpers.

use super::*;

/// What the completion list offers right now.
pub(crate) struct Suggestion {
    /// Char index where the word being completed starts, and the cursor's char index.
    start: usize,
    cursor: usize,
    word: String,
    candidates: Vec<complete::Candidate>,
}

pub(crate) fn cursor_index(ctx: &egui::Context, id: egui::Id, text: &str) -> usize {
    let total = text.chars().count();
    egui::TextEdit::load_state(ctx, id).and_then(|s| s.cursor.char_range()).map_or(total, |r| usize::from(r.primary.index).min(total))
}

pub(crate) fn suggest(ctx: &egui::Context, id: egui::Id, text: &str, names: &[String]) -> Option<Suggestion> {
    let dismissed = id.with("dismissed");
    let cursor = cursor_index(ctx, id, text);
    let found = complete::word_before(text, cursor).filter(|_| !complete::in_comment_or_string(text, cursor));
    let Some((start, word)) = found else {
        ctx.data_mut(|d| d.remove::<usize>(dismissed));
        return None;
    };
    if ctx.data(|d| d.get_temp::<usize>(dismissed)) == Some(start) {
        return None;
    }
    let candidates = complete::candidates(&word, names);
    (!candidates.is_empty()).then_some(Suggestion { start, cursor, word, candidates })
}

/// The highlighted row, kept per editor and reset when the word changes.
pub(crate) fn selected_row(ctx: &egui::Context, id: egui::Id, s: &Suggestion) -> usize {
    let stored = ctx.data(|d| d.get_temp::<(String, usize)>(id.with("selected")));
    match stored {
        Some((word, row)) if word == s.word => row.min(s.candidates.len().saturating_sub(1)),
        _ => 0,
    }
}

/// Up/Down move in the list, Tab or Enter accept, Escape closes it. Runs before the text field sees the keys.
pub(crate) fn handle_completion_keys(ctx: &egui::Context, id: egui::Id, text: &mut String, names: &[String]) -> bool {
    let Some(s) = suggest(ctx, id, text, names) else { return false };
    let row = selected_row(ctx, id, &s);
    let (up, down, accept, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::Tab) || i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    let last = s.candidates.len().saturating_sub(1);
    let moved = if up {
        if row == 0 { last } else { row - 1 }
    } else if down {
        if row >= last { 0 } else { row + 1 }
    } else {
        row
    };
    ctx.data_mut(|d| d.insert_temp(id.with("selected"), (s.word.clone(), moved)));
    if escape {
        ctx.data_mut(|d| d.insert_temp(id.with("dismissed"), s.start));
    }
    match s.candidates.get(row) {
        Some(c) if accept => accept_candidate(ctx, id, text, &s, c),
        _ => false,
    }
}

pub(crate) fn accept_candidate(ctx: &egui::Context, id: egui::Id, text: &mut String, s: &Suggestion, c: &complete::Candidate) -> bool {
    let ins = if c.is_function { format!("{}(", c.name) } else { c.name.clone() };
    replace_chars(ctx, id, text, s.start, s.cursor, &ins);
    true
}

/// The list itself, drawn under the cursor. Returns whether a click accepted an entry (which edits `text`).
pub(crate) fn completion_popup(ctx: &egui::Context, theme: &Theme, id: egui::Id, text: &mut String, names: &[String], at: egui::Pos2) -> bool {
    let Some(s) = suggest(ctx, id, text, names) else { return false };
    let row = selected_row(ctx, id, &s);
    let mut clicked = None;
    egui::Area::new(id.with("popup")).order(egui::Order::Foreground).fixed_pos(at + vec2(-EDITOR_PAD_X - 4.0, 6.0)).show(ctx, |ui| {
        egui::Frame::new()
            .fill(theme.surface)
            .stroke(Stroke::new(1.0, if theme.dark { theme.border_strong } else { theme.border }))
            .corner_radius(theme::RADIUS_LARGE)
            .shadow(theme.popup_shadow())
            .inner_margin(Margin::same(4))
            .show(ui, |ui| {
                ui.set_min_width(280.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let mono = TextStyle::Monospace.resolve(ui.style());
                for (i, c) in s.candidates.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), egui::Sense::click());
                    let selected = i == row;
                    if selected {
                        ui.painter().rect_filled(rect, theme::RADIUS, theme.selected_row());
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, theme::RADIUS, theme.surface_hover);
                    }
                    // f for a function, x for a name defined in the notebook.
                    let (letter, kind) = if c.is_function { ("f", theme.syntax.function) } else { ("x", theme.syntax.keyword) };
                    icons::badge(ui.painter(), egui::Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(16.0, 16.0)), letter, kind);
                    let name_end =
                        ui.painter().text(rect.left_center() + vec2(30.0, 0.0), egui::Align2::LEFT_CENTER, &c.name, mono.clone(), theme.text).right();
                    let detail = c.detail.strip_prefix(c.name.as_str()).filter(|_| c.is_function).unwrap_or(&c.detail);
                    ui.painter().text(
                        egui::pos2(name_end + 2.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        detail,
                        egui::FontId::monospace(12.0),
                        if selected { theme.text_dim } else { theme.text_faint },
                    );
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        clicked = Some(c.clone());
                    }
                }
            });
    });
    match clicked {
        Some(c) => {
            accept_candidate(ctx, id, text, &s, &c);
            ctx.memory_mut(|m| m.request_focus(id));
            true
        }
        None => false,
    }
}

/// Text before and after the cursor.
pub(crate) fn split_at_cursor(ctx: &egui::Context, id: egui::Id, text: &str) -> (String, String) {
    let idx = egui::TextEdit::load_state(ctx, id).and_then(|s| s.cursor.char_range()).map_or_else(|| text.chars().count(), |r| usize::from(r.primary.index));
    (text.chars().take(idx).collect(), text.chars().skip(idx).collect())
}

/// Insert text at the cursor (replacing any selection) and put the cursor after it. Returns whether text changed.
pub(crate) fn stops_key(editor: egui::Id) -> egui::Id {
    editor.with("placeholders")
}

/// Tab after a symbol was inserted from the palette: select the next placeholder (or move past the template).
/// `false` when no walk is in progress, so Tab indents as usual.
pub(crate) fn next_placeholder(ctx: &egui::Context, id: egui::Id, text: &str) -> bool {
    let key = stops_key(id);
    let Some(mut stops) = ctx.data(|d| d.get_temp::<symbols::Stops>(key)) else { return false };
    let cursor = egui::TextEdit::load_state(ctx, id)
        .and_then(|s| s.cursor.char_range())
        .map_or(usize::MAX, |r| usize::from(r.primary.index).min(usize::from(r.secondary.index)));
    let next = stops.advance(text.chars().count(), cursor);
    if stops.at.is_empty() {
        ctx.data_mut(|d| d.remove::<symbols::Stops>(key));
    } else {
        ctx.data_mut(|d| d.insert_temp(key, stops));
    }
    let Some((from, to)) = next else { return false };
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        state.cursor.set_char_range(Some(CCursorRange::two(egui::text::CCursor::new(from), egui::text::CCursor::new(to))));
        state.store(ctx, id);
    }
    true
}

pub(crate) fn insert_at_cursor(ctx: &egui::Context, id: egui::Id, text: &mut String, ins: &str) -> bool {
    let Some(state) = egui::TextEdit::load_state(ctx, id) else { return false };
    let total = text.chars().count();
    let (lo, hi) = match state.cursor.char_range() {
        Some(r) => {
            let (a, b) = (usize::from(r.primary.index), usize::from(r.secondary.index));
            (a.min(b).min(total), a.max(b).min(total))
        }
        None => (total, total),
    };
    replace_chars(ctx, id, text, lo, hi, ins);
    true
}

/// Replace the chars `lo..hi` with `ins` and put the cursor after it.
pub(crate) fn replace_chars(ctx: &egui::Context, id: egui::Id, text: &mut String, lo: usize, hi: usize, ins: &str) {
    let new: String = text.chars().take(lo).chain(ins.chars()).chain(text.chars().skip(hi)).collect();
    *text = new;
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        let at = egui::text::CCursor::new(lo + ins.chars().count());
        state.cursor.set_char_range(Some(CCursorRange::one(at)));
        state.store(ctx, id);
    }
}
