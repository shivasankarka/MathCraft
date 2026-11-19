//! Cell results: text, typeset math, arrays and copy-as.

use super::*;

pub(crate) fn output_ui(ui: &mut egui::Ui, env: &mut Env, cell: &Cell) {
    let Some(o) = &cell.output else { return };
    let theme = env.theme;
    if o.stdout.is_empty() && matches!(o.result, Ok(None)) {
        return;
    }
    let stale = cell.state == CellState::Stale;
    let has_value = matches!(o.result, Ok(Some(_)));
    if !o.stdout.is_empty() || has_value {
        ui.add_space(10.0);
        // Outputs line up with the code above them, not with the editor's outline.
        egui::Frame::new().inner_margin(Margin { left: EDITOR_PAD_X as i8, right: 0, top: 0, bottom: 2 }).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if stale {
                ui.set_opacity(0.45);
            }
            if !o.stdout.is_empty() {
                stdout_ui(ui, theme, cell.id, o.stdout.trim_end());
                if has_value {
                    ui.add_space(8.0);
                }
            }
            match &o.result {
                Ok(Some(Value::Plot(p))) => plot_ui(ui, theme, cell.id, p, env.actions),
                Ok(Some(v)) => value_ui(ui, env, cell, v),
                _ => {}
            }
        });
    }
    if let Err(e) = &o.result {
        ui.add_space(6.0);
        ui.scope(|ui| {
            if stale {
                ui.set_opacity(0.45);
            }
            banner(ui, theme, Icon::Error, theme.danger, None, &e.to_string());
        });
    }
}

/// Printed output longer than this many lines is cut, with a button to show the rest.
pub(crate) const STDOUT_LINES: usize = 20;

/// The text a cell printed. Long output shows its first lines and "Show N more lines".
pub(crate) fn stdout_ui(ui: &mut egui::Ui, theme: &Theme, id: CellId, text: &str) {
    let total = text.lines().count();
    let key = egui::Id::unique(("stdout-open", id));
    let open = ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let shown: String = if open || total <= STDOUT_LINES + 4 { text.to_owned() } else { text.lines().take(STDOUT_LINES).collect::<Vec<_>>().join("\n") };
    ui.add(egui::Label::new(RichText::new(shown).monospace().size(13.0).color(theme.text_dim)).selectable(true));
    if total > STDOUT_LINES + 4 {
        let label = if open { "Show less".to_owned() } else { format!("Show {} more lines", total - STDOUT_LINES) };
        if ui.add(egui::Button::new(RichText::new(label).size(12.0).color(theme.accent)).frame(false)).clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(key, !open));
        }
    }
}

pub(crate) fn value_ui(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, v: &Value) {
    if let Value::Array(a) = v {
        array_ui(ui, env, cell, v, &a.describe());
        return;
    }
    value_math(ui, env, cell, v);
    if let Some(note) = unevaluated_note(v) {
        ui.add_space(4.0);
        ui.label(RichText::new(note).size(11.5).color(env.theme.text_faint));
    }
}

/// A fade from the background colour into transparent over the edge of `rect`: the right edge if `right`, else the left.
pub(crate) fn edge_fade(ui: &egui::Ui, rect: egui::Rect, right: bool, bg: Color32) {
    const WIDTH: f32 = 36.0;
    let (x0, x1) = if right { (rect.right() - WIDTH, rect.right()) } else { (rect.left(), rect.left() + WIDTH) };
    let (c0, c1) = if right { (Color32::TRANSPARENT, bg) } else { (bg, Color32::TRANSPARENT) };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(egui::pos2(x0, rect.top()), c0);
    mesh.colored_vertex(egui::pos2(x1, rect.top()), c1);
    mesh.colored_vertex(egui::pos2(x1, rect.bottom()), c1);
    mesh.colored_vertex(egui::pos2(x0, rect.bottom()), c0);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// A result that is still an unevaluated call means no closed form was found; say so rather than leave it unexplained.
pub(crate) fn unevaluated_note(v: &Value) -> Option<&'static str> {
    let Value::Expr(e) = v else { return None };
    match e.kind() {
        mathcraft_core::Kind::Call(name, _) if &**name == "integrate" => {
            Some("No closed form found. A definite integral, integrate(f, (x, a, b)), is computed numerically.")
        }
        _ => None,
    }
}

/// A vector or matrix: typeset, with its kind, shape and element type underneath.
pub(crate) fn array_ui(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, v: &Value, caption: &str) {
    value_math(ui, env, cell, v);
    ui.add_space(4.0);
    ui.label(RichText::new(caption).size(11.5).color(env.theme.text_faint));
}

pub(crate) fn value_math(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, v: &Value) {
    let generation = env.generations.get(&cell.id).copied().unwrap_or(0);
    match env.math.get(ui.ctx(), cell.id, generation, v, (ui.available_width() - 8.0).max(120.0)) {
        Some(layout) => {
            let theme = env.theme;
            let resp = if layout.width + 8.0 > ui.available_width() {
                let shown = egui::ScrollArea::horizontal().id_salt(("out", cell.id)).show(ui, |ui| math_view::show(ui, &layout, theme, math_view::SIZE));
                // a soft edge on the side that has more, so a cut-off result reads as scrollable
                let view = shown.inner_rect;
                if shown.state.clamped_offset().x > 1.0 {
                    edge_fade(ui, view, false, theme.bg);
                }
                if shown.state.clamped_offset().x + view.width() < shown.content_size.x - 1.0 {
                    edge_fade(ui, view, true, theme.bg);
                }
                shown.inner
            } else {
                math_view::show(ui, &layout, theme, math_view::SIZE)
            };
            resp.context_menu(|ui| copy_as_menu(ui, theme, v));
        }
        None => match v {
            Value::Str(s) => {
                ui.add(egui::Label::new(RichText::new(s.as_ref()).size(15.0).color(env.theme.text)).selectable(true));
            }
            other => {
                ui.add(egui::Label::new(RichText::new(other.to_string()).monospace().size(14.0).color(env.theme.text)).selectable(true));
            }
        },
    }
}

/// The ways a result can leave MathCraft, each with the text to copy (`None` when this value has no such form).
pub(crate) fn copy_formats(v: &Value) -> Vec<(&'static str, Option<String>)> {
    use mathcraft_core::codegen::{Target, expression};
    let code = |target| match v {
        Value::Expr(e) => expression(e, target).ok(),
        _ => None,
    };
    vec![("Text", Some(v.to_string())), ("LaTeX", v.latex()), ("Python", code(Target::Python)), ("NumPy", code(Target::NumPy)), ("Rust", code(Target::Rust))]
}

/// The output's context menu: copy the value as text, LaTeX or source code in another language.
pub(crate) fn copy_as_menu(ui: &mut egui::Ui, theme: &Theme, v: &Value) {
    ui.menu_button("Copy as", |ui| {
        for (label, text) in copy_formats(v) {
            let enabled = text.is_some();
            if ui.add_enabled_ui(enabled, |ui| menu_row(ui, theme, label, None)).inner.clicked()
                && let Some(text) = text
            {
                ui.ctx().copy_text(text);
                ui.close();
            }
        }
    });
}
