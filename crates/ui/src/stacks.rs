//! The control that switches the stack of a series holding several sets of
//! images at the same slice positions, and says what tells the stack shown
//! apart ("Time point 3 / 20", "Echo 2 / 2 · TE 90 ms", "b = 1000 s/mm²").

use egui::{Button, ComboBox, RichText, Ui};
use egui_material_icons::icons::{ICON_CHEVRON_LEFT, ICON_CHEVRON_RIGHT};
use summrise_core::{SeriesData, ViewState};
use summrise_i18n::tr;

use crate::widgets::icon;

/// "Image set  ‹  Time point 2 / 3 ▾  ›" for a series split into stacks;
/// nothing for one that is not.
pub(crate) fn control(ui: &mut Ui, series: &SeriesData, state: &mut ViewState) {
    let Some(stacks) = series.stacks() else { return };
    let current = state.stack();
    let label = ui.label(RichText::new(tr!("Image set")).small()).on_hover_text(stacks.summary());

    let previous = ui
        .add_enabled(current > 0, Button::new(icon(ICON_CHEVRON_LEFT)))
        .accessible_name(tr!("Previous image set"))
        .on_hover_text(tr!("Previous image set (Left arrow key)"))
        .on_disabled_hover_text(tr!("This is the first image set"));
    if previous.clicked() {
        state.step_stack(series, -1);
    }

    let mut chosen = current;
    // As wide as there is room for beside the next button, in a narrow view too.
    let width = (ui.available_width() - 2.0 * ui.spacing().interact_size.y - 2.0 * ui.spacing().item_spacing.x)
        .clamp(80.0, 170.0);
    ComboBox::from_id_salt("image-set")
        .selected_text(stacks.label(current))
        .width(width)
        .show_ui(ui, |ui| {
            for stack in 0..stacks.count() {
                ui.selectable_value(&mut chosen, stack, stacks.label(stack));
            }
        })
        .response
        .labelled_by(label.id)
        .on_hover_text(tr!(
            "This series holds several sets of images of the same slices. The images shown are those of this set."
        ));
    if chosen != current {
        state.set_stack(series, chosen);
    }

    let next = ui
        .add_enabled(current + 1 < stacks.count(), Button::new(icon(ICON_CHEVRON_RIGHT)))
        .accessible_name(tr!("Next image set"))
        .on_hover_text(tr!("Next image set (Right arrow key)"))
        .on_disabled_hover_text(tr!("This is the last image set"));
    if next.clicked() {
        state.step_stack(series, 1);
    }
}
