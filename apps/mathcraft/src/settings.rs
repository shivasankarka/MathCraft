//! Settings that outlive a session: Vim keys, appearance and the accent colour, in `settings.json` in the config
//! folder, plus an optional `theme.json` of colour overrides. Both are plain JSON a person can edit; unknown or
//! malformed entries are ignored so a typo never stops the app from starting.

use crate::theme::Theme;
use egui::{Color32, ThemePreference};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub vim: bool,
    pub appearance: ThemePreference,
    /// A preset name (`purple`) or a `#rrggbb` colour; `None` keeps the theme's own accent.
    pub accent: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { vim: false, appearance: ThemePreference::System, accent: None }
    }
}

/// Built-in accent colours: name, dark theme, light theme.
pub const ACCENTS: [(&str, [u8; 3], [u8; 3]); 6] = [
    ("blue", [84, 150, 255], [20, 104, 225]),
    ("purple", [168, 128, 255], [112, 64, 220]),
    ("pink", [255, 120, 170], [206, 40, 110]),
    ("orange", [255, 150, 70], [200, 92, 0]),
    ("green", [80, 200, 130], [20, 130, 72]),
    ("graphite", [170, 176, 190], [84, 90, 104]),
];

impl Settings {
    /// Read settings from JSON text; anything missing or invalid keeps its default.
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let Ok(Value::Object(m)) = serde_json::from_str::<Value>(text) else { return s };
        if let Some(Value::Bool(v)) = m.get("vim") {
            s.vim = *v;
        }
        s.appearance = match m.get("appearance").and_then(Value::as_str) {
            Some("light") => ThemePreference::Light,
            Some("dark") => ThemePreference::Dark,
            _ => ThemePreference::System,
        };
        s.accent = m.get("accent").and_then(Value::as_str).filter(|a| accent_colors(a).is_some()).map(str::to_owned);
        s
    }

    pub fn to_json(&self) -> String {
        let appearance = match self.appearance {
            ThemePreference::Light => "light",
            ThemePreference::Dark => "dark",
            ThemePreference::System => "system",
        };
        let mut m = Map::new();
        m.insert("vim".into(), json!(self.vim));
        m.insert("appearance".into(), json!(appearance));
        if let Some(a) = &self.accent {
            m.insert("accent".into(), json!(a));
        }
        serde_json::to_string_pretty(&Value::Object(m)).unwrap_or_default() + "\n"
    }

    pub fn load(dir: &Path) -> Settings {
        std::fs::read_to_string(dir.join("settings.json")).map(|t| Settings::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("settings.json"), self.to_json())
    }
}

/// The accent for the dark and light themes: a preset name or `#rrggbb` (the same colour in both).
pub fn accent_colors(spec: &str) -> Option<(Color32, Color32)> {
    if let Some(c) = parse_hex(spec) {
        return Some((c, c));
    }
    let (_, d, l) = ACCENTS.iter().find(|(n, ..)| *n == spec)?;
    Some((Color32::from_rgb(d[0], d[1], d[2]), Color32::from_rgb(l[0], l[1], l[2])))
}

pub fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// Where settings live: the platform's config folder, or `MATHCRAFT_CONFIG_DIR` when set.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("MATHCRAFT_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support/MathCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("MathCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home().map(|h| h.join(".config"))).map(|c| c.join("mathcraft"))
    }
}

/// The two themes with the user's `theme.json` (`{"dark": {"bg": "#101014"}, "light": {...}}`) and accent applied.
pub fn themes(dir: Option<&Path>, accent: Option<&str>) -> (Theme, Theme) {
    let file: Value = dir.and_then(|d| std::fs::read_to_string(d.join("theme.json")).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let (mut dark, mut light) = (Theme::dark(), Theme::light());
    if let Some(Value::Object(o)) = file.get("dark") {
        dark.override_colors(o);
    }
    if let Some(Value::Object(o)) = file.get("light") {
        light.override_colors(o);
    }
    if let Some((d, l)) = accent.and_then(accent_colors) {
        dark.accent = d;
        light.accent = l;
    }
    (dark, light)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_ignores_junk() {
        let s = Settings { vim: true, appearance: ThemePreference::Dark, accent: Some("purple".into()) };
        assert_eq!(Settings::parse(&s.to_json()), s);
        assert_eq!(Settings::parse("not json"), Settings::default());
        assert_eq!(Settings::parse(r##"{"vim": 3, "appearance": "sepia", "accent": "#12"}"##), Settings::default());
        assert_eq!(Settings::parse(r##"{"accent": "#ff8800"}"##).accent.as_deref(), Some("#ff8800"));
    }

    #[test]
    fn hex_colours() {
        assert_eq!(parse_hex("#ff8000"), Some(Color32::from_rgb(255, 128, 0)));
        assert_eq!(parse_hex("ff8000"), None);
        assert_eq!(parse_hex("#ff80"), None);
        assert_eq!(parse_hex("#gg0000"), None);
        assert!(accent_colors("green").is_some() && accent_colors("teal").is_none());
    }

    #[test]
    fn theme_file_overrides_colours() {
        let dir = std::env::temp_dir().join(format!("mathcraft-theme-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("theme.json"), r##"{"dark": {"bg": "#101014", "nonsense": "#fff", "text": "oops"}}"##);
        let (dark, light) = themes(Some(&dir), Some("orange"));
        assert_eq!(dark.bg, Color32::from_rgb(16, 16, 20));
        assert_eq!(dark.text, Theme::dark().text, "an invalid colour is ignored");
        assert_eq!(light.bg, Theme::light().bg);
        assert_eq!(dark.accent, Color32::from_rgb(255, 150, 70));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saves_to_a_folder() {
        let dir = std::env::temp_dir().join(format!("mathcraft-settings-{}", std::process::id()));
        let s = Settings { vim: true, ..Settings::default() };
        assert!(s.save(&dir).is_ok());
        assert_eq!(Settings::load(&dir), s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
