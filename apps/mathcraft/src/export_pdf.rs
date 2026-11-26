//! Export a notebook as a PDF: text cells (headings, paragraphs, lists, quotes, tables, code, `$maths$` and links),
//! code cells in a monospace block, and under them what the cell printed, its result typeset as maths (or as a
//! picture for a plot) and its error. The pages are A4 in the light colours; the writer is in `pdf.rs`.

use crate::markdown::{self, Piece};
use crate::pdf::{FaceId, PAGE_H, PAGE_W, Pdf, Rgb};
use crate::theme::Theme;
use mathcraft_eval::Value;
use mathcraft_notebook::{Cell, CellKind, Notebook};
use mathcraft_typeset::{Doc, Item, Layout, Metrics, Role};

const MARGIN_X: f32 = 56.0;
const MARGIN_TOP: f32 = 58.0;
const MARGIN_BOTTOM: f32 = 66.0;
const BODY: f32 = 10.5;
const MONO: f32 = 8.8;
const MATH: f32 = 15.0;
/// Line height as a multiple of the type size.
const LEADING: f32 = 1.5;
const OUT_INDENT: f32 = 14.0;

const INK: Rgb = [0x1d, 0x1d, 0x1f];
const DIM: Rgb = [0x6e, 0x6e, 0x73];
const CODE_BG: Rgb = [0xf5, 0xf5, 0xf7];
const LINE: Rgb = [0xd2, 0xd2, 0xd7];
const ACCENT: Rgb = [0x0a, 0x64, 0xd6];
const BAD: Rgb = [0xc5, 0x22, 0x1f];

/// Widths of math text, measured with the same fonts the page is drawn with.
struct Measure<'a>(&'a crate::pdf::Faces);

impl Metrics for Measure<'_> {
    fn advance(&self, text: &str, size: f32) -> f32 {
        self.0.width(FaceId::Math, text, size)
    }
}

#[derive(Clone, Copy)]
struct Style {
    face: FaceId,
    size: f32,
    color: Rgb,
    slant: f32,
    strike: bool,
    underline: bool,
}

impl Style {
    fn body() -> Style {
        Style { face: FaceId::Text, size: BODY, color: INK, slant: 0.0, strike: false, underline: false }
    }
}

enum Atom {
    Word { text: String, style: Style, link: Option<String>, width: f32 },
    Space { width: f32 },
    Math(Layout),
}

struct Line {
    atoms: Vec<Atom>,
    height: f32,
    /// Distance from the top of the line to its baseline.
    baseline: f32,
}

/// The blocks of a text cell.
#[derive(Debug, PartialEq)]
enum Block {
    Heading(usize, String),
    Para(String),
    Item(String, String),
    Quote(String),
    Rule,
    Code(Vec<String>),
    Display(String),
    Table(Vec<Vec<String>>),
}

fn blocks(source: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let (mut para, mut rows): (String, Vec<Vec<String>>) = (String::new(), Vec::new());
    let mut code: Option<Vec<String>> = None;
    // the last block is a list item that an indented line may continue
    let mut item_open = false;
    fn flush(out: &mut Vec<Block>, para: &mut String, rows: &mut Vec<Vec<String>>) {
        if !para.trim().is_empty() {
            out.push(Block::Para(para.trim().to_owned()));
        }
        para.clear();
        if !rows.is_empty() {
            out.push(Block::Table(std::mem::take(rows)));
        }
    }
    for raw in source.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            match code.take() {
                Some(lines) => out.push(Block::Code(lines)),
                None => {
                    flush(&mut out, &mut para, &mut rows);
                    code = Some(Vec::new());
                }
            }
            continue;
        }
        if let Some(lines) = code.as_mut() {
            lines.push(line.to_owned());
            continue;
        }
        if trimmed.starts_with('|') {
            if !para.is_empty() {
                flush(&mut out, &mut para, &mut rows);
            }
            rows.push(markdown::table_row(trimmed));
            continue;
        }
        let continues = item_open && line.starts_with(char::is_whitespace) && !trimmed.is_empty();
        item_open = false;
        if continues && let Some(Block::Item(_, text)) = out.last_mut() {
            text.push(' ');
            text.push_str(trimmed);
            item_open = true;
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut out, &mut para, &mut rows);
        } else if let Some(src) = trimmed.strip_prefix("$$").and_then(|r| r.strip_suffix("$$")).filter(|r| !r.trim().is_empty()) {
            flush(&mut out, &mut para, &mut rows);
            out.push(Block::Display(src.trim().to_owned()));
        } else if let Some((level, text)) = markdown::heading(line) {
            flush(&mut out, &mut para, &mut rows);
            out.push(Block::Heading(level, text.to_owned()));
        } else if matches!(trimmed, "---" | "***" | "___") {
            flush(&mut out, &mut para, &mut rows);
            out.push(Block::Rule);
        } else if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            flush(&mut out, &mut para, &mut rows);
            out.push(Block::Item("•".into(), item.trim().to_owned()));
            item_open = true;
        } else if let Some((n, item)) = markdown::numbered(trimmed) {
            flush(&mut out, &mut para, &mut rows);
            out.push(Block::Item(format!("{n}."), item.trim().to_owned()));
            item_open = true;
        } else if let Some(q) = trimmed.strip_prefix('>') {
            flush(&mut out, &mut para, &mut rows);
            out.push(Block::Quote(q.trim().to_owned()));
        } else {
            if !rows.is_empty() {
                flush(&mut out, &mut para, &mut rows);
            }
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(trimmed);
        }
    }
    if let Some(lines) = code {
        out.push(Block::Code(lines));
    }
    flush(&mut out, &mut para, &mut rows);
    out
}

struct Writer {
    pdf: Pdf,
    theme: Theme,
    title: String,
    /// Distance from the top of the page to where the next block starts.
    y: f32,
}

/// The notebook as a PDF file.
pub fn notebook_pdf(nb: &Notebook, title: &str) -> Result<Vec<u8>, String> {
    let pdf = Pdf::new().ok_or("the fonts could not be read")?;
    let mut w = Writer { pdf, theme: Theme::light(), title: title.to_owned(), y: MARGIN_TOP };
    for cell in nb.cells() {
        match cell.kind {
            CellKind::Markdown => w.text_cell(&cell.source),
            CellKind::Code => w.code_cell(cell),
        }
    }
    w.footer();
    Ok(w.pdf.finish(title))
}

impl Writer {
    fn width(&self) -> f32 {
        PAGE_W - 2.0 * MARGIN_X
    }

    fn footer(&mut self) {
        let size = 8.0;
        let n = self.pdf.page_number().to_string();
        let title = self.title.clone();
        self.pdf.text(FaceId::Text, &title, MARGIN_X, PAGE_H - 36.0, size, DIM, 0.0);
        let w = self.pdf.faces.width(FaceId::Text, &n, size);
        self.pdf.text(FaceId::Text, &n, PAGE_W - MARGIN_X - w, PAGE_H - 36.0, size, DIM, 0.0);
    }

    /// Room for a block `h` tall: start a new page when it does not fit (a block taller than a page just starts one).
    fn need(&mut self, h: f32) {
        if self.y + h > PAGE_H - MARGIN_BOTTOM && self.y > MARGIN_TOP {
            self.footer();
            self.pdf.new_page();
            self.y = MARGIN_TOP;
        }
    }

    /// Space between blocks, none at the top of a page.
    fn gap(&mut self, h: f32) {
        if self.y > MARGIN_TOP {
            self.y += h;
        }
    }

    // ---- text ----------------------------------------------------------------------------------------------------

    fn word(&self, text: &str, style: Style, link: Option<&str>) -> Atom {
        Atom::Word { text: text.to_owned(), style, link: link.map(str::to_owned), width: self.pdf.faces.width(style.face, text, style.size) }
    }

    /// Cut `text` into words and spaces in one style.
    fn words(&self, text: &str, style: Style, link: Option<&str>, out: &mut Vec<Atom>) {
        let space = self.pdf.faces.advance(style.face, ' ', style.size);
        let mut word = String::new();
        for c in text.chars() {
            if c.is_whitespace() {
                if !word.is_empty() {
                    out.push(self.word(&std::mem::take(&mut word), style, link));
                }
                if !matches!(out.last(), Some(Atom::Space { .. })) {
                    out.push(Atom::Space { width: space });
                }
            } else {
                word.push(c);
            }
        }
        if !word.is_empty() {
            out.push(self.word(&word, style, link));
        }
    }

    fn inline(&self, text: &str, base: Style) -> Vec<Atom> {
        let mut out = Vec::new();
        for piece in markdown::parts(text) {
            match piece {
                Piece::Text(t) => {
                    for s in markdown::inline(&t) {
                        let mut st = base;
                        if s.bold {
                            st.face = FaceId::Strong;
                        }
                        if s.italic {
                            st.slant = 0.2;
                        }
                        if s.code {
                            st = Style { face: FaceId::Mono, size: base.size * 0.92, ..st };
                        }
                        st.strike |= s.strike;
                        self.words(&s.text, st, None, &mut out);
                    }
                }
                Piece::Math(src) => {
                    match crate::math_view::inline_expr(&src).and_then(|e| mathcraft_typeset::layout(&Doc::Expr(e), &Measure(&self.pdf.faces), base.size)) {
                        Some(l) => out.push(Atom::Math(l)),
                        None => self.words(&src, Style { face: FaceId::Mono, size: base.size * 0.92, ..base }, None, &mut out),
                    }
                }
                Piece::Link(label, url) => self.words(&label, Style { color: ACCENT, underline: true, ..base }, Some(&url), &mut out),
            }
        }
        out
    }

    /// Break atoms into lines no wider than `avail`; a line is at least `min_height` tall.
    fn wrap(&self, atoms: Vec<Atom>, avail: f32, min_height: f32) -> Vec<Line> {
        let mut lines = Vec::new();
        let mut cur: Vec<Atom> = Vec::new();
        let mut width = 0.0;
        let width_of = |a: &Atom| match a {
            Atom::Word { width, .. } | Atom::Space { width } => *width,
            Atom::Math(l) => l.width,
        };
        for atom in atoms {
            let w = width_of(&atom);
            let space = matches!(atom, Atom::Space { .. });
            if cur.is_empty() && space {
                continue;
            }
            if !space && width + w > avail && !cur.is_empty() {
                lines.push(Self::line(std::mem::take(&mut cur), min_height));
                width = 0.0;
            }
            width += w;
            cur.push(atom);
        }
        if !cur.is_empty() {
            lines.push(Self::line(cur, min_height));
        }
        lines
    }

    fn line(mut atoms: Vec<Atom>, min_height: f32) -> Line {
        while matches!(atoms.last(), Some(Atom::Space { .. })) {
            atoms.pop();
        }
        let (mut up, mut down) = (0.0_f32, 0.0_f32);
        for a in &atoms {
            let (u, d) = match a {
                Atom::Word { style, .. } => (style.size * 0.8, style.size * 0.22),
                Atom::Space { .. } => (0.0, 0.0),
                Atom::Math(l) => (l.ascent + 1.0, l.descent + 1.0),
            };
            up = up.max(u);
            down = down.max(d);
        }
        let height = (up + down).max(min_height);
        Line { atoms, height, baseline: up + (height - up - down) / 2.0 }
    }

    fn draw_line(&mut self, line: &Line, x: f32, top: f32) {
        let base = top + line.baseline;
        let mut cx = x;
        for a in &line.atoms {
            match a {
                Atom::Word { text, style, link, width } => {
                    self.pdf.text(style.face, text, cx, base, style.size, style.color, style.slant);
                    if style.underline {
                        self.pdf.line([cx, base + 1.3], [cx + width, base + 1.3], 0.6, style.color);
                    }
                    if style.strike {
                        self.pdf.line([cx, base - style.size * 0.28], [cx + width, base - style.size * 0.28], 0.6, style.color);
                    }
                    if let Some(url) = link {
                        self.pdf.link(cx, base - style.size * 0.8, *width, style.size * 1.1, url);
                    }
                    cx += width;
                }
                Atom::Space { width } => cx += width,
                Atom::Math(l) => {
                    self.draw_math(l, cx, base);
                    cx += l.width;
                }
            }
        }
    }

    /// Lines of text from `x`, with `avail` room, breaking pages between lines.
    fn flow(&mut self, atoms: Vec<Atom>, x: f32, avail: f32, size: f32) {
        for line in self.wrap(atoms, avail, size * LEADING) {
            self.need(line.height);
            self.draw_line(&line, x, self.y);
            self.y += line.height;
        }
    }

    fn math_color(&self, role: Role) -> Rgb {
        let m = &self.theme.math;
        let c = match role {
            Role::Text => m.text,
            Role::Variable => m.variable,
            Role::Number => m.number,
            Role::Operator => m.operator,
            Role::Function => m.function,
            Role::Constant => m.constant,
            Role::Delim => m.delim,
        };
        [c.r(), c.g(), c.b()]
    }

    /// Draw a typeset layout with its baseline origin at `(x, baseline)`.
    fn draw_math(&mut self, l: &Layout, x: f32, baseline: f32) {
        for item in &l.items {
            match item {
                Item::Text { x: ix, y, text, size, role } => {
                    let c = self.math_color(*role);
                    self.pdf.text(FaceId::Math, text, x + ix, baseline + y, *size, c, 0.0);
                }
                Item::Rule { x: ix, y, w, h, role } => {
                    let c = self.math_color(*role);
                    self.pdf.rect(x + ix, baseline + y, *w, h.max(0.6), c);
                }
                Item::Stroke { points, width, role } => {
                    let c = self.math_color(*role);
                    let pts: Vec<[f32; 2]> = points.iter().map(|[px, py]| [x + px, baseline + py]).collect();
                    self.pdf.polyline(&pts, width.max(0.6), c);
                }
            }
        }
    }

    // ---- text cells ----------------------------------------------------------------------------------------------

    fn text_cell(&mut self, source: &str) {
        self.gap(12.0);
        for (n, block) in blocks(source).iter().enumerate() {
            if n > 0 {
                self.gap(5.0);
            }
            self.block(block);
        }
    }

    fn block(&mut self, block: &Block) {
        let (left, full) = (MARGIN_X, self.width());
        match block {
            Block::Heading(level, text) => {
                let size = match level {
                    1 => 20.0,
                    2 => 15.5,
                    _ => 12.5,
                };
                self.gap(size * 0.4);
                self.need(size * 2.4);
                let atoms = self.inline(text, Style { face: FaceId::Strong, size, ..Style::body() });
                self.flow(atoms, left, full, size);
            }
            Block::Para(text) => {
                let atoms = self.inline(text, Style::body());
                self.flow(atoms, left, full, BODY);
            }
            Block::Item(marker, text) => {
                let indent = 20.0;
                self.need(BODY * LEADING);
                let (y, w) = (self.y, self.pdf.faces.width(FaceId::Text, marker, BODY));
                self.pdf.text(FaceId::Text, marker, left + indent - 6.0 - w, y + BODY * (0.5 + (LEADING - 1.0) / 2.0 + 0.3), BODY, DIM, 0.0);
                let atoms = self.inline(text, Style::body());
                self.flow(atoms, left + indent, full - indent, BODY);
            }
            Block::Quote(text) => {
                let top = self.y;
                let atoms = self.inline(text, Style { color: DIM, ..Style::body() });
                self.flow(atoms, left + 14.0, full - 14.0, BODY);
                if self.y > top {
                    self.pdf.rect(left + 2.0, top + 1.0, 2.5, self.y - top - 2.0, LINE);
                }
            }
            Block::Rule => {
                self.need(10.0);
                let y = self.y + 5.0;
                self.pdf.line([left, y], [left + full, y], 0.6, LINE);
                self.y += 10.0;
            }
            Block::Code(lines) => self.code_block(&lines.join("\n"), MONO, INK),
            Block::Display(src) => match crate::math_view::inline_expr(src)
                .and_then(|e| mathcraft_typeset::layout_wrapped(&Doc::Expr(e), &Measure(&self.pdf.faces), MATH, full - 8.0))
            {
                Some(l) => self.math_block(&l, left + 8.0),
                None => self.code_block(src, MONO, INK),
            },
            Block::Table(rows) => self.table(rows),
        }
    }

    fn table(&mut self, rows: &[Vec<String>]) {
        let header = rows.get(1).is_some_and(|r| markdown::is_rule_row(r));
        let body: Vec<(usize, &Vec<String>)> = rows.iter().enumerate().filter(|(n, _)| !(header && *n == 1)).collect();
        let cols = body.iter().map(|(_, r)| r.len()).max().unwrap_or(0);
        if cols == 0 {
            return;
        }
        let col_w = self.width() / cols as f32;
        for (n, row) in body {
            let style = if header && n == 0 { Style { face: FaceId::Strong, ..Style::body() } } else { Style::body() };
            let cells: Vec<Vec<Line>> = row.iter().map(|c| self.wrap(self.inline(c, style), col_w - 10.0, BODY * LEADING)).collect();
            let h = cells.iter().map(|l| l.iter().map(|x| x.height).sum::<f32>()).fold(BODY * LEADING, f32::max);
            self.need(h + 4.0);
            for (i, lines) in cells.iter().enumerate() {
                let mut y = self.y;
                for l in lines {
                    self.draw_line(l, MARGIN_X + col_w * i as f32, y);
                    y += l.height;
                }
            }
            self.y += h + 2.0;
            let y = self.y;
            self.pdf.line([MARGIN_X, y], [MARGIN_X + self.width(), y], 0.5, LINE);
            self.y += 2.0;
        }
    }

    /// Monospace text on a grey ground, broken at the width of the page.
    fn code_block(&mut self, text: &str, size: f32, color: Rgb) {
        let adv = self.pdf.faces.advance(FaceId::Mono, 'm', size);
        let (left, full) = (MARGIN_X, self.width());
        let per_line = (((full - 16.0) / adv).floor() as usize).max(8);
        let lh = size * 1.45;
        self.need(lh * 2.0 + 8.0);
        let pad = 4.0;
        self.pdf.rect(left, self.y, full, pad, CODE_BG);
        self.y += pad;
        for raw in text.lines().chain(text.is_empty().then_some("")) {
            let chars: Vec<char> = raw.chars().collect();
            let pieces: Vec<String> = if chars.is_empty() { vec![String::new()] } else { chars.chunks(per_line).map(|c| c.iter().collect()).collect() };
            for piece in pieces {
                if self.y + lh > PAGE_H - MARGIN_BOTTOM {
                    self.footer();
                    self.pdf.new_page();
                    self.y = MARGIN_TOP;
                }
                let y = self.y;
                self.pdf.rect(left, y, full, lh, CODE_BG);
                self.pdf.text(FaceId::Mono, &piece, left + 8.0, y + lh * 0.72, size, color, 0.0);
                self.y += lh;
            }
        }
        self.pdf.rect(left, self.y, full, pad, CODE_BG);
        self.y += pad;
    }

    fn math_block(&mut self, l: &Layout, x: f32) {
        self.need(l.height() + 10.0);
        let base = self.y + 5.0 + l.ascent;
        self.draw_math(l, x, base);
        self.y += l.height() + 10.0;
    }

    // ---- code cells ----------------------------------------------------------------------------------------------

    fn code_cell(&mut self, cell: &Cell) {
        self.gap(12.0);
        self.code_block(cell.source.trim_end(), MONO, INK);
        let Some(o) = &cell.output else { return };
        let (x, avail) = (MARGIN_X + OUT_INDENT, self.width() - OUT_INDENT);
        if !o.stdout.trim().is_empty() {
            self.y += 6.0;
            self.plain_lines(o.stdout.trim_end(), x, avail, DIM);
        }
        match &o.result {
            Ok(Some(v)) => self.value(v, x, avail),
            Ok(None) => {}
            Err(e) => {
                self.y += 6.0;
                let atoms = self.inline_plain(&e.to_string(), Style { color: BAD, ..Style::body() });
                self.flow(atoms, x, avail, BODY);
            }
        }
    }

    /// Words with no Markdown in them.
    fn inline_plain(&self, text: &str, style: Style) -> Vec<Atom> {
        let mut out = Vec::new();
        self.words(text, style, None, &mut out);
        out
    }

    /// Printed text, keeping its line breaks and spacing.
    fn plain_lines(&mut self, text: &str, x: f32, avail: f32, color: Rgb) {
        let adv = self.pdf.faces.advance(FaceId::Mono, 'm', MONO);
        let per_line = (((avail) / adv).floor() as usize).max(8);
        let lh = MONO * 1.45;
        for raw in text.lines() {
            let chars: Vec<char> = raw.chars().collect();
            let pieces: Vec<String> = if chars.is_empty() { vec![String::new()] } else { chars.chunks(per_line).map(|c| c.iter().collect()).collect() };
            for piece in pieces {
                self.need(lh);
                let y = self.y;
                self.pdf.text(FaceId::Mono, &piece, x, y + lh * 0.72, MONO, color, 0.0);
                self.y += lh;
            }
        }
    }

    fn value(&mut self, v: &Value, x: f32, avail: f32) {
        self.y += 6.0;
        match v {
            Value::Plot(p) => self.plot(p, x, avail),
            Value::Str(s) => {
                let atoms = self.inline_plain(s, Style::body());
                self.flow(atoms, x, avail, BODY);
            }
            _ => match crate::math_view::to_doc(v).and_then(|d| mathcraft_typeset::layout_wrapped(&d, &Measure(&self.pdf.faces), MATH, avail - 8.0)) {
                Some(l) => {
                    self.math_block(&l, x + 4.0);
                    if let Value::Array(a) = v {
                        self.y -= 4.0;
                        let caption = a.describe();
                        self.plain_lines_small(&caption, x);
                    }
                }
                None => self.plain_lines(&v.to_string(), x, avail, INK),
            },
        }
    }

    fn plain_lines_small(&mut self, text: &str, x: f32) {
        self.need(11.0);
        let y = self.y;
        self.pdf.text(FaceId::Text, text, x + 4.0, y + 8.0, 8.0, DIM, 0.0);
        self.y += 12.0;
    }

    fn plot(&mut self, p: &mathcraft_eval::PlotSpec, x: f32, avail: f32) {
        let series: Vec<crate::plot_png::Rgb> = Theme::light().plot.iter().map(|c| [c.r(), c.g(), c.b()]).collect();
        let colors = crate::plot_png::Colors { series: &series, text: [0x33, 0x33, 0x33], grid: [0xe4, 0xe4, 0xe8] };
        let view = crate::plot_export::data_view(p);
        let picture = crate::plot_png::png(p, view, &colors, crate::plot_export::Look::default(), &crate::plot_view::tick_label, crate::fonts::INTER, 3.0)
            .ok()
            .and_then(|bytes| decode_png(&bytes));
        match picture {
            Some((w, h, rgb)) => {
                let width = avail.min(430.0);
                let height = width * h as f32 / w as f32;
                self.need(height + 6.0);
                let n = self.pdf.add_image(w, h, &rgb);
                let y = self.y;
                self.pdf.image(n, x, y, width, height);
                self.y += height + 6.0;
            }
            None => self.plain_lines("(plot)", x, avail, DIM),
        }
    }
}

/// RGB pixels of a PNG file: width, height, `width * height * 3` bytes.
fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let data = buf.get(..info.buffer_size())?;
    let rgb = match info.color_type {
        png::ColorType::Rgb => data.to_vec(),
        png::ColorType::Rgba => data.as_chunks::<4>().0.iter().flat_map(|p| p.iter().take(3).copied()).collect(),
        _ => return None,
    };
    Some((info.width, info.height, rgb))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathcraft_notebook::{RunRequest, Runner};

    #[test]
    fn markdown_is_cut_into_blocks() {
        let b = blocks(
            "# Title\n\nSome **bold** text\nover two lines.\n\n- one\n  goes on\n- two\n\n3. third\n\n> quoted\n\n---\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n$$x**2$$\n\n```\ncode\n```\n",
        );
        assert_eq!(b.first(), Some(&Block::Heading(1, "Title".into())));
        assert!(b.contains(&Block::Para("Some **bold** text over two lines.".into())));
        assert!(b.contains(&Block::Item("•".into(), "one goes on".into())) && b.contains(&Block::Item("3.".into(), "third".into())));
        assert!(b.contains(&Block::Quote("quoted".into())) && b.contains(&Block::Rule));
        assert!(b.iter().any(|x| matches!(x, Block::Table(rows) if rows.len() == 3)));
        assert!(b.contains(&Block::Display("x**2".into())) && b.contains(&Block::Code(vec!["code".into()])));
    }

    fn run(text: &str) -> Notebook {
        let mut nb = Notebook::from_text(text);
        nb.run_blocking(&mut Runner::new(Default::default()), RunRequest::All);
        nb
    }

    #[test]
    fn a_notebook_becomes_a_pdf_with_a_picture_for_the_plot_and_pages_as_needed() {
        let nb = run(
            "# %% [markdown]\n# # Heading\n# Text with $x**2/2$ and [a link](https://example.org).\n\n# %%\nf = diff(x**3, x)\nf\n\n# %%\nplot(sin(x), (x, 0, 3))\n\n# %%\nprint('hi')\n1/0\n",
        );
        let pdf = notebook_pdf(&nb, "demo").unwrap_or_default();
        assert!(!pdf.is_empty(), "the export failed");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.5") && text.contains("/Subtype /Image") && text.contains("/Subtype /Link"));
        assert!(text.contains("/Count 1"), "{}", text.lines().find(|l| l.contains("/Count")).unwrap_or(""));
        let long: String = (0..300).map(|i| format!("# %%\nv{i} = {i}\n\n")).collect();
        let big = notebook_pdf(&run(&long), "long").unwrap_or_default();
        assert!(
            String::from_utf8_lossy(&big).contains("/Type /Pages") && !String::from_utf8_lossy(&big).contains("/Count 1 "),
            "many cells need several pages"
        );
    }
}

#[cfg(test)]
mod sample {
    use super::*;
    use mathcraft_notebook::{RunRequest, Runner};

    /// Writes the tutorial notebook as `$MATHCRAFT_SHOTS/tutorial.pdf` to look at by eye.
    #[test]
    #[ignore = "writes a file; needs MATHCRAFT_SHOTS=<dir>"]
    fn tutorial_pdf() {
        let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS") else { return };
        let mut nb = Notebook::from_text(crate::WELCOME);
        nb.run_blocking(&mut Runner::new(Default::default()), RunRequest::All);
        let bytes = notebook_pdf(&nb, "tutorial").unwrap_or_default();
        let _ = std::fs::write(std::path::Path::new(&dir).join("tutorial.pdf"), bytes);
    }
}
