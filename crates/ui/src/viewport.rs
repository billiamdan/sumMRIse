//! The interactive image viewport: display with correct physical aspect,
//! mouse/trackpad interaction, radiological overlays and MPR crosshair.

use egui::{Align2, Color32, CursorIcon, Id, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use summrise_core::dicom::{LoadedSeries, Series, Study, format};
use summrise_core::imaging::{DVec3, EdgeLabels};
use summrise_core::{Settings, Tool, ViewKind, ViewState, ViewTransform};

use crate::render::Rendered;
use crate::theme::{self, VIEWPORT_BG};
use crate::widgets::{overlay_block, overlay_text};

/// Scroll distance (points) per slice step.
const SCROLL_STEP: f32 = 18.0;
const OVERLAY_SIZE: f32 = 12.0;

pub(crate) struct ViewportInput<'a> {
    pub id: Id,
    pub kind: ViewKind,
    pub series: &'a LoadedSeries,
    pub meta: &'a Series,
    pub study: &'a Study,
    pub rendered: Option<&'a Rendered>,
    pub tool: Tool,
    pub settings: &'a Settings,
    pub is_active: bool,
    pub multi_view: bool,
    /// Show patient/study annotations (all views in single layout, first view otherwise).
    pub show_details: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Hover {
    pub pixel: (usize, usize),
    pub patient: Option<DVec3>,
    pub value: Option<f32>,
}

#[derive(Default)]
pub(crate) struct ViewportResult {
    pub hover: Option<Hover>,
    pub activated: bool,
}

pub(crate) fn show(ui: &mut Ui, rect: Rect, input: &ViewportInput<'_>, state: &mut ViewState) -> ViewportResult {
    let mut result = ViewportResult::default();
    let response = ui.interact(rect, input.id, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, VIEWPORT_BG);
    let accent = theme::palette(ui).accent;

    let Some(rendered) = input.rendered else {
        let message = match (input.kind, &input.series.volume_issue) {
            (ViewKind::Mpr(_), Some(reason)) => format!("Reformatting is not available:\n{reason}"),
            _ => "No image".to_owned(),
        };
        painter.text(rect.center(), Align2::CENTER_CENTER, message, egui::FontId::proportional(13.0), Color32::GRAY);
        return result;
    };

    let displayed = &rendered.displayed;
    let phys = displayed.physical_size();
    let transform = state.transform(input.kind);
    let fit = (rect.width() / phys.x).min(rect.height() / phys.y).max(1e-3);
    let img_rect =
        Rect::from_center_size(rect.center() + vec2(transform.pan[0], transform.pan[1]), phys * fit * transform.zoom);
    painter.image(rendered.texture.id(), img_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);

    let (pw, ph) = displayed.pixel_size();
    let to_pixel = |pos: Pos2| {
        (
            f64::from((pos.x - img_rect.min.x) / img_rect.width()) * pw as f64 - 0.5,
            f64::from((pos.y - img_rect.min.y) / img_rect.height()) * ph as f64 - 0.5,
        )
    };
    let to_screen = |x: f64, y: f64| {
        pos2(
            img_rect.min.x + ((x + 0.5) / pw as f64) as f32 * img_rect.width(),
            img_rect.min.y + ((y + 0.5) / ph as f64) as f32 * img_rect.height(),
        )
    };

    // ---- Interaction -------------------------------------------------------
    let (modifiers, scroll, zoom, pointer) =
        ui.input(|i| (i.modifiers, i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
    if response.hovered() {
        if (zoom - 1.0).abs() > f32::EPSILON {
            let anchor = pointer.map_or([0.0, 0.0], |p| [p.x - rect.center().x, p.y - rect.center().y]);
            state.transform_mut(input.kind).zoom_about(zoom, anchor);
        } else if scroll.y.abs() > 0.0 {
            let steps = ui.data_mut(|d| {
                let acc = d.get_temp_mut_or_default::<f32>(input.id.with("scroll"));
                *acc += scroll.y;
                let steps = (*acc / SCROLL_STEP).trunc();
                *acc -= steps * SCROLL_STEP;
                steps
            });
            if steps != 0.0 {
                state.scroll(input.kind, input.series, -(steps as i64));
            }
        }
        ui.ctx().set_cursor_icon(match input.tool {
            Tool::WindowLevel => CursorIcon::Crosshair,
            Tool::Pan => CursorIcon::Grab,
            Tool::Zoom => CursorIcon::ZoomIn,
        });
    }
    let delta = response.drag_delta();
    let primary = response.dragged_by(PointerButton::Primary);
    if delta != egui::Vec2::ZERO {
        result.activated = true;
        let pan =
            response.dragged_by(PointerButton::Middle) || (primary && (input.tool == Tool::Pan || modifiers.shift));
        if pan {
            let t = state.transform_mut(input.kind);
            t.pan = [t.pan[0] + delta.x, t.pan[1] + delta.y];
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if response.dragged_by(PointerButton::Secondary) || (primary && input.tool == Tool::WindowLevel) {
            if !input.series.is_color {
                state.drag_window(f64::from(delta.x), f64::from(delta.y), input.series);
            }
        } else if primary && input.tool == Tool::Zoom {
            let origin = response.interact_pointer_pos().unwrap_or(rect.center()) - delta;
            state
                .transform_mut(input.kind)
                .zoom_about((-delta.y * 0.01).exp(), [origin.x - rect.center().x, origin.y - rect.center().y]);
        }
    }
    if response.double_clicked() {
        *state.transform_mut(input.kind) = ViewTransform::default();
    } else if response.clicked() {
        result.activated = true;
        if let Some(pos) = response.interact_pointer_pos() {
            let (x, y) = to_pixel(pos);
            if let Some(p) = displayed.pixel_to_patient(x, y) {
                state.set_cursor(p, input.series);
            }
        }
    }
    if response.clicked_by(PointerButton::Secondary) || response.drag_started() {
        result.activated = true;
    }

    if let Some(pos) = pointer.filter(|p| img_rect.contains(*p) && rect.contains(*p)) {
        let (x, y) = to_pixel(pos);
        let (px, py) = (x.round().max(0.0) as usize, y.round().max(0.0) as usize);
        if px < pw && py < ph {
            result.hover = Some(Hover {
                pixel: (px, py),
                patient: displayed.pixel_to_patient(x, y),
                value: displayed.value_at(input.series, px, py),
            });
        }
    }

    // ---- Crosshair ----------------------------------------------------------
    if let ViewKind::Mpr(axis) = input.kind
        && input.settings.show_overlays
        && let Some((cx, cy)) = displayed.patient_to_pixel(state.cursor)
    {
        let c = to_screen(cx, cy);
        let (vertical, horizontal) = axis.crosshair_partners();
        let gap = 12.0;
        let vs = Stroke::new(1.0, theme::view_color(ViewKind::Mpr(vertical)).gamma_multiply(0.85));
        let hs = Stroke::new(1.0, theme::view_color(ViewKind::Mpr(horizontal)).gamma_multiply(0.85));
        let r = img_rect.intersect(rect);
        painter.line_segment([pos2(c.x, r.top()), pos2(c.x, c.y - gap)], vs);
        painter.line_segment([pos2(c.x, c.y + gap), pos2(c.x, r.bottom())], vs);
        painter.line_segment([pos2(r.left(), c.y), pos2(c.x - gap, c.y)], hs);
        painter.line_segment([pos2(c.x + gap, c.y), pos2(r.right(), c.y)], hs);
    }

    // ---- Overlays -----------------------------------------------------------
    if input.settings.show_overlays {
        draw_overlays(&painter, rect, img_rect, input, state, rendered, fit);
    }
    if input.multi_view {
        let color = theme::view_color(input.kind);
        let label = format!(
            "{}  {}/{}",
            input.kind.label().to_uppercase(),
            state.slice_index(input.kind, input.series) + 1,
            state.slice_count(input.kind, input.series)
        );
        overlay_text(&painter, pos2(rect.left() + 10.0, rect.top() + 8.0), Align2::LEFT_TOP, &label, 11.5, color);
        painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(2.0, color.gamma_multiply(0.8)));
        if input.is_active {
            painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(1.5, accent), StrokeKind::Inside);
        }
    }
    result
}

fn draw_overlays(
    painter: &egui::Painter,
    rect: Rect,
    img_rect: Rect,
    input: &ViewportInput<'_>,
    state: &ViewState,
    rendered: &Rendered,
    fit: f32,
) {
    let m = 10.0;
    let top = if input.multi_view { 28.0 } else { m };
    let compact = input.multi_view;
    let study = input.study;
    let series = input.series;

    // Top-left: patient (hidden in privacy mode).
    if input.show_details && !input.settings.hide_patient_details {
        let p = &study.patient;
        let mut lines = vec![p.name.as_deref().map(format::person_name).unwrap_or_else(|| "Unknown patient".into())];
        if let Some(id) = p.id.as_ref().filter(|_| !compact) {
            lines.push(format!("ID {id}"));
        }
        let demographics: Vec<String> = [
            p.birth_date.as_deref().and_then(format::date).map(|d| format!("Born {d}")),
            p.age.as_deref().and_then(format::age),
            p.sex.as_deref().map(format::sex),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !demographics.is_empty() && !compact {
            lines.push(demographics.join(" · "));
        }
        overlay_block(painter, rect.left_top() + vec2(m, top), Align2::LEFT_TOP, &lines, OVERLAY_SIZE);
    }

    // Top-right: study and series.
    let mut lines = Vec::new();
    if !compact {
        if let Some(inst) = &study.institution {
            lines.push(inst.clone());
        }
        lines.push(study.title());
        let date = study.date.as_deref().and_then(format::date);
        let time = study.time.as_deref().and_then(format::time);
        if let Some(d) = date {
            lines.push(time.map_or_else(|| d.clone(), |t| format!("{d} {t}")));
        }
    }
    lines.push(format!("{}{}", input.meta.number.map(|n| format!("S{n}: ")).unwrap_or_default(), input.meta.title()));
    if input.show_details {
        overlay_block(painter, rect.right_top() + vec2(-m, top), Align2::RIGHT_TOP, &lines, OVERLAY_SIZE);
    }

    // Bottom-left: position.
    let index = state.slice_index(input.kind, series);
    let count = state.slice_count(input.kind, series);
    let mut lines = Vec::new();
    match input.kind {
        ViewKind::Stack => {
            lines.push(format!("Image {} / {count}", index + 1));
            if let Some(info) = series.frames.get(index) {
                if let Some(n) = info.instance_number {
                    lines.push(format!(
                        "Instance {n}{}",
                        if info.frame > 0 { format!(" · frame {}", info.frame + 1) } else { String::new() }
                    ));
                }
                if let Some(plane) = info.plane {
                    let n = plane.normal();
                    let label = summrise_core::imaging::orientation_label(n);
                    lines.push(format!("{label} {} mm", one_decimal(plane.position.dot(n))));
                }
            }
        }
        ViewKind::Mpr(axis) => {
            lines.push(format!("{} {} / {count}", axis.label(), index + 1));
            lines.push(format!("{} mm", one_decimal(state.mpr_position(axis))));
            if let Some(slab) = state.slab {
                lines.push(format!("{} slab {:.0} mm", slab.mode.label(), slab.thickness_mm));
            }
        }
    }
    overlay_block(painter, rect.left_bottom() + vec2(m, -m), Align2::LEFT_BOTTOM, &lines, OVERLAY_SIZE);

    // Bottom-right: contrast and zoom.
    let mut lines = Vec::new();
    if !series.is_color {
        let units = series.units.as_deref().map(|u| format!(" {u}")).unwrap_or_default();
        lines.push(format!("W {:.0}  L {:.0}{units}", state.window.width, state.window.center));
        lines.push(state.preset.clone().unwrap_or_else(|| "Custom window".into()));
    }
    let zoom_pct = state.transform(input.kind).zoom * 100.0;
    lines.push(format!("Zoom {zoom_pct:.0}%{}", if state.invert { " · Inverted" } else { "" }));
    overlay_block(painter, rect.right_bottom() + vec2(-m, -m), Align2::RIGHT_BOTTOM, &lines, OVERLAY_SIZE);

    // Orientation labels at the image edges (kept inside the viewport).
    if let Some((right, down)) = rendered.displayed.screen_axes() {
        let labels = EdgeLabels::new(right, down);
        let r = img_rect.intersect(rect.shrink(4.0));
        if r.is_positive() {
            let size = 15.0;
            let color = Color32::from_rgb(0xFF, 0xD3, 0x6E);
            overlay_text(painter, pos2(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER, &labels.left, size, color);
            overlay_text(
                painter,
                pos2(r.right() - 4.0, r.center().y),
                Align2::RIGHT_CENTER,
                &labels.right,
                size,
                color,
            );
            overlay_text(painter, pos2(r.center().x, r.top() + 4.0), Align2::CENTER_TOP, &labels.top, size, color);
            overlay_text(
                painter,
                pos2(r.center().x, (r.bottom() - 4.0).min(rect.bottom() - 32.0)),
                Align2::CENTER_BOTTOM,
                &labels.bottom,
                size,
                color,
            );
        }
    }

    // Scale bar.
    if let Some(mm_per_px) = rendered.displayed.mm_per_pixel_x() {
        let (pw, _) = rendered.displayed.pixel_size();
        let points_per_mm = img_rect.width() / (pw as f64 * mm_per_px) as f32;
        let _ = fit;
        let max_len = (rect.width() * 0.25).clamp(40.0, 160.0);
        if let Some(&len) = [1.0f32, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0]
            .iter()
            .rev()
            .find(|&&l| (30.0..=max_len).contains(&(l * points_per_mm)))
        {
            let w = len * points_per_mm;
            let y = rect.bottom() - 12.0;
            let x0 = rect.center().x - w / 2.0;
            let stroke = Stroke::new(1.5, theme::OVERLAY_TEXT);
            let shadow = Stroke::new(3.5, Color32::from_black_alpha(150));
            for s in [shadow, stroke] {
                painter.line_segment([pos2(x0, y), pos2(x0 + w, y)], s);
                painter.line_segment([pos2(x0, y - 4.0), pos2(x0, y + 4.0)], s);
                painter.line_segment([pos2(x0 + w, y - 4.0), pos2(x0 + w, y + 4.0)], s);
            }
            overlay_text(
                painter,
                pos2(rect.center().x, y - 5.0),
                Align2::CENTER_BOTTOM,
                &format!("{len:.0} mm"),
                11.0,
                theme::OVERLAY_TEXT,
            );
        }
    }
}

/// One decimal place without printing negative zero ("-0.0").
fn one_decimal(v: f64) -> String {
    format!("{:.1}", (v * 10.0).round() / 10.0 + 0.0)
}
