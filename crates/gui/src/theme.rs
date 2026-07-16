//! Dark, high-density design tokens (GTO-Wizard-style: information density
//! over accessibility contrast ratios). Applied once at startup via
//! [`apply`].

use eframe::egui;
use egui::{Color32, FontFamily, FontId, Stroke};

pub const BG: Color32 = Color32::from_rgb(0x0f, 0x12, 0x16);
pub const PANEL: Color32 = Color32::from_rgb(0x16, 0x1b, 0x22);
pub const STROKE: Color32 = Color32::from_rgb(0x23, 0x2a, 0x33);
pub const TEXT: Color32 = Color32::from_rgb(0xd7, 0xdd, 0xe3);
pub const ACCENT: Color32 = Color32::from_rgb(0x2b, 0xb5, 0x97);

/// Semantic action colors used by the Results matrix (see
/// `matrix::action_color`).
pub const FOLD: Color32 = Color32::from_rgb(0x3a, 0x45, 0x40);
pub const CALL: Color32 = Color32::from_rgb(0x2b, 0xb5, 0x97);
pub const LIMP: Color32 = Color32::from_rgb(0x3f, 0x7a, 0xc9);
pub const RAISE_RAMP: [Color32; 3] = [
    Color32::from_rgb(0xe8, 0xb2, 0x3e),
    Color32::from_rgb(0xe0, 0x81, 0x3a),
    Color32::from_rgb(0xd9, 0x4a, 0x3a),
];
pub const ALL_IN: Color32 = Color32::from_rgb(0x8f, 0x2a, 0x2a);
pub const UNKNOWN: Color32 = Color32::from_rgb(0x55, 0x5d, 0x66);

pub fn apply(ctx: &egui::Context) {
    install_cjk_fallback_font(ctx);
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = PANEL;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = PANEL;
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = BG;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, STROKE);
    visuals.widgets.inactive.bg_fill = PANEL;
    visuals.widgets.inactive.weak_bg_fill = PANEL;
    visuals.widgets.hovered.bg_fill = STROKE;
    visuals.widgets.active.bg_fill = ACCENT.linear_multiply(0.6);
    visuals.selection.bg_fill = ACCENT.linear_multiply(0.5);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.window_stroke = Stroke::new(1.0, STROKE);
    style.visuals = visuals;

    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(6.0, 3.0);

    style.text_styles = [
        (
            egui::TextStyle::Small,
            FontId::new(10.0, FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Body,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Button,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Heading,
            FontId::new(15.0, FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Monospace,
            FontId::new(12.0, FontFamily::Monospace),
        ),
    ]
    .into();

    ctx.set_style_of(egui::Theme::Dark, style);
}

/// egui's bundled fonts have no CJK glyphs, so Japanese UI strings render
/// as tofu without a system fallback. Missing/unreadable fonts degrade to
/// the default (Latin-only) set rather than failing startup.
fn install_cjk_fallback_font(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        "C:/Windows/Fonts/meiryo.ttc",
        "C:/Windows/Fonts/YuGothM.ttc",
        "C:/Windows/Fonts/msgothic.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    ];
    for path in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "cjk-fallback".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            if let Some(list) = fonts.families.get_mut(&family) {
                list.push("cjk-fallback".to_owned());
            }
        }
        ctx.set_fonts(fonts);
        return;
    }
}

/// Tiny monospace font used for matrix cell hand labels (~9px).
pub fn matrix_label_font() -> FontId {
    FontId::new(9.0, FontFamily::Monospace)
}
