//! Small painted icons and icon buttons. Drawn as shapes so they look the same on every platform and in both themes.

use crate::fonts::strong_family;
use crate::theme::Theme;
use egui::{Color32, CursorIcon, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Play,
    PlayAll,
    Stop,
    Up,
    Down,
    Close,
    /// A circle with one half filled: the light/dark switch.
    Contrast,
    Plus,
    /// A triangle with an exclamation mark: problems found before running.
    Warning,
    /// A disc with an exclamation mark: an error from running.
    Error,
    /// A magnifying glass: search fields.
    Search,
}

/// Paint `icon` centred in `rect`.
pub fn paint(painter: &Painter, icon: Icon, rect: Rect, color: Color32) {
    let c = rect.center();
    let w = rect.width().min(rect.height()) * 0.5;
    let stroke = Stroke::new(1.6, color);
    let line = |a: Pos2, b: Pos2| painter.line_segment([a, b], stroke);
    match icon {
        Icon::Play => {
            painter.add(Shape::convex_polygon(
                vec![pos2(c.x - 0.55 * w, c.y - 0.8 * w), pos2(c.x + 0.8 * w, c.y), pos2(c.x - 0.55 * w, c.y + 0.8 * w)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::PlayAll => {
            for dx in [-0.5, 0.45] {
                painter.add(Shape::convex_polygon(
                    vec![pos2(c.x + (dx - 0.45) * w, c.y - 0.7 * w), pos2(c.x + (dx + 0.45) * w, c.y), pos2(c.x + (dx - 0.45) * w, c.y + 0.7 * w)],
                    color,
                    Stroke::NONE,
                ));
            }
        }
        Icon::Stop => {
            painter.add(Shape::rect_filled(Rect::from_center_size(c, vec2(1.3 * w, 1.3 * w)), 2.0, color));
        }
        Icon::Up => {
            painter.add(Shape::line(vec![pos2(c.x - 0.8 * w, c.y + 0.35 * w), pos2(c.x, c.y - 0.45 * w), pos2(c.x + 0.8 * w, c.y + 0.35 * w)], stroke));
        }
        Icon::Down => {
            painter.add(Shape::line(vec![pos2(c.x - 0.8 * w, c.y - 0.35 * w), pos2(c.x, c.y + 0.45 * w), pos2(c.x + 0.8 * w, c.y - 0.35 * w)], stroke));
        }
        Icon::Close => {
            line(pos2(c.x - 0.65 * w, c.y - 0.65 * w), pos2(c.x + 0.65 * w, c.y + 0.65 * w));
            line(pos2(c.x - 0.65 * w, c.y + 0.65 * w), pos2(c.x + 0.65 * w, c.y - 0.65 * w));
        }
        Icon::Contrast => {
            let r = 0.9 * w;
            painter.circle_stroke(c, r, stroke);
            // The left half of the disc, from the top round to the bottom.
            let half: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 16.0;
                    pos2(c.x + r * a.cos(), c.y - r * a.sin())
                })
                .collect();
            painter.add(Shape::convex_polygon(half, color, Stroke::NONE));
        }
        Icon::Plus => {
            line(pos2(c.x - 0.7 * w, c.y), pos2(c.x + 0.7 * w, c.y));
            line(pos2(c.x, c.y - 0.7 * w), pos2(c.x, c.y + 0.7 * w));
        }
        Icon::Warning => {
            let tri = vec![pos2(c.x, c.y - 0.95 * w), pos2(c.x + 1.0 * w, c.y + 0.8 * w), pos2(c.x - 1.0 * w, c.y + 0.8 * w)];
            painter.add(Shape::convex_polygon(tri, color, Stroke::new(1.0, color)));
            exclamation(painter, pos2(c.x, c.y + 0.12 * w), 0.85 * w, color);
        }
        Icon::Error => {
            painter.circle_filled(c, 0.95 * w, color);
            exclamation(painter, c, w, color);
        }
        Icon::Search => {
            let lens = pos2(c.x - 0.15 * w, c.y - 0.15 * w);
            painter.circle_stroke(lens, 0.62 * w, stroke);
            line(pos2(lens.x + 0.46 * w, lens.y + 0.46 * w), pos2(c.x + 0.9 * w, c.y + 0.9 * w));
        }
    }
}

/// The mark inside the warning and error icons: dark on a light shape, white on a dark one.
fn exclamation(painter: &Painter, c: Pos2, w: f32, shape: Color32) {
    let luma = 0.299 * f32::from(shape.r()) + 0.587 * f32::from(shape.g()) + 0.114 * f32::from(shape.b());
    let ink = if luma > 150.0 { Color32::from_rgb(30, 30, 34) } else { Color32::WHITE };
    let s = Stroke::new((0.22 * w).max(1.3), ink);
    painter.line_segment([pos2(c.x, c.y - 0.5 * w), pos2(c.x, c.y + 0.12 * w)], s);
    painter.circle_filled(pos2(c.x, c.y + 0.45 * w), (0.12 * w).max(0.75), ink);
}

/// A small rounded badge with one letter, telling apart kinds of completion (function, name).
pub fn badge(painter: &Painter, rect: Rect, letter: &str, color: Color32) {
    painter.rect_filled(rect, 4.0, color.gamma_multiply(0.16));
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, letter, FontId::new(rect.height() * 0.68, strong_family()), color);
}

/// Split a shortcut as egui formats it ("⇧⌘S", "Ctrl+Shift+S", "F1") into its keys, one per cap.
pub fn split_keys(shortcut: &str) -> Vec<String> {
    let s = shortcut.trim();
    if s.chars().count() > 1 && s.contains('+') {
        return s.split('+').filter(|k| !k.is_empty()).map(str::to_owned).collect();
    }
    let mut keys = Vec::new();
    let mut rest = String::new();
    for c in s.chars() {
        if matches!(c, '⌃' | '⌥' | '⇧' | '⌘') && rest.is_empty() {
            keys.push(c.to_string());
        } else if !c.is_whitespace() {
            rest.push(c);
        }
    }
    if !rest.is_empty() {
        keys.push(rest);
    }
    keys
}

/// Paint `shortcut` as a row of small keycaps ending at `right` (vertically centred on it). Returns the width used.
pub fn keycaps(painter: &Painter, theme: &Theme, right: Pos2, shortcut: &str) -> f32 {
    const H: f32 = 18.0;
    const GAP: f32 = 3.0;
    let font = FontId::proportional(crate::theme::TEXT_CAPTION - 0.5);
    let galleys: Vec<_> = split_keys(shortcut).into_iter().map(|k| painter.layout_no_wrap(k, font.clone(), theme.text_dim)).collect();
    let widths: Vec<f32> = galleys.iter().map(|g| (g.size().x + 10.0).max(H)).collect();
    let total = widths.iter().sum::<f32>() + GAP * widths.len().saturating_sub(1) as f32;
    let mut x = right.x - total;
    for (g, w) in galleys.into_iter().zip(widths) {
        let r = Rect::from_min_size(pos2(x, right.y - H / 2.0), vec2(w, H));
        // a cap: the face, a hairline edge and a slightly heavier lower edge, as printed keys look
        painter.rect_filled(r.translate(vec2(0.0, 1.0)), crate::theme::RADIUS_SMALL, theme.border_strong);
        painter.rect(r, crate::theme::RADIUS_SMALL, theme.surface, Stroke::new(1.0, theme.border_strong), egui::StrokeKind::Inside);
        painter.galley(pos2(r.center().x - g.size().x / 2.0, r.center().y - g.size().y / 2.0), g, theme.text_dim);
        x += w + GAP;
    }
    total
}

/// The small "x" of a close button, drawn thin and round-capped so it reads as a control rather than a cross.
pub fn close_glyph(painter: &Painter, center: Pos2, half: f32, color: Color32) {
    let s = Stroke::new(1.25, color);
    painter.line_segment([center + vec2(-half, -half), center + vec2(half, half)], s);
    painter.line_segment([center + vec2(-half, half), center + vec2(half, -half)], s);
}

/// A flat square button holding one icon. Dim until hovered.
pub fn button(ui: &mut Ui, theme: &Theme, icon: Icon, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hot = resp.hovered() || resp.has_focus();
        if hot {
            ui.painter().rect_filled(rect, 6.0, if resp.is_pointer_button_down_on() { theme.surface_pressed } else { theme.surface_hover });
        }
        paint(ui.painter(), icon, rect.shrink(6.0), if hot { theme.text } else { theme.text_dim });
    }
    resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(tip)
}

/// The main action of a bar: a push button with an accent icon and a label, raised slightly off the chrome.
pub fn primary_button(ui: &mut Ui, theme: &Theme, icon: Icon, label: &str, tip: &str) -> Response {
    let font = FontId::new(13.0, strong_family());
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, theme.text);
    let size = vec2(9.0 + 12.0 + 6.0 + galley.size().x + 11.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if resp.is_pointer_button_down_on() {
            theme.surface_pressed
        } else if resp.hovered() {
            theme.surface_hover
        } else {
            theme.surface
        };
        // A one-point shadow under the button, as native push buttons have.
        ui.painter().rect_filled(rect.translate(vec2(0.0, 0.5)).expand(0.5), 6.5, theme.shadow);
        ui.painter().rect(rect, 6.0, fill, Stroke::new(1.0, if theme.dark { theme.surface_pressed } else { theme.border_strong }), egui::StrokeKind::Inside);
        if resp.has_focus() {
            ui.painter().rect_stroke(rect.expand(2.0), 8.0, Stroke::new(3.0, theme.focus_ring()), egui::StrokeKind::Middle);
        }
        paint(ui.painter(), icon, Rect::from_center_size(pos2(rect.left() + 9.0 + 6.0, rect.center().y), vec2(12.0, 12.0)), theme.accent);
        ui.painter().galley(pos2(rect.left() + 9.0 + 12.0 + 6.0, rect.center().y - galley.size().y / 2.0), galley, theme.text);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(tip)
}

#[cfg(test)]
mod tests {
    use super::split_keys;

    #[test]
    fn shortcuts_split_into_keys() {
        assert_eq!(split_keys("⇧⌘S"), ["⇧", "⌘", "S"]);
        assert_eq!(split_keys("Ctrl+Shift+Enter"), ["Ctrl", "Shift", "Enter"]);
        assert_eq!(split_keys("F1"), ["F1"]);
        assert_eq!(split_keys("+"), ["+"]);
        assert!(split_keys("").is_empty());
    }
}
