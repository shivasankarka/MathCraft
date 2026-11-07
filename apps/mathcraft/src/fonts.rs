//! Fonts: Inter for the interface, JetBrains Mono for code, STIX Two Math for typeset mathematics (all SIL OFL 1.1,
//! licences in `assets/fonts/`).

use egui::epaint::text::VariationCoords;
use egui::{FontData, FontDefinitions, FontFamily, FontId};
use std::sync::Arc;

const INTER: &[u8] = include_bytes!("../../../assets/fonts/InterVariable.ttf");
const JETBRAINS: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");
const STIX_MATH: &[u8] = include_bytes!("../../../assets/fonts/STIX2Math.otf");

pub fn math_family() -> FontFamily {
    FontFamily::Name("math".into())
}

/// Semibold interface text (headings, emphasis).
pub fn strong_family() -> FontFamily {
    FontFamily::Name("strong".into())
}

pub fn math_font(size: f32) -> FontId {
    FontId::new(size, math_family())
}

pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let regular = Arc::new(FontData::from_static(INTER));
    let semibold = FontData::from_static(INTER).tweak(egui::FontTweak { coords: VariationCoords::new([(b"wght", 600.0)]), ..Default::default() });
    defs.font_data.insert("inter".into(), regular);
    defs.font_data.insert("inter-semibold".into(), Arc::new(semibold));
    defs.font_data.insert("jetbrains-mono".into(), Arc::new(FontData::from_static(JETBRAINS)));
    defs.font_data.insert("stix-two-math".into(), Arc::new(FontData::from_static(STIX_MATH)));

    defs.families.entry(FontFamily::Proportional).or_default().insert(0, "inter".into());
    defs.families.entry(FontFamily::Monospace).or_default().insert(0, "jetbrains-mono".into());
    defs.families.insert(math_family(), vec!["stix-two-math".into(), "inter".into()]);
    defs.families.insert(strong_family(), vec!["inter-semibold".into(), "inter".into()]);
    ctx.set_fonts(defs);
}
