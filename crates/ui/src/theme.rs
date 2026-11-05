//! Design system: palette, typography, spacing and platform fonts.
//!
//! Dark is the default because image perception (contrast, windowing) is
//! best on a dark surround; the light theme is provided for the chrome only —
//! viewports always stay black.

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{
    Color32, Context, CornerRadius, FontData, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme, Ui, vec2,
};
use summrise_core::ViewKind;
use summrise_core::imaging::MprAxis;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub accent: Color32,
    pub accent_text: Color32,
    pub bg: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub warn: Color32,
    pub error: Color32,
}

pub(crate) const DARK: Palette = Palette {
    accent: Color32::from_rgb(0x4C, 0x9A, 0xFF),
    accent_text: Color32::WHITE,
    bg: Color32::from_rgb(0x0F, 0x11, 0x15),
    panel: Color32::from_rgb(0x16, 0x19, 0x1F),
    surface: Color32::from_rgb(0x1E, 0x22, 0x2A),
    surface_hover: Color32::from_rgb(0x27, 0x2C, 0x36),
    border: Color32::from_rgb(0x2B, 0x31, 0x3C),
    text: Color32::from_rgb(0xE6, 0xE9, 0xEE),
    text_weak: Color32::from_rgb(0x9A, 0xA3, 0xB2),
    warn: Color32::from_rgb(0xF5, 0xA5, 0x24),
    error: Color32::from_rgb(0xF0, 0x5D, 0x5E),
};

pub(crate) const LIGHT: Palette = Palette {
    accent: Color32::from_rgb(0x1F, 0x6F, 0xEB),
    accent_text: Color32::WHITE,
    bg: Color32::from_rgb(0xF4, 0xF6, 0xF9),
    panel: Color32::from_rgb(0xFB, 0xFC, 0xFD),
    surface: Color32::from_rgb(0xEE, 0xF1, 0xF5),
    surface_hover: Color32::from_rgb(0xE3, 0xE8, 0xEF),
    border: Color32::from_rgb(0xD9, 0xDE, 0xE6),
    text: Color32::from_rgb(0x1A, 0x1F, 0x29),
    text_weak: Color32::from_rgb(0x5C, 0x66, 0x75),
    warn: Color32::from_rgb(0xB4, 0x6B, 0x00),
    error: Color32::from_rgb(0xC8, 0x2F, 0x2F),
};

/// Viewport background (always black for image perception).
pub(crate) const VIEWPORT_BG: Color32 = Color32::BLACK;
/// Overlay text colour on images.
pub(crate) const OVERLAY_TEXT: Color32 = Color32::from_rgb(0xE8, 0xE8, 0xE8);

pub(crate) fn palette(ui: &Ui) -> Palette {
    if ui.visuals().dark_mode { DARK } else { LIGHT }
}

/// View colours follow the 3D Slicer convention (axial red, sagittal yellow,
/// coronal green) so crosshair lines can be matched to their views.
pub(crate) fn view_color(kind: ViewKind) -> Color32 {
    match kind {
        ViewKind::Stack => Color32::from_rgb(0x8A, 0xB4, 0xF8),
        ViewKind::Mpr(MprAxis::Axial) => Color32::from_rgb(0xF2, 0x5C, 0x54),
        ViewKind::Mpr(MprAxis::Coronal) => Color32::from_rgb(0x4C, 0xC3, 0x5A),
        ViewKind::Mpr(MprAxis::Sagittal) => Color32::from_rgb(0xF4, 0xC4, 0x30),
    }
}

pub(crate) const TITLE: &str = "title";
pub(crate) const SUBTITLE: &str = "subtitle";

pub(crate) fn install(ctx: &Context) {
    install_fonts(ctx);
    egui_material_icons::initialize(ctx);
    ctx.style_mut_of(Theme::Dark, |style| apply(style, &DARK, true));
    ctx.style_mut_of(Theme::Light, |style| apply(style, &LIGHT, false));
}

fn apply(style: &mut egui::Style, p: &Palette, dark: bool) {
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(18.0)),
        (TextStyle::Body, FontId::proportional(13.5)),
        (TextStyle::Button, FontId::proportional(13.5)),
        (TextStyle::Small, FontId::proportional(11.5)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
        (TextStyle::Name(TITLE.into()), FontId::proportional(30.0)),
        (TextStyle::Name(SUBTITLE.into()), FontId::proportional(15.5)),
    ]
    .into();

    let s = &mut style.spacing;
    s.item_spacing = vec2(8.0, 6.0);
    s.button_padding = vec2(10.0, 5.0);
    s.window_margin = Margin::same(16);
    s.menu_margin = Margin::same(6);
    s.interact_size = vec2(28.0, 26.0);
    s.combo_width = 140.0;
    s.slider_width = 120.0;
    s.indent = 16.0;

    let v = &mut style.visuals;
    *v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow =
        Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if dark { 120 } else { 40 }) };
    v.popup_shadow =
        Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(if dark { 100 } else { 30 }) };
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.surface;
    v.code_bg_color = p.surface;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.error;
    v.selection.bg_fill = p.accent.gamma_multiply(if dark { 0.45 } else { 0.3 });
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.striped = false;
    v.indent_has_left_vline = true;

    let radius = CornerRadius::same(6);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;
    for (state, fill) in
        [(&mut w.inactive, p.surface), (&mut w.hovered, p.surface_hover), (&mut w.open, p.surface_hover)]
    {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, p.border);
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.6));
    w.active.bg_fill = p.accent.gamma_multiply(0.8);
    w.active.weak_bg_fill = p.accent.gamma_multiply(0.8);
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.0, p.accent_text);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;
}

/// Uses the platform's own UI font when available (loaded at run time from
/// the OS, never redistributed); egui's bundled fonts remain as fallback and
/// eframe's system-font provider covers other scripts (CJK, Cyrillic…).
fn install_fonts(ctx: &Context) {
    let ui_fonts: &[(&str, u32)] = if cfg!(target_os = "macos") {
        &[("/System/Library/Fonts/SFNS.ttf", 0), ("/System/Library/Fonts/HelveticaNeue.ttc", 0)]
    } else if cfg!(windows) {
        &[("C:\\Windows\\Fonts\\segoeui.ttf", 0)]
    } else {
        &[
            ("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", 0),
            ("/usr/share/fonts/noto/NotoSans-Regular.ttf", 0),
            ("/usr/share/fonts/google-noto/NotoSans-Regular.ttf", 0),
            ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0),
            ("/usr/share/fonts/TTF/DejaVuSans.ttf", 0),
        ]
    };
    let mono_fonts: &[(&str, u32)] = if cfg!(target_os = "macos") {
        &[("/System/Library/Fonts/SFNSMono.ttf", 0), ("/System/Library/Fonts/Menlo.ttc", 0)]
    } else if cfg!(windows) {
        &[("C:\\Windows\\Fonts\\consola.ttf", 0)]
    } else {
        &[("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 0)]
    };
    for (name, candidates, family) in
        [("system-ui", ui_fonts, FontFamily::Proportional), ("system-mono", mono_fonts, FontFamily::Monospace)]
    {
        if let Some((path, index, bytes)) =
            candidates.iter().find_map(|(p, i)| std::fs::read(p).ok().map(|b| (*p, *i, b)))
        {
            log::debug!("using {path} as {name}");
            let mut data = FontData::from_owned(bytes);
            data.index = index;
            ctx.add_font(FontInsert::new(
                name,
                data,
                vec![InsertFontFamily { family, priority: FontPriority::Highest }],
            ));
        }
    }
}
