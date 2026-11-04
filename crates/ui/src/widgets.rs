//! Small reusable UI building blocks.

use egui::{
    Align2, Color32, CornerRadius, FontId, Frame, Margin, Painter, Pos2, Rect, Response, RichText, Stroke, Ui, Vec2,
    pos2, vec2,
};
use egui_material_icons::MaterialIcon;

use crate::theme;

/// Icon codepoint for inline use in labels (`format!("{} Open", icon(...))`).
pub(crate) fn icon(icon: MaterialIcon) -> &'static str {
    icon.codepoint
}

/// A rounded surface card.
pub(crate) fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = theme::palette(ui);
    Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(14))
        .show(ui, add)
        .inner
}

/// Small rounded label, e.g. a modality badge.
pub(crate) fn chip(ui: &mut Ui, text: &str, color: Color32) -> Response {
    Frame::new()
        .fill(color.gamma_multiply(0.18))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.55)))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| ui.label(RichText::new(text).small().color(color)))
        .response
}

pub(crate) fn section_title(ui: &mut Ui, text: &str) {
    let p = theme::palette(ui);
    ui.add_space(4.0);
    ui.label(RichText::new(text.to_uppercase()).small().color(p.text_weak).strong());
}

/// Primary call-to-action button.
pub(crate) fn primary_button(ui: &mut Ui, text: &str, min_size: Vec2) -> Response {
    let p = theme::palette(ui);
    ui.add(
        egui::Button::new(RichText::new(text).color(p.accent_text).size(15.0))
            .fill(p.accent)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(8))
            .min_size(min_size),
    )
}

/// Text with a dark halo so it stays legible over any image.
pub(crate) fn overlay_text(
    painter: &Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    size: f32,
    color: Color32,
) -> Rect {
    let font = FontId::proportional(size);
    for d in [vec2(1.0, 1.0), vec2(-1.0, 1.0), vec2(1.0, -1.0), vec2(-1.0, -1.0)] {
        painter.text(pos + d, anchor, text, font.clone(), Color32::from_black_alpha(170));
    }
    painter.text(pos, anchor, text, font, color)
}

/// Stacked overlay lines anchored at a viewport corner.
pub(crate) fn overlay_block(painter: &Painter, corner: Pos2, anchor: Align2, lines: &[String], size: f32) {
    let line_height = size + 3.0;
    let n = lines.len() as f32;
    for (i, line) in lines.iter().enumerate() {
        let dy =
            if anchor.y() == egui::Align::Max { -(n - 1.0 - i as f32) * line_height } else { i as f32 * line_height };
        overlay_text(painter, corner + vec2(0.0, dy), anchor, line, size, theme::OVERLAY_TEXT);
    }
}

/// The SumMRIse mark (an axial head slice with a scan line), drawn with
/// vector shapes so it is crisp at any size. Matches the app icon.
pub(crate) fn paint_logo(painter: &Painter, rect: Rect) {
    let s = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(s, s));
    painter.rect_filled(r, CornerRadius::same((s * 0.22) as u8), Color32::from_rgb(0x17, 0x22, 0x3A));
    let c = r.center() + vec2(0.0, s * 0.01);
    let ellipse = |rx: f32, ry: f32, color: Color32| {
        painter.add(egui::Shape::ellipse_filled(c, vec2(rx, ry), color));
    };
    ellipse(s * 0.256, s * 0.312, Color32::from_rgb(0xE6, 0xED, 0xF7));
    ellipse(s * 0.230, s * 0.287, Color32::from_rgb(0x6F, 0x98, 0xD0));
    let dark = Color32::from_rgb(0x13, 0x20, 0x3A);
    for dx in [-0.045f32, 0.045] {
        painter.add(egui::Shape::ellipse_filled(c + vec2(dx * s, -s * 0.016), vec2(s * 0.03, s * 0.09), dark));
    }
    let stroke = Stroke::new((s * 0.014).max(1.0), Color32::from_rgb(0xF2, 0x5C, 0x54));
    painter.line_segment([pos2(c.x - s * 0.345, c.y), pos2(c.x + s * 0.345, c.y)], stroke);
}

/// A key/value grid with weak labels.
pub(crate) fn kv_grid(ui: &mut Ui, id: impl egui::AsIdSalt, rows: &[(String, String)]) {
    let p = theme::palette(ui);
    egui::Grid::new(id).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        for (label, value) in rows {
            ui.label(RichText::new(label).color(p.text_weak));
            ui.add(egui::Label::new(value).wrap().selectable(true));
            ui.end_row();
        }
    });
}
