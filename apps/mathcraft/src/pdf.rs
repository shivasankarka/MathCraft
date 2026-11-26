//! A small PDF writer for notebook export. Text is drawn as the filled outlines of the interface, code and math fonts'
//! glyphs (read with `skrifa`), each defined once as a form and reused, so no font is embedded and every character the
//! fonts have prints exactly as on screen; the cost is that the text of the file cannot be selected or searched.
//! Pictures are RGB images, links are annotations. Everything is written in one go by [`Pdf::finish`].

use flate2::Compression;
use flate2::write::ZlibEncoder;
use skrifa::instance::{Location, LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;

/// A4, in points.
pub const PAGE_W: f32 = 595.28;
pub const PAGE_H: f32 = 841.89;

pub type Rgb = [u8; 3];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FaceId {
    Text,
    Strong,
    Mono,
    Math,
}

const ALL: [FaceId; 4] = [FaceId::Text, FaceId::Strong, FaceId::Mono, FaceId::Math];

struct Face {
    font: FontRef<'static>,
    location: Location,
    upem: f32,
}

/// The fonts, and which of them has a character.
pub struct Faces {
    faces: Vec<(FaceId, Face)>,
}

impl Faces {
    pub fn new() -> Option<Faces> {
        let load = |id: FaceId, data: &'static [u8], axes: &[(&str, f32)]| -> Option<(FaceId, Face)> {
            let font = FontRef::new(data).ok()?;
            let location = font.axes().location(axes.iter().copied());
            let upem = f32::from(font.metrics(Size::unscaled(), LocationRef::default()).units_per_em);
            Some((id, Face { font, location, upem }))
        };
        Some(Faces {
            faces: vec![
                load(FaceId::Text, crate::fonts::INTER, &[("wght", 400.0)])?,
                load(FaceId::Strong, crate::fonts::INTER, &[("wght", 600.0)])?,
                load(FaceId::Mono, crate::fonts::JETBRAINS, &[])?,
                load(FaceId::Math, crate::fonts::STIX_MATH, &[])?,
            ],
        })
    }

    fn face(&self, id: FaceId) -> Option<&Face> {
        self.faces.iter().find(|(i, _)| *i == id).map(|(_, f)| f)
    }

    /// The face to draw `c` in: the wanted one if it has the character, else the first that does.
    pub fn pick(&self, want: FaceId, c: char) -> Option<(FaceId, GlyphId)> {
        std::iter::once(want).chain(ALL).find_map(|id| {
            let gid = self.face(id)?.font.charmap().map(c).filter(|g| g.to_u32() != 0)?;
            Some((id, gid))
        })
    }

    pub fn advance(&self, want: FaceId, c: char, size: f32) -> f32 {
        let Some((id, gid)) = self.pick(want, c) else { return size * 0.5 };
        let Some(f) = self.face(id) else { return size * 0.5 };
        f.font.glyph_metrics(Size::unscaled(), LocationRef::from(&f.location)).advance_width(gid).map_or(size * 0.5, |a| a / f.upem * size)
    }

    pub fn width(&self, want: FaceId, s: &str, size: f32) -> f32 {
        s.chars().map(|c| self.advance(want, c, size)).sum()
    }
}

/// A glyph outline as PDF path operators in a 1000-unit em.
struct Outline {
    ops: String,
    at: [f32; 2],
    k: f32,
}

impl Outline {
    fn p(&self, v: f32) -> String {
        num(v * self.k)
    }
}

impl OutlinePen for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = writeln!(self.ops, "{} {} m", self.p(x), self.p(y));
        self.at = [x, y];
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let _ = writeln!(self.ops, "{} {} l", self.p(x), self.p(y));
        self.at = [x, y];
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        // a quadratic curve as the equal cubic one
        let [x0, y0] = self.at;
        let (c1x, c1y) = (x0 + 2.0 / 3.0 * (cx - x0), y0 + 2.0 / 3.0 * (cy - y0));
        let (c2x, c2y) = (x + 2.0 / 3.0 * (cx - x), y + 2.0 / 3.0 * (cy - y));
        self.curve_to(c1x, c1y, c2x, c2y, x, y);
    }
    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let _ = writeln!(self.ops, "{} {} {} {} {} {} c", self.p(c1x), self.p(c1y), self.p(c2x), self.p(c2y), self.p(x), self.p(y));
        self.at = [x, y];
    }
    fn close(&mut self) {
        self.ops.push_str("h\n");
    }
}

/// A small factor (a glyph scale) with enough digits that sizes like 15.5 and 20 stay different.
fn fine(v: f32) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".to_owned() } else { s.to_owned() }
}

/// A number as short PDF text.
pub fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".to_owned() } else { s.to_owned() }
}

fn color_ops(c: Rgb, stroke: bool) -> String {
    let f = |v: u8| num(f32::from(v) / 255.0);
    format!("{} {} {} {}\n", f(c[0]), f(c[1]), f(c[2]), if stroke { "RG" } else { "rg" })
}

#[derive(Default)]
struct Page {
    content: String,
    links: Vec<([f32; 4], String)>,
}

struct Image {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

pub struct Pdf {
    pub faces: Faces,
    glyphs: HashMap<(FaceId, u32), usize>,
    forms: Vec<String>,
    images: Vec<Image>,
    /// The pages finished, and the one being drawn.
    done: Vec<Page>,
    page: Page,
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

/// A PDF text string: UTF-16 with a byte order mark, in hex.
fn text_string(s: &str) -> String {
    let mut out = String::from("<FEFF");
    for u in s.encode_utf16() {
        let _ = write!(out, "{u:04X}");
    }
    out.push('>');
    out
}

fn escape(s: &str) -> String {
    s.chars().filter(char::is_ascii).flat_map(|c| if matches!(c, '(' | ')' | '\\') { vec!['\\', c] } else { vec![c] }).collect()
}

impl Pdf {
    pub fn new() -> Option<Pdf> {
        Some(Pdf { faces: Faces::new()?, glyphs: HashMap::new(), forms: Vec::new(), images: Vec::new(), done: Vec::new(), page: Page::default() })
    }

    /// Finish the page being drawn and start another.
    pub fn new_page(&mut self) {
        let finished = std::mem::take(&mut self.page);
        self.done.push(finished);
    }

    /// The number of the page being drawn (1-based).
    pub fn page_number(&self) -> usize {
        self.done.len() + 1
    }

    fn glyph_name(&mut self, face: FaceId, gid: GlyphId) -> Option<String> {
        let key = (face, gid.to_u32());
        if let Some(i) = self.glyphs.get(&key) {
            return Some(format!("G{i}"));
        }
        let f = self.faces.face(face)?;
        let mut pen = Outline { ops: String::new(), at: [0.0, 0.0], k: 1000.0 / f.upem };
        let glyph = f.font.outline_glyphs().get(gid)?;
        glyph.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::from(&f.location)), &mut pen).ok()?;
        pen.ops.push_str("f\n");
        let i = self.forms.len();
        self.forms.push(pen.ops);
        self.glyphs.insert(key, i);
        Some(format!("G{i}"))
    }

    /// Draw `s` with its baseline-left corner at `(x, y)` (y measured from the top of the page); returns its width.
    /// `slant` shears the glyphs for italics.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, want: FaceId, s: &str, x: f32, y: f32, size: f32, color: Rgb, slant: f32) -> f32 {
        let mut pen_x = x;
        let mut ops = color_ops(color, false);
        for c in s.chars() {
            let adv = self.faces.advance(want, c, size);
            if !c.is_whitespace()
                && let Some((face, gid)) = self.faces.pick(want, c)
                && let Some(name) = self.glyph_name(face, gid)
            {
                let k = size / 1000.0;
                let _ = writeln!(ops, "q {} 0 {} {} {} {} cm /{name} Do Q", fine(k), fine(k * slant), fine(k), num(pen_x), num(PAGE_H - y));
            }
            pen_x += adv;
        }
        self.page.content.push_str(&ops);
        pen_x - x
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb) {
        let ops = format!("{}{} {} {} {} re f\n", color_ops(color, false), num(x), num(PAGE_H - y - h), num(w), num(h));
        self.page.content.push_str(&ops);
    }

    pub fn polyline(&mut self, points: &[[f32; 2]], width: f32, color: Rgb) {
        let mut ops = format!("{}{} w 1 J 1 j\n", color_ops(color, true), num(width));
        for (i, [x, y]) in points.iter().enumerate() {
            let _ = writeln!(ops, "{} {} {}", num(*x), num(PAGE_H - y), if i == 0 { "m" } else { "l" });
        }
        ops.push_str("S\n");
        self.page.content.push_str(&ops);
    }

    pub fn line(&mut self, from: [f32; 2], to: [f32; 2], width: f32, color: Rgb) {
        self.polyline(&[from, to], width, color);
    }

    /// Add a picture (`rgb` is `width * height * 3` bytes); returns its number for [`Pdf::image`].
    pub fn add_image(&mut self, width: u32, height: u32, rgb: &[u8]) -> usize {
        self.images.push(Image { width, height, data: deflate(rgb) });
        self.images.len() - 1
    }

    pub fn image(&mut self, n: usize, x: f32, y: f32, w: f32, h: f32) {
        let ops = format!("q {} 0 0 {} {} {} cm /I{n} Do Q\n", num(w), num(h), num(x), num(PAGE_H - y - h));
        self.page.content.push_str(&ops);
    }

    /// Make the box (top-left corner, size) a link to `url`.
    pub fn link(&mut self, x: f32, y: f32, w: f32, h: f32, url: &str) {
        self.page.links.push(([x, PAGE_H - y - h, x + w, PAGE_H - y], url.to_owned()));
    }

    pub fn finish(mut self, title: &str) -> Vec<u8> {
        let last = std::mem::take(&mut self.page);
        self.done.push(last);
        let (forms, images, pages) = (self.forms.len(), self.images.len(), self.done.len());
        let first_page = 5 + forms + images;
        let first_link = first_page + 2 * pages;
        let mut objects: Vec<Vec<u8>> = Vec::new();
        let obj = |body: String| body.into_bytes();
        let stream = |dict: &str, data: &[u8]| -> Vec<u8> {
            let mut o = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
            o.extend_from_slice(data);
            o.extend_from_slice(b"\nendstream");
            o
        };
        objects.push(obj("<< /Type /Catalog /Pages 2 0 R >>".into()));
        let kids: String = (0..pages).map(|k| format!("{} 0 R", first_page + 2 * k + 1)).collect::<Vec<_>>().join(" ");
        objects.push(obj(format!("<< /Type /Pages /Kids [{kids}] /Count {pages} >>")));
        objects.push(obj(format!("<< /Title {} /Producer (MathCraft) >>", text_string(title))));
        let xobjects: String =
            (0..forms).map(|i| format!("/G{i} {} 0 R", 5 + i)).chain((0..images).map(|i| format!("/I{i} {} 0 R", 5 + forms + i))).collect::<Vec<_>>().join(" ");
        objects.push(obj(format!("<< /XObject << {xobjects} >> >>")));
        for f in &self.forms {
            objects.push(stream("/Type /XObject /Subtype /Form /BBox [-2000 -2000 4000 4000]", f.as_bytes()));
        }
        for im in &self.images {
            let dict = format!(
                "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode",
                im.width, im.height
            );
            objects.push(stream(&dict, &im.data));
        }
        let mut link_id = first_link;
        for (k, page) in self.done.iter().enumerate() {
            objects.push(stream("/Filter /FlateDecode", &deflate(page.content.as_bytes())));
            let annots: String = (0..page.links.len()).map(|i| format!("{} 0 R", link_id + i)).collect::<Vec<_>>().join(" ");
            link_id += page.links.len();
            objects.push(obj(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources 4 0 R /Contents {} 0 R /Annots [{annots}] >>",
                num(PAGE_W),
                num(PAGE_H),
                first_page + 2 * k
            )));
        }
        for page in &self.done {
            for ([x0, y0, x1, y1], url) in &page.links {
                objects.push(obj(format!(
                    "<< /Type /Annot /Subtype /Link /Rect [{} {} {} {}] /Border [0 0 0] /A << /S /URI /URI ({}) >> >>",
                    num(*x0),
                    num(*y0),
                    num(*x1),
                    num(*y1),
                    escape(url)
                )));
            }
        }
        let mut out = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for off in offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_scales_keep_their_precision() {
        assert_eq!((fine(0.0155), fine(0.02), fine(0.0)), ("0.0155".into(), "0.02".into(), "0".into()));
    }

    #[test]
    fn numbers_are_short() {
        assert_eq!((num(1.0), num(0.5), num(-0.001), num(12.345)), ("1".into(), "0.5".into(), "0".into(), "12.35".into()));
    }

    #[test]
    fn a_document_has_pages_glyphs_links_and_a_valid_cross_reference_table() {
        let Some(mut pdf) = Pdf::new() else { return };
        let w = pdf.text(FaceId::Text, "Hello ∑", 50.0, 100.0, 12.0, [0, 0, 0], 0.0);
        assert!(w > 20.0);
        pdf.rect(50.0, 120.0, 100.0, 10.0, [200, 200, 200]);
        pdf.link(50.0, 90.0, w, 12.0, "https://example.org/a(b)");
        let img = pdf.add_image(1, 1, &[255, 0, 0]);
        pdf.image(img, 50.0, 200.0, 20.0, 20.0);
        pdf.new_page();
        pdf.text(FaceId::Mono, "x = 1", 50.0, 100.0, 9.0, [0, 0, 0], 0.2);
        let bytes = pdf.finish("Título");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("%PDF-1.5") && text.trim_end().ends_with("%%EOF"));
        assert!(text.contains("/Count 2") && text.contains("/Subtype /Link") && text.contains("https://example.org/a\\(b\\)"));
        // every offset in the cross-reference table points at its object
        let tail = String::from_utf8_lossy(bytes.get(bytes.len().saturating_sub(120)..).unwrap_or(&[])).into_owned();
        let at = tail.rfind("startxref\n").map_or(0, |i| i + 10);
        let xref: usize = tail.get(at..).and_then(|t| t.lines().next()).and_then(|l| l.parse().ok()).unwrap_or(0);
        let rest = String::from_utf8_lossy(bytes.get(xref..).unwrap_or(&[])).into_owned();
        let table: Vec<&str> = rest.lines().skip(3).take_while(|l| l.ends_with(" n ")).collect();
        assert!(table.len() > 8);
        for (i, line) in table.iter().enumerate() {
            let off: usize = line.get(..10).and_then(|o| o.parse().ok()).unwrap_or(usize::MAX);
            assert!(bytes.get(off..).is_some_and(|b| b.starts_with(format!("{} 0 obj", i + 1).as_bytes())), "object {}", i + 1);
        }
    }
}
