//! A PNG picture of a plot, drawn by a small software rasteriser: anti-aliased lines, and text from the interface
//! font's glyph outlines (read with `skrifa`, filled here by sub-scanline coverage). No rendering library; the
//! picture mirrors the SVG export (`plot_export::svg`) and is written with the `png` encoder.

use crate::plot_export::{View, ticks};
use mathcraft_eval::PlotSpec;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

const W: f32 = 720.0;
const H: f32 = 420.0;
const LEFT: f32 = 58.0;
const RIGHT: f32 = 18.0;
const TOP: f32 = 18.0;
const BOTTOM: f32 = 44.0;
/// Horizontal text anchor.
#[derive(Clone, Copy)]
enum Anchor {
    Start,
    Middle,
    End,
}

pub type Rgb = [u8; 3];

/// Colours of the picture (a white page, so it pastes into documents).
pub struct Colors<'a> {
    pub series: &'a [Rgb],
    pub text: Rgb,
    pub grid: Rgb,
}

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, px: vec![255; w * h * 3] }
    }

    /// Mix `color` into the pixel at `alpha`.
    fn blend(&mut self, x: i64, y: i64, color: Rgb, alpha: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h || alpha <= 0.0 {
            return;
        }
        let a = alpha.min(1.0);
        let i = (y as usize * self.w + x as usize) * 3;
        for (k, c) in color.iter().enumerate() {
            if let Some(p) = self.px.get_mut(i + k) {
                *p = (f32::from(*p) * (1.0 - a) + f32::from(*c) * a).round() as u8;
            }
        }
    }

    /// A line segment `width` thick, clipped to `clip = [x0, y0, x1, y1]`, with round, anti-aliased edges.
    fn line(&mut self, a: [f32; 2], b: [f32; 2], width: f32, color: Rgb, clip: [f32; 4]) {
        let half = width / 2.0;
        let (lo_x, hi_x) = (a[0].min(b[0]) - half - 1.0, a[0].max(b[0]) + half + 1.0);
        let (lo_y, hi_y) = (a[1].min(b[1]) - half - 1.0, a[1].max(b[1]) + half + 1.0);
        let (x0, x1) = (lo_x.max(clip[0]).floor() as i64, hi_x.min(clip[2]).ceil() as i64);
        let (y0, y1) = (lo_y.max(clip[1]).floor() as i64, hi_y.min(clip[3]).ceil() as i64);
        if !(a.iter().chain(b.iter()).all(|v| v.is_finite())) || x1 - x0 > 20_000 || y1 - y0 > 20_000 {
            return;
        }
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = dx * dx + dy * dy;
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = if len2 > 0.0 { (((px - a[0]) * dx + (py - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let (cx, cy) = (a[0] + t * dx, a[1] + t * dy);
                let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
                self.blend(x, y, color, half + 0.5 - d);
            }
        }
    }

    fn rect_outline(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Rgb) {
        let all = [0.0, 0.0, self.w as f32, self.h as f32];
        for (p, q) in [([x0, y0], [x1, y0]), ([x1, y0], [x1, y1]), ([x1, y1], [x0, y1]), ([x0, y1], [x0, y0])] {
            self.line(p, q, width, color, all);
        }
    }
}

/// The outline of one glyph as closed polylines, in pixels with `y` up.
#[derive(Default)]
struct Pen {
    contours: Vec<Vec<[f32; 2]>>,
}

impl Pen {
    fn last(&self) -> [f32; 2] {
        self.contours.last().and_then(|c| c.last()).copied().unwrap_or([0.0, 0.0])
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.contours.push(vec![[x, y]]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        if let Some(c) = self.contours.last_mut() {
            c.push([x, y]);
        }
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let p0 = self.last();
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let u = 1.0 - t;
            self.line_to(u * u * p0[0] + 2.0 * u * t * cx + t * t * x, u * u * p0[1] + 2.0 * u * t * cy + t * t * y);
        }
    }
    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let p0 = self.last();
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.line_to(a * p0[0] + b * c1x + c * c2x + d * x, a * p0[1] + b * c1y + c * c2y + d * y);
        }
    }
    fn close(&mut self) {}
}

/// Text in the interface font.
struct Face<'a> {
    font: FontRef<'a>,
}

impl<'a> Face<'a> {
    fn new(data: &'a [u8]) -> Option<Face<'a>> {
        FontRef::new(data).ok().map(|font| Face { font })
    }

    fn advance(&self, c: char, px: f32) -> f32 {
        let Some(gid) = self.font.charmap().map(c) else { return px * 0.5 };
        self.font.glyph_metrics(Size::new(px), LocationRef::default()).advance_width(gid).unwrap_or(px * 0.5)
    }

    fn width(&self, s: &str, px: f32) -> f32 {
        s.chars().map(|c| self.advance(c, px)).sum()
    }

    /// Draw `s` with its baseline at `y`.
    fn text(&self, cv: &mut Canvas, s: &str, at: [f32; 2], px: f32, anchor: Anchor, color: Rgb) {
        let [x, y] = at;
        let total = self.width(s, px);
        let mut pen_x = match anchor {
            Anchor::Start => x,
            Anchor::Middle => x - total / 2.0,
            Anchor::End => x - total,
        };
        let outlines = self.font.outline_glyphs();
        for c in s.chars() {
            if let Some(gid) = self.font.charmap().map(c)
                && let Some(glyph) = outlines.get(gid)
            {
                let mut pen = Pen::default();
                if glyph.draw(DrawSettings::unhinted(Size::new(px), LocationRef::default()), &mut pen).is_ok() {
                    fill(cv, &pen.contours, pen_x, y, color);
                }
            }
            pen_x += self.advance(c, px);
        }
    }
}

/// Fill closed polylines (`y` up, origin at `(ox, oy)` in the picture) with the non-zero rule: four sub-scanlines per
/// pixel row, the covered part of each pixel is the exact length of the span in it.
fn fill(cv: &mut Canvas, contours: &[Vec<[f32; 2]>], ox: f32, oy: f32, color: Rgb) {
    const SUB: usize = 4;
    let pts = |c: &Vec<[f32; 2]>| -> Vec<[f32; 2]> { c.iter().map(|p| [ox + p[0], oy - p[1]]).collect() };
    let polys: Vec<Vec<[f32; 2]>> = contours.iter().map(pts).collect();
    let (mut y_min, mut y_max) = (f32::INFINITY, f32::NEG_INFINITY);
    for p in polys.iter().flatten() {
        y_min = y_min.min(p[1]);
        y_max = y_max.max(p[1]);
    }
    if !y_min.is_finite() {
        return;
    }
    let width = cv.w;
    for row in (y_min.floor().max(0.0) as i64)..(y_max.ceil().min(cv.h as f32) as i64) {
        let mut cover = vec![0.0f32; width + 1];
        for s in 0..SUB {
            let sy = row as f32 + (s as f32 + 0.5) / SUB as f32;
            let mut crossings: Vec<(f32, i32)> = Vec::new();
            for poly in &polys {
                for (i, a) in poly.iter().enumerate() {
                    let b = poly.get((i + 1) % poly.len()).copied().unwrap_or(*a);
                    if (a[1] <= sy) != (b[1] <= sy) {
                        let t = (sy - a[1]) / (b[1] - a[1]);
                        crossings.push((a[0] + t * (b[0] - a[0]), if b[1] > a[1] { 1 } else { -1 }));
                    }
                }
            }
            crossings.sort_by(|p, q| p.0.total_cmp(&q.0));
            let mut winding = 0;
            for (i, (x, dir)) in crossings.iter().enumerate() {
                let before = winding;
                winding += dir;
                if before == 0 && winding != 0 {
                    // the span runs until the winding returns to zero
                    let mut end = *x;
                    let mut w2 = winding;
                    for (x2, d2) in crossings.iter().skip(i + 1) {
                        w2 += d2;
                        end = *x2;
                        if w2 == 0 {
                            break;
                        }
                    }
                    add_span(&mut cover, *x, end);
                }
            }
        }
        for (x, c) in cover.iter().enumerate().take(width) {
            cv.blend(x as i64, row, color, c / SUB as f32);
        }
    }
}

/// Add the coverage of the span `[a, b)` to `cover`, per pixel.
fn add_span(cover: &mut [f32], a: f32, b: f32) {
    let (a, b) = (a.max(0.0), b.min(cover.len() as f32));
    if b <= a {
        return;
    }
    let (first, last) = (a.floor() as usize, (b.ceil() as usize).saturating_sub(1));
    for px in first..=last {
        let lo = a.max(px as f32);
        let hi = b.min(px as f32 + 1.0);
        if let Some(c) = cover.get_mut(px) {
            *c += (hi - lo).max(0.0);
        }
    }
}

/// The picture as PNG bytes, `scale` times the SVG's 720 by 420 points (2 gives a sharp 1440 by 840).
pub fn png(p: &PlotSpec, view: View, colors: &Colors, tick_label: &dyn Fn(f64) -> String, font: &[u8], scale: f32) -> Result<Vec<u8>, String> {
    let face = Face::new(font).ok_or("the interface font could not be read")?;
    let (wpx, hpx) = ((W * scale) as usize, (H * scale) as usize);
    let mut cv = Canvas::new(wpx, hpx);
    let s = scale;
    let [x0, x1, y0, y1] = view;
    let (pw, ph) = (W - LEFT - RIGHT, H - TOP - BOTTOM);
    let sx = |x: f64| (LEFT + ((x - x0) / (x1 - x0)) as f32 * pw) * s;
    let sy = |y: f64| (TOP + (1.0 - ((y - y0) / (y1 - y0)) as f32) * ph) * s;
    let all = [0.0, 0.0, wpx as f32, hpx as f32];
    let frame = [LEFT * s, TOP * s, (LEFT + pw) * s, (TOP + ph) * s];
    let font_px = 11.0 * s;
    for t in ticks(x0, x1) {
        let x = sx(t);
        cv.line([x, frame[1]], [x, frame[3]], s, colors.grid, all);
        face.text(&mut cv, &tick_label(t), [x, (TOP + ph + 16.0) * s], font_px, Anchor::Middle, colors.text);
    }
    for t in ticks(y0, y1) {
        let y = sy(t);
        cv.line([frame[0], y], [frame[2], y], s, colors.grid, all);
        face.text(&mut cv, &tick_label(t), [(LEFT - 6.0) * s, y + 4.0 * s], font_px, Anchor::End, colors.text);
    }
    cv.rect_outline(frame[0], frame[1], frame[2], frame[3], s, colors.text);
    if !p.x_label.is_empty() {
        face.text(&mut cv, &p.x_label, [(LEFT + pw / 2.0) * s, (H - 8.0) * s], font_px, Anchor::Middle, colors.text);
    }
    let color_of = |i: usize| colors.series.get(i % colors.series.len().max(1)).copied().unwrap_or([31, 111, 235]);
    for (i, series) in p.series.iter().enumerate() {
        for seg in &series.segments {
            for pair in seg.windows(2) {
                if let [a, b] = pair {
                    cv.line([sx(a[0]), sy(a[1])], [sx(b[0]), sy(b[1])], 1.75 * s, color_of(i), frame);
                }
            }
        }
    }
    if p.series.len() > 1 {
        for (i, series) in p.series.iter().enumerate() {
            let y = (TOP + 14.0 + i as f32 * 16.0) * s;
            let x = (LEFT + pw - 120.0) * s;
            cv.line([x, y - 4.0 * s], [x + 18.0 * s, y - 4.0 * s], 2.0 * s, color_of(i), all);
            face.text(&mut cv, &series.label, [x + 24.0 * s, y], font_px, Anchor::Start, colors.text);
        }
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, wpx as u32, hpx as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&cv.px).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathcraft_eval::PlotSeries;

    fn spec() -> PlotSpec {
        PlotSpec {
            x_label: "x".into(),
            series: vec![
                PlotSeries { label: "sin".into(), segments: vec![vec![[0.0, 0.0], [1.0, 1.0], [2.0, 0.0]]] },
                PlotSeries { label: "flat".into(), segments: vec![vec![[0.0, 0.5], [2.0, 0.5]]] },
            ],
        }
    }

    fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap_or_else(|e| panic!("{e}"));
        let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
        let info = reader.next_frame(&mut buf).unwrap_or_else(|e| panic!("{e}"));
        (info.width, info.height, buf)
    }

    #[test]
    fn draws_a_valid_png_with_lines_and_text() {
        let colors = Colors { series: &[[200, 0, 0], [0, 0, 200]], text: [0, 0, 0], grid: [220, 220, 220] };
        let bytes = png(&spec(), [0.0, 2.0, -0.1, 1.1], &colors, &|v| format!("{v}"), crate::fonts::INTER, 1.0).unwrap_or_else(|e| panic!("{e}"));
        let (w, h, px) = decode(&bytes);
        assert_eq!((w, h), (720, 420));
        let at = |x: usize, y: usize| px.get((y * 720 + x) * 3..(y * 720 + x) * 3 + 3).map(<[u8]>::to_vec).unwrap_or_default();
        // the page is white, the red curve passes the peak, the blue line is at y = 0.5
        assert_eq!(at(5, 5), vec![255, 255, 255]);
        let peak_x = (58.0 + (1.0 / 2.0) * (720.0 - 58.0 - 18.0)) as usize;
        let peak_y = (18.0 + (1.0 - (1.0 - -0.1) / 1.2) * (420.0 - 18.0 - 44.0)) as usize;
        assert!(at(peak_x, peak_y + 1)[0] > 150 && at(peak_x, peak_y + 1)[1] < 120, "{:?}", at(peak_x, peak_y + 1));
        // some dark text pixels exist below the frame (tick labels)
        let dark_below = (0..720).flat_map(|x| (380..400).map(move |y| (x, y))).filter(|(x, y)| at(*x, *y).iter().all(|c| *c < 100)).count();
        assert!(dark_below > 20, "{dark_below}");
    }

    #[test]
    fn scale_makes_a_bigger_picture() {
        let colors = Colors { series: &[[0, 0, 0]], text: [0, 0, 0], grid: [200, 200, 200] };
        let bytes = png(&spec(), [0.0, 2.0, 0.0, 1.0], &colors, &|v| format!("{v}"), crate::fonts::INTER, 2.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((decode(&bytes).0, decode(&bytes).1), (1440, 840));
    }

    #[test]
    #[ignore = "writes a picture to look at; needs MATHCRAFT_SHOTS=<dir>"]
    fn plot_png_screenshot() {
        let Some(dir) = std::env::var_os("MATHCRAFT_SHOTS") else { return };
        let curve = |f: fn(f64) -> f64| -> Vec<[f64; 2]> { (0..=200).map(|i| i as f64 * 0.05).map(|x| [x, f(x)]).collect() };
        let p = PlotSpec {
            x_label: "x".into(),
            series: vec![
                PlotSeries { label: "sin(x)".into(), segments: vec![curve(f64::sin)] },
                PlotSeries { label: "cos(x)/2".into(), segments: vec![curve(|x| x.cos() / 2.0)] },
            ],
        };
        let colors = Colors { series: &[[31, 111, 235], [230, 120, 20]], text: [0x33, 0x33, 0x33], grid: [0xe4, 0xe4, 0xe8] };
        let bytes = png(&p, [0.0, 10.0, -1.2, 1.2], &colors, &|v| format!("{v}"), crate::fonts::INTER, 2.0).unwrap_or_default();
        let _ = std::fs::write(std::path::Path::new(&dir).join("plot.png"), bytes);
    }
}
