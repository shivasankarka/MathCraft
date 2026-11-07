//! Vim-style modal editing (layer L0): a state machine that turns keys into edits of a text and a cursor.
//! It knows nothing about the UI. The caller feeds it keys and reads back the text, the cursor (a char index), the mode
//! and a short "pending" string for a status line. In insert mode every key except Esc is passed back to the editor.
//!
//! Supported: counts; motions `h j k l w b e 0 ^ $ gg G f F t T ; , n N`; operators `d c y` with motions and doubled
//! (`dd cc yy`); `x X D C s S r J ~ p P u Ctrl-r`; inserts `i a I A o O`; visual `v V` with `d c y x o`; `:` command line
//! (returned to the caller as [`Reply::Ex`]) and `/` search. Not yet: dot repeat, text objects, marks, macros, named
//! registers.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    Command,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Esc,
    Enter,
    Backspace,
    Ctrl(char),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Reply {
    /// Not ours (insert mode): let the editor handle the key.
    Pass,
    /// Handled; the text, cursor or mode may have changed.
    Eaten,
    /// `:` command line was submitted, without the colon (`w`, `q`, `wq`, ...).
    Ex(String),
}

const MAX_COUNT: usize = 9999;
const MAX_UNDO: usize = 200;

struct State {
    chars: Vec<char>,
    cur: usize,
}

#[derive(Clone, Copy)]
struct Motion {
    to: usize,
    inclusive: bool,
    linewise: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punct,
}

#[derive(Debug, Clone)]
pub struct Vim {
    mode: Mode,
    count: usize,
    op: Option<char>,
    pending_g: bool,
    pending_find: Option<char>,
    pending_replace: bool,
    /// `i` or `a` after an operator or in visual mode, waiting for the object (`w`, `"`, `(` ...).
    pending_obj: Option<char>,
    /// Dot repeat: the keys of the command being typed, whether it is worth recording, and whether it changed text.
    rec: Vec<Key>,
    rec_ok: bool,
    dirty: bool,
    insert_from: usize,
    last_change: Option<(Vec<Key>, String)>,
    replaying: bool,
    reg: String,
    reg_linewise: bool,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    anchor: usize,
    cmd: String,
    cmd_kind: char,
    search: String,
    last_find: Option<(char, char)>,
}

impl Default for Vim {
    fn default() -> Self {
        Self::new()
    }
}

// ---- text helpers ------------------------------------------------------------------------------------------------

fn at(c: &[char], i: usize) -> Option<char> {
    c.get(i).copied()
}

fn line_start(c: &[char], i: usize) -> usize {
    let mut i = i.min(c.len());
    while i > 0 && at(c, i - 1) != Some('\n') {
        i -= 1;
    }
    i
}

fn line_end(c: &[char], i: usize) -> usize {
    let mut i = i.min(c.len());
    while i < c.len() && at(c, i) != Some('\n') {
        i += 1;
    }
    i
}

/// Last cursor position on the line of `i` in normal mode (the last character, or the start of an empty line).
fn last_in_line(c: &[char], i: usize) -> usize {
    let (s, e) = (line_start(c, i), line_end(c, i));
    if e > s { e - 1 } else { s }
}

fn first_nonblank(c: &[char], i: usize) -> usize {
    let (s, e) = (line_start(c, i), line_end(c, i));
    let mut j = s;
    while j < e && matches!(at(c, j), Some(' ' | '\t')) {
        j += 1;
    }
    if j >= e { last_in_line(c, i) } else { j }
}

fn class(ch: Option<char>) -> Class {
    match ch {
        None => Class::Space,
        Some(c) if c.is_whitespace() => Class::Space,
        Some(c) if c.is_alphanumeric() || c == '_' => Class::Word,
        Some(_) => Class::Punct,
    }
}

fn next_line_start(c: &[char], i: usize) -> Option<usize> {
    let e = line_end(c, i);
    (e < c.len()).then_some(e + 1)
}

fn prev_line_start(c: &[char], i: usize) -> Option<usize> {
    let s = line_start(c, i);
    (s > 0).then(|| line_start(c, s - 1))
}

fn line_number(c: &[char], i: usize) -> usize {
    c.iter().take(i).filter(|ch| **ch == '\n').count()
}

fn line_count(c: &[char]) -> usize {
    c.iter().filter(|ch| **ch == '\n').count() + 1
}

fn start_of_line_number(c: &[char], n: usize) -> usize {
    let mut line = 0;
    for (i, ch) in c.iter().enumerate() {
        if line == n {
            return line_start(c, i);
        }
        if *ch == '\n' {
            line += 1;
        }
    }
    if line == n { c.len() } else { line_start(c, c.len()) }
}

fn word_forward(c: &[char], mut i: usize) -> usize {
    let cl = class(at(c, i));
    if cl != Class::Space {
        while i < c.len() && class(at(c, i)) == cl {
            i += 1;
        }
    }
    while i < c.len() && class(at(c, i)) == Class::Space {
        // an empty line counts as a word
        if at(c, i) == Some('\n') && (i == 0 || at(c, i - 1) == Some('\n')) && i != 0 {
            break;
        }
        i += 1;
    }
    i.min(c.len())
}

fn word_back(c: &[char], mut i: usize) -> usize {
    if i == 0 {
        return 0;
    }
    i -= 1;
    while i > 0 && class(at(c, i)) == Class::Space {
        i -= 1;
    }
    let cl = class(at(c, i));
    while i > 0 && class(at(c, i - 1)) == cl && cl != Class::Space {
        i -= 1;
    }
    i
}

fn word_end(c: &[char], mut i: usize) -> usize {
    if i + 1 >= c.len() {
        return c.len().saturating_sub(1);
    }
    i += 1;
    while i + 1 < c.len() && class(at(c, i)) == Class::Space {
        i += 1;
    }
    let cl = class(at(c, i));
    while i + 1 < c.len() && class(at(c, i + 1)) == cl && cl != Class::Space {
        i += 1;
    }
    i
}

fn find_in_line(c: &[char], i: usize, kind: char, target: char, count: usize) -> Option<(usize, bool)> {
    let (s, e) = (line_start(c, i), line_end(c, i));
    let forward = matches!(kind, 'f' | 't');
    let mut pos = i;
    for n in 0..count.max(1) {
        let mut j = pos;
        loop {
            if forward {
                j += 1;
                if j >= e {
                    return None;
                }
            } else {
                if j <= s {
                    return None;
                }
                j -= 1;
            }
            // `t` repeated with `;` must not get stuck on the char right next to the cursor
            if at(c, j) == Some(target) && !(n == 0 && matches!(kind, 't' | 'T') && j.abs_diff(i) == 1 && count == 0) {
                break;
            }
        }
        pos = j;
    }
    let landed = match kind {
        't' => pos.saturating_sub(1),
        'T' => pos + 1,
        _ => pos,
    };
    Some((landed, forward))
}

fn text_of(c: &[char], a: usize, b: usize) -> String {
    c.iter().skip(a).take(b.saturating_sub(a)).collect()
}

fn search_from(c: &[char], from: usize, needle: &str, forward: bool) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() || c.len() < n.len() {
        return None;
    }
    let matches_at = |i: usize| c.iter().skip(i).take(n.len()).eq(n.iter());
    let last = c.len() - n.len();
    if forward {
        ((from + 1)..=last).chain(0..=from.min(last)).find(|i| matches_at(*i))
    } else {
        (0..from.min(last + 1))
            .rev()
            .chain(((from + 1).min(last + 1))..=last)
            .rev()
            .find(|i| matches_at(*i))
            .or_else(|| (0..=last).rev().find(|i| matches_at(*i)))
    }
}

// ---- the machine -------------------------------------------------------------------------------------------------

impl Vim {
    pub fn new() -> Self {
        Self {
            mode: Mode::Normal,
            count: 0,
            op: None,
            pending_g: false,
            pending_find: None,
            pending_replace: false,
            pending_obj: None,
            rec: Vec::new(),
            rec_ok: false,
            dirty: false,
            insert_from: 0,
            last_change: None,
            replaying: false,
            reg: String::new(),
            reg_linewise: false,
            undo: Vec::new(),
            redo: Vec::new(),
            anchor: 0,
            cmd: String::new(),
            cmd_kind: ':',
            search: String::new(),
            last_find: None,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Where the selection starts in visual modes (a char index).
    pub fn anchor(&self) -> usize {
        self.anchor
    }

    /// Text for a status line: the mode name, then what is typed so far (`2d`, `:wq`, `/foo`).
    pub fn status(&self) -> String {
        let name = match self.mode {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
            Mode::Command => "",
        };
        let mut pending = String::new();
        if self.count > 0 {
            pending.push_str(&self.count.to_string());
        }
        pending.extend(self.op);
        if self.pending_g {
            pending.push('g');
        }
        pending.extend(self.pending_find);
        pending.extend(self.pending_obj);
        if self.mode == Mode::Command {
            return format!("{}{}", self.cmd_kind, self.cmd);
        }
        if pending.is_empty() { name.to_owned() } else { format!("{name}  {pending}") }
    }

    /// Back to normal mode with nothing pending (also used when focus moves to another editor).
    pub fn reset(&mut self) {
        self.mode = Mode::Normal;
        self.clear_pending();
    }

    fn clear_pending(&mut self) {
        self.count = 0;
        self.op = None;
        self.pending_g = false;
        self.pending_find = None;
        self.pending_replace = false;
        self.pending_obj = None;
    }

    /// Nothing typed yet for the next command.
    fn idle(&self) -> bool {
        self.mode == Mode::Normal
            && self.count == 0
            && self.op.is_none()
            && !self.pending_g
            && self.pending_find.is_none()
            && !self.pending_replace
            && self.pending_obj.is_none()
    }

    /// Feed one key. `text` and `cursor` (char index) are edited in place.
    pub fn handle(&mut self, text: &mut String, cursor: &mut usize, key: Key) -> Reply {
        if self.mode == Mode::Insert {
            if key == Key::Esc || key == Key::Ctrl('[') {
                let chars: Vec<char> = text.chars().collect();
                let cur = (*cursor).min(chars.len());
                if self.rec_ok && !self.replaying {
                    // what was typed, for `.` (a plain run of insertions; edits with backspace are not replayed)
                    let typed = if cur >= self.insert_from { text_of(&chars, self.insert_from, cur) } else { String::new() };
                    self.last_change = Some((std::mem::take(&mut self.rec), typed));
                }
                self.rec_ok = false;
                self.mode = Mode::Normal;
                // like Vim, the cursor steps back onto the last typed character
                *cursor = if cur > line_start(&chars, cur) { cur - 1 } else { cur };
                self.clear_pending();
                return Reply::Eaten;
            }
            return Reply::Pass;
        }
        if key == Key::Char('.')
            && self.mode == Mode::Normal
            && self.op.is_none()
            && !self.pending_g
            && self.pending_find.is_none()
            && !self.pending_replace
            && self.pending_obj.is_none()
            && !self.replaying
        {
            self.repeat(text, cursor);
            return Reply::Eaten;
        }
        if !self.replaying {
            if self.idle() {
                self.rec.clear();
                self.rec_ok = true;
                self.dirty = false;
            }
            if self.rec_ok {
                self.rec.push(key);
            }
        }
        let mut st = State { chars: text.chars().collect(), cur: (*cursor).min(text.chars().count()) };
        let before = st.chars.len();
        let reply = if self.mode == Mode::Command { self.command_key(&mut st, key) } else { self.normal_key(&mut st, key) };
        if !self.replaying {
            if matches!(self.mode, Mode::Visual | Mode::VisualLine | Mode::Command) {
                self.rec_ok = false;
            } else if self.mode == Mode::Insert {
                self.insert_from = st.cur;
            } else if self.rec_ok && self.idle() {
                if self.dirty {
                    self.last_change = Some((std::mem::take(&mut self.rec), String::new()));
                }
                self.rec_ok = false;
            }
        }
        // keep the cursor on a real character outside insert mode
        if self.mode != Mode::Insert && self.mode != Mode::Command {
            st.cur = st.cur.min(last_in_line(&st.chars, st.cur.min(st.chars.len())));
        }
        let _ = before;
        let new_text: String = st.chars.iter().collect();
        if new_text != *text {
            *text = new_text;
        }
        *cursor = st.cur;
        reply
    }

    /// `.`: run the last change again, typing its inserted text if it entered insert mode.
    fn repeat(&mut self, text: &mut String, cursor: &mut usize) {
        let Some((keys, typed)) = self.last_change.clone() else { return };
        self.replaying = true;
        let times = self.count1();
        self.count = 0;
        for _ in 0..times {
            self.replay(&keys, &typed, text, cursor);
        }
        self.replaying = false;
    }

    fn replay(&mut self, keys: &[Key], typed: &str, text: &mut String, cursor: &mut usize) {
        for k in keys {
            self.handle(text, cursor, *k);
        }
        if self.mode == Mode::Insert {
            let mut chars: Vec<char> = text.chars().collect();
            let at = (*cursor).min(chars.len());
            for (i, ch) in typed.chars().enumerate() {
                chars.insert(at + i, ch);
            }
            *cursor = at + typed.chars().count();
            *text = chars.into_iter().collect();
            self.handle(text, cursor, Key::Esc);
        }
    }

    fn snapshot(&mut self, st: &State) {
        self.dirty = true;
        self.undo.push((st.chars.iter().collect(), st.cur));
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn count1(&self) -> usize {
        self.count.max(1)
    }

    fn command_key(&mut self, st: &mut State, key: Key) -> Reply {
        match key {
            Key::Esc | Key::Ctrl('[') => {
                self.mode = Mode::Normal;
                self.cmd.clear();
            }
            Key::Backspace => {
                if self.cmd.pop().is_none() {
                    self.mode = Mode::Normal;
                }
            }
            Key::Char(c) => self.cmd.push(c),
            Key::Enter => {
                let line = std::mem::take(&mut self.cmd);
                self.mode = Mode::Normal;
                if self.cmd_kind == ':' {
                    return Reply::Ex(line);
                }
                if !line.is_empty() {
                    self.search = line;
                }
                if let Some(p) = search_from(&st.chars, st.cur, &self.search, true) {
                    st.cur = p;
                }
            }
            Key::Ctrl(_) => {}
        }
        Reply::Eaten
    }

    fn normal_key(&mut self, st: &mut State, key: Key) -> Reply {
        let visual = matches!(self.mode, Mode::Visual | Mode::VisualLine);
        match key {
            Key::Esc | Key::Ctrl('[') => {
                self.mode = Mode::Normal;
                self.clear_pending();
                return Reply::Eaten;
            }
            Key::Ctrl('r') => {
                if let Some((t, c)) = self.redo.pop() {
                    self.undo.push((st.chars.iter().collect(), st.cur));
                    st.chars = t.chars().collect();
                    st.cur = c;
                }
                self.clear_pending();
                return Reply::Eaten;
            }
            Key::Enter => {
                self.move_to(
                    st,
                    Motion { to: next_line_start(&st.chars, st.cur).map_or(st.cur, |s| first_nonblank(&st.chars, s)), inclusive: false, linewise: true },
                );
                self.clear_pending();
                return Reply::Eaten;
            }
            Key::Backspace => {
                st.cur = st.cur.saturating_sub(1);
                return Reply::Eaten;
            }
            Key::Ctrl(_) => return Reply::Eaten,
            Key::Char(_) => {}
        }
        let Key::Char(c) = key else { return Reply::Eaten };

        if self.pending_replace {
            self.pending_replace = false;
            if st.cur < line_end(&st.chars, st.cur) {
                self.snapshot(st);
                let n = self.count1().min(line_end(&st.chars, st.cur) - st.cur);
                for k in 0..n {
                    if let Some(slot) = st.chars.get_mut(st.cur + k) {
                        *slot = c;
                    }
                }
                st.cur = (st.cur + n).saturating_sub(1);
            }
            self.clear_pending();
            return Reply::Eaten;
        }
        if let Some(kind) = self.pending_find.take() {
            self.last_find = Some((kind, c));
            self.run_find(st, kind, c, false);
            return Reply::Eaten;
        }
        if self.pending_g {
            self.pending_g = false;
            if c == 'g' {
                let line = if self.count > 0 { self.count - 1 } else { 0 };
                let to = first_nonblank(&st.chars, start_of_line_number(&st.chars, line.min(line_count(&st.chars) - 1)));
                self.motion_or_operate(st, Motion { to, inclusive: false, linewise: true });
            } else {
                self.clear_pending();
            }
            return Reply::Eaten;
        }
        if c.is_ascii_digit() && (c != '0' || self.count > 0) {
            let d = c.to_digit(10).unwrap_or(0) as usize;
            self.count = (self.count * 10 + d).min(MAX_COUNT);
            return Reply::Eaten;
        }

        if let Some(kind) = self.pending_obj.take() {
            if let Some((a, b)) = object(&st.chars, st.cur, kind == 'a', c) {
                if visual {
                    self.anchor = a;
                    st.cur = b.saturating_sub(1).max(a);
                } else if let Some(op) = self.op {
                    self.operate(st, op, a, b, false);
                }
            }
            self.clear_pending();
            return Reply::Eaten;
        }
        if (self.op.is_some() || visual) && matches!(c, 'i' | 'a') {
            self.pending_obj = Some(c);
            return Reply::Eaten;
        }

        // operators
        if matches!(c, 'd' | 'c' | 'y') {
            if visual {
                self.apply_visual(st, c);
                return Reply::Eaten;
            }
            if self.op == Some(c) {
                let n = self.count1();
                let s = line_start(&st.chars, st.cur);
                let mut end = st.cur;
                for _ in 1..n {
                    if let Some(next) = next_line_start(&st.chars, end) {
                        end = next;
                    }
                }
                self.operate(st, c, s, line_end(&st.chars, end), true);
                self.clear_pending();
            } else {
                self.op = Some(c);
            }
            return Reply::Eaten;
        }

        // motions
        let n = self.count1();
        let motion = match c {
            'h' => Some(Motion { to: st.cur.saturating_sub(n).max(line_start(&st.chars, st.cur)), inclusive: false, linewise: false }),
            'l' => {
                let limit = if self.op.is_some() || visual { line_end(&st.chars, st.cur) } else { last_in_line(&st.chars, st.cur) };
                Some(Motion { to: (st.cur + n).min(limit), inclusive: false, linewise: false })
            }
            'j' | 'k' => {
                let col = st.cur - line_start(&st.chars, st.cur);
                let mut s = line_start(&st.chars, st.cur);
                for _ in 0..n {
                    let next = if c == 'j' { next_line_start(&st.chars, s) } else { prev_line_start(&st.chars, s) };
                    if let Some(next) = next {
                        s = next;
                    }
                }
                Some(Motion { to: (s + col).min(last_in_line(&st.chars, s)), inclusive: false, linewise: true })
            }
            'w' => {
                let mut to = st.cur;
                for _ in 0..n {
                    to = word_forward(&st.chars, to);
                }
                // `dw` and `cw` stop at the end of the line
                if self.op.is_some()
                    && line_end(&st.chars, st.cur) < to
                    && !st.chars.get(st.cur..to).is_some_and(|s| s.iter().filter(|c| **c == '\n').count() > 1)
                {
                    to = line_end(&st.chars, st.cur);
                }
                if self.op == Some('c') && class(at(&st.chars, st.cur)) != Class::Space {
                    let mut e = st.cur;
                    for i in 0..n {
                        e = if i == 0 && class(at(&st.chars, e + 1)) != class(at(&st.chars, e)) { e } else { word_end(&st.chars, e) };
                    }
                    Some(Motion { to: e, inclusive: true, linewise: false })
                } else {
                    Some(Motion { to, inclusive: false, linewise: false })
                }
            }
            'b' => {
                let mut to = st.cur;
                for _ in 0..n {
                    to = word_back(&st.chars, to);
                }
                Some(Motion { to, inclusive: false, linewise: false })
            }
            'e' => {
                let mut to = st.cur;
                for _ in 0..n {
                    to = word_end(&st.chars, to);
                }
                Some(Motion { to, inclusive: true, linewise: false })
            }
            '0' => Some(Motion { to: line_start(&st.chars, st.cur), inclusive: false, linewise: false }),
            '^' => Some(Motion { to: first_nonblank(&st.chars, st.cur), inclusive: false, linewise: false }),
            '$' => Some(Motion { to: last_in_line(&st.chars, st.cur), inclusive: true, linewise: false }),
            'G' => {
                let line = if self.count > 0 { self.count - 1 } else { line_count(&st.chars) - 1 };
                let to = first_nonblank(&st.chars, start_of_line_number(&st.chars, line.min(line_count(&st.chars) - 1)));
                Some(Motion { to, inclusive: false, linewise: true })
            }
            'n' | 'N' => search_from(&st.chars, st.cur, &self.search, c == 'n').map(|to| Motion { to, inclusive: false, linewise: false }),
            _ => None,
        };
        if let Some(m) = motion {
            self.motion_or_operate(st, m);
            return Reply::Eaten;
        }
        match c {
            'g' => self.pending_g = true,
            'f' | 'F' | 't' | 'T' => self.pending_find = Some(c),
            ';' | ',' => {
                if let Some((kind, target)) = self.last_find {
                    let kind = if c == ';' { kind } else { flip(kind) };
                    self.run_find(st, kind, target, true);
                }
            }
            ':' | '/' => {
                self.mode = Mode::Command;
                self.cmd_kind = c;
                self.cmd.clear();
                self.clear_pending();
            }
            'v' | 'V' => {
                let target = if c == 'v' { Mode::Visual } else { Mode::VisualLine };
                if self.mode == target {
                    self.mode = Mode::Normal;
                } else {
                    if !visual {
                        self.anchor = st.cur;
                    }
                    self.mode = target;
                }
                self.clear_pending();
            }
            'o' if visual => {
                std::mem::swap(&mut self.anchor, &mut st.cur);
            }
            'x' | 'X' | 's' | 'S' | 'D' | 'C' if visual => {
                self.apply_visual(st, if matches!(c, 's' | 'C' | 'S') { 'c' } else { 'd' });
            }
            _ if visual => self.clear_pending(),
            'i' | 'a' | 'I' | 'A' | 'o' | 'O' => self.start_insert(st, c),
            'x' | 'X' | 'D' | 'C' | 's' | 'S' => {
                let n = self.count1();
                let (s, e) = (line_start(&st.chars, st.cur), line_end(&st.chars, st.cur));
                match c {
                    'x' if st.cur < e => self.operate(st, 'd', st.cur, (st.cur + n).min(e), false),
                    'X' if st.cur > s => self.operate(st, 'd', st.cur.saturating_sub(n).max(s), st.cur, false),
                    'D' => self.operate(st, 'd', st.cur, e, false),
                    'C' => self.operate(st, 'c', st.cur, e, false),
                    's' => self.operate(st, 'c', st.cur, (st.cur + n).min(e), false),
                    'S' => self.operate(st, 'c', s, e, true),
                    _ => {}
                }
                self.clear_pending();
            }
            'r' => self.pending_replace = true,
            '~' => {
                if st.cur < line_end(&st.chars, st.cur) {
                    self.snapshot(st);
                    let n = self.count1().min(line_end(&st.chars, st.cur) - st.cur);
                    for k in 0..n {
                        if let Some(slot) = st.chars.get_mut(st.cur + k) {
                            let flipped: String = if slot.is_uppercase() { slot.to_lowercase().collect() } else { slot.to_uppercase().collect() };
                            if let Some(f) = flipped.chars().next().filter(|_| flipped.chars().count() == 1) {
                                *slot = f;
                            }
                        }
                    }
                    st.cur = (st.cur + n).min(last_in_line(&st.chars, st.cur));
                }
                self.clear_pending();
            }
            'J' => {
                let e = line_end(&st.chars, st.cur);
                if e < st.chars.len() {
                    self.snapshot(st);
                    st.chars.remove(e);
                    let mut j = e;
                    while matches!(at(&st.chars, j), Some(' ' | '\t')) {
                        st.chars.remove(j);
                    }
                    if j > 0 && at(&st.chars, j).is_some_and(|c| c != '\n') && at(&st.chars, j - 1) != Some(' ') {
                        st.chars.insert(j, ' ');
                    } else {
                        j = j.saturating_sub(1);
                    }
                    st.cur = j;
                }
                self.clear_pending();
            }
            'p' | 'P' => {
                self.paste(st, c == 'p');
                self.clear_pending();
            }
            'u' => {
                if let Some((t, cur)) = self.undo.pop() {
                    self.redo.push((st.chars.iter().collect(), st.cur));
                    st.chars = t.chars().collect();
                    st.cur = cur;
                }
                self.clear_pending();
            }
            _ => self.clear_pending(),
        }
        Reply::Eaten
    }

    fn run_find(&mut self, st: &mut State, kind: char, target: char, repeat: bool) {
        let count = self.count1();
        // `;` after `t` should not stay put: start the search one further
        let from = if repeat && matches!(kind, 't') {
            st.cur + 1
        } else if repeat && matches!(kind, 'T') {
            st.cur.saturating_sub(1)
        } else {
            st.cur
        };
        if let Some((to, forward)) = find_in_line(&st.chars, from, kind, target, count) {
            self.motion_or_operate(st, Motion { to, inclusive: forward, linewise: false });
        } else {
            self.clear_pending();
        }
    }

    fn motion_or_operate(&mut self, st: &mut State, m: Motion) {
        match self.op {
            Some(op) if !matches!(self.mode, Mode::Visual | Mode::VisualLine) => {
                let (a, b) = if m.to >= st.cur { (st.cur, m.to + usize::from(m.inclusive)) } else { (m.to, st.cur) };
                self.operate(st, op, a, b.min(st.chars.len()), m.linewise);
                self.clear_pending();
            }
            _ => {
                self.move_to(st, m);
                self.clear_pending();
            }
        }
    }

    fn move_to(&mut self, st: &mut State, m: Motion) {
        st.cur = m.to.min(st.chars.len());
    }

    /// Apply operator `op` to `[a, b)`; linewise ranges are widened to whole lines.
    fn operate(&mut self, st: &mut State, op: char, a: usize, b: usize, linewise: bool) {
        let (mut a, mut b) = (a.min(b), a.max(b));
        if linewise {
            let (ls, le) = (line_start(&st.chars, a), line_end(&st.chars, b.min(st.chars.len())));
            if op == 'c' {
                // keep one empty line to type into
                self.reg = text_of(&st.chars, ls, le);
                self.reg_linewise = true;
                self.snapshot(st);
                st.chars.drain(ls..le);
                st.cur = ls;
                self.mode = Mode::Insert;
                return;
            }
            a = ls;
            b = if le < st.chars.len() { le + 1 } else { le };
            self.reg = text_of(&st.chars, a, b);
            if !self.reg.ends_with('\n') {
                self.reg.push('\n');
            }
            self.reg_linewise = true;
            if op == 'y' {
                st.cur = st.cur.min(a.max(line_start(&st.chars, st.cur)));
                st.cur = if line_number(&st.chars, st.cur) > line_number(&st.chars, a) { a } else { st.cur };
                return;
            }
            self.snapshot(st);
            let trailing_removed = b >= st.chars.len() && a > 0;
            st.chars.drain(a..b);
            if trailing_removed {
                st.chars.pop();
                a = line_start(&st.chars, a.saturating_sub(1));
            }
            st.cur = first_nonblank(&st.chars, a.min(st.chars.len()));
            return;
        }
        if a >= b {
            if op == 'c' {
                self.snapshot(st);
                self.mode = Mode::Insert;
            }
            return;
        }
        self.reg = text_of(&st.chars, a, b);
        self.reg_linewise = false;
        if op == 'y' {
            st.cur = a;
            return;
        }
        self.snapshot(st);
        st.chars.drain(a..b);
        st.cur = a;
        if op == 'c' {
            self.mode = Mode::Insert;
        }
    }

    fn apply_visual(&mut self, st: &mut State, op: char) {
        let (a, b) = (self.anchor.min(st.cur), self.anchor.max(st.cur));
        let linewise = self.mode == Mode::VisualLine;
        self.mode = Mode::Normal;
        self.operate(st, op, a, (b + 1).min(st.chars.len()), linewise);
        self.clear_pending();
    }

    fn start_insert(&mut self, st: &mut State, c: char) {
        self.snapshot(st);
        match c {
            'a' => {
                if st.cur < line_end(&st.chars, st.cur) {
                    st.cur += 1;
                }
            }
            'I' => st.cur = first_nonblank(&st.chars, st.cur),
            'A' => st.cur = line_end(&st.chars, st.cur),
            'o' => {
                let e = line_end(&st.chars, st.cur);
                let indent: String = st.chars.iter().skip(line_start(&st.chars, st.cur)).take_while(|c| matches!(**c, ' ' | '\t')).collect();
                st.chars.insert(e, '\n');
                for (k, ch) in indent.chars().enumerate() {
                    st.chars.insert(e + 1 + k, ch);
                }
                st.cur = e + 1 + indent.chars().count();
            }
            'O' => {
                let s = line_start(&st.chars, st.cur);
                let indent: String = st.chars.iter().skip(s).take_while(|c| matches!(**c, ' ' | '\t')).collect();
                st.chars.insert(s, '\n');
                for (k, ch) in indent.chars().enumerate() {
                    st.chars.insert(s + k, ch);
                }
                st.cur = s + indent.chars().count();
            }
            _ => {}
        }
        self.mode = Mode::Insert;
        self.clear_pending();
    }

    fn paste(&mut self, st: &mut State, after: bool) {
        if self.reg.is_empty() {
            return;
        }
        self.snapshot(st);
        for _ in 0..self.count1() {
            if self.reg_linewise {
                let at_line = if after { next_line_start(&st.chars, st.cur).unwrap_or(st.chars.len()) } else { line_start(&st.chars, st.cur) };
                let mut body = self.reg.clone();
                if after && at_line == st.chars.len() && !st.chars.is_empty() && st.chars.last() != Some(&'\n') {
                    st.chars.push('\n');
                    body.pop();
                    for (k, ch) in body.chars().enumerate() {
                        st.chars.insert(at_line + 1 + k, ch);
                    }
                    st.cur = at_line + 1;
                    continue;
                }
                for (k, ch) in body.chars().enumerate() {
                    st.chars.insert(at_line + k, ch);
                }
                st.cur = first_nonblank(&st.chars, at_line);
            } else {
                let pos = if after && st.cur < line_end(&st.chars, st.cur) { st.cur + 1 } else { st.cur };
                let n = self.reg.chars().count();
                for (k, ch) in self.reg.chars().enumerate() {
                    st.chars.insert(pos + k, ch);
                }
                st.cur = pos + n - 1;
            }
        }
    }
}

/// The range `[a, b)` of a text object around `cur`: `w` (word), a quote (`"`, `'`, `` ` ``) or a bracket pair
/// (`(` `)` `b`, `[` `]`, `{` `}` `B`, `<` `>`). `around` includes the delimiters (or the trailing space of a word).
fn object(c: &[char], cur: usize, around: bool, kind: char) -> Option<(usize, usize)> {
    match kind {
        'w' => {
            let here = class(at(c, cur));
            let (mut a, mut b) = (cur, cur + 1);
            while a > line_start(c, cur) && class(at(c, a - 1)) == here {
                a -= 1;
            }
            while b < line_end(c, cur) && class(at(c, b)) == here {
                b += 1;
            }
            if around && here != Class::Space {
                let end = b;
                while b < line_end(c, cur) && class(at(c, b)) == Class::Space {
                    b += 1;
                }
                if b == end {
                    while a > line_start(c, cur) && class(at(c, a - 1)) == Class::Space {
                        a -= 1;
                    }
                }
            }
            (cur < c.len()).then_some((a, b))
        }
        '"' | '\'' | '`' => {
            let (s, e) = (line_start(c, cur), line_end(c, cur));
            let marks: Vec<usize> = (s..e).filter(|i| at(c, *i) == Some(kind) && (*i == s || at(c, *i - 1) != Some('\\'))).collect();
            let pair = marks.chunks(2).find(|p| p.len() == 2 && p.first().is_some_and(|a| *a <= cur) && p.get(1).is_some_and(|b| cur <= *b))?;
            let (a, b) = (*pair.first()?, *pair.get(1)?);
            Some(if around { (a, b + 1) } else { (a + 1, b) })
        }
        '(' | ')' | 'b' | '[' | ']' | '{' | '}' | 'B' | '<' | '>' => {
            let (open, close) = match kind {
                '(' | ')' | 'b' => ('(', ')'),
                '[' | ']' => ('[', ']'),
                '{' | '}' | 'B' => ('{', '}'),
                _ => ('<', '>'),
            };
            // the nearest unmatched opener at or before the cursor
            let mut depth = 0usize;
            let mut a = None;
            for i in (0..=cur.min(c.len().saturating_sub(1))).rev() {
                match at(c, i) {
                    Some(ch) if ch == close && i != cur => depth += 1,
                    Some(ch) if ch == open => {
                        if depth == 0 {
                            a = Some(i);
                            break;
                        }
                        depth -= 1;
                    }
                    _ => {}
                }
            }
            let a = a?;
            let mut depth = 0usize;
            let b = (a + 1..c.len()).find(|i| match at(c, *i) {
                Some(ch) if ch == open => {
                    depth += 1;
                    false
                }
                Some(ch) if ch == close => {
                    if depth == 0 {
                        true
                    } else {
                        depth -= 1;
                        false
                    }
                }
                _ => false,
            })?;
            Some(if around { (a, b + 1) } else { (a + 1, b) })
        }
        _ => None,
    }
}

fn flip(kind: char) -> char {
    match kind {
        'f' => 'F',
        'F' => 'f',
        't' => 'T',
        _ => 't',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Type `keys` (`<Esc>`, `<CR>`, `<C-r>` are special) into `text` with the cursor at `cur`; returns text and cursor.
    fn run(text: &str, cur: usize, keys: &str) -> (String, usize, Vim) {
        let mut v = Vim::new();
        let mut t = text.to_owned();
        let mut c = cur;
        let mut rest = keys;
        while !rest.is_empty() {
            let (key, len) = if rest.starts_with("<Esc>") {
                (Key::Esc, 5)
            } else if rest.starts_with("<CR>") {
                (Key::Enter, 4)
            } else if rest.starts_with("<C-r>") {
                (Key::Ctrl('r'), 5)
            } else {
                let ch = rest.chars().next().unwrap_or(' ');
                (Key::Char(ch), ch.len_utf8())
            };
            rest = rest.get(len..).unwrap_or("");
            if v.mode() == Mode::Insert && key != Key::Esc {
                // emulate the editor typing
                if let Key::Char(ch) = key {
                    let idx = t.char_indices().nth(c).map_or(t.len(), |(i, _)| i);
                    t.insert(idx, ch);
                    c += 1;
                } else if key == Key::Enter {
                    let idx = t.char_indices().nth(c).map_or(t.len(), |(i, _)| i);
                    t.insert(idx, '\n');
                    c += 1;
                }
                continue;
            }
            v.handle(&mut t, &mut c, key);
        }
        (t, c, v)
    }
    fn text(t: &str, cur: usize, keys: &str) -> String {
        run(t, cur, keys).0
    }

    #[test]
    fn motions_and_counts() {
        assert_eq!(run("hello world", 0, "w").1, 6);
        assert_eq!(run("hello world", 0, "$").1, 10);
        assert_eq!(run("hello world", 8, "0").1, 0);
        assert_eq!(run("a b c d", 0, "3w").1, 6);
        assert_eq!(run("hello world", 0, "e").1, 4);
        assert_eq!(run("hello world", 10, "b").1, 6);
        assert_eq!(run("a\nbc\nd", 0, "j").1, 2);
        assert_eq!(run("a\nbc\nd", 0, "G").1, 5);
        assert_eq!(run("a\nbc\nd", 5, "gg").1, 0);
        assert_eq!(run("a,b,c", 0, "f,").1, 1);
        assert_eq!(run("a,b,c", 0, "f,;").1, 3);
        assert_eq!(run("abc", 0, "5l").1, 2, "stops on the last char");
    }

    #[test]
    fn delete_change_yank() {
        assert_eq!(text("hello world", 0, "dw"), "world");
        assert_eq!(text("hello world", 0, "cwbye<Esc>"), "bye world");
        assert_eq!(text("one\ntwo\nthree", 4, "dd"), "one\nthree");
        assert_eq!(text("one\ntwo\nthree", 0, "2dd"), "three");
        assert_eq!(text("one\ntwo", 4, "dd"), "one");
        assert_eq!(text("hello", 0, "x"), "ello");
        assert_eq!(text("hello", 0, "3x"), "lo");
        assert_eq!(text("hello world", 6, "D"), "hello ");
        assert_eq!(text("hello world", 0, "d$"), "");
        assert_eq!(text("a(b)c", 0, "dt)"), ")c");
        assert_eq!(text("one\ntwo", 0, "ccX<Esc>"), "X\ntwo");
    }

    #[test]
    fn paste_and_undo() {
        assert_eq!(text("ab", 0, "xp"), "ba");
        assert_eq!(text("one\ntwo", 0, "yyp"), "one\none\ntwo");
        assert_eq!(text("one\ntwo", 0, "ddp"), "two\none");
        assert_eq!(text("one\ntwo", 0, "yyjp"), "one\ntwo\none");
        assert_eq!(text("one\ntwo", 0, "dwu"), "one\ntwo");
        assert_eq!(text("hello", 0, "xxu"), "ello");
        assert_eq!(text("hello", 0, "xu<C-r>"), "ello");
        assert_eq!(text("a", 0, "yy3p"), "a\na\na\na");
    }

    #[test]
    fn insert_modes() {
        assert_eq!(text("bc", 0, "ia<Esc>"), "abc");
        assert_eq!(text("ab", 0, "Ac<Esc>"), "abc");
        assert_eq!(text("bc", 1, "Ia<Esc>"), "abc");
        assert_eq!(text("a\nc", 0, "ob<Esc>"), "a\nb\nc");
        assert_eq!(text("b", 0, "Oa<Esc>"), "a\nb");
        assert_eq!(text("    x", 4, "oy<Esc>"), "    x\n    y", "keeps indentation");
        let (t, c, v) = run("ab", 0, "ix<Esc>");
        assert_eq!((t.as_str(), c, v.mode()), ("xab", 0, Mode::Normal));
        assert_eq!(text("abc", 0, "rx"), "xbc");
        assert_eq!(text("abc", 0, "~"), "Abc");
        assert_eq!(text("a\n  b", 0, "J"), "a b");
        assert_eq!(text("abc", 0, "sX<Esc>"), "Xbc");
    }

    #[test]
    fn visual_mode() {
        assert_eq!(text("hello world", 0, "vld"), "llo world");
        assert_eq!(text("hello world", 0, "ve"), "hello world");
        assert_eq!(text("hello world", 0, "vey$p"), "hello worldhello");
        assert_eq!(text("a\nb\nc", 0, "Vjd"), "c");
        assert_eq!(text("abc", 0, "vlcX<Esc>"), "Xc");
        assert_eq!(run("abc", 0, "v").2.mode(), Mode::Visual);
        assert_eq!(run("abc", 0, "v<Esc>").2.mode(), Mode::Normal);
    }

    #[test]
    fn text_objects() {
        assert_eq!(text("foo bar baz", 5, "diw"), "foo  baz");
        assert_eq!(text("foo bar baz", 5, "daw"), "foo baz");
        assert_eq!(text("foo bar", 5, "daw"), "foo", "no trailing space: takes the leading one");
        assert_eq!(text("foo bar baz", 5, "ciwX<Esc>"), "foo X baz");
        assert_eq!(text("f(a, b) + 1", 3, "di("), "f() + 1");
        assert_eq!(text("f(a, b) + 1", 3, "da("), "f + 1");
        assert_eq!(text("f(g(x), y)", 4, "di("), "f(g(), y)", "innermost pair");
        assert_eq!(text("say \"hi there\" ok", 6, "di\""), "say \"\" ok");
        assert_eq!(text("say \"hi\" ok", 6, "da\""), "say  ok");
        assert_eq!(text("x = [1, 2]", 6, "yi[$p"), "x = [1, 2]1, 2");
        assert_eq!(text("a (b c) d", 4, "vi(d"), "a () d");
        assert_eq!(text("no pair here", 3, "di("), "no pair here");
    }

    #[test]
    fn dot_repeats_the_last_change() {
        assert_eq!(text("a b c d", 0, "dw."), "c d");
        assert_eq!(text("a b c d e", 0, "dw2."), "d e", "2. repeats it twice");
        assert_eq!(text("one two three", 0, "cwX<Esc>w."), "X X three");
        assert_eq!(text("a\nb\nc", 0, "dd."), "c");
        assert_eq!(text("abc", 0, "x."), "c");
        assert_eq!(text("a b", 0, "Ax<Esc>0."), "a bxx");
        assert_eq!(text("foo(a) bar(b)", 4, "ci(Z<Esc>f(l."), "foo(Z) bar(Z)");
        // yank and motions are not changes
        assert_eq!(text("a b c", 0, "xwyw."), "  c", "a yank is not a change, so . repeats the x");
    }

    #[test]
    fn command_line_and_search() {
        let mut v = Vim::new();
        let (mut t, mut c) = ("one two".to_owned(), 0usize);
        for k in [Key::Char(':'), Key::Char('w'), Key::Char('q')] {
            assert_eq!(v.handle(&mut t, &mut c, k), Reply::Eaten);
        }
        assert_eq!(v.status(), ":wq");
        assert_eq!(v.handle(&mut t, &mut c, Key::Enter), Reply::Ex("wq".to_owned()));
        assert_eq!(v.mode(), Mode::Normal);
        assert_eq!(run("one two one", 0, "/two<CR>").1, 4);
        assert_eq!(run("one two one", 0, "/one<CR>").1, 8);
        assert_eq!(run("one two one", 0, "/one<CR>n").1, 0, "wraps");
        assert_eq!(run("ab", 0, ":x<Esc>").2.mode(), Mode::Normal);
    }

    #[test]
    fn insert_mode_passes_keys_through() {
        let mut v = Vim::new();
        let (mut t, mut c) = (String::new(), 0usize);
        v.handle(&mut t, &mut c, Key::Char('i'));
        assert_eq!(v.handle(&mut t, &mut c, Key::Char('x')), Reply::Pass);
        assert_eq!(v.handle(&mut t, &mut c, Key::Esc), Reply::Eaten);
        assert_eq!(v.mode(), Mode::Normal);
    }

    #[test]
    fn never_panics_on_odd_input() {
        for text in ["", "\n", "a", "\n\n\n", "é\nü"] {
            for keys in [
                "dd", "x", "p", "dw", "cw<Esc>", "J", "G", "gg", "$", "vd", "Vd", "ddp", "ol<Esc>", "yyP", "5x", "dj", "dk", "de", "db", "r", "u", "/a<CR>",
                "fa", "dfa", "~",
            ] {
                let _ = run(text, 0, keys);
                let _ = run(text, 5, keys);
            }
        }
    }

    #[test]
    fn random_key_sequences_never_panic() {
        let alphabet: Vec<Key> = "dcyxXDCsSrJ~pPuhjklwbe0^$GgfFtT;,nNiaIAoOvV:/ab12\n."
            .chars()
            .map(Key::Char)
            .chain([Key::Esc, Key::Enter, Key::Backspace, Key::Ctrl('r')])
            .collect();
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for round in 0..300 {
            let mut v = Vim::new();
            let mut t = ["", "ab\n\ncd e", "x", "\n", "é ü\nfoo(bar)"][round % 5].to_owned();
            let mut c = 0usize;
            for _ in 0..80 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let k = alphabet.get((seed >> 33) as usize % alphabet.len()).copied().unwrap_or(Key::Esc);
                if v.handle(&mut t, &mut c, k) == Reply::Pass {
                    // emulate typing a letter
                    let idx = t.char_indices().nth(c).map_or(t.len(), |(i, _)| i);
                    t.insert(idx, 'z');
                    c += 1;
                }
                assert!(c <= t.chars().count());
            }
        }
    }
}
