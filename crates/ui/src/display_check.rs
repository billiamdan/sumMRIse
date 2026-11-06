//! The display check (`openspec/changes/display-fidelity/design.md`, D9): a
//! picture to check the screen in the style of the SMPTE test pattern,
//! generated in memory for the pixel size of the area it fills and drawn 1:1
//! without smoothing, with plain instructions. It holds no patient data and is
//! not a calibration.

use egui::emath::GuiRounding as _;
use egui::{
    Align, Align2, Area, Button, Color32, ColorImage, Context, FontId, Frame, Id, Key, Label, Layout, Margin,
    Modifiers, Order, Rect, RichText, Role, Sense, TextureHandle, TextureOptions, Ui, UiBuilder, WidgetInfo, pos2,
    vec2,
};
use summrise_core::imaging::test_pattern::{DisplayCheck, PixelRect};
use summrise_i18n::tr;

use crate::theme;

/// Height of the instructions under the picture, in points.
const INSTRUCTIONS_HEIGHT: f32 = 236.0;

/// The open display check.
#[derive(Default)]
pub(crate) struct DisplayCheckScreen {
    /// The picture for the current pixel size, its layout (without the
    /// pixels, which live in the texture) and its texture.
    picture: Option<(DisplayCheck, TextureHandle)>,
}

impl DisplayCheckScreen {
    /// Shows the display check over the whole window. Returns `false` once
    /// the user has closed it (Close or Escape).
    pub(crate) fn show(&mut self, ctx: &Context) -> bool {
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            return false;
        }
        let screen = ctx.content_rect();
        let mut open = true;
        Area::new(Id::unique("display-check")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            ui.set_min_size(screen.size());
            ui.painter().rect_filled(screen, 0.0, Color32::BLACK);
            let split = (screen.max.y - INSTRUCTIONS_HEIGHT).max(screen.min.y + 40.0);
            let picture_area = Rect::from_min_max(screen.min, pos2(screen.max.x, split));
            let instructions = Rect::from_min_max(pos2(screen.min.x, split), screen.max);
            self.picture(ui, picture_area);
            ui.scope_builder(UiBuilder::new().max_rect(instructions), |ui| open = instructions_panel(ui));
        });
        open
    }

    /// Draws the picture 1:1 into `area`: generated for its size in physical
    /// pixels, at a pixel-aligned position, with a nearest-neighbour texture.
    fn picture(&mut self, ui: &mut Ui, area: Rect) {
        let ppp = ui.ctx().pixels_per_point();
        let min = area.min.round_to_pixels(ppp);
        let (width, height) =
            (((area.max.x - min.x) * ppp).floor() as u32, ((area.max.y - min.y) * ppp).floor() as u32);
        if width < 16 || height < 16 {
            return;
        }
        let stale = self.picture.as_ref().is_none_or(|(check, _)| (check.width, check.height) != (width, height));
        if stale {
            let mut check = DisplayCheck::new(width, height);
            let image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &check.rgba);
            check.rgba = Vec::new();
            let texture = ui.ctx().load_texture("display-check", image, TextureOptions::NEAREST);
            self.picture = Some((check, texture));
        }
        let Some((check, texture)) = &self.picture else { return };
        let rect = Rect::from_min_size(min, vec2(width as f32 / ppp, height as f32 / ppp));
        let painter = ui.painter();
        painter.image(texture.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        // The level of each step, under it.
        let to_points = |r: PixelRect| {
            Rect::from_min_size(
                min + vec2(r.x as f32 / ppp, r.y as f32 / ppp),
                vec2(r.width as f32 / ppp, r.height as f32 / ppp),
            )
        };
        let labels = to_points(check.labels);
        if labels.height() >= 18.0 {
            for step in &check.steps {
                let x = to_points(step.rect).center().x;
                let text = tr!("{percent} %", percent = step.percent);
                painter.text(
                    pos2(x, labels.top() + 5.0),
                    Align2::CENTER_TOP,
                    text,
                    FontId::proportional(11.0),
                    Color32::BLACK,
                );
            }
        }
        let response = ui.interact(rect, Id::unique("display-check-picture"), Sense::hover());
        response.widget_info(|| {
            WidgetInfo::labeled(
                Role::Image,
                true,
                tr!("Display check picture: grey steps, a ramp, fine lines and a border"),
            )
        });
    }
}

/// The button that opens the display check (in the settings window and the
/// About dialog).
pub(crate) fn open_button(ui: &mut Ui) -> egui::Response {
    ui.button(tr!("Check the display…")).on_hover_text(tr!(
        "Shows a test picture to check that the screen shows dark and light greys and fine detail. A quick check, not a calibration."
    ))
}

/// The instructions under the picture; `false` once Close is clicked.
fn instructions_panel(ui: &mut Ui) -> bool {
    let p = theme::palette(ui);
    let mut open = true;
    Frame::new().fill(p.panel).inner_margin(Margin::symmetric(20, 12)).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr!("Display check")).heading().strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let close = ui.add(Button::new(tr!("Close"))).on_hover_text(tr!("Close the display check (Esc)"));
                if close.clicked() {
                    open = false;
                }
            });
        });
        ui.label(
            RichText::new(tr!(
                "A quick check of your screen in the style of the SMPTE test pattern. It is not a calibration and does not show that the screen meets any standard."
            ))
            .color(p.text_weak),
        );
        ui.add_space(6.0);
        let columns = (ui.available_width() / 2.0 - 12.0).max(200.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(columns);
                ui.label(RichText::new(tr!("What you should see")).strong());
                for line in [
                    tr!("Grey steps: 11 steps from black to white, each a little brighter than the one before."),
                    tr!("Small squares: the square inside the black step (5 %) and the one inside the white step (95 %)."),
                    tr!("Ramp: a band that turns smoothly from black to white, without stripes."),
                    tr!("Fine lines: separate lines in the four patterns at the bottom, not an even grey."),
                    tr!("Border: a thin line all around the edge of the picture."),
                ] {
                    ui.add(Label::new(format!("• {line}")).wrap());
                }
            });
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.set_width(columns);
                ui.label(RichText::new(tr!("If something is not visible")).strong());
                for line in [
                    tr!("The square in the black step is missing: turn the brightness of the screen up or dim the room."),
                    tr!("The square in the white step is missing: turn the contrast of the screen down."),
                    tr!("Steps run together or the ramp shows stripes: switch off picture modes such as vivid or dynamic contrast."),
                    tr!("The fine lines look blurred: set the screen to its native resolution and switch off scaling."),
                ] {
                    ui.add(Label::new(format!("• {line}")).wrap());
                }
            });
        });
    });
    open
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    /// The screen alone, in both themes: the picture is drawn at the pixel
    /// size of its area, the instructions say what it is, Close and Escape
    /// close it. Set `SUMMRISE_SNAPSHOT_DIR` to keep renderings.
    #[test]
    fn the_display_check_draws_its_picture_and_closes() {
        let snapshots = std::env::var_os("SUMMRISE_SNAPSHOT_DIR").map(std::path::PathBuf::from);
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let mut builder =
                Harness::builder().with_size([1280.0, 800.0]).with_theme(theme).with_context_setup(theme::install);
            if snapshots.is_some() {
                builder = builder.wgpu();
            }
            let mut harness = builder.build_ui_state(
                |ui, state: &mut (DisplayCheckScreen, bool)| {
                    if state.1 {
                        state.1 = state.0.show(ui.ctx());
                    }
                },
                (DisplayCheckScreen::default(), true),
            );
            harness.run();
            let (check, _) = harness.state().0.picture.as_ref().expect("a picture");
            // 1280 × (800 − 236) points at the harness' pixels per point.
            let ppp = harness.ctx.pixels_per_point();
            let expected = ((1280.0 * ppp) as u32, ((800.0 - INSTRUCTIONS_HEIGHT) * ppp) as u32);
            assert_eq!((check.width, check.height), expected);
            assert!(check.rgba.is_empty(), "the pixels live in the texture only");
            assert_eq!(check.steps.len(), 11);
            assert!(harness.query_all_by_label_contains("It is not a calibration").next().is_some());
            assert!(
                harness.query_by_label("Display check picture: grey steps, a ramp, fine lines and a border").is_some()
            );
            harness.check_accessibility();
            if let Some(dir) = &snapshots {
                let image = harness.render().expect("wgpu render");
                std::fs::create_dir_all(dir).expect("snapshot dir");
                image.save(dir.join(format!("fidelity-7-display-check-{theme:?}.png").to_lowercase())).expect("png");
            }
            harness.get_by_label("Close").click();
            harness.run();
            assert!(!harness.state().1, "closed");
            // Escape closes it as well.
            harness.state_mut().1 = true;
            harness.run();
            harness.key_press(Key::Escape);
            harness.run();
            assert!(!harness.state().1);
        }
    }
}
