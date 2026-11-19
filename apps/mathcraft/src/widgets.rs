//! Small drawing helpers: style, rows, banners and menu parts.

use super::*;

pub(crate) fn configure_style(ctx: &egui::Context) {
    use egui::{FontFamily, FontId};
    ctx.all_styles_mut(|s| {
        // One type scale: 11.5 captions, 13 chrome, 15 document text, 13.5 code; headings in the markdown renderer.
        s.text_styles.insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(13.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(11.5, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Heading, FontId::new(17.0, fonts::strong_family()));
        s.text_styles.insert(TextStyle::Monospace, FontId::new(13.5, FontFamily::Monospace));
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(8.0, 4.0);
        s.spacing.interact_size.y = 24.0;
        s.spacing.menu_margin = Margin::same(5);
        s.spacing.menu_spacing = 4.0;
        s.spacing.window_margin = Margin::same(14);
        s.spacing.scroll = egui::style::ScrollStyle { bar_width: 7.0, floating_width: 3.0, ..egui::style::ScrollStyle::floating() };
    });
}

pub(crate) fn state_color(theme: &Theme, state: CellState) -> Color32 {
    match state {
        CellState::New => theme.text_faint,
        CellState::Fresh => theme.ok,
        CellState::Stale | CellState::Blocked => theme.warn,
        CellState::Queued | CellState::Running => theme.accent,
        CellState::Error => theme.danger,
    }
}

pub(crate) fn state_label(state: CellState) -> &'static str {
    match state {
        CellState::New => "not run",
        CellState::Fresh => "",
        CellState::Stale => "stale",
        CellState::Queued => "queued",
        CellState::Running => "running…",
        CellState::Error => "error",
        CellState::Blocked => "blocked",
    }
}

/// The Vim mode as a status-bar segment: a coloured pill with the mode name, then any keys typed so far
/// (`2d`, `:wq`, `/foo`) in monospace. Insert is the accent, visual the warning colour, normal and command quiet.
pub(crate) fn vim_segment(ui: &mut egui::Ui, theme: &Theme, status: &str) {
    let (mode, pending) = if status.starts_with(':') || status.starts_with('/') {
        ("COMMAND", status)
    } else {
        let mut parts = status.splitn(2, char::is_whitespace);
        (parts.next().unwrap_or(""), parts.next().unwrap_or("").trim())
    };
    let color = match mode {
        "INSERT" => theme.accent,
        "VISUAL" | "V-LINE" => theme.warn,
        _ => theme.text_dim,
    };
    let font = egui::FontId::new(10.5, fonts::strong_family());
    let galley = ui.painter().layout_no_wrap(mode.to_owned(), font, color);
    let (r, resp) = ui.allocate_exact_size(vec2(galley.size().x + 14.0, 17.0), egui::Sense::hover());
    ui.painter().rect_filled(r, theme::RADIUS_SMALL, theme.pill(color));
    ui.painter().galley(r.center() - galley.size() / 2.0, galley, color);
    resp.on_hover_text("Vim keys are on (Settings > Editing)");
    if !pending.is_empty() {
        ui.label(RichText::new(pending).monospace().size(theme::TEXT_CAPTION).color(theme.text));
    }
}

pub(crate) fn dot(ui: &mut egui::Ui, color: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), 3.5, color);
}

/// Shown under the single empty cell of a new notebook: what to type and the keys that matter.
pub(crate) fn empty_state(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(28.0);
    ui.horizontal(|ui| {
        ui.add_space(GUTTER + EDITOR_PAD_X);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(RichText::new("A new notebook").font(egui::FontId::new(15.0, fonts::strong_family())).color(theme.text));
            ui.label(RichText::new("Type an expression in the cell above, such as integrate(x*sin(x), x), and run it.").size(13.5).color(theme.text_dim));
            ui.add_space(4.0);
            egui::Grid::new("empty-keys").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                for (keys, what) in [
                    ("⇧↵", "Run the cell and move to the next one"),
                    ("⌘↵", "Run the cell and stay"),
                    ("⇧⌘↵", "Run every cell"),
                    ("F1", "Browse all functions"),
                ] {
                    keycap(ui, theme, keys);
                    ui.label(RichText::new(what).size(13.0).color(theme.text_dim));
                    ui.end_row();
                }
            });
        });
    });
}

/// A key or key combination drawn as a small cap.
pub(crate) fn keycap(ui: &mut egui::Ui, theme: &Theme, keys: &str) {
    // measure on a throwaway painter first, then paint into the allocated space, left-aligned in the column
    let width = icons::keycaps(&ui.painter().with_clip_rect(egui::Rect::NOTHING), theme, egui::Pos2::ZERO, keys);
    let (r, _) = ui.allocate_exact_size(vec2(width.max(54.0), 20.0), egui::Sense::hover());
    icons::keycaps(ui.painter(), theme, egui::pos2(r.left() + width, r.center().y), keys);
}

/// A one-pixel line across the window at `y`, between chrome and document.
pub(crate) fn hairline(ui: &egui::Ui, theme: &Theme, x: egui::Rangef, y: f32) {
    ui.painter().hline(x, y, Stroke::new(1.0, theme.separator));
}

/// A short vertical divider between groups of toolbar items.
pub(crate) fn vertical_rule(ui: &mut egui::Ui, theme: &Theme) {
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 16.0), egui::Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, theme.border_strong));
}

/// The file name in the toolbar, with "Edited" beside it while there are unsaved changes.
pub(crate) fn document_title(ui: &mut egui::Ui, theme: &Theme, path: Option<&PathBuf>, dirty: bool) {
    let name = path.and_then(|p| p.file_name()).map_or("Untitled".to_string(), |n| n.to_string_lossy().to_string());
    if dirty {
        ui.label(RichText::new("Edited").size(12.0).color(theme.text_faint));
    }
    let tip = path.map_or_else(|| "Not saved yet".to_string(), |p| p.display().to_string());
    ui.label(RichText::new(name).size(13.0).font(egui::FontId::new(13.0, fonts::strong_family())).color(theme.text_dim)).on_hover_text(tip);
}

/// One row of a menu: the label on the left, the shortcut as keycaps on the right, tinted with the accent under the
/// pointer and dimmed when disabled. All rows in a menu are as wide as the widest, so the highlight is a clean bar.
pub(crate) fn menu_row(ui: &mut egui::Ui, theme: &Theme, label: &str, shortcut: Option<&str>) -> egui::Response {
    const PAD: f32 = 12.0;
    let enabled = ui.is_enabled();
    let font = TextStyle::Button.resolve(ui.style());
    let color = if enabled { theme.text } else { theme.text_faint };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, color);
    let keys_w = shortcut.map_or(0.0, |s| icons::keycaps(&ui.painter().with_clip_rect(egui::Rect::NOTHING), theme, egui::Pos2::ZERO, s) + 28.0);
    let need = PAD + galley.size().x + keys_w + PAD;
    let size = vec2(ui.available_width().min(MENU_WIDTH).max(need), 26.0);
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        if enabled && (resp.hovered() || resp.has_focus()) {
            ui.painter().rect_filled(rect.shrink2(vec2(0.0, 1.0)), 5.0, theme.selected_row());
        }
        ui.painter().galley(egui::pos2(rect.left() + PAD, rect.center().y - galley.size().y / 2.0), galley, color);
        if let (Some(s), true) = (shortcut, enabled) {
            icons::keycaps(ui.painter(), theme, egui::pos2(rect.right() - PAD + 2.0, rect.center().y), s);
        }
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// How wide a menu is drawn (wider only when a label and its shortcut need it).
pub(crate) const MENU_WIDTH: f32 = 250.0;

/// A quiet heading inside a menu.
/// A menu row for a setting that is on or off, or one of several: a check mark in front of the chosen one.
/// Returns true when clicked. Does not close the menu, so several settings can be changed in one visit.
pub(crate) fn choice(ui: &mut egui::Ui, theme: &Theme, label: &str, selected: bool) -> bool {
    let font = TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, theme.text);
    // a popup offers the whole window as its width, so rows ask for a sensible one
    let size = vec2(ui.available_width().min(330.0).max(galley.size().x + 38.0), 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        if resp.hovered() {
            ui.painter().rect_filled(rect.shrink2(vec2(0.0, 1.0)), 5.0, theme.selected_row());
        }
        if selected {
            let c = egui::pos2(rect.left() + 14.0, rect.center().y);
            let stroke = Stroke::new(1.7, theme.accent);
            ui.painter().line_segment([c + vec2(-4.0, 0.5), c + vec2(-1.5, 3.5)], stroke);
            ui.painter().line_segment([c + vec2(-1.5, 3.5), c + vec2(4.5, -3.5)], stroke);
        }
        let at = egui::pos2(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, theme.text);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// The accent colours as a row of dots; the chosen one has a ring. Returns the name clicked.
pub(crate) fn accent_row(ui: &mut egui::Ui, theme: &Theme, current: Option<&str>) -> Option<&'static str> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.spacing_mut().item_spacing.x = 6.0;
        for (name, dark, light) in settings::ACCENTS {
            let [r, g, b] = if theme.dark { dark } else { light };
            let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
            let selected = current.unwrap_or("blue") == name;
            ui.painter().circle_filled(rect.center(), 7.0, Color32::from_rgb(r, g, b));
            if selected || resp.hovered() {
                ui.painter().circle_stroke(rect.center(), 10.0, Stroke::new(1.5, if selected { theme.text } else { theme.border_strong }));
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(name).clicked() {
                picked = Some(name);
            }
        }
    });
    picked
}

pub(crate) fn menu_section(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(title).font(egui::FontId::new(11.5, fonts::strong_family())).color(theme.text_faint));
}

/// A note under a cell: problems found before running (warning) or the error from running (error). A tinted strip
/// with a coloured rule and icon on the left; the message itself is in the normal text colour so it stays readable.
pub(crate) fn banner(ui: &mut egui::Ui, theme: &Theme, icon: Icon, color: Color32, code: Option<&str>, message: &str) {
    let inner = egui::Frame::new().fill(theme.tint(color)).corner_radius(theme::RADIUS).inner_margin(Margin { left: 34, right: 12, top: 7, bottom: 7 }).show(
        ui,
        |ui| {
            ui.set_min_width(ui.available_width());
            // One paragraph (code, then message) so long messages wrap under themselves instead of leaving the code
            // stranded on a line of its own.
            let mut job = egui::text::LayoutJob::default();
            if let Some(code) = code {
                job.append(
                    code,
                    0.0,
                    egui::TextFormat { valign: egui::Align::Center, ..egui::TextFormat::simple(egui::FontId::monospace(theme::TEXT_CAPTION), color) },
                );
                job.append("  ", 0.0, egui::TextFormat::simple(egui::FontId::proportional(13.5), theme.text));
            }
            job.append(message, 0.0, egui::TextFormat { line_height: Some(20.0), ..egui::TextFormat::simple(egui::FontId::proportional(13.5), theme.text) });
            ui.add(egui::Label::new(job).wrap().selectable(true));
        },
    );
    let r = inner.response.rect;
    let rule = egui::Rect::from_min_size(r.min, vec2(3.0, r.height()));
    ui.painter().rect_filled(rule, egui::CornerRadius { nw: theme::RADIUS, sw: theme::RADIUS, ne: 0, se: 0 }, color);
    icons::paint(ui.painter(), icon, egui::Rect::from_center_size(egui::pos2(r.left() + 19.0, r.top() + 16.5), vec2(14.0, 14.0)), color);
}
