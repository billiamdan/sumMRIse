//! Small reusable UI building blocks.

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Id, Layout, Margin, Painter, Pos2, Rect, Response, RichText,
    Stroke, Ui, Vec2, pos2, vec2,
};
use egui_material_icons::MaterialIcon;
use summrise_i18n::tr_ctx;

use crate::theme;

/// Name of the primary shortcut modifier: "⌘" on macOS, "Ctrl" elsewhere.
/// (⌘ is only shown on macOS, whose UI font has the glyph.)
pub(crate) fn command_key() -> &'static str {
    // Translators: the names of modifier keys as printed on keyboards of your language (German: Strg).
    if cfg!(target_os = "macos") { "⌘" } else { tr_ctx!("key name", "Ctrl") }
}

/// Platform shortcut label: "⌘B" on macOS, "Ctrl+B" on Windows and Linux.
pub(crate) fn shortcut(key: &str) -> String {
    if cfg!(target_os = "macos") { format!("⌘{key}") } else { format!("{}+{key}", command_key()) }
}

/// Name of the Alt modifier key: "Option" on macOS, "Alt" elsewhere.
pub(crate) fn alt_key() -> &'static str {
    if cfg!(target_os = "macos") { tr_ctx!("key name", "Option") } else { tr_ctx!("key name", "Alt") }
}

/// Shortcut label with the Alt modifier: "Option+R" on macOS, "Alt+R" elsewhere.
pub(crate) fn alt_shortcut(key: &str) -> String {
    format!("{}+{key}", alt_key())
}

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

/// Width of the widest of `lines` in the overlay font of `size`.
pub(crate) fn text_width(painter: &Painter, lines: &[String], size: f32) -> f32 {
    let font = FontId::proportional(size);
    lines
        .iter()
        .map(|line| painter.layout_no_wrap(line.clone(), font.clone(), Color32::WHITE).size().x)
        .fold(0.0, f32::max)
}

/// `lines`, each shortened with "…" to at most `max_width` points in the
/// overlay font of `size` (unchanged when `max_width` is infinite).
pub(crate) fn elide_lines(painter: &Painter, lines: Vec<String>, size: f32, max_width: f32) -> Vec<String> {
    if !max_width.is_finite() {
        return lines;
    }
    let font = FontId::proportional(size);
    let width = |text: String| painter.layout_no_wrap(text, font.clone(), Color32::WHITE).size().x;
    lines
        .into_iter()
        .map(|line| {
            if width(line.clone()) <= max_width {
                return line;
            }
            // The longest start of the line that fits together with the ellipsis.
            let chars: Vec<char> = line.chars().collect();
            let (mut lo, mut hi) = (0, chars.len());
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                let candidate: String = chars[..mid].iter().chain(std::iter::once(&'…')).collect();
                if width(candidate) <= max_width {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            let kept: String = chars[..lo].iter().collect();
            format!("{}…", kept.trim_end())
        })
        .collect()
}

/// How wide a block of text in the left and one in the right corner of the
/// same rows may be, in a row `available` points wide: as wide as they are
/// when both fit; else the narrower keeps its width if it takes at most half
/// and the other gets the rest; else each gets half.
pub(crate) fn share_row(left: f32, right: f32, available: f32) -> (f32, f32) {
    let half = (available / 2.0).max(0.0);
    if left + right <= available {
        (f32::INFINITY, f32::INFINITY)
    } else if left <= half {
        (f32::INFINITY, available - left)
    } else if right <= half {
        (available - right, f32::INFINITY)
    } else {
        (half, half)
    }
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

/// Lays out `add` from left to right as one piece that a wrapping layout
/// moves to the next row as a whole, so that a label stays with its control:
/// the piece is given the width it took in the previous frame (`name` tells
/// the pieces apart).
pub(crate) fn keep_together<R>(ui: &mut Ui, name: &'static str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let id = Id::unique("keep together").with(name);
    let known = ui.ctx().data(|d| d.get_temp::<f32>(id));
    // Never wider than a whole row: a piece wider than that gets a row of its own and
    // its widgets the row's width (to shrink into, where they can).
    let size = vec2(known.unwrap_or(0.0).min(ui.max_rect().width()), ui.spacing().interact_size.y);
    let inner = ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), add);
    let width = inner.response.rect.width();
    if known.is_none_or(|known| (known - width).abs() > 0.5) {
        ui.ctx().data_mut(|d| d.insert_temp(id, width));
        ui.ctx().request_discard("toolbar piece width");
    }
    inner.inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_blocks_share_a_row() {
        // Both fit: no limit.
        assert_eq!(share_row(100.0, 80.0, 200.0), (f32::INFINITY, f32::INFINITY));
        // A narrow block keeps its width; the other gets the rest.
        assert_eq!(share_row(60.0, 300.0, 200.0), (f32::INFINITY, 140.0));
        assert_eq!(share_row(300.0, 60.0, 200.0), (140.0, f32::INFINITY));
        // Both wide: half each.
        assert_eq!(share_row(300.0, 300.0, 200.0), (100.0, 100.0));
    }

    #[test]
    fn a_line_too_long_is_shortened_with_an_ellipsis() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter();
            let lines = vec!["S1: CT CHEST 2.5mm (synthetic)".to_owned(), "W 400".to_owned()];
            let full = text_width(painter, &lines[..1], 12.0);
            let short = elide_lines(painter, lines.clone(), 12.0, full / 2.0);
            assert!(short[0].ends_with('…') && short[0].chars().count() < lines[0].chars().count(), "{short:?}");
            assert!(text_width(painter, &short[..1], 12.0) <= full / 2.0, "{short:?}");
            assert_eq!(short[1], "W 400", "a line that fits stays as it is");
            assert_eq!(elide_lines(painter, lines.clone(), 12.0, f32::INFINITY), lines);
            // Nothing fits: the ellipsis alone.
            assert_eq!(elide_lines(painter, vec!["Mediastinum".to_owned()], 12.0, 1.0), ["…"]);
        });
        // No renderer takes the font texture here.
        output.textures_delta.clear();
    }
}
