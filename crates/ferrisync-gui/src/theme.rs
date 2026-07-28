//! Visual tokens: survey-instrument panel (slate + amber caution), not generic dark/purple.

use egui::{Color32, Stroke, Style, Visuals};

pub const BG: Color32 = Color32::from_rgb(22, 26, 30);
pub const PANEL: Color32 = Color32::from_rgb(34, 40, 46);
pub const PANEL_EDGE: Color32 = Color32::from_rgb(52, 60, 68);
pub const TEXT: Color32 = Color32::from_rgb(230, 226, 214);
pub const MUTED: Color32 = Color32::from_rgb(140, 148, 156);
pub const AMBER: Color32 = Color32::from_rgb(232, 168, 56);
pub const AMBER_DIM: Color32 = Color32::from_rgb(120, 88, 28);
pub const GREEN: Color32 = Color32::from_rgb(90, 168, 120);
pub const RED: Color32 = Color32::from_rgb(200, 90, 80);
pub const INPUT_BG: Color32 = Color32::from_rgb(18, 21, 24);

pub fn apply(ctx: &egui::Context) {
    let mut style = Style::default();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);
    style.visuals = Visuals::dark();

    let mut v = style.visuals.clone();
    v.override_text_color = Some(TEXT);
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = INPUT_BG;
    v.faint_bg_color = BG;
    v.widgets.inactive.bg_fill = Color32::from_rgb(44, 52, 60);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_fill = Color32::from_rgb(58, 68, 78);
    v.widgets.active.bg_fill = AMBER_DIM;
    v.widgets.active.fg_stroke = Stroke::new(1.0, AMBER);
    v.selection.bg_fill = Color32::from_rgb(70, 55, 20);
    v.selection.stroke = Stroke::new(1.0, AMBER);
    v.window_stroke = Stroke::new(1.0, PANEL_EDGE);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, PANEL_EDGE);
    style.visuals = v;

    ctx.set_style(style);
}
