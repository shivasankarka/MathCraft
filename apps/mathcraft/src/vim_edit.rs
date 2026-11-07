//! Connects the Vim engine (`mathcraft-vim`) to an egui text editor: takes the keyboard events the editor would
//! otherwise see, feeds them to the engine in the modes that are not insert, and writes the text and cursor back.

use eframe::egui::{self, Event, Key as EKey};
use egui::text::{CCursor, CCursorRange};
use mathcraft_vim::{Key, Mode, Reply, Vim};

/// Per-editor Vim state, kept in egui's temporary data.
#[derive(Clone, Default)]
pub struct VimEditor {
    vim: Vim,
    /// The Vim cursor (a char index). Outside insert mode the egui selection is drawn around it, so it cannot be
    /// read back from egui; `last` remembers what we wrote to notice when the user moved the cursor with the mouse.
    cur: usize,
    last: Option<(usize, usize)>,
}

pub struct VimResult {
    pub edited: bool,
    /// A submitted `:` command line.
    pub commands: Vec<String>,
    pub status: String,
    pub mode: Mode,
}

/// Translate one egui event into engine keys; `None` means "leave the event alone".
fn translate(event: &Event, mode: Mode) -> Option<Vec<Key>> {
    match event {
        Event::Text(t) if mode != Mode::Insert => Some(t.chars().map(Key::Char).collect()),
        Event::Key { key, pressed: true, modifiers, .. } => {
            if modifiers.command || modifiers.alt || modifiers.mac_cmd {
                return None;
            }
            match key {
                EKey::Escape => Some(vec![Key::Esc]),
                _ if mode == Mode::Insert => None,
                EKey::Tab | EKey::Space => Some(Vec::new()),
                EKey::Enter if !modifiers.shift => Some(vec![Key::Enter]),
                EKey::Backspace => Some(vec![Key::Backspace]),
                EKey::ArrowLeft => Some(vec![Key::Char('h')]),
                EKey::ArrowRight => Some(vec![Key::Char('l')]),
                EKey::ArrowUp => Some(vec![Key::Char('k')]),
                EKey::ArrowDown => Some(vec![Key::Char('j')]),
                EKey::Delete => Some(vec![Key::Char('x')]),
                EKey::R if modifiers.ctrl => Some(vec![Key::Ctrl('r')]),
                // letters and digits also arrive as Text; drop the key event so the editor does not see it twice
                k if k.symbol_or_name().chars().count() == 1 && !modifiers.ctrl => Some(Vec::new()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Run the engine on this frame's keys for the focused editor `id`. Returns `None` when nothing happened.
pub fn drive(ctx: &egui::Context, id: egui::Id, text: &mut String) -> VimResult {
    let state_id = id.with("vim");
    let mut ed: VimEditor = ctx.data_mut(|d| d.get_temp(state_id)).unwrap_or_default();
    let total = text.chars().count();

    // pick up a cursor the user moved (mouse, or arrow keys in insert mode)
    let shown = egui::TextEdit::load_state(ctx, id).and_then(|s| s.cursor.char_range()).map(|r| (usize::from(r.primary.index), usize::from(r.secondary.index)));
    match (shown, ed.last) {
        (Some(now), last) if Some(now) != last => ed.cur = now.0.min(total),
        _ => {}
    }

    let mode_now = ed.vim.mode();
    let mut keys: Vec<Key> = Vec::new();
    ctx.input_mut(|i| {
        let mut mode = mode_now;
        i.events.retain(|e| match translate(e, mode) {
            Some(k) => {
                // keys that switch modes change how the rest of this frame's events are read, so stop guessing: a
                // frame normally carries a single key
                if k.contains(&Key::Esc) {
                    mode = Mode::Normal;
                }
                keys.extend(k);
                false
            }
            None => true,
        });
    });

    let before = text.clone();
    let mut commands = Vec::new();
    for key in keys {
        match ed.vim.handle(text, &mut ed.cur, key) {
            Reply::Ex(c) => commands.push(c),
            // a key that arrived in the same frame as the one that entered insert mode: type it ourselves
            Reply::Pass => type_key(text, &mut ed.cur, key),
            Reply::Eaten => {}
        }
    }
    ed.cur = ed.cur.min(text.chars().count());
    let edited = *text != before;

    // write the cursor (and selection) back for egui to draw
    let len = text.chars().count();
    let range = match ed.vim.mode() {
        Mode::Insert | Mode::Command => (ed.cur, ed.cur),
        Mode::Normal => block(text, ed.cur, len),
        Mode::Visual | Mode::VisualLine => {
            let (a, c) = (ed.vim.anchor().min(len), ed.cur);
            if c >= a { (c + 1, a) } else { (c, a + 1) }
        }
    };
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        let wanted = CCursorRange { primary: CCursor::new(range.0.min(len)), secondary: CCursor::new(range.1.min(len)), h_pos: None };
        if state.cursor.char_range() != Some(wanted) {
            state.cursor.set_char_range(Some(wanted));
            state.store(ctx, id);
        }
    }
    ed.last = Some((range.0.min(len), range.1.min(len)));
    let result = VimResult { edited, commands, status: ed.vim.status(), mode: ed.vim.mode() };
    ctx.data_mut(|d| d.insert_temp(state_id, ed));
    result
}

/// The "block cursor" in normal mode: the character under the cursor drawn as a one-character selection.
fn block(text: &str, cur: usize, len: usize) -> (usize, usize) {
    let on_char = text.chars().nth(cur).is_some_and(|c| c != '\n');
    if cur < len && on_char { (cur, cur + 1) } else { (cur, cur) }
}

/// Insert-mode typing for keys the engine passed back (the editor never saw them).
fn type_key(text: &mut String, cur: &mut usize, key: Key) {
    let at = text.char_indices().nth(*cur).map_or(text.len(), |(i, _)| i);
    match key {
        Key::Char(c) => {
            text.insert(at, c);
            *cur += 1;
        }
        Key::Enter => {
            text.insert(at, '\n');
            *cur += 1;
        }
        Key::Backspace if *cur > 0 => {
            if let Some((i, _)) = text.char_indices().nth(*cur - 1) {
                text.remove(i);
                *cur -= 1;
            }
        }
        _ => {}
    }
}
