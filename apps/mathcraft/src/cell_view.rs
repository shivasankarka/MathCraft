//! One cell on screen: gutter, editor, slider row and drag handle.

use super::*;

pub(crate) fn editor_id(id: CellId) -> egui::Id {
    egui::Id::unique(("cell-source", id))
}

/// The thin "add a cell" strip between cells; its buttons appear while the pointer is over it.
pub(crate) fn add_row(ui: &mut egui::Ui, env: &mut Env, id: CellId) {
    let theme = env.theme;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), egui::Sense::hover());
    if !ui.rect_contains_pointer(rect) {
        return;
    }
    let mid = rect.center() + vec2(GUTTER / 2.0, 0.0);
    let line = egui::Rangef::new(rect.left() + GUTTER, rect.right());
    ui.painter().hline(line, mid.y, Stroke::new(1.0, theme.border));
    for (dx, label, kind) in [(-40.0, "Code", CellKind::Code), (40.0, "Text", CellKind::Markdown)] {
        let r = egui::Rect::from_center_size(mid + vec2(dx, 0.0), vec2(72.0, 22.0));
        let resp = ui.interact(r, ui.scope_id().with(("add", id, dx as i32)), egui::Sense::click());
        let hot = resp.hovered();
        ui.painter().rect(r, 11.0, if hot { theme.surface_hover } else { theme.bg }, Stroke::new(1.0, theme.border_strong), egui::StrokeKind::Inside);
        let ink = if hot { theme.text } else { theme.text_dim };
        icons::paint(
            ui.painter(),
            Icon::Plus,
            egui::Rect::from_center_size(r.left_center() + vec2(17.0, 0.0), vec2(9.0, 9.0)),
            if hot { theme.accent } else { ink },
        );
        ui.painter().text(r.left_center() + vec2(26.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.0), ink);
        let tip = if kind == CellKind::Code { "Add a code cell here" } else { "Add a text cell here" };
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked() {
            env.actions.push(Action::AddAfter(Some(id), kind));
        }
    }
}

/// Width reserved left of every cell for its number and run button.
pub(crate) const GUTTER: f32 = 46.0;
/// Horizontal padding inside the editor; outputs are indented by the same amount so they line up with the code.
pub(crate) const EDITOR_PAD_X: f32 = 12.0;

pub(crate) fn cell_ui(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, index: usize, nb: &Notebook) -> egui::Rect {
    let theme = env.theme;
    let is_code = cell.kind == CellKind::Code;
    let editing_text = !is_code && (env.editing_md.contains(&cell.id) || cell.source.trim().is_empty());
    let row = ui
        .horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ui.add_space(GUTTER);
            ui.vertical(|ui| {
                ui.set_min_width(ui.available_width());
                match cell.kind {
                    CellKind::Code => {
                        editor(ui, env, cell, true);
                        slider_row(ui, env, cell);
                        for p in &cell.problems {
                            ui.add_space(6.0);
                            banner(ui, theme, Icon::Warning, theme.warn, Some(p.code()), &p.message());
                        }
                        if let (CellState::Blocked, Some(note), true) = (cell.state, &cell.note, cell.problems.is_empty()) {
                            ui.add_space(6.0);
                            banner(ui, theme, Icon::Warning, theme.warn, None, note);
                        }
                        output_ui(ui, env, cell);
                    }
                    CellKind::Markdown => {
                        if editing_text {
                            if editor(ui, env, cell, false) {
                                env.editing_md.remove(&cell.id);
                            }
                        } else {
                            let r = markdown::show(ui, &cell.source, theme);
                            if r.double_clicked() {
                                env.editing_md.insert(cell.id);
                            }
                            r.on_hover_text("Double-click to edit");
                        }
                    }
                }
            });
        })
        .response
        .rect;

    let focused = ui.ctx().memory(|m| m.has_focus(editor_id(cell.id)));
    let dragging = dragged_cell(ui.ctx()).is_some_and(|d| d.id == cell.id);
    let active = focused || dragging || ui.rect_contains_pointer(row);
    if dragging {
        ui.painter().rect_stroke(row.expand2(vec2(2.0, 0.0)), theme::RADIUS_LARGE, Stroke::new(1.5, theme.accent), egui::StrokeKind::Inside);
    }

    // A rule beside the cell says when it needs attention; a cell that is up to date has none.
    if is_code && !matches!(cell.state, CellState::Fresh) {
        let x = row.left() + GUTTER - 9.0;
        let bar = egui::Rect::from_min_max(egui::pos2(x, row.top() + 4.0), egui::pos2(x + 2.0, row.bottom() - 4.0));
        let alpha = if cell.state == CellState::New { 0.4 } else { 1.0 };
        ui.painter().rect_filled(bar, 1.0, state_color(theme, cell.state).gamma_multiply(alpha));
    }

    // Gutter: the cell number, which becomes the run button while the cell is active.
    let slot = egui::Rect::from_min_size(row.min + vec2(4.0, 3.0), vec2(26.0, 26.0));
    if is_code {
        let resp = ui.interact(slot, ui.scope_id().with(("run", cell.id)), egui::Sense::click());
        if active || resp.hovered() {
            let hot = resp.hovered();
            if hot {
                ui.painter().rect_filled(slot, theme::RADIUS, if resp.is_pointer_button_down_on() { theme.surface_pressed } else { theme.surface_hover });
            }
            icons::paint(ui.painter(), Icon::Play, slot.shrink(8.0), if hot { theme.accent } else { theme.text_dim });
        } else {
            ui.painter().text(slot.right_center(), egui::Align2::RIGHT_CENTER, format!("{}", index + 1), egui::FontId::monospace(11.5), theme.text_faint);
        }
        let mut tip = String::from("Run this cell (⌘↵)");
        let defs: Vec<&str> = cell.defs().map(String::as_str).collect();
        if !defs.is_empty() {
            tip.push_str(&format!("\nDefines {}", defs.join(", ")));
        }
        let deps: Vec<String> = nb.dependencies(cell.id).iter().filter_map(|d| nb.index_of(*d)).map(|i| (i + 1).to_string()).collect();
        if !deps.is_empty() {
            tip.push_str(&format!("\nUses cell {}", deps.join(", ")));
        }
        let label = state_label(cell.state);
        if !label.is_empty() {
            tip.push_str(&format!("\nState: {label}"));
        }
        if let Some(o) = &cell.output {
            tip.push_str(&format!("\nRan in {}", format_elapsed(o.elapsed)));
        }
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked() {
            env.actions.push(Action::Run(cell.id));
        }
    }

    // Move and delete appear only while the cell is active.
    if active {
        let size = vec2(26.0 * 4.0 + 6.0, 26.0);
        let at = egui::Rect::from_min_size(egui::pos2(row.right() - size.x - 5.0, row.top() + 3.0), size);
        let mut tools = ui.new_child(egui::UiBuilder::new().max_rect(at).layout(egui::Layout::left_to_right(egui::Align::Center)));
        tools.spacing_mut().item_spacing.x = 2.0;
        let strip = at.expand(2.0);
        tools.painter().add(theme.popup_shadow_small().as_shape(strip, theme::RADIUS_LARGE));
        tools.painter().rect(strip, theme::RADIUS_LARGE, theme.surface, Stroke::new(1.0, theme.border_strong), egui::StrokeKind::Inside);
        let grip = icons::drag_handle(&mut tools, theme, "Drag to move this cell");
        if grip.drag_started() {
            set_dragged_cell(ui.ctx(), Some(Drag { id: cell.id, moved: false }));
        }
        if icons::button(&mut tools, theme, Icon::Up, "Move cell up").clicked() {
            env.actions.push(Action::Move(cell.id, -1));
        }
        if icons::button(&mut tools, theme, Icon::Down, "Move cell down").clicked() {
            env.actions.push(Action::Move(cell.id, 1));
        }
        if icons::button(&mut tools, theme, Icon::Close, "Delete this cell").clicked() {
            env.actions.push(Action::Delete(cell.id));
        }
    }
    row
}

/// The cell being dragged by its handle, and whether it has moved yet (the first move saves the undo point).
#[derive(Clone, Copy)]
pub(crate) struct Drag {
    id: CellId,
    moved: bool,
}

pub(crate) fn dragged_cell(ctx: &egui::Context) -> Option<Drag> {
    ctx.data(|d| d.get_temp::<Option<Drag>>(egui::Id::NULL)).flatten()
}

pub(crate) fn set_dragged_cell(ctx: &egui::Context, drag: Option<Drag>) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::NULL, drag));
}

/// While a cell's handle is held, move the cell past each neighbour the pointer crosses. `rows` are this frame's
/// `(top, bottom)` of every cell, in order.
pub(crate) fn drag_cells(ui: &egui::Ui, env: &mut Env, rows: &[(f32, f32)], nb: &Notebook) {
    let ctx = ui.ctx();
    let Some(mut drag) = dragged_cell(ctx) else { return };
    if !ctx.input(|i| i.pointer.any_down()) {
        set_dragged_cell(ctx, None);
        return;
    }
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let (Some(at), Some(cur)) = (ctx.pointer_latest_pos(), nb.index_of(drag.id)) else { return };
    if let Some(to) = tabs::drop_index(rows, cur, at.y) {
        env.actions.push(Action::Shift(drag.id, to as isize - cur as isize, !drag.moved));
        drag.moved = true;
        set_dragged_cell(ctx, Some(drag));
    }
}

/// The slider under a `name = slider(lo, hi, value)` cell. It rewrites the cell's value as it moves and re-runs the
/// cell (and its dependents), but while the kernel is still busy with the last value it waits instead of queueing
/// more runs; releasing the handle always runs the final value.
pub(crate) fn slider_row(ui: &mut egui::Ui, env: &mut Env, cell: &Cell) {
    let Some(spec) = slider::parse(&cell.source) else { return };
    ui.add_space(6.0);
    let mut value = spec.value;
    let r = ui
        .horizontal(|ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 150.0).clamp(120.0, 420.0);
            let mut s =
                egui::Slider::new(&mut value, spec.lo..=spec.hi).smart_aim(false).text(RichText::new(&spec.name).monospace().color(env.theme.text_faint));
            if let Some(step) = spec.step {
                s = s.step_by(step);
            }
            if spec.whole {
                s = s.integer();
            }
            ui.add(s)
        })
        .inner;
    if r.changed() {
        let text = spec.with_value(value);
        if text != cell.source {
            env.actions.push(Action::SetSource(cell.id, text));
            if !r.dragged() || !env.busy {
                env.actions.push(Action::Run(cell.id));
            }
        }
    }
    if r.drag_stopped() {
        env.actions.push(Action::Run(cell.id));
    }
}

pub(crate) fn format_elapsed(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 1.0 {
        "<1 ms".into()
    } else if ms < 1000.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", d.as_secs_f64())
    }
}

/// The text editor of a cell. Returns true when the user pressed Shift+Enter in a text cell.
pub(crate) fn editor(ui: &mut egui::Ui, env: &mut Env, cell: &Cell, code: bool) -> bool {
    let theme = env.theme;
    let id = editor_id(cell.id);
    let ctx = ui.ctx().clone();
    let mut text = cell.source.clone();
    let mut edited = false;
    let mut run_advance = false;
    let mut run_stay = false;

    let mut insertish = true;
    if ctx.memory(|m| m.has_focus(id)) {
        if env.vim {
            let r = vim_edit::drive(&ctx, id, &mut text);
            edited |= r.edited;
            env.actions.extend(r.commands.into_iter().map(Action::Command));
            *env.vim_status = r.status;
            insertish = r.mode == mathcraft_vim::Mode::Insert;
        }
        if code && insertish {
            edited |= handle_completion_keys(&ctx, id, &mut text, env.names);
        }
        let (shift_enter, cmd_enter, tab, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::SHIFT, Key::Enter),
                i.consume_key(Modifiers::COMMAND, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Tab),
                code && i.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        run_advance = shift_enter;
        run_stay = cmd_enter;
        if tab && code {
            if !next_placeholder(&ctx, id, &text) {
                edited |= insert_at_cursor(&ctx, id, &mut text, "    ");
            }
        } else if tab {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        if enter {
            let (before, _) = split_at_cursor(&ctx, id, &text);
            let line = before.rsplit('\n').next().unwrap_or("");
            let mut indent: String = line.chars().take_while(|c| *c == ' ').collect();
            if line.trim_end().ends_with(':') {
                indent.push_str("    ");
            }
            edited |= insert_at_cursor(&ctx, id, &mut text, &format!("\n{indent}"));
        }
    }

    // While the completion list is open, Escape closes it instead of leaving the editor.
    let list_open = code && insertish && ctx.memory(|m| m.has_focus(id)) && suggest(&ctx, id, &text, env.names).is_some();
    let keys = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: list_open || env.vim };
    let font = TextStyle::Monospace.resolve(ui.style());
    let syntax = theme.syntax.clone();
    let plain = theme.text;
    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let mut job = if code {
            highlight::highlight(buf.as_str(), font.clone(), &syntax, env.pretty)
        } else {
            egui::text::LayoutJob::simple(buf.as_str().to_owned(), font.clone(), plain, wrap_width)
        };
        job.wrap.max_width = wrap_width;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let hint = if code { "Type code, e.g. diff(x**2, x), then press Shift+Enter" } else { "Write text; # makes a heading" };
    let has_focus = ctx.memory(|m| m.has_focus(id));
    // Code sits on a soft panel; a text cell being edited gets the same panel so it reads as a field.
    let frame = egui::Frame::new()
        .fill(theme.code_bg)
        .stroke(Stroke::new(1.0, if has_focus { theme.accent } else { theme.cell_outline }))
        .corner_radius(theme::RADIUS_LARGE)
        .inner_margin(Margin::symmetric(EDITOR_PAD_X as i8, 9));
    let shown = frame.show(ui, |ui| {
        egui::TextEdit::multiline(&mut text)
            .id(id)
            .frame(egui::Frame::NONE)
            .desired_width(f32::INFINITY)
            .desired_rows(1)
            .lock_focus(true)
            .event_filter(keys)
            .hint_text(RichText::new(hint).color(theme.text_faint))
            .layouter(&mut layouter)
            .show(ui)
    });
    let out = shown.inner;
    if has_focus {
        // The macOS focus halo, outside the outline.
        let r = shown.response.rect.expand(2.5);
        ui.painter().rect_stroke(r, f32::from(theme::RADIUS_LARGE) + 2.5, Stroke::new(3.5, theme.focus_ring()), egui::StrokeKind::Middle);
    }
    edited |= out.response.changed();
    if env.focus == Some(cell.id) {
        out.response.request_focus();
    }
    if code
        && insertish
        && out.response.has_focus()
        && let Some(range) = out.cursor_range
    {
        let at = out.galley_pos + out.galley.pos_from_cursor(range.primary).left_bottom().to_vec2();
        edited |= completion_popup(&ctx, theme, id, &mut text, env.names, at);
    }
    if code && !edited {
        hover_help(&out, &text, theme);
    }
    if edited {
        env.actions.push(Action::SetSource(cell.id, text));
    }
    if run_advance {
        env.actions.push(if code { Action::RunAndAdvance(cell.id) } else { Action::Run(cell.id) });
    } else if run_stay && code {
        env.actions.push(Action::Run(cell.id));
    }
    run_advance && !code
}

/// A tooltip with the signature and summary of the built-in function under the pointer.
pub(crate) fn hover_help(out: &egui::text_edit::TextEditOutput, text: &str, theme: &Theme) {
    let Some(pos) = out.response.hover_pos() else { return };
    let at = usize::from(out.galley.cursor_from_pos(pos - out.galley_pos).index);
    if complete::in_comment_or_string(text, at) {
        return;
    }
    let Some((name, (sig, summary))) = complete::word_at(text, at).and_then(|w| complete::doc_for(&w).map(|d| (w, d))) else { return };
    egui::Response::clone(&out.response).on_hover_ui(|ui| {
        ui.set_max_width(380.0);
        ui.label(RichText::new(sig).monospace().color(theme.accent));
        ui.label(RichText::new(summary).color(theme.text_dim));
        ui.label(RichText::new(format!("help('{name}') shows more")).size(11.5).color(theme.text_faint));
    });
}
