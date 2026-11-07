//! Colours and a few sizes for the whole app. A [`Theme`] is plain data: every colour the UI uses is a field, so a
//! custom theme is just another value (loading one from a file is planned). Built-in dark and light themes are provided.
//!
//! The look: a neutral, slightly cool grey scale; the window chrome (toolbar, status bar) is one step away from the
//! document so the page reads as paper; one accent (blue) used only for focus, selection and the primary action.

use egui::{Color32, CornerRadius, Shadow, Stroke, Theme as Mode};

/// Colours of the code editor's syntax highlighting.
#[derive(Clone)]
pub struct Syntax {
    pub text: Color32,
    pub keyword: Color32,
    pub constant: Color32,
    pub function: Color32,
    pub number: Color32,
    pub string: Color32,
    pub comment: Color32,
}

/// Colours of typeset mathematics, by role.
#[derive(Clone)]
pub struct MathColors {
    pub text: Color32,
    pub variable: Color32,
    pub number: Color32,
    pub operator: Color32,
    pub function: Color32,
    pub constant: Color32,
    pub delim: Color32,
}

#[derive(Clone)]
pub struct Theme {
    pub name: &'static str,
    pub dark: bool,
    /// The document (notebook page) background.
    pub bg: Color32,
    /// Toolbar and status bar.
    pub chrome: Color32,
    /// Hairline between chrome and document.
    pub separator: Color32,
    /// Popups, windows, floating tool strips.
    pub surface: Color32,
    /// Hovered rows and buttons.
    pub surface_hover: Color32,
    /// Pressed buttons.
    pub surface_pressed: Color32,
    /// Behind code and other monospace content.
    pub code_bg: Color32,
    /// Quiet outline (code cells at rest, plot frame).
    pub border: Color32,
    /// Outline of a code cell at rest (transparent when the fill alone separates it from the page).
    pub cell_outline: Color32,
    /// Outline of controls (buttons, hovered cells).
    pub border_strong: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub danger: Color32,
    /// Colour of drop shadows under popups and windows.
    pub shadow: Color32,
    /// Line colours of plot series, in order.
    pub plot: [Color32; 6],
    pub syntax: Syntax,
    pub math: MathColors,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Corner radius of controls (buttons, rows); cells and popups use [`RADIUS_LARGE`]; keycaps and badges [`RADIUS_SMALL`].
pub const RADIUS: u8 = 6;
pub const RADIUS_LARGE: u8 = 8;
pub const RADIUS_SMALL: u8 = 4;

/// The interface type scale (points). Document text and code sizes live in `configure_style`.
pub const TEXT_CAPTION: f32 = 11.5;
pub const TEXT_SMALL: f32 = 12.0;
pub const TEXT_UI: f32 = 13.0;
pub const TEXT_TITLE: f32 = 14.0;

/// Height of a row in lists (sidebar, palette uses [`ROW_LARGE`]).
pub const ROW: f32 = 26.0;
pub const ROW_LARGE: f32 = 32.0;

/// Space kept free at the left of the toolbar for the macOS window buttons when the title bar is unified.
pub const TRAFFIC_LIGHTS_INSET: f32 = 76.0;

impl Theme {
    pub fn dark() -> Theme {
        let text = rgb(228, 229, 232);
        Theme {
            name: "Dark",
            dark: true,
            bg: rgb(30, 31, 34),
            chrome: rgb(37, 38, 42),
            separator: rgb(15, 15, 17),
            surface: rgb(44, 45, 50),
            surface_hover: rgb(50, 52, 57),
            surface_pressed: rgb(60, 62, 68),
            code_bg: rgb(37, 38, 42),
            border: rgb(46, 47, 52),
            cell_outline: rgb(44, 45, 50),
            border_strong: rgb(66, 68, 75),
            text,
            text_dim: rgb(160, 163, 171),
            text_faint: rgb(112, 116, 124),
            accent: rgb(84, 150, 255),
            ok: rgb(80, 196, 128),
            warn: rgb(232, 178, 74),
            danger: rgb(250, 112, 108),
            shadow: Color32::from_black_alpha(110),
            plot: [rgb(98, 160, 255), rgb(240, 140, 90), rgb(96, 200, 150), rgb(206, 132, 230), rgb(234, 196, 82), rgb(110, 200, 220)],
            syntax: Syntax {
                text: rgb(218, 220, 225),
                keyword: rgb(208, 150, 236),
                constant: rgb(236, 176, 110),
                function: rgb(118, 178, 250),
                number: rgb(150, 208, 168),
                string: rgb(232, 150, 118),
                comment: rgb(118, 123, 132),
            },
            math: MathColors {
                text,
                variable: rgb(240, 241, 245),
                number: text,
                operator: rgb(170, 175, 186),
                function: text,
                constant: text,
                delim: rgb(150, 155, 166),
            },
        }
    }

    pub fn light() -> Theme {
        let text = rgb(29, 30, 34);
        Theme {
            name: "Light",
            dark: false,
            bg: rgb(255, 255, 255),
            chrome: rgb(246, 246, 248),
            separator: rgb(222, 223, 227),
            surface: rgb(255, 255, 255),
            surface_hover: rgb(238, 239, 242),
            surface_pressed: rgb(226, 228, 232),
            code_bg: rgb(246, 246, 248),
            border: rgb(232, 233, 237),
            cell_outline: rgb(240, 240, 243),
            border_strong: rgb(208, 210, 216),
            text,
            text_dim: rgb(92, 97, 108),
            text_faint: rgb(140, 145, 155),
            accent: rgb(20, 104, 225),
            ok: rgb(22, 140, 78),
            warn: rgb(168, 104, 0),
            danger: rgb(200, 38, 44),
            shadow: Color32::from_black_alpha(34),
            plot: [rgb(20, 104, 225), rgb(220, 98, 40), rgb(22, 150, 100), rgb(150, 70, 200), rgb(196, 146, 0), rgb(0, 150, 180)],
            syntax: Syntax {
                text: rgb(29, 30, 34),
                keyword: rgb(150, 50, 170),
                constant: rgb(176, 88, 0),
                function: rgb(24, 96, 186),
                number: rgb(18, 120, 88),
                string: rgb(178, 56, 34),
                comment: rgb(128, 133, 143),
            },
            math: MathColors {
                text,
                variable: rgb(14, 16, 22),
                number: text,
                operator: rgb(92, 98, 112),
                function: text,
                constant: text,
                delim: rgb(108, 114, 128),
            },
        }
    }

    /// A tinted background for banners: `color` at low strength over the document.
    pub fn tint(&self, color: Color32) -> Color32 {
        color.gamma_multiply(if self.dark { 0.13 } else { 0.08 })
    }

    /// The fill of the chosen row in a list (palette, completion, sidebar): the accent, quietly.
    pub fn selected_row(&self) -> Color32 {
        self.accent.gamma_multiply(if self.dark { 0.30 } else { 0.13 })
    }

    /// A filled marker such as a status pill: `color` strong enough to read as a shape, text stays legible on it.
    pub fn pill(&self, color: Color32) -> Color32 {
        color.gamma_multiply(if self.dark { 0.24 } else { 0.14 })
    }

    /// The soft halo drawn around a focused field, as macOS does.
    pub fn focus_ring(&self) -> Color32 {
        self.accent.gamma_multiply(if self.dark { 0.35 } else { 0.25 })
    }

    pub fn popup_shadow(&self) -> Shadow {
        Shadow { offset: [0, 6], blur: 18, spread: 0, color: self.shadow }
    }

    /// The deep shadow under windows and the command palette.
    pub fn window_shadow(&self) -> Shadow {
        Shadow { offset: [0, 12], blur: 36, spread: 0, color: self.shadow }
    }

    /// A tighter shadow for small floating strips (the cell tools).
    pub fn popup_shadow_small(&self) -> Shadow {
        Shadow { offset: [0, 2], blur: 8, spread: 0, color: self.shadow.gamma_multiply(0.8) }
    }

    /// Install this theme's visuals for its mode.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        let radius = CornerRadius::same(RADIUS);
        v.panel_fill = self.bg;
        v.window_fill = self.surface;
        v.extreme_bg_color = self.code_bg;
        v.text_edit_bg_color = Some(self.code_bg);
        v.faint_bg_color = self.surface_hover;
        v.code_bg_color = self.code_bg;
        v.window_stroke = Stroke::new(1.0, if self.dark { self.border_strong } else { self.border });
        v.window_corner_radius = CornerRadius::same(12);
        v.window_shadow = self.window_shadow();
        v.window_highlight_topmost = false;
        v.popup_shadow = self.popup_shadow();
        v.menu_corner_radius = CornerRadius::same(RADIUS_LARGE);
        v.hyperlink_color = self.accent;
        v.warn_fg_color = self.warn;
        v.error_fg_color = self.danger;
        v.override_text_color = None;
        v.selection.bg_fill = self.accent.gamma_multiply(if self.dark { 0.38 } else { 0.22 });
        v.selection.stroke = Stroke::new(1.0, self.accent);
        v.text_cursor.stroke = Stroke::new(2.0, self.accent);
        v.striped = false;
        v.indent_has_left_vline = false;
        v.interact_cursor = None;

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.surface;
        w.noninteractive.weak_bg_fill = self.surface;
        w.noninteractive.bg_stroke = Stroke::new(1.0, self.separator_soft());
        w.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        w.noninteractive.corner_radius = radius;

        w.inactive.bg_fill = self.surface_hover;
        w.inactive.weak_bg_fill = Color32::TRANSPARENT;
        w.inactive.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
        w.inactive.fg_stroke = Stroke::new(1.0, self.text);
        w.inactive.corner_radius = radius;

        w.hovered.bg_fill = self.surface_hover;
        w.hovered.weak_bg_fill = self.surface_hover;
        w.hovered.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
        w.hovered.fg_stroke = Stroke::new(1.0, self.text);
        w.hovered.corner_radius = radius;
        w.hovered.expansion = 0.0;

        w.active.bg_fill = self.surface_pressed;
        w.active.weak_bg_fill = self.surface_pressed;
        w.active.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
        w.active.fg_stroke = Stroke::new(1.0, self.text);
        w.active.corner_radius = radius;
        w.active.expansion = 0.0;

        w.open.bg_fill = self.surface_hover;
        w.open.weak_bg_fill = self.surface_hover;
        w.open.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
        w.open.fg_stroke = Stroke::new(1.0, self.text);
        w.open.corner_radius = radius;

        let mode = if self.dark { Mode::Dark } else { Mode::Light };
        ctx.set_visuals_of(mode, v);
    }

    /// Separators inside popups and menus: quieter than the chrome hairline.
    pub fn separator_soft(&self) -> Color32 {
        if self.dark { self.border_strong.gamma_multiply(0.8) } else { self.border }
    }
}
