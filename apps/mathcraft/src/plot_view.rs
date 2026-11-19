//! Plot cells: options, marks, export and ticks.

use super::*;

/// Remembers, per plot, that "Reset view" was chosen (applied on the next frame).
#[derive(Clone, Copy, Default)]
pub(crate) struct ResetPlot(pub(crate) bool);

/// What one plot shows, chosen from its context menu and kept while the app runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlotOptions {
    pub(crate) grid: bool,
    pub(crate) axes: bool,
    pub(crate) marks: Marks,
}

impl PlotOptions {
    /// The same choices for a saved picture.
    pub(crate) fn look(self) -> plot_export::Look {
        let density = match self.marks {
            Marks::Auto => 1.0,
            Marks::Wide => 0.5,
            Marks::Tight => 2.0,
        };
        plot_export::Look { grid: self.grid, axes: self.axes, density }
    }
}

impl Default for PlotOptions {
    fn default() -> Self {
        PlotOptions { grid: false, axes: true, marks: Marks::Auto }
    }
}

/// How far apart the tick marks (and grid lines) are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) enum Marks {
    #[default]
    Auto,
    Wide,
    Tight,
}

impl Marks {
    pub(crate) fn next(self) -> Marks {
        match self {
            Marks::Auto => Marks::Wide,
            Marks::Wide => Marks::Tight,
            Marks::Tight => Marks::Auto,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Marks::Auto => "automatic",
            Marks::Wide => "far apart",
            Marks::Tight => "close together",
        }
    }

    /// The range of pixels between two marks.
    pub(crate) fn spacing(self) -> std::ops::RangeInclusive<f32> {
        match self {
            Marks::Auto => 80.0..=400.0,
            Marks::Wide => 160.0..=800.0,
            Marks::Tight => 40.0..=200.0,
        }
    }
}

/// A plot you can look around in: drag to pan, pinch or Ctrl+scroll to zoom, right-drag a box to zoom into it,
/// double-click to reset. The coordinates under the pointer show in the corner; the context menu resets the view,
/// shows or hides the grid and the axes, changes how far apart the marks are, copies the points as CSV or saves the view as a picture.
pub(crate) fn plot_ui(ui: &mut egui::Ui, theme: &Theme, id: CellId, p: &mathcraft_eval::PlotSpec, actions: &mut Vec<Action>) {
    let plot_id = egui::Id::unique(("plot", id));
    let reset_key = plot_id.with("reset");
    let options_key = plot_id.with("options");
    let options = ui.ctx().data(|d| d.get_temp::<PlotOptions>(options_key)).unwrap_or_default();
    ui.scope(|ui| {
        // The plot draws its frame, labels and legend from the visuals; keep them quiet and on the page colour.
        let v = ui.visuals_mut();
        v.override_text_color = Some(theme.text_dim);
        v.extreme_bg_color = theme.bg;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, theme.border);
        let mut plot = egui_plot::Plot::new(plot_id)
            .height(280.0)
            .grid_spacing(options.marks.spacing())
            .x_grid_spacer(nice_grid(1.0))
            .y_grid_spacer(nice_grid(0.25))
            .x_axis_formatter(|m, _| category_label(&p.x_ticks, m.value).unwrap_or_else(|| tick_label(m.value)))
            .y_axis_formatter(|m, _| tick_label(m.value))
            .coordinates_formatter(egui_plot::Corner::LeftBottom, egui_plot::CoordinatesFormatter::new(|pt, b| coordinate_text(pt.x, pt.y, b)))
            .allow_scroll(false)
            .allow_boxed_zoom(true)
            .show_grid(options.grid)
            .show_axes(options.axes)
            .x_axis_label(p.x_label.clone());
        if ui.ctx().data_mut(|d| d.get_temp::<ResetPlot>(reset_key).is_some_and(|r| r.0)) {
            ui.ctx().data_mut(|d| d.insert_temp(reset_key, ResetPlot(false)));
            plot = plot.reset();
        }
        if p.series.len() > 1 {
            plot = plot.legend(egui_plot::Legend::default().position(egui_plot::Corner::RightTop));
        }
        let shown = plot.show(ui, |plot| {
            for (i, s) in p.series.iter().enumerate() {
                let color = theme.plot.get(i % theme.plot.len()).copied().unwrap_or(theme.accent);
                for seg in &s.segments {
                    match s.kind {
                        mathcraft_eval::SeriesKind::Line => {
                            plot.line(egui_plot::Line::new(s.label.clone(), egui_plot::PlotPoints::from(seg.clone())).color(color).width(1.75))
                        }
                        mathcraft_eval::SeriesKind::Points => {
                            plot.points(egui_plot::Points::new(s.label.clone(), egui_plot::PlotPoints::from(seg.clone())).color(color).radius(3.0))
                        }
                        mathcraft_eval::SeriesKind::Bars { width } => {
                            let bars =
                                seg.iter().map(|[x, h]| egui_plot::Bar::new(*x, *h).width(width).fill(color).stroke(Stroke::new(1.0, theme.bg))).collect();
                            plot.bar_chart(egui_plot::BarChart::new(s.label.clone(), bars).color(color));
                        }
                    }
                }
            }
        });
        let b = shown.transform.bounds();
        let view = [b.min()[0], b.max()[0], b.min()[1], b.max()[1]];
        shown.response.context_menu(|ui| {
            if menu_row(ui, theme, "Reset view", None).clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(reset_key, ResetPlot(true)));
                ui.close();
            }
            let mut changed = options;
            if menu_row(ui, theme, if options.grid { "Hide grid" } else { "Show grid" }, None).clicked() {
                changed.grid = !options.grid;
                ui.close();
            }
            if menu_row(ui, theme, if options.axes { "Hide axes" } else { "Show axes" }, None).clicked() {
                changed.axes = !options.axes;
                ui.close();
            }
            if menu_row(ui, theme, &format!("Marks: {}", options.marks.name()), None).clicked() {
                changed.marks = options.marks.next();
                ui.close();
            }
            if changed != options {
                ui.ctx().data_mut(|d| d.insert_temp(options_key, changed));
            }
            ui.separator();
            if menu_row(ui, theme, "Copy points as CSV", None).clicked() {
                ui.ctx().copy_text(plot_export::csv(p));
                actions.push(Action::Status("Copied the plot's points as CSV".into()));
                ui.close();
            }
            if menu_row(ui, theme, "Save as SVG…", None).clicked() {
                ui.close();
                actions.push(Action::Status(save_svg(p, view, options.look())));
            }
            if menu_row(ui, theme, "Save as PNG…", None).clicked() {
                ui.close();
                actions.push(Action::Status(save_png(p, view, options.look())));
            }
        });
    });
}

/// The category name at `x` when the axis carries names (a bar chart): empty between the names, `None` when it carries numbers.
pub(crate) fn category_label(names: &[(f64, String)], x: f64) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    Some(names.iter().find(|(at, _)| (at - x).abs() < 1e-9).map(|(_, n)| n.clone()).unwrap_or_default())
}

/// "x = 1.25   y = -0.5", with as many decimals as the visible range needs.
pub(crate) fn coordinate_text(x: f64, y: f64, b: &egui_plot::PlotBounds) -> String {
    let decimals = |span: f64| (3.0 - span.abs().max(f64::MIN_POSITIVE).log10().floor()).clamp(0.0, 10.0) as usize;
    let fmt = |v: f64, span: f64| {
        let s = format!("{v:.*}", decimals(span));
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
    };
    format!("x = {}   y = {}", fmt(x, b.width()), fmt(y, b.height()))
}

/// The plot as an SVG picture on a white page, over `view` (the box around the data when `view` is not usable).
pub(crate) fn svg_text(p: &mathcraft_eval::PlotSpec, view: [f64; 4], look: plot_export::Look) -> String {
    let sane = view.iter().all(|v| v.is_finite()) && view[1] > view[0] && view[3] > view[2];
    let view = if sane { view } else { plot_export::data_view(p) };
    let hex = |c: Color32| format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
    let series: Vec<String> = Theme::light().plot.iter().map(|c| hex(*c)).collect();
    let colors = plot_export::SvgColors { series: &series, text: "#333333", grid: "#e4e4e8" };
    plot_export::svg(p, view, &colors, look, &tick_label)
}

/// Ask where to save, write the picture, and say what happened.
pub(crate) fn save_svg(p: &mathcraft_eval::PlotSpec, view: [f64; 4], look: plot_export::Look) -> String {
    let Some(path) = rfd::FileDialog::new().add_filter("SVG picture", &["svg"]).set_file_name("plot.svg").save_file() else { return "Save cancelled".into() };
    match std::fs::write(&path, svg_text(p, view, look)) {
        Ok(()) => format!("Saved {}", file_name(&path)),
        Err(e) => format!("Could not save the plot: {e}"),
    }
}

/// Ask for a file and write the plot over `view` as a PNG picture (twice the SVG's size, so it stays sharp).
pub(crate) fn save_png(p: &mathcraft_eval::PlotSpec, view: [f64; 4], look: plot_export::Look) -> String {
    let Some(path) = rfd::FileDialog::new().add_filter("PNG picture", &["png"]).set_file_name("plot.png").save_file() else { return "Save cancelled".into() };
    let sane = view.iter().all(|v| v.is_finite()) && view[1] > view[0] && view[3] > view[2];
    let view = if sane { view } else { plot_export::data_view(p) };
    let series: Vec<plot_png::Rgb> = Theme::light().plot.iter().map(|c| [c.r(), c.g(), c.b()]).collect();
    let colors = plot_png::Colors { series: &series, text: [0x33, 0x33, 0x33], grid: [0xe4, 0xe4, 0xe8] };
    match plot_png::png(p, view, &colors, look, &tick_label, fonts::INTER, 2.0).and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string())) {
        Ok(()) => format!("Saved {}", file_name(&path)),
        Err(e) => format!("Could not save the plot: {e}"),
    }
}

/// Axis ticks at 1, 2 or 5 times a power of ten, about `fraction` of the plot's minimum grid spacing apart, so an axis
/// carries several labels (the default base-10 spacer often leaves a short y axis with a single `0`).
pub(crate) fn nice_grid(fraction: f64) -> Box<dyn Fn(egui_plot::GridInput) -> Vec<egui_plot::GridMark>> {
    egui_plot::uniform_grid_spacer(move |input| {
        let want = (input.base_step_size * fraction).abs().max(f64::MIN_POSITIVE);
        let power = 10f64.powf(want.log10().floor());
        let step = [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * power).find(|s| *s >= want).unwrap_or(power * 10.0);
        [step, step * 10.0, step * 100.0]
    })
}

/// An axis tick: the shortest decimal for the value (ticks are 1, 2 or 5 times a power of ten), `0` not `-0`.
pub(crate) fn tick_label(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s.as_str() };
    if s == "-0" { "0".to_owned() } else { s.to_owned() }
}
