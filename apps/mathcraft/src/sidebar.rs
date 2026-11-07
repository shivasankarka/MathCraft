//! The folder sidebar: the notebooks of the opened folder as a tree.

use crate::fonts;
use crate::theme::{self, Theme};
use crate::workspace::Node;
use eframe::egui::{self, RichText, Sense, Stroke, vec2};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub enum SideAction {
    Open(PathBuf),
    NewFile,
    Refresh,
    OpenFolder,
}

/// What the sidebar needs to know about the open documents to mark them.
pub struct OpenFile<'a> {
    pub path: &'a Path,
    pub dirty: bool,
    pub active: bool,
}

const ROW: f32 = theme::ROW;
const HEADER: f32 = 28.0;

pub fn sidebar(ui: &mut egui::Ui, theme: &Theme, folder: &Path, tree: &[Node], collapsed: &mut HashSet<PathBuf>, open: &[OpenFile<'_>]) -> Option<SideAction> {
    let mut action = None;
    ui.add_space(6.0);
    // The header: folder name on the left, its buttons on the right. The buttons are laid out first so a long name
    // is truncated instead of pushing them out of the panel.
    let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER), Sense::hover());
    let mut header_ui = ui.new_child(egui::UiBuilder::new().max_rect(header.shrink2(vec2(4.0, 0.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
    header_ui.spacing_mut().item_spacing.x = 1.0;
    if small_button(&mut header_ui, theme, Glyph::Refresh, "Refresh the file list").clicked() {
        action = Some(SideAction::Refresh);
    }
    if small_button(&mut header_ui, theme, Glyph::Plus, "New notebook in this folder").clicked() {
        action = Some(SideAction::NewFile);
    }
    if small_button(&mut header_ui, theme, Glyph::More, "Open another folder…").clicked() {
        action = Some(SideAction::OpenFolder);
    }
    header_ui.add_space(6.0);
    let name = folder.file_name().map_or_else(|| folder.display().to_string(), |n| n.to_string_lossy().to_string());
    header_ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(name).font(egui::FontId::new(theme::TEXT_SMALL, fonts::strong_family())).color(theme.text_dim)).truncate())
            .on_hover_text(folder.display().to_string());
    });
    ui.add_space(2.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        if tree.is_empty() {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(RichText::new("No notebooks in this folder").size(theme::TEXT_SMALL).color(theme.text_dim));
                    ui.label(RichText::new("Create one with the + button above.").size(theme::TEXT_CAPTION).color(theme.text_faint));
                });
            });
        }
        rows(ui, theme, tree, 0, collapsed, open, &mut action);
        ui.add_space(8.0);
    });
    action
}

#[derive(Clone, Copy)]
enum Glyph {
    Plus,
    Refresh,
    More,
}

fn small_button(ui: &mut egui::Ui, theme: &Theme, glyph: Glyph, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
    if resp.hovered() || resp.has_focus() {
        ui.painter().rect_filled(rect, theme::RADIUS, if resp.is_pointer_button_down_on() { theme.surface_pressed } else { theme.surface_hover });
    }
    let color = if resp.hovered() { theme.text } else { theme.text_dim };
    let stroke = Stroke::new(1.3, color);
    let (p, c) = (ui.painter(), rect.center());
    match glyph {
        Glyph::Plus => {
            p.line_segment([c + vec2(-4.5, 0.0), c + vec2(4.5, 0.0)], stroke);
            p.line_segment([c + vec2(0.0, -4.5), c + vec2(0.0, 4.5)], stroke);
        }
        Glyph::More => {
            for dx in [-4.5, 0.0, 4.5] {
                p.circle_filled(c + vec2(dx, 0.0), 1.2, color);
            }
        }
        Glyph::Refresh => {
            // three quarters of a circle with an arrow head at the open end
            let r = 4.8;
            let pts: Vec<egui::Pos2> = (0..=18)
                .map(|i| {
                    let a = -0.2 + i as f32 / 18.0 * std::f32::consts::PI * 1.5;
                    c + vec2(a.cos(), a.sin()) * r
                })
                .collect();
            p.add(egui::Shape::line(pts.clone(), stroke));
            if let Some(end) = pts.last() {
                p.line_segment([*end, *end + vec2(3.0, -0.5)], stroke);
                p.line_segment([*end, *end + vec2(0.5, -3.2)], stroke);
            }
        }
    }
    resp.on_hover_text(tip)
}

fn chevron(painter: &egui::Painter, center: egui::Pos2, down: bool, color: egui::Color32) {
    let s = Stroke::new(1.3, color);
    if down {
        painter.line_segment([center + vec2(-3.0, -1.5), center + vec2(0.0, 1.5)], s);
        painter.line_segment([center + vec2(0.0, 1.5), center + vec2(3.0, -1.5)], s);
    } else {
        painter.line_segment([center + vec2(-1.5, -3.0), center + vec2(1.5, 0.0)], s);
        painter.line_segment([center + vec2(1.5, 0.0), center + vec2(-1.5, 3.0)], s);
    }
}

fn rows(
    ui: &mut egui::Ui,
    theme: &Theme,
    nodes: &[Node],
    depth: usize,
    collapsed: &mut HashSet<PathBuf>,
    open: &[OpenFile<'_>],
    action: &mut Option<SideAction>,
) {
    for node in nodes {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
        let indent = 8.0 + depth as f32 * 14.0;
        match node {
            Node::Dir { path, name, children } => {
                let is_collapsed = collapsed.contains(path);
                if resp.hovered() {
                    ui.painter().rect_filled(rect.shrink2(vec2(4.0, 1.0)), theme::RADIUS, theme.surface_hover);
                }
                chevron(ui.painter(), egui::pos2(rect.left() + indent + 4.0, rect.center().y), !is_collapsed, theme.text_faint);
                ui.painter().text(
                    egui::pos2(rect.left() + indent + 14.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    name,
                    egui::FontId::new(theme::TEXT_UI, fonts::strong_family()),
                    theme.text_dim,
                );
                if resp.clicked() && !collapsed.remove(path) {
                    collapsed.insert(path.clone());
                }
                if !is_collapsed {
                    rows(ui, theme, children, depth + 1, collapsed, open, action);
                }
            }
            Node::File { path, name } => {
                let state = open.iter().find(|o| o.path == path.as_path());
                let active = state.is_some_and(|s| s.active);
                let row = rect.shrink2(vec2(4.0, 1.0));
                if active {
                    ui.painter().rect_filled(row, theme::RADIUS, theme.selected_row());
                } else if resp.hovered() {
                    ui.painter().rect_filled(row, theme::RADIUS, if resp.is_pointer_button_down_on() { theme.surface_pressed } else { theme.surface_hover });
                }
                let color = if active { theme.text } else { theme.text_dim };
                let font = egui::FontId::new(theme::TEXT_UI, egui::FontFamily::Proportional);
                let galley = ui.painter().layout_no_wrap(name.clone(), font, color);
                let x = rect.left() + indent + 14.0;
                let max = rect.right() - 22.0;
                ui.painter().with_clip_rect(egui::Rect::from_min_max(rect.min, egui::pos2(max, rect.max.y))).galley(
                    egui::pos2(x, rect.center().y - galley.size().y / 2.0),
                    galley,
                    color,
                );
                // open files carry a small mark: hollow when saved, filled while there are unsaved changes
                if let Some(s) = state {
                    let c = egui::pos2(rect.right() - 14.0, rect.center().y);
                    if s.dirty {
                        ui.painter().circle_filled(c, 3.25, theme.text_dim);
                    } else {
                        ui.painter().circle_stroke(c, 2.75, Stroke::new(1.0, theme.text_faint));
                    }
                }
                if resp.clicked() {
                    *action = Some(SideAction::Open(path.clone()));
                }
                resp.on_hover_text(path.display().to_string());
            }
        }
    }
}
