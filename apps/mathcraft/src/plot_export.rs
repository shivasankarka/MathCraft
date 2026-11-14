//! Plots out of the app: an SVG picture of what is on screen, and the sampled points as CSV. Both are written by
//! hand (no rendering dependency) from the same `PlotSpec` the window draws.

use mathcraft_eval::{PlotSpec, SeriesKind};
use std::fmt::Write;

const W: f64 = 720.0;
const H: f64 = 420.0;
const LEFT: f64 = 58.0;
const RIGHT: f64 = 18.0;
const TOP: f64 = 18.0;
const BOTTOM: f64 = 44.0;

/// Colours for an exported picture (a white page, so it pastes into documents).
pub struct SvgColors<'a> {
    pub series: &'a [String],
    pub text: &'a str,
    pub grid: &'a str,
}

impl SvgColors<'_> {
    /// Grid lines are pale; the short tick marks drawn without a grid use the text colour so they can be seen.
    fn grid_or_text(&self, look: Look) -> &str {
        if look.grid { self.grid } else { self.text }
    }
}

/// The view an SVG shows: `[x_min, x_max, y_min, y_max]`.
pub type View = [f64; 4];

/// The box around all points, with a little room; the view when nothing was zoomed.
pub fn data_view(p: &PlotSpec) -> View {
    let mut v = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
    for s in &p.series {
        // a bar stands on zero and is `width` wide
        let (half, base) = match s.kind {
            SeriesKind::Bars { width } => (width / 2.0, Some(0.0)),
            _ => (0.0, None),
        };
        for [x, y] in s.segments.iter().flatten() {
            v = [v[0].min(x - half), v[1].max(x + half), v[2].min(*y).min(base.unwrap_or(*y)), v[3].max(*y).max(base.unwrap_or(*y))];
        }
    }
    if !v.iter().all(|c| c.is_finite()) {
        return [-1.0, 1.0, -1.0, 1.0];
    }
    let pad = |lo: f64, hi: f64| if hi > lo { (lo, hi) } else { (lo - 1.0, hi + 1.0) };
    let (x0, x1) = pad(v[0], v[1]);
    let (y0, y1) = pad(v[2], v[3]);
    let m = (y1 - y0) * 0.05;
    [x0, x1, y0 - m, y1 + m]
}

/// What a saved picture shows, the same choices as the plot's right-click menu.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// Grid lines across the plot; without them only short tick marks are drawn.
    pub grid: bool,
    /// The numbers beside the axes.
    pub axes: bool,
    /// How many marks to draw compared with the usual: 1 is the usual, 0.5 half as many, 2 twice as many.
    pub density: f64,
}

impl Default for Look {
    fn default() -> Self {
        Look { grid: false, axes: true, density: 1.0 }
    }
}

/// Tick positions inside `lo..=hi`, 1, 2 or 5 times a power of ten, about six of them (more when `density` is above 1).
pub fn ticks(lo: f64, hi: f64, density: f64) -> Vec<f64> {
    let want = (hi - lo) / (6.0 * density);
    if !(want.is_finite() && want > 0.0) {
        return Vec::new();
    }
    let power = 10f64.powf(want.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * power).find(|s| *s >= want).unwrap_or(power * 10.0);
    let first = (lo / step).ceil() as i64;
    let last = (hi / step).floor() as i64;
    (first..=last).take(64).map(|k| k as f64 * step).collect()
}

/// The marks under the x axis: the plot's own names, else round numbers.
pub fn x_marks(p: &PlotSpec, lo: f64, hi: f64, density: f64, tick_label: &dyn Fn(f64) -> String) -> Vec<(f64, String)> {
    if p.x_ticks.is_empty() {
        return ticks(lo, hi, density).into_iter().map(|t| (t, tick_label(t))).collect();
    }
    p.x_ticks.iter().filter(|(x, _)| *x >= lo && *x <= hi).cloned().collect()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// An SVG picture of the plot over `view`.
pub fn svg(p: &PlotSpec, view: View, colors: &SvgColors, look: Look, tick_label: &dyn Fn(f64) -> String) -> String {
    let [x0, x1, y0, y1] = view;
    let (pw, ph) = (W - LEFT - RIGHT, H - TOP - BOTTOM);
    let sx = |x: f64| LEFT + (x - x0) / (x1 - x0) * pw;
    let sy = |y: f64| TOP + (1.0 - (y - y0) / (y1 - y0)) * ph;
    let mut o = String::new();
    let _ = writeln!(o, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" font-family="sans-serif" font-size="11">"#);
    let _ = writeln!(o, r##"<rect width="{W}" height="{H}" fill="#ffffff"/>"##);
    let _ = writeln!(o, r#"<defs><clipPath id="c"><rect x="{LEFT}" y="{TOP}" width="{pw}" height="{ph}"/></clipPath></defs>"#);
    for (t, label) in x_marks(p, x0, x1, look.density, tick_label) {
        let x = sx(t);
        let top = if look.grid { TOP } else { TOP + ph - 4.0 };
        let _ = writeln!(o, r#"<line x1="{x:.2}" y1="{top}" x2="{x:.2}" y2="{:.2}" stroke="{}"/>"#, TOP + ph, colors.grid_or_text(look));
        if look.axes {
            let _ = writeln!(o, r#"<text x="{x:.2}" y="{:.2}" text-anchor="middle" fill="{}">{}</text>"#, TOP + ph + 16.0, colors.text, escape(&label));
        }
    }
    for t in ticks(y0, y1, look.density) {
        let y = sy(t);
        let right = if look.grid { LEFT + pw } else { LEFT + 4.0 };
        let _ = writeln!(o, r#"<line x1="{LEFT}" y1="{y:.2}" x2="{right:.2}" y2="{y:.2}" stroke="{}"/>"#, colors.grid_or_text(look));
        if look.axes {
            let _ = writeln!(o, r#"<text x="{:.2}" y="{:.2}" text-anchor="end" fill="{}">{}</text>"#, LEFT - 6.0, y + 4.0, colors.text, escape(&tick_label(t)));
        }
    }
    let _ = writeln!(o, r#"<rect x="{LEFT}" y="{TOP}" width="{pw}" height="{ph}" fill="none" stroke="{}"/>"#, colors.text);
    if !p.x_label.is_empty() {
        let _ = writeln!(o, r#"<text x="{:.2}" y="{:.2}" text-anchor="middle" fill="{}">{}</text>"#, LEFT + pw / 2.0, H - 8.0, colors.text, escape(&p.x_label));
    }
    for (i, s) in p.series.iter().enumerate() {
        let color = colors.series.get(i % colors.series.len().max(1)).map_or("#1f6feb", String::as_str);
        for seg in &s.segments {
            match s.kind {
                SeriesKind::Line => {
                    let pts: Vec<String> = seg.iter().map(|[x, y]| format!("{:.2},{:.2}", sx(*x), sy(*y))).collect();
                    let _ = writeln!(
                        o,
                        r#"<polyline clip-path="url(#c)" fill="none" stroke="{color}" stroke-width="1.75" stroke-linejoin="round" points="{}"/>"#,
                        pts.join(" ")
                    );
                }
                SeriesKind::Points => {
                    for [x, y] in seg {
                        let _ = writeln!(o, r#"<circle clip-path="url(#c)" cx="{:.2}" cy="{:.2}" r="3" fill="{color}"/>"#, sx(*x), sy(*y));
                    }
                }
                SeriesKind::Bars { width } => {
                    for [x, h] in seg {
                        let (left, right) = (sx(x - width / 2.0), sx(x + width / 2.0));
                        let (top, base) = (sy(h.max(0.0)), sy(h.min(0.0)));
                        let _ = writeln!(
                            o,
                            r##"<rect clip-path="url(#c)" x="{left:.2}" y="{top:.2}" width="{:.2}" height="{:.2}" fill="{color}" stroke="#ffffff"/>"##,
                            right - left,
                            base - top
                        );
                    }
                }
            }
        }
    }
    if p.series.len() > 1 {
        for (i, s) in p.series.iter().enumerate() {
            let color = colors.series.get(i % colors.series.len().max(1)).map_or("#1f6feb", String::as_str);
            let y = TOP + 14.0 + i as f64 * 16.0;
            let x = LEFT + pw - 120.0;
            let _ = writeln!(o, r#"<line x1="{x}" y1="{:.1}" x2="{}" y2="{:.1}" stroke="{color}" stroke-width="2"/>"#, y - 4.0, x + 18.0, y - 4.0);
            let _ = writeln!(o, r#"<text x="{}" y="{y}" fill="{}">{}</text>"#, x + 24.0, colors.text, escape(&s.label));
        }
    }
    o.push_str("</svg>\n");
    o
}

/// The sampled points: `series,x,y` rows (a gap between segments is an empty line break in the data, not a row).
pub fn csv(p: &PlotSpec) -> String {
    let mut o = String::from("series,x,y\n");
    for s in &p.series {
        let label = if s.label.contains([',', '"', '\n']) { format!("\"{}\"", s.label.replace('"', "\"\"")) } else { s.label.clone() };
        for [x, y] in s.segments.iter().flatten() {
            let _ = writeln!(o, "{label},{x},{y}");
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathcraft_eval::PlotSeries;

    fn spec() -> PlotSpec {
        PlotSpec {
            x_label: "x".into(),
            series: vec![
                PlotSeries::line("a<b", vec![vec![[0.0, 0.0], [1.0, 1.0]], vec![[2.0, 4.0], [3.0, 9.0]]]),
                PlotSeries::line("c,d", vec![vec![[0.0, 1.0], [3.0, 1.0]]]),
            ],
            x_ticks: vec![],
        }
    }

    #[test]
    fn view_covers_the_data() {
        let [x0, x1, y0, y1] = data_view(&spec());
        assert!(x0 == 0.0 && x1 == 3.0 && y0 < 0.0 && y1 > 9.0);
        assert_eq!(data_view(&PlotSpec { series: vec![], x_label: String::new(), x_ticks: vec![] }), [-1.0, 1.0, -1.0, 1.0]);
    }

    #[test]
    fn ticks_are_round_numbers_inside_the_range() {
        assert_eq!(ticks(0.0, 10.0, 1.0), vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
        assert_eq!(ticks(-1.0, 1.0, 1.0), vec![-1.0, -0.5, 0.0, 0.5, 1.0]);
        assert!(ticks(0.0, 10.0, 2.0).len() > ticks(0.0, 10.0, 1.0).len() && ticks(0.0, 10.0, 0.5).len() < ticks(0.0, 10.0, 1.0).len());
        assert!(ticks(1.0, 1.0, 1.0).is_empty() && ticks(f64::NAN, 1.0, 1.0).is_empty());
    }

    #[test]
    fn svg_has_curves_labels_and_escapes() {
        let colors = SvgColors { series: &["#111".to_owned(), "#222".to_owned()], text: "#000", grid: "#ddd" };
        let s = svg(&spec(), data_view(&spec()), &colors, Look::default(), &|v| format!("{v}"));
        assert!(s.starts_with("<svg") && s.trim_end().ends_with("</svg>"));
        assert_eq!(s.matches("<polyline").count(), 3, "one per segment");
        assert!(s.contains("a&lt;b") && !s.contains("a<b"));
        assert!(s.contains("stroke=\"#222\""));
    }

    #[test]
    fn dots_and_bars_are_drawn_and_bars_stand_on_zero() {
        let colors = SvgColors { series: &["#111".to_owned()], text: "#000", grid: "#ddd" };
        let bars = PlotSpec {
            x_label: String::new(),
            series: vec![PlotSeries { label: String::new(), segments: vec![vec![[0.0, 3.0], [1.0, 5.0]]], kind: SeriesKind::Bars { width: 0.7 } }],
            x_ticks: vec![(0.0, "a".into()), (1.0, "b".into())],
        };
        let [x0, x1, y0, y1] = data_view(&bars);
        assert!(x0 < -0.3 && x1 > 1.3 && y0 <= 0.0 && y1 >= 5.0, "{x0} {x1} {y0} {y1}");
        let s = svg(&bars, data_view(&bars), &colors, Look::default(), &|v| format!("{v}"));
        assert_eq!(s.matches("<rect clip-path").count(), 2);
        assert!(s.contains(">a</text>") && s.contains(">b</text>"), "category names replace numbers");
        let dots = PlotSpec {
            series: vec![PlotSeries { label: String::new(), segments: vec![vec![[0.0, 1.0], [2.0, 3.0]]], kind: SeriesKind::Points }],
            x_label: String::new(),
            x_ticks: vec![],
        };
        assert_eq!(svg(&dots, data_view(&dots), &colors, Look::default(), &|v| format!("{v}")).matches("<circle").count(), 2);
    }

    #[test]
    fn csv_quotes_labels() {
        let c = csv(&spec());
        assert!(c.starts_with("series,x,y\n"));
        assert!(c.contains("a<b,0,0\n") && c.contains("\"c,d\",3,1\n"));
        assert_eq!(c.lines().count(), 1 + 4 + 2);
    }

    #[test]
    fn the_look_changes_the_picture() {
        let colors = SvgColors { series: &["#111".to_owned()], text: "#000", grid: "#ddd" };
        let text = |look| svg(&spec(), data_view(&spec()), &colors, look, &|v| format!("{v}")).matches("<text").count();
        let usual = text(Look::default());
        assert!(usual > 2, "tick numbers are drawn by default");
        assert!(text(Look { axes: false, ..Look::default() }) < usual, "no numbers without axes");
        let lines = |look| svg(&spec(), data_view(&spec()), &colors, look, &|v| format!("{v}")).matches(r##"stroke="#ddd""##).count();
        assert_eq!(lines(Look::default()), 0, "no pale grid lines unless asked");
        assert!(lines(Look { grid: true, ..Look::default() }) > 0);
    }
}
