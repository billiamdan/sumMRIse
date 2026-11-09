//! Several series at once: the "Compare" menu of the toolbar and its keys, the
//! cells of a multi-series grid, what such a viewport says about itself (series
//! and study date, link, "outside range"), the cross-reference line and
//! dragging series onto viewports
//! (`openspec/changes/series-comparison/design.md`).
//!
//! The model is `summrise_core::workspace`; `viewer/workspace.rs` carries out
//! what is chosen here.

use egui::containers::menu::{MenuButton, MenuConfig};
use egui::{
    Align, Align2, Button, Color32, Context, CornerRadius, CursorIcon, FontId, Id, Key, Label, LayerId, Layout,
    Modifiers, Order, Painter, PopupCloseBehavior, Rect, Response, RichText, Role, Sense, Stroke, StrokeKind, Ui,
    WidgetInfo, pos2, vec2,
};
use egui_material_icons::icons::{ICON_LINK, ICON_LINK_OFF, ICON_SPLITSCREEN};
use summrise_core::dicom::{Series, Study, format};
use summrise_core::imaging::ImagePlane;
use summrise_core::imaging::crossref::intersection_segment;
use summrise_core::workspace::Views;
use summrise_core::{Grid, LinkStatus, NotLinked, SeriesKey, Session, Tiles, Workspace};
use summrise_i18n::{tr, tr_ctx};

use crate::orient;
use crate::placement::Placement;
use crate::render::Displayed;
use crate::theme::{self, VIEWPORT_BG};
use crate::viewport::ViewportResult;
use crate::widgets::{elide_lines, icon, overlay_text, text_width};

/// Size of the line with the series description and the study date.
const IDENTITY_SIZE: f32 = 11.5;
/// Colour of the cross-reference lines, on any image.
const CROSS_REFERENCE: Color32 = Color32::from_rgb(0x5E, 0xE0, 0xFF);
/// The row of "outside range" among the badges of a picture (see `orient::badge`).
const OUTSIDE_ROW: usize = 3;
/// A multi-series grid whose viewports would be smaller than this (points) is not offered.
const MIN_CELL: [f32; 2] = [150.0, 110.0];
/// A viewport narrower than this (points) has only the icon on its link indicator.
const WORD_ON_INDICATOR: f32 = 300.0;
/// Height of the strip with the series description and the study date, in points.
const HEADER_HEIGHT: f32 = 26.0;

/// What the user chose in the Compare menu or with its keys.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Command {
    /// A multi-series grid; `None`: back to one series.
    Grid(Option<Grid>),
    /// The tile grid of the active viewport.
    Tiles(Tiles),
    /// The next tile grid that fits (key T).
    NextTiles,
    /// The link of the active viewport off or on (key K).
    ToggleLink,
    LinkFromHere,
    AutomaticLinks,
    /// The next viewport becomes active (key N).
    NextViewport,
    /// These two studies side by side, the earlier on the left.
    CompareStudies {
        earlier: usize,
        later: usize,
    },
}

/// The cells of a multi-series grid with a thin gutter.
pub(crate) fn cells(rect: Rect, grid: Grid) -> Vec<Rect> {
    let gap = 2.0;
    let (columns, rows) = grid.shape();
    let width = (rect.width() - gap * (columns - 1) as f32) / columns as f32;
    let height = (rect.height() - gap * (rows - 1) as f32) / rows as f32;
    (0..grid.count())
        .map(|i| {
            let (column, row) = ((i % columns) as f32, (i / columns) as f32);
            Rect::from_min_size(rect.min + vec2(column * (width + gap), row * (height + gap)), vec2(width, height))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// What a viewport says

/// What a viewport of a multi-series layout adds to its picture.
pub(crate) struct ViewportInfo {
    /// The series description and the study date: always on screen.
    title: String,
    date: String,
    link: LinkStatus,
    /// The active viewport's position lies beyond this series.
    outside: bool,
    /// The plane of the active viewport's current image, whose line this
    /// viewport's pictures draw.
    pub cross: Option<ImagePlane>,
}

impl ViewportInfo {
    pub(crate) fn of(
        workspace: &Workspace,
        index: usize,
        session: &Session,
        views: &Views,
        meta: &Series,
        study: &Study,
    ) -> Self {
        Self {
            title: meta.title(),
            date: study_date(study),
            link: workspace.link_status(index, session, views),
            outside: workspace.is_outside_range(index, session, views),
            cross: workspace.cross_reference(index, session, views),
        }
    }
}

/// The date of a study as a viewport says it.
fn study_date(study: &Study) -> String {
    study.date.as_deref().and_then(format::date).unwrap_or_else(|| tr!("Undated").to_owned())
}

/// How far the top and the bottom edge letter of a picture stay from the edges
/// of a viewport `rect` of several series, in points: below the header and
/// the patient's name; above the text at the bottom, which in a narrow
/// viewport meets in the middle.
pub(crate) fn clear_of_text(rect: Rect) -> (f32, f32) {
    (HEADER_HEIGHT + 18.0, if rect.width() < WORD_ON_INDICATOR { 58.0 } else { 32.0 })
}

/// The series description and the study date on one line of at most `max`
/// points: the description is shortened, never the date.
pub(crate) fn identity_line(painter: &Painter, title: &str, date: &str, max: f32) -> String {
    let date = format!(" · {date}");
    let whole = format!("{title}{date}");
    if text_width(painter, std::slice::from_ref(&whole), IDENTITY_SIZE) <= max {
        return whole;
    }
    let room = max - text_width(painter, std::slice::from_ref(&date), IDENTITY_SIZE);
    let title = elide_lines(painter, vec![title.to_owned()], IDENTITY_SIZE, room.max(0.0)).concat();
    format!("{title}{date}")
}

/// The top line of a viewport: series description and study date, whether
/// annotations are shown or not, and the link indicator. Sets
/// [`ViewportResult::toggle_link`] when the indicator is clicked.
pub(crate) fn header(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    id: Id,
    info: &ViewportInfo,
    color: Color32,
    result: &mut ViewportResult,
) {
    // A strip of its own, legible over any picture.
    let strip = Rect::from_min_size(rect.left_top(), vec2(rect.width(), HEADER_HEIGHT));
    painter.rect_filled(strip, 0.0, Color32::from_black_alpha(170));
    let link = link_indicator(ui, painter, rect, id, info.link);
    if link.clicked {
        result.toggle_link = true;
    }
    let max = (rect.width() - 20.0 - link.width - 6.0).max(0.0);
    let line = identity_line(painter, &info.title, &info.date, max);
    let width = text_width(painter, std::slice::from_ref(&line), IDENTITY_SIZE).min(max);
    overlay_text(painter, pos2(rect.left() + 10.0, rect.top() + 8.0), Align2::LEFT_TOP, &line, IDENTITY_SIZE, color);
    // In full for assistive technology (and UI tests), and on hover where it is shortened.
    let whole = format!("{} · {}", info.title, info.date);
    let area = Rect::from_min_size(rect.left_top() + vec2(10.0, 6.0), vec2(width, IDENTITY_SIZE + 6.0));
    let response = ui.interact(area, id.with("identity"), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, whole.clone()));
    response.on_hover_text(whole);
}

struct Indicator {
    width: f32,
    clicked: bool,
}

/// What the link indicator says, its icon and its colour on the black viewport.
fn link_words(status: LinkStatus) -> (&'static str, bool, Color32) {
    match status {
        // Translators: the link indicator of a viewport: it moves with the active viewport.
        LinkStatus::Linked => (tr_ctx!("link indicator", "linked"), true, theme::DARK.accent),
        // Translators: the link indicator of a viewport: it moves by equal distances, aligned by hand.
        LinkStatus::Manual => (tr_ctx!("link indicator", "manual"), true, theme::DARK.warn),
        LinkStatus::Off => (tr_ctx!("link indicator", "link off"), false, theme::DARK.text_weak),
        LinkStatus::NotLinked(_) => (tr_ctx!("link indicator", "not linked"), false, theme::DARK.text_weak),
    }
}

fn link_hint(status: LinkStatus) -> &'static str {
    match status {
        LinkStatus::Linked => {
            tr!(
                "This viewport follows the active one by the position in the body: scrolling, zoom and pan. Click, or K in the active viewport, to switch the link off."
            )
        }
        LinkStatus::Manual => {
            tr!(
                "This viewport moves by the same distances as the active one, from where you aligned them. It rests on your alignment, not on scanner positions. Click, or K in the active viewport, to switch the link off."
            )
        }
        LinkStatus::Off => {
            tr!(
                "The link of this viewport is off: it stays where it is. Click, or K in the active viewport, to switch it on."
            )
        }
        LinkStatus::NotLinked(NotLinked::Alone) => tr!("There is no other series to follow."),
        LinkStatus::NotLinked(NotLinked::NoCommonFrame) => {
            tr!(
                "These series do not share scanner positions, for example because they are from two examinations. Scroll both to the same place and choose Link from here in the Compare menu."
            )
        }
        LinkStatus::NotLinked(NotLinked::OtherDirection) => {
            tr!(
                "The slices of these series run in directions 30° or more apart, so they do not scroll together. A line shows where the active image cuts this one."
            )
        }
        LinkStatus::NotLinked(NotLinked::NoPosition) => tr!("These images do not record where they lie in the body."),
        LinkStatus::NotLinked(NotLinked::OtherLinkOff) => {
            tr!("The link of the other viewport is off, so this one follows nothing.")
        }
    }
}

/// The link indicator at the right end of the top line: it says how the
/// viewport is linked and switches the link off and on.
fn link_indicator(ui: &Ui, painter: &Painter, viewport: Rect, id: Id, status: LinkStatus) -> Indicator {
    let (word, on, color) = link_words(status);
    let glyph = if on { ICON_LINK } else { ICON_LINK_OFF };
    // In a narrow viewport the series and the date keep the line: the icon alone, its colour
    // saying which link. The word stays on hover and for assistive technology.
    let text =
        if viewport.width() < WORD_ON_INDICATOR { icon(glyph).to_owned() } else { format!("{} {word}", icon(glyph)) };
    let galley = painter.layout_no_wrap(text, FontId::proportional(11.0), color);
    let size = galley.size() + vec2(10.0, 4.0);
    let rect = Rect::from_min_size(pos2(viewport.right() - size.x - 6.0, viewport.top() + 5.0), size);
    let response = ui.interact(rect, id.with("link"), Sense::click());
    let hover = response.hovered() || response.has_focus();
    let fill = Color32::from_black_alpha(if hover { 240 } else { 170 });
    let stroke = Stroke::new(1.0, if hover { color } else { color.gamma_multiply(0.5) });
    painter.rect(rect, CornerRadius::same(5), fill, stroke, StrokeKind::Inside);
    painter.galley(rect.min + vec2(5.0, 2.0), galley, color);
    let name = tr!("Link: {state}", state = word);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, name.clone()));
    let clicked = response.clicked();
    let hint = format!("{name}\n{}", link_hint(status));
    response.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(hint);
    Indicator { width: size.x + 6.0, clicked }
}

/// What a viewport says about its position: "outside range", whether
/// annotations are shown or not.
pub(crate) fn outside_badge(ui: &Ui, painter: &Painter, viewport: Rect, picture: Rect, id: Id, info: &ViewportInfo) {
    if info.outside {
        // Translators: the active viewport is scrolled beyond the slices of this series.
        let text = tr!("outside range");
        let hint = tr!(
            "The active viewport shows a position beyond the slices of this series. This viewport stays on its nearest end image."
        );
        orient::badge(ui, painter, viewport, picture, id.with("outside"), OUTSIDE_ROW, true, text, text, hint);
    }
}

/// The line along which the plane `cutting` crosses the image of a picture,
/// through the mapping of the picture, so that it is right in every display
/// orientation. Returns its ends on screen.
pub(crate) fn draw_cross_reference(
    painter: &Painter,
    place: &Placement,
    displayed: &Displayed,
    cutting: &ImagePlane,
) -> Option<[egui::Pos2; 2]> {
    let Displayed::Frame { plane: Some(image), .. } = displayed else { return None };
    let [a, b] = intersection_segment(cutting, image)?;
    let ends = [place.to_screen(a.0, a.1), place.to_screen(b.0, b.1)];
    painter.line_segment(ends, Stroke::new(3.0, Color32::from_black_alpha(150)));
    painter.line_segment(ends, Stroke::new(1.2, CROSS_REFERENCE));
    Some(ends)
}

// ---------------------------------------------------------------------------
// Empty viewports and dragging

/// A viewport waiting for a series: it says what to do, also for those who
/// use the keyboard, in the viewport's colours (black in both themes).
pub(crate) fn empty_viewport(ui: &Ui, rect: Rect, id: Id, is_active: bool) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, VIEWPORT_BG);
    painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(1.0, Color32::from_gray(70)), StrokeKind::Inside);
    let text = tr!("Drop a series here");
    painter.text(
        rect.center() - vec2(0.0, 10.0),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(14.0),
        theme::OVERLAY_TEXT,
    );
    let hint = tr!("or click this viewport and then a series in the browser");
    let galley = painter.layout(
        hint.to_owned(),
        FontId::proportional(11.5),
        theme::DARK.text_weak,
        (rect.width() - 24.0).max(60.0),
    );
    painter.galley(rect.center() + vec2(-galley.size().x / 2.0, 6.0), galley, theme::DARK.text_weak);
    if is_active {
        highlight(ui, &painter, rect);
    }
    response.widget_info(|| WidgetInfo::labeled(Role::Image, true, text));
    response
}

/// The border of the active viewport.
pub(crate) fn highlight(ui: &Ui, painter: &Painter, rect: Rect) {
    painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(1.5, theme::palette(ui).accent), StrokeKind::Inside);
}

/// Makes a row of the series browser a source of series to drop on viewports.
pub(crate) fn drag_source(ui: &Ui, response: &Response, key: SeriesKey, title: &str) {
    response.dnd_set_drag_payload(key);
    if response.dragged()
        && let Some(pointer) = ui.ctx().pointer_interact_pos()
    {
        // The series goes with the pointer.
        let p = theme::palette(ui);
        let layer = LayerId::new(Order::Tooltip, response.id.with("dragged series"));
        let painter = ui.ctx().layer_painter(layer);
        let galley = painter.layout_no_wrap(title.to_owned(), FontId::proportional(12.5), p.text);
        let rect = Rect::from_min_size(pointer + vec2(12.0, 12.0), galley.size() + vec2(14.0, 8.0));
        painter.rect(rect, CornerRadius::same(6), p.surface, Stroke::new(1.0, p.accent), StrokeKind::Inside);
        painter.galley(rect.min + vec2(7.0, 4.0), galley, p.text);
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
}

/// A viewport as a drop target: it lights up while a series is held over it,
/// and returns the series dropped on it.
pub(crate) fn drop_target(ui: &Ui, rect: Rect, id: Id) -> Option<SeriesKey> {
    let response = ui.interact(rect, id.with("drop target"), Sense::hover());
    if response.dnd_hover_payload::<SeriesKey>().is_some() {
        let accent = theme::palette(ui).accent;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, accent.gamma_multiply(0.15));
        painter.rect_stroke(rect.shrink(1.5), 0.0, Stroke::new(3.0, accent), StrokeKind::Inside);
    }
    response.dnd_release_payload::<SeriesKey>().map(|key| *key)
}

// ---------------------------------------------------------------------------
// The menu and the keys

/// What the Compare menu shows.
pub(crate) struct MenuInput<'a> {
    pub workspace: &'a Workspace,
    pub session: &'a Session,
    pub views: &'a Views,
    /// Where the viewports are drawn, altogether.
    pub central: Rect,
    /// Size of the area the tiles of the active viewport share; `None` when it shows no acquired images.
    pub tile_area: Option<[f32; 2]>,
    /// The tile grid the active viewport shows.
    pub tiles: Tiles,
}

/// The toolbar menu "Compare": the grids, the tile grids of the active viewport,
/// the link and "Compare studies". It stays open while its entries are used.
pub(crate) fn menu(ui: &mut Ui, input: &MenuInput<'_>) -> Option<Command> {
    let config = MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside);
    let comparing = input.workspace.is_comparing();
    // Translators: the toolbar menu with several series at once and comparing two studies.
    let name = tr!("Compare");
    let text = format!("{} {name}", icon(ICON_SPLITSCREEN));
    let mut command = None;
    let (button, _) = MenuButton::from_button(Button::new(text).selected(comparing)).config(config).ui(ui, |ui| {
        ui.set_min_width(270.0);
        ui.set_max_width(360.0);
        command = entries(ui, input);
    });
    button.accessible_name(name).on_hover_text(tr!(
        "Several series at once, several images of a series at once, and comparing two studies (T, K, N)"
    ));
    command
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().strong().color(theme::palette(ui).text_weak));
}

/// A note in the menu, wrapped, in the weak colour.
fn note(ui: &mut Ui, text: &str) {
    let weak = theme::palette(ui).text_weak;
    // Left-aligned: a menu stretches its entries, which would space the words out.
    ui.with_layout(Layout::top_down(Align::Min), |ui| {
        ui.add(Label::new(RichText::new(text).small().color(weak)).wrap());
    });
}

fn entries(ui: &mut Ui, input: &MenuInput<'_>) -> Option<Command> {
    let mut command = None;
    let (workspace, session, views) = (input.workspace, input.session, input.views);

    // Translators: the layouts of several series at once, in the Compare menu.
    heading(ui, tr!("Viewports"));
    let one = Button::selectable(!workspace.is_comparing(), tr!("One series"));
    if ui.add(one).on_hover_text(tr!("Back to the layouts of one series: Images, MPR, 2 × 2 and 3D")).clicked() {
        command = Some(Command::Grid(None));
        ui.close();
    }
    for grid in Grid::ALL {
        let (columns, rows) = grid.shape();
        let cell = [
            (input.central.width() - 2.0 * (columns - 1) as f32) / columns as f32,
            (input.central.height() - 2.0 * (rows - 1) as f32) / rows as f32,
        ];
        let fits = cell[0] >= MIN_CELL[0] && cell[1] >= MIN_CELL[1];
        let entry = Button::selectable(workspace.grid() == Some(grid), grid.label());
        let response = ui
            .add_enabled(fits, entry)
            .on_hover_text(tr!("Each viewport shows a series of your choice; the active one has the highlight"))
            // Translators: the window of the program on the screen, not a contrast window.
            .on_disabled_hover_text(tr!("The window is too small for this: hide a panel or enlarge the window"));
        if response.clicked() {
            command = Some(Command::Grid(Some(grid)));
            ui.close();
        }
    }
    let next = Button::new(tr!("Next viewport")).shortcut_text("N");
    let enabled = workspace.viewports().len() > 1;
    if ui
        .add_enabled(enabled, next)
        .on_hover_text(tr!("Make the next viewport the active one; a series clicked in the browser then goes there"))
        .on_disabled_hover_text(tr!("There is only one viewport"))
        .clicked()
    {
        command = Some(Command::NextViewport);
    }

    ui.separator();
    // Translators: how many images of a series a viewport shows at the same time (key T).
    heading(ui, tr!("Images at once (T)"));
    ui.horizontal_wrapped(|ui| {
        for tiles in Tiles::ALL {
            let offered = tiles == Tiles::ONE || input.tile_area.is_some_and(|area| tiles.fits(area));
            let name = tr!("{grid} images", grid = tiles.label());
            let response = ui
                .add_enabled(offered, Button::selectable(input.tiles == tiles, tiles.label()))
                .accessible_name(name)
                .on_hover_text(tr!("The current image in the first tile, the next images after it"))
                .on_disabled_hover_text(tr!(
                    "Only for acquired images, and only while each image stays large enough on screen"
                ));
            if response.clicked() {
                command = Some(Command::Tiles(tiles));
            }
        }
    });

    if workspace.is_comparing() {
        ui.separator();
        // Translators: viewports that move together.
        heading(ui, tr!("Link"));
        let linked = workspace.active_viewport().is_some_and(|viewport| viewport.link != summrise_core::Link::Off);
        let toggle = Button::selectable(linked, tr!("Link this viewport")).shortcut_text("K");
        if ui
            .add(toggle)
            .on_hover_text(tr!(
                "Linked viewports follow each other's position, zoom and pan; contrast, turn and mirror stay their own"
            ))
            .clicked()
        {
            command = Some(Command::ToggleLink);
        }
        let possible = workspace.can_link_from_here(session, views);
        if ui
            .add_enabled(possible, Button::new(tr!("Link from here")))
            .on_hover_text(tr!("Keeps the present offset between the viewports and moves them by equal distances from now on. Scroll them to the same place first."))
            .on_disabled_hover_text(tr!("Needs another viewport with its link on, showing another series in a similar slice direction"))
            .clicked()
        {
            command = Some(Command::LinkFromHere);
        }
        if workspace.has_manual_links()
            && ui
                .button(tr!("Link by scanner position"))
                .on_hover_text(tr!("Ends the manual link: series with common scanner positions follow by them again"))
                .clicked()
        {
            command = Some(Command::AutomaticLinks);
        }
        note(
            ui,
            tr!(
                "Linking uses the positions the scanner recorded. Nothing is aligned afterwards: if the patient moved between two series, they can be shifted against each other. The lines on the images show where the active image cuts them; they are a guide, not a measurement."
            ),
        );
    }

    // Offered only when the media hold two studies of one patient.
    let pairs = summrise_core::workspace::comparable_studies(session.studies());
    let current = session.active_series.map_or(session.active_study, |key| key.study);
    let own: Vec<(usize, usize)> = pairs.iter().copied().filter(|(a, b)| *a == current || *b == current).collect();
    let pairs = if own.is_empty() { pairs } else { own };
    if !pairs.is_empty() {
        ui.separator();
        let single = pairs.len() == 1;
        for (earlier, later) in pairs {
            let dates = (study_date(&session.studies()[earlier]), study_date(&session.studies()[later]));
            let label = if single {
                tr!("Compare studies").to_owned()
            } else {
                tr!("Compare studies of {earlier} and {later}", earlier = dates.0.clone(), later = dates.1.clone())
            };
            let hint = tr!(
                "Matching series of the studies of {earlier} and {later} side by side, the earlier one on the left. Replace either side by dropping another series on it.",
                earlier = dates.0,
                later = dates.1
            );
            if ui.button(label).on_hover_text(hint).clicked() {
                command = Some(Command::CompareStudies { earlier, later });
                ui.close();
            }
        }
    }
    command
}

/// The keys of the viewports: T the next tile grid, K the link of the active
/// viewport, N the next viewport.
pub(crate) fn keys(ctx: &Context, comparing: bool) -> Option<Command> {
    if ctx.egui_wants_keyboard_input() {
        return None;
    }
    let pressed = |key: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
    if pressed(Key::T) {
        return Some(Command::NextTiles);
    }
    if pressed(Key::N) {
        return Some(Command::NextViewport);
    }
    if comparing && pressed(Key::K) {
        return Some(Command::ToggleLink);
    }
    None
}
