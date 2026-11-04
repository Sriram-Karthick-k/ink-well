//! The Library on a device used by touch.
//!
//! Drawings are big cards with their picture, in two columns on a phone and more on a tablet, and folders are tiles
//! above them. Nothing waits for a pointer to hover: every card has its own menu button, and holding a card opens the
//! same menu. The menu, the choice of a folder to move to and the new name of a drawing are bottom sheets (see
//! `sheet`), not small menus beside a button. A floating button makes a new drawing, and a deleted one can be brought
//! back from a message at the bottom.
//!
//! It does what the desktop page does through the same operations (`library_ops`) and the same state, but none of its
//! layout.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use eframe::egui::{
    Align2, Area, Color32, Context, CornerRadius, FontId, Frame, Id, Margin, Order, Rect, Response,
    RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, pos2, vec2,
};

use super::sheet::{self, Tone};
use crate::app::InkApp;
use crate::density;
use crate::icons::{self, Icon};
use crate::library::{self, FolderCard, ItemMenu, MovePicker, Note, Sort};
use crate::library_ops::name_of;
use crate::theme;
use crate::thumbnail;
use crate::widgets;

/// The room between cards, across and down.
const GAP: f32 = 14.0;
/// The narrowest a card is made when the width is shared out, so a tablet gets more columns and a phone two.
const MIN_CARD: f32 = 200.0;
const MAX_COLUMNS: usize = 5;
/// The narrowest a folder tile is made.
const MIN_FOLDER: f32 = 260.0;
const FOLDER_HEIGHT: f32 = 76.0;
/// The height of a card under its picture, for the name and the line below it.
const CARD_TEXT: f32 = 70.0;
/// The room round the picture on a card.
const THUMB_MARGIN: f32 = 8.0;
/// A chip is a fingertip tall.
const CHIP_HEIGHT: f32 = 40.0;
/// How long a card is held for its menu.
const LONG_PRESS: f64 = 0.45;
/// How far a held finger may drift and still be holding.
const LONG_PRESS_SLOP: f32 = 10.0;
/// How long the message after a delete, with its undo, stays.
const UNDO_SECONDS: f64 = 8.0;
const BACKDROP: Color32 = Color32::from_rgb(0x17, 0x16, 0x13);
const FAB_SIZE: (f32, f32) = (188.0, 58.0);
const FAB_MARGIN: f32 = 20.0;

/// Something done on the page, applied once it has been drawn.
enum Act {
    New,
    NewFolder,
    Import,
    GoTo(PathBuf),
    Open(PathBuf),
    Menu(PathBuf),
    Undo,
}

/// How many columns of cards fit `width`.
#[must_use]
fn columns(width: f32) -> usize {
    (((width + GAP) / (MIN_CARD + GAP)).floor() as usize).clamp(2, MAX_COLUMNS)
}

/// How wide each of `columns` cards is when they share `width`.
#[must_use]
fn card_width(width: f32, columns: usize) -> f32 {
    (width - GAP * (columns.saturating_sub(1)) as f32) / columns.max(1) as f32
}

/// The height of a card's picture, a little shorter than a sheet of paper is wide.
#[must_use]
fn thumb_height(card_width: f32) -> f32 {
    (card_width - 2.0 * THUMB_MARGIN) * 0.72
}

/// How many folder tiles fit `width`.
#[must_use]
fn folder_columns(width: f32) -> usize {
    (((width + GAP) / (MIN_FOLDER + GAP)).floor() as usize).clamp(1, 4)
}

/// What is under a place on the page, which the tests tap.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Hit {
    Card(PathBuf),
    More(PathBuf),
    Folder(PathBuf),
    Fab,
    /// The sheet that is up, whichever it is.
    Sheet,
}

#[cfg(test)]
fn record(ctx: &Context, rect: Rect, hit: Hit) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Vec<(Rect, Hit)>>(Id::NULL)
            .push((rect, hit));
    });
}

#[cfg(test)]
pub(crate) fn hits(ctx: &Context) -> Vec<(Rect, Hit)> {
    ctx.data(|d| d.get_temp(Id::NULL)).unwrap_or_default()
}

/// Shows the page while the Library is open.
pub fn show(ctx: &Context, app: &mut InkApp) {
    let t = theme::pop(ctx, Id::unique("touch_library_pop"), app.ui.library_open);
    if t <= 0.001 {
        return;
    }
    #[cfg(test)]
    ctx.data_mut(|d| d.insert_temp(Id::NULL, Vec::<(Rect, Hit)>::new()));
    app.library.poll();
    let whole = ctx.content_rect();
    let screen = Rect::from_min_max(
        pos2(whole.left(), whole.top() + density::bar_height(ctx)),
        whole.max,
    );
    let now = SystemTime::now();
    let mut acts = Vec::new();

    Area::new(Id::unique("touch_library"))
        .fixed_pos(pos2(screen.left(), screen.top() + (1.0 - t) * 14.0))
        .order(Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_opacity(t);
            // One surface that swallows every press, so nothing reaches the paper underneath.
            let (backdrop, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            ui.painter().rect_filled(backdrop, 0.0, BACKDROP);
            let pad = if screen.width() < 600.0 { 16.0 } else { 28.0 };
            let width = (backdrop.width() - 2.0 * pad).min(1100.0);
            let column = Rect::from_min_max(
                pos2(backdrop.center().x - width / 2.0, backdrop.top()),
                pos2(backdrop.center().x + width / 2.0, backdrop.bottom()),
            );
            let mut inner = ui.new_child(UiBuilder::new().max_rect(column));
            ScrollArea::vertical()
                .id_salt("touch_library_scroll")
                .auto_shrink([false, false])
                .show(&mut inner, |ui| {
                    page(ui, app, &mut acts, width, now);
                    // Room under the last card for the floating button.
                    ui.add_space(FAB_SIZE.1 + 2.0 * FAB_MARGIN + 24.0);
                });
        });

    new_button(ctx, &mut acts);
    item_sheet(ctx, app);
    move_sheet(ctx, app);
    rename_sheet(ctx, app);
    undo_message(ctx, app, &mut acts);
    for act in acts {
        apply(ctx, app, act);
    }
}

/// Opens a drawing as the only one open.
fn open_alone(ctx: &Context, app: &mut InkApp, path: &Path) {
    app.close_all_tabs(ctx);
    app.open_note(ctx, path);
}

fn apply(ctx: &Context, app: &mut InkApp, act: Act) {
    match act {
        Act::New => {
            // One drawing is shown at a time: the one made now replaces the one that was open.
            app.close_all_tabs(ctx);
            app.new_tab(ctx);
        }
        Act::NewFolder => app.library_new_folder(ctx),
        Act::Import => app.import_notes(ctx),
        Act::GoTo(folder) => {
            app.library.menu = None;
            app.library.query.clear();
            app.library.go_to(folder);
        }
        Act::Open(path) => open_alone(ctx, app, &path),
        Act::Menu(path) => {
            app.library.move_picker = None;
            app.library.menu = Some(ItemMenu {
                path,
                at: [0.0, 0.0],
            });
        }
        Act::Undo => app.library_undo_delete(ctx),
    }
}

/// What the page shows, from the top.
fn page(ui: &mut Ui, app: &mut InkApp, acts: &mut Vec<Act>, width: f32, now: SystemTime) {
    ui.spacing_mut().item_spacing = vec2(GAP, GAP);
    ui.add_space(18.0);
    header(ui, app, acts);
    search(ui, &mut app.library.query, width);
    chips(ui, app, acts);

    let searching = app.library.is_searching();
    let (folders, notes): (Vec<FolderCard>, Vec<Note>) = if searching {
        let (folders, notes) = app.library.search_results();
        (folders, notes.into_iter().cloned().collect())
    } else {
        (
            app.library.folders_here(),
            app.library.notes_here().into_iter().cloned().collect(),
        )
    };
    let app: &InkApp = app;

    if folders.is_empty() && notes.is_empty() {
        empty(ui, app, searching);
        return;
    }
    if !folders.is_empty() {
        heading(ui, "FOLDERS", folders.len());
        let per_row = folder_columns(width);
        let w = card_width(width, per_row);
        grid(ui, &folders, per_row, |ui, folder| {
            folder_tile(ui, acts, folder, w);
        });
    }
    if !notes.is_empty() {
        heading(
            ui,
            if searching { "RESULTS" } else { "DRAWINGS" },
            notes.len(),
        );
        let per_row = columns(width);
        let w = card_width(width, per_row);
        grid(ui, &notes, per_row, |ui, note| {
            note_card(ui, app, acts, note, now, w, searching);
        });
    }
}

/// Lays `items` out `columns` to a row.
fn grid<T>(ui: &mut Ui, items: &[T], columns: usize, mut each: impl FnMut(&mut Ui, &T)) {
    for row in items.chunks(columns.max(1)) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for item in row {
                each(ui, item);
            }
        });
    }
}

/// The title of the page; inside a folder, a way back out of it and the folder's name.
fn header(ui: &mut Ui, app: &InkApp, acts: &mut Vec<Act>) {
    if app.library.at_root() {
        ui.label(RichText::new("Library").size(34.0).strong());
        ui.label(
            RichText::new(subtitle(app))
                .size(14.0)
                .color(theme::TEXT_DIM),
        );
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        if widgets::round_button(ui, 44.0, Icon::Left, true, false).clicked() {
            let parent = app
                .library
                .current
                .parent()
                .map_or_else(|| app.library.root.clone(), Path::to_path_buf);
            acts.push(Act::GoTo(parent));
        }
        ui.label(RichText::new(app.library.title()).size(28.0).strong());
    });
}

/// "12 drawings", or what the page is doing while it reads the folders.
fn subtitle(app: &InkApp) -> String {
    if app.library.is_scanning() && app.library.count() == 0 {
        return "Looking for your drawings\u{2026}".to_owned();
    }
    match app.library.count() {
        1 => "1 drawing \u{b7} saved automatically".to_owned(),
        n => format!("{n} drawings \u{b7} saved automatically"),
    }
}

/// The search field: a rounded bar the width of the page.
fn search(ui: &mut Ui, query: &mut String, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 50.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(25), theme::CARD);
    painter.rect_stroke(
        rect,
        CornerRadius::same(25),
        sheet::edge(),
        StrokeKind::Inside,
    );
    // A magnifying glass.
    let glass = pos2(rect.left() + 28.0, rect.center().y - 1.0);
    painter.circle_stroke(glass, 6.5, Stroke::new(1.8, theme::TEXT_DIM));
    painter.line_segment(
        [glass + vec2(4.8, 4.8), glass + vec2(10.0, 10.0)],
        Stroke::new(1.8, theme::TEXT_DIM),
    );
    let clear_room = if query.is_empty() { 0.0 } else { 44.0 };
    let field = Rect::from_min_max(
        pos2(rect.left() + 50.0, rect.top()),
        pos2(rect.right() - 14.0 - clear_room, rect.bottom()),
    );
    ui.put(
        field,
        TextEdit::singleline(query)
            .hint_text("Search drawings")
            .frame(Frame::NONE)
            .align(Align2::LEFT_CENTER)
            .font(FontId::proportional(17.0))
            .desired_width(field.width()),
    );
    if !query.is_empty() {
        let cross =
            Rect::from_center_size(pos2(rect.right() - 28.0, rect.center().y), vec2(40.0, 40.0));
        let response = ui.interact(cross, Id::unique("touch_library_clear"), Sense::click());
        icons::paint(
            ui.painter(),
            Rect::from_center_size(cross.center(), vec2(14.0, 14.0)),
            Icon::Close,
            theme::TEXT_DIM,
        );
        if response.clicked() {
            query.clear();
        }
    }
}

/// How the drawings are ordered, and the things that are not drawings: a folder, a file to bring in.
fn chips(ui: &mut Ui, app: &mut InkApp, acts: &mut Vec<Act>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (sort, label) in [(Sort::Recent, "Recent"), (Sort::Name, "Name")] {
            if chip(ui, label, app.library.sort == sort, None).clicked() {
                app.library.sort = sort;
            }
        }
        ui.add_space(8.0);
        if chip(ui, "New folder", false, Some(Icon::Plus)).clicked() {
            acts.push(Act::NewFolder);
        }
        if chip(ui, "Import", false, None).clicked() {
            acts.push(Act::Import);
        }
    });
}

/// A rounded chip. `selected` fills it with the accent.
fn chip(ui: &mut Ui, label: &str, selected: bool, icon: Option<Icon>) -> Response {
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), FontId::proportional(15.5), Color32::WHITE);
    let icon_room = if icon.is_some() { 26.0 } else { 0.0 };
    let width = galley.size().x + 36.0 + icon_room;
    let (rect, response) = ui.allocate_exact_size(vec2(width, CHIP_HEIGHT), Sense::click());
    let painter = ui.painter();
    let (fill, text) = if selected {
        (theme::ACCENT.gamma_multiply(0.24), theme::ACCENT)
    } else if response.is_pointer_button_down_on() {
        (theme::CHROME_HOVER, theme::TEXT)
    } else {
        (theme::CHROME_HI, theme::TEXT)
    };
    painter.rect_filled(rect, CornerRadius::same(20), fill);
    if selected {
        painter.rect_stroke(
            rect,
            CornerRadius::same(20),
            Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.7)),
            StrokeKind::Inside,
        );
    }
    let mut x = rect.left() + 18.0;
    if let Some(icon) = icon {
        icons::paint(
            painter,
            Rect::from_center_size(pos2(x + 8.0, rect.center().y), vec2(15.0, 15.0)),
            icon,
            text,
        );
        x += icon_room;
    }
    painter.text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(15.5),
        text,
    );
    response
}

/// A small label over a group of cards, with how many there are.
fn heading(ui: &mut Ui, text: &str, count: usize) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(format!("{text}   {count}"))
            .size(12.5)
            .color(theme::TEXT_DIM),
    );
}

/// Nothing to show: the folder is empty, or the search found nothing.
fn empty(ui: &mut Ui, app: &InkApp, searching: bool) {
    ui.add_space(70.0);
    let (title, hint) = if searching {
        ("Nothing found", "Try another name.")
    } else if app.library.is_scanning() {
        ("Looking\u{2026}", "")
    } else {
        ("No drawings here yet", "Tap New drawing to start one.")
    };
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).size(22.0).strong());
        if !hint.is_empty() {
            ui.label(RichText::new(hint).size(15.0).color(theme::TEXT_DIM));
        }
    });
}

/// One drawing: its picture, its name, when it was edited, and a menu button that is always there.
fn note_card(
    ui: &mut Ui,
    app: &InkApp,
    acts: &mut Vec<Act>,
    note: &Note,
    now: SystemTime,
    width: f32,
    show_location: bool,
) {
    let thumb_h = thumb_height(width);
    let height = thumb_h + THUMB_MARGIN + CARD_TEXT;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let more_rect = Rect::from_center_size(
        pos2(rect.right() - 28.0, rect.bottom() - CARD_TEXT / 2.0 - 2.0),
        vec2(48.0, 48.0),
    );
    let more = ui.interact(more_rect, response.id.with("more"), Sense::click());
    #[cfg(test)]
    record(ui.ctx(), rect, Hit::Card(note.path.clone()));
    #[cfg(test)]
    record(ui.ctx(), more_rect, Hit::More(note.path.clone()));

    let painter = ui.painter();
    painter.extend(theme::surface(rect, [24.0; 4]));
    let thumb = Rect::from_min_size(
        rect.min + vec2(THUMB_MARGIN, THUMB_MARGIN),
        vec2(width - 2.0 * THUMB_MARGIN, thumb_h),
    );
    let [r, g, b] = note.preview.paper;
    painter.rect_filled(thumb, CornerRadius::same(17), Color32::from_rgb(r, g, b));
    if note.unreadable {
        painter.text(
            thumb.center(),
            Align2::CENTER_CENTER,
            "Can\u{2019}t be read",
            FontId::proportional(13.0),
            Color32::from_black_alpha(110),
        );
    } else {
        thumbnail::paint_preview(painter, thumb, &note.preview);
    }
    painter.rect_stroke(
        thumb,
        CornerRadius::same(17),
        Stroke::new(1.0, Color32::from_black_alpha(40)),
        StrokeKind::Inside,
    );
    let text_x = rect.left() + 16.0;
    let room = width - 32.0 - 40.0;
    let name = widgets::elided(
        painter,
        &note.name,
        FontId::proportional(16.5),
        theme::TEXT,
        room,
    );
    painter.galley(pos2(text_x, thumb.bottom() + 12.0), name, theme::TEXT);
    let line = match app
        .library
        .location_of(&note.path)
        .filter(|_| show_location)
    {
        Some(place) => format!("{} \u{b7} {place}", library::format_age(now, note.modified)),
        None => age_line(now, note),
    };
    let line = widgets::elided(
        painter,
        &line,
        FontId::proportional(12.5),
        theme::TEXT_DIM,
        room,
    );
    painter.galley(pos2(text_x, thumb.bottom() + 36.0), line, theme::TEXT_DIM);
    let menu_open = app
        .library
        .menu
        .as_ref()
        .is_some_and(|m| m.path == note.path);
    more_dots(painter, &more, more_rect.center(), menu_open);
    if response.is_pointer_button_down_on() && !more.is_pointer_button_down_on() {
        painter.rect_filled(rect, CornerRadius::same(24), Color32::from_white_alpha(12));
    }

    let held = long_press(ui.ctx(), &response);
    if more.clicked() || held {
        acts.push(Act::Menu(note.path.clone()));
    } else if response.clicked() && !after_long_press(ui.ctx(), &response) {
        acts.push(Act::Open(note.path.clone()));
    }
}

/// "5 min ago \u{b7} 12 strokes": when it was last edited and how much is on it.
fn age_line(now: SystemTime, note: &Note) -> String {
    let age = library::format_age(now, note.modified);
    match note.strokes {
        0 => format!("{age} \u{b7} empty"),
        1 => format!("{age} \u{b7} 1 stroke"),
        n => format!("{age} \u{b7} {n} strokes"),
    }
}

/// A folder: a tile with the folder mark, its name and how much is inside.
fn folder_tile(ui: &mut Ui, acts: &mut Vec<Act>, folder: &FolderCard, width: f32) {
    let (rect, response) = ui.allocate_exact_size(vec2(width, FOLDER_HEIGHT), Sense::click());
    let more_rect =
        Rect::from_center_size(pos2(rect.right() - 30.0, rect.center().y), vec2(48.0, 48.0));
    let more = ui.interact(more_rect, response.id.with("more"), Sense::click());
    #[cfg(test)]
    record(ui.ctx(), rect, Hit::Folder(folder.path.clone()));
    let painter = ui.painter();
    painter.extend(theme::surface(rect, [22.0; 4]));
    thumbnail::paint_folder_glyph(
        painter,
        pos2(rect.left() + 38.0, rect.center().y),
        28.0,
        theme::ACCENT,
    );
    let name = widgets::elided(
        painter,
        &folder.name,
        FontId::proportional(16.5),
        theme::TEXT,
        width - 150.0,
    );
    painter.galley(
        pos2(rect.left() + 70.0, rect.center().y - 20.0),
        name,
        theme::TEXT,
    );
    let count = match folder.drawings {
        0 => "Empty".to_owned(),
        1 => "1 drawing".to_owned(),
        n => format!("{n} drawings"),
    };
    painter.text(
        pos2(rect.left() + 70.0, rect.center().y + 14.0),
        Align2::LEFT_CENTER,
        count,
        FontId::proportional(12.5),
        theme::TEXT_DIM,
    );
    more_dots(painter, &more, more_rect.center(), false);
    if response.is_pointer_button_down_on() && !more.is_pointer_button_down_on() {
        painter.rect_filled(rect, CornerRadius::same(22), Color32::from_white_alpha(12));
    }
    let held = long_press(ui.ctx(), &response);
    if more.clicked() || held {
        acts.push(Act::Menu(folder.path.clone()));
    } else if response.clicked() && !after_long_press(ui.ctx(), &response) {
        acts.push(Act::GoTo(folder.path.clone()));
    }
}

/// Three dots one over another: the menu of what they are on. Lit while its menu is open or the finger is on it.
fn more_dots(
    painter: &eframe::egui::Painter,
    response: &Response,
    at: eframe::egui::Pos2,
    open: bool,
) {
    if open || response.is_pointer_button_down_on() {
        painter.circle_filled(
            at,
            20.0,
            Color32::from_white_alpha(if open { 26 } else { 18 }),
        );
    }
    let color = if open { theme::ACCENT } else { theme::TEXT_DIM };
    for dy in [-6.5, 0.0, 6.5] {
        painter.circle_filled(at + vec2(0.0, dy), 2.3, color);
    }
}

/// `true` once, when a press on `response` has been held in place for a moment.
fn long_press(ctx: &Context, response: &Response) -> bool {
    if !response.is_pointer_button_down_on() {
        return false;
    }
    let key = response.id.with("long_press");
    let (start, now, origin, at) = ctx.input(|i| {
        (
            i.pointer.press_start_time(),
            i.time,
            i.pointer.press_origin(),
            i.pointer.interact_pos(),
        )
    });
    let (Some(start), Some(origin), Some(at)) = (start, origin, at) else {
        return false;
    };
    if at.distance(origin) > LONG_PRESS_SLOP {
        return false;
    }
    if ctx.data(|d| d.get_temp::<f64>(key)) == Some(start) {
        return false;
    }
    if now - start >= LONG_PRESS {
        ctx.data_mut(|d| d.insert_temp(key, start));
        return true;
    }
    ctx.request_repaint_after(Duration::from_millis(60));
    false
}

/// `true` when the click that ends a press is the end of one that already opened the menu, and so is not a tap.
fn after_long_press(ctx: &Context, response: &Response) -> bool {
    let key = response.id.with("long_press");
    let now = ctx.input(|i| i.time);
    ctx.data(|d| d.get_temp::<f64>(key))
        .is_some_and(|start| now - start < 1.2)
}

/// The floating button that makes a drawing.
fn new_button(ctx: &Context, acts: &mut Vec<Act>) {
    let response = Area::new(Id::unique("touch_library_new"))
        .anchor(Align2::RIGHT_BOTTOM, vec2(-FAB_MARGIN, -FAB_MARGIN))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            let (rect, response) =
                ui.allocate_exact_size(vec2(FAB_SIZE.0, FAB_SIZE.1), Sense::click());
            let painter = ui.painter();
            let radius = CornerRadius::same((FAB_SIZE.1 / 2.0) as u8);
            painter.add(
                eframe::egui::epaint::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: Color32::from_black_alpha(130),
                }
                .as_shape(rect, radius),
            );
            let fill = if response.is_pointer_button_down_on() {
                theme::ACCENT.gamma_multiply(0.82)
            } else {
                theme::ACCENT
            };
            painter.rect_filled(rect, radius, fill);
            let ink = Color32::from_rgb(0x1B, 0x1A, 0x18);
            icons::paint(
                painter,
                Rect::from_center_size(pos2(rect.left() + 34.0, rect.center().y), vec2(22.0, 22.0)),
                Icon::Plus,
                ink,
            );
            painter.text(
                pos2(rect.left() + 58.0, rect.center().y),
                Align2::LEFT_CENTER,
                "New drawing",
                FontId::proportional(17.0),
                ink,
            );
            #[cfg(test)]
            record(ui.ctx(), rect, Hit::Fab);
            response
        });
    if response.inner.clicked() {
        acts.push(Act::New);
    }
}

/// What can be done with a drawing or a folder, as a sheet.
fn item_sheet(ctx: &Context, app: &mut InkApp) {
    let Some(menu) = app.library.menu.clone() else {
        return;
    };
    let path = menu.path;
    let is_folder = path.is_dir();
    let answer = sheet::show(ctx, "library_item", Some(&name_of(&path)), |ui| {
        let mut chosen = None;
        if sheet::row(
            ui,
            None,
            if is_folder { "Open folder" } else { "Open" },
            Tone::Normal,
        ) {
            chosen = Some(Choice::Open);
        }
        if sheet::row(ui, None, "Rename", Tone::Normal) {
            chosen = Some(Choice::Rename);
        }
        if !is_folder && sheet::row(ui, Some(Icon::Duplicate), "Duplicate", Tone::Normal) {
            chosen = Some(Choice::Duplicate);
        }
        if sheet::row(ui, None, "Move to\u{2026}", Tone::Normal) {
            chosen = Some(Choice::Move);
        }
        sheet::divider(ui);
        if sheet::row(ui, Some(Icon::Trash), "Delete", Tone::Danger) {
            chosen = Some(Choice::Delete);
        }
        chosen
    });
    #[cfg(test)]
    record(ctx, answer.rect, Hit::Sheet);
    if answer.dismissed {
        app.library.menu = None;
    }
    match answer.inner {
        Some(Choice::Open) => {
            app.library.menu = None;
            if is_folder {
                app.library.query.clear();
                app.library.go_to(path);
            } else {
                open_alone(ctx, app, &path);
            }
        }
        Some(Choice::Rename) => app.library_ask_rename(path),
        Some(Choice::Duplicate) => app.library_duplicate(ctx, &path),
        Some(Choice::Move) => {
            app.library.menu = None;
            app.library.move_picker = Some(MovePicker {
                path,
                at: [0.0, 0.0],
            });
        }
        Some(Choice::Delete) => app.library_delete(ctx, &path),
        None => {}
    }
}

#[derive(Clone, Copy)]
enum Choice {
    Open,
    Rename,
    Duplicate,
    Move,
    Delete,
}

/// The folders a drawing can be moved to, as a sheet.
fn move_sheet(ctx: &Context, app: &mut InkApp) {
    let Some(picker) = app.library.move_picker.clone() else {
        return;
    };
    let choices = app.library.folder_choices();
    let item = picker.path.clone();
    let answer = sheet::show(
        ctx,
        "library_move",
        Some(&format!("Move \u{201c}{}\u{201d} to", name_of(&item))),
        |ui| {
            let mut chosen = None;
            for (folder, depth) in &choices {
                if !library::can_move_into(&item, folder) {
                    continue;
                }
                let name = if *depth == 0 {
                    "Library".to_owned()
                } else {
                    format!(
                        "{}{}",
                        "      ".repeat(depth.saturating_sub(1)),
                        name_of(folder)
                    )
                };
                let icon = (*depth > 0).then_some(Icon::Right);
                if sheet::row(ui, icon, &name, Tone::Normal) {
                    chosen = Some(folder.clone());
                }
            }
            chosen
        },
    );
    #[cfg(test)]
    record(ctx, answer.rect, Hit::Sheet);
    if answer.dismissed {
        app.library.move_picker = None;
    }
    if let Some(folder) = answer.inner {
        app.library_move_into(ctx, &item, &folder);
    }
}

/// The new name of a drawing or folder, as a sheet with the text in it.
fn rename_sheet(ctx: &Context, app: &mut InkApp) {
    let Some((path, mut name)) = app.library.renaming.clone() else {
        return;
    };
    let (typed, _) = sheet::ask_text(ctx, "library_rename", "Rename", &mut name, "Save");
    match typed {
        sheet::Typed::Cancel => app.library.renaming = None,
        sheet::Typed::Save => {
            app.library.renaming = None;
            app.library_rename(ctx, &path, name);
        }
        sheet::Typed::Editing => app.library.renaming = Some((path, name)),
    }
}

/// The message after a delete, with the way back.
fn undo_message(ctx: &Context, app: &mut InkApp, acts: &mut Vec<Act>) {
    let now = ctx.input(|i| i.time);
    let Some(deleted) = &app.library.deleted else {
        return;
    };
    if now - deleted.at > UNDO_SECONDS {
        app.library.deleted = None;
        return;
    }
    let name = deleted.name.clone();
    let response = Area::new(Id::unique("touch_library_undo"))
        .anchor(
            Align2::CENTER_BOTTOM,
            vec2(0.0, -(FAB_SIZE.1 + 2.0 * FAB_MARGIN)),
        )
        .order(Order::Tooltip)
        .show(ctx, |ui| {
            theme::floating(ui, [26.0; 4], Margin::symmetric(22, 6), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 18.0;
                    ui.label(RichText::new(format!("Deleted \u{201c}{name}\u{201d}")).size(15.0));
                    let (rect, response) = ui.allocate_exact_size(vec2(64.0, 44.0), Sense::click());
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "Undo",
                        FontId::proportional(15.5),
                        theme::ACCENT,
                    );
                    response
                })
                .inner
            })
            .inner
        });
    if response.inner.clicked() {
        acts.push(Act::Undo);
    }
    ctx.request_repaint_after(Duration::from_millis(500));
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, RawInput};

    use super::*;
    use crate::Options;
    use crate::density::Density;

    #[test]
    fn a_phone_gets_two_columns_and_a_tablet_more_and_never_more_than_five() {
        assert_eq!(columns(328.0), 2);
        assert_eq!(columns(744.0), 3);
        assert_eq!(columns(1100.0), 5);
        assert_eq!(columns(4000.0), 5);
    }

    #[test]
    fn the_cards_share_the_width_without_a_gap_left_over() {
        for width in [328.0, 744.0, 1100.0] {
            let n = columns(width);
            let total = n as f32 * card_width(width, n) + (n - 1) as f32 * GAP;
            assert!((total - width).abs() < 1e-3, "{width}: {total}");
        }
    }

    #[test]
    fn a_card_is_wide_enough_for_a_fingertip_menu_and_a_name() {
        assert!(card_width(328.0, columns(328.0)) >= 150.0);
        assert!(thumb_height(157.0) > 80.0);
    }

    #[test]
    fn folders_go_one_to_a_row_on_a_phone() {
        assert_eq!(folder_columns(328.0), 1);
        assert!(folder_columns(1100.0) > 1);
    }

    /// The app showing only the touch Library, a frame at a time, on a screen of `size`.
    struct Screen {
        ctx: Context,
        app: InkApp,
        time: f64,
        size: eframe::egui::Vec2,
        root: PathBuf,
    }

    impl Screen {
        /// A Library of `names` drawings, each with a stroke, on a phone.
        fn new(tag: &str, names: &[&str]) -> Self {
            let root = std::env::temp_dir().join(format!(
                "inkwell-touch-library-{}-{tag}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("folder");
            let mut source = InkApp::blank(Options::default());
            source.new_document();
            let bytes = source.encode().expect("bytes");
            for name in names {
                std::fs::write(root.join(format!("{name}.ink")), &bytes).expect("drawing");
            }
            let mut app = InkApp::blank(Options::default());
            app.config.files.library_folder = Some(root.display().to_string());
            app.ui.library_open = true;
            let mut s = Self {
                ctx: Context::default(),
                app,
                time: 0.0,
                size: vec2(360.0, 780.0),
                root,
            };
            let ctx = s.ctx.clone();
            s.app.refresh_library(&ctx);
            for _ in 0..200 {
                s.frame(Vec::new());
                if s.app.library.count() == names.len() && !s.app.library.is_scanning() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            s.settle();
            s
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), self.size)),
                events,
                time: Some(self.time),
                ..RawInput::default()
            };
            density::set(&self.ctx, Density::TOUCH);
            let app = &mut self.app;
            let mut output = self.ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                show(&ctx, app);
            });
            output.textures_delta.clear(); // nothing is rendered in tests
        }

        fn settle(&mut self) {
            for _ in 0..5 {
                self.frame(Vec::new());
            }
        }

        fn hit(&self, wanted: &Hit) -> Pos2 {
            hits(&self.ctx)
                .into_iter()
                .find(|(_, hit)| hit == wanted)
                .map_or_else(
                    || panic!("{wanted:?} is not on the page"),
                    |(rect, _)| rect.center(),
                )
        }

        fn tap(&mut self, at: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            self.settle();
        }

        /// Holds a finger at `at` for `seconds`.
        fn hold(&mut self, at: Pos2, seconds: f64) {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![button(true)]);
            let until = self.time + seconds;
            while self.time < until {
                self.frame(Vec::new());
            }
            self.frame(vec![button(false)]);
            self.settle();
        }

        fn path(&self, name: &str) -> PathBuf {
            self.root.join(format!("{name}.ink"))
        }
    }

    impl Drop for Screen {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn every_drawing_is_a_card_with_a_menu_button_that_is_there_without_hovering() {
        let s = Screen::new("cards", &["Plan", "Sketch", "Notes"]);
        for name in ["Plan", "Sketch", "Notes"] {
            let path = s.path(name);
            let card = s.hit(&Hit::Card(path.clone()));
            let more = s.hit(&Hit::More(path));
            assert!(
                more.x > card.x,
                "{name}: the menu button is on the card, at its right"
            );
        }
    }

    #[test]
    fn tapping_a_card_opens_the_drawing() {
        let mut s = Screen::new("open", &["Plan"]);
        let at = s.hit(&Hit::Card(s.path("Plan"))) - vec2(40.0, 30.0);
        s.tap(at);
        assert!(!s.app.ui.library_open, "the Library is left");
        assert_eq!(s.app.document_name(), "Plan");
    }

    #[test]
    fn the_menu_button_opens_a_sheet_and_not_the_drawing() {
        let mut s = Screen::new("menu", &["Plan"]);
        let more = s.hit(&Hit::More(s.path("Plan")));
        s.tap(more);
        assert!(s.app.ui.library_open, "still on the Library");
        assert_eq!(
            s.app.library.menu.as_ref().map(|m| m.path.clone()),
            Some(s.path("Plan"))
        );
    }

    #[test]
    fn holding_a_card_opens_its_menu_and_letting_go_does_not_open_the_drawing() {
        let mut s = Screen::new("hold", &["Plan"]);
        let at = s.hit(&Hit::Card(s.path("Plan"))) - vec2(40.0, 30.0);
        s.hold(at, 0.8);
        assert!(s.app.library.menu.is_some(), "the menu is open");
        assert!(
            s.app.ui.library_open,
            "and the drawing was not opened by the release"
        );
    }

    #[test]
    fn opening_another_drawing_replaces_the_one_that_was_open() {
        let mut s = Screen::new("one-at-a-time", &["Plan", "Sketch"]);
        let plan = s.hit(&Hit::Card(s.path("Plan"))) - vec2(40.0, 30.0);
        s.tap(plan);
        assert_eq!(s.app.document_name(), "Plan");
        let ctx = s.ctx.clone();
        s.app.open_library(&ctx);
        s.settle();
        let sketch = s.hit(&Hit::Card(s.path("Sketch"))) - vec2(40.0, 30.0);
        s.tap(sketch);
        assert_eq!(s.app.document_name(), "Sketch");
        assert_eq!(
            s.app.tabs.len(),
            1,
            "there are no tabs to pile up on a touch screen"
        );
    }

    #[test]
    fn the_floating_button_makes_a_new_drawing() {
        let mut s = Screen::new("new", &["Plan"]);
        let fab = s.hit(&Hit::Fab);
        s.tap(fab);
        assert!(!s.app.ui.library_open, "a new drawing is open");
    }

    #[test]
    fn a_folder_tile_opens_the_folder_and_the_back_button_leaves_it() {
        let mut s = Screen::new("folder", &["Plan"]);
        let folder = s.root.join("Work");
        std::fs::create_dir_all(&folder).expect("folder");
        let ctx = s.ctx.clone();
        s.app.refresh_library(&ctx);
        for _ in 0..100 {
            s.frame(Vec::new());
            if !s.app.library.is_scanning() && !s.app.library.folders_here().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        s.settle();
        let tile = s.hit(&Hit::Folder(folder.clone())) - vec2(60.0, 0.0);
        s.tap(tile);
        assert_eq!(s.app.library.current, folder);
        // The back button is the first thing of the page, at its left.
        s.tap(pos2(16.0 + 22.0, 56.0 + 18.0 + 22.0));
        assert_eq!(s.app.library.current, s.root, "back to the Library");
    }

    /// The place of a row of the sheet that is up, with `rows` rows and `lines` dividers above it.
    fn sheet_row(s: &Screen, rows: usize, lines: usize) -> Pos2 {
        let sheet = hits(&s.ctx)
            .into_iter()
            .find(|(_, hit)| *hit == Hit::Sheet)
            .map(|(rect, _)| rect)
            .expect("a sheet is up");
        // After the handle and the title; a divider is a line with a little room on each side.
        let first = sheet.top() + 10.0 + 10.0 + 6.0 + 24.0 + 6.0 + sheet::ROW / 2.0;
        pos2(
            sheet.center().x,
            first + rows as f32 * (sheet::ROW + 2.0) + lines as f32 * 11.0,
        )
    }

    /// Opens the menu of `name` and waits for the sheet to finish sliding in.
    fn open_menu(s: &mut Screen, name: &str) {
        let more = s.hit(&Hit::More(s.path(name)));
        s.tap(more);
        for _ in 0..40 {
            s.frame(Vec::new());
        }
    }

    #[test]
    fn delete_in_the_menu_deletes_the_drawing_and_offers_to_bring_it_back() {
        let mut s = Screen::new("delete", &["Plan"]);
        open_menu(&mut s, "Plan");
        let delete = sheet_row(&s, 4, 1);
        s.tap(delete);
        assert!(!s.path("Plan").exists(), "the drawing is gone");
        assert!(s.app.library.deleted.is_some(), "and can be brought back");
        assert!(s.app.library.menu.is_none(), "the sheet is put away");
        s.app.library_undo_delete(&s.ctx.clone());
        assert!(s.path("Plan").exists(), "undo brings it back");
    }

    #[test]
    fn move_in_the_menu_asks_for_a_folder_and_a_tap_on_one_moves_the_drawing() {
        let mut s = Screen::new("move", &["Plan"]);
        let folder = s.root.join("Work");
        std::fs::create_dir_all(&folder).expect("folder");
        let ctx = s.ctx.clone();
        s.app.refresh_library(&ctx);
        for _ in 0..100 {
            s.frame(Vec::new());
            if !s.app.library.is_scanning() && !s.app.library.folders_here().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        s.settle();
        open_menu(&mut s, "Plan");
        let to = sheet_row(&s, 3, 0);
        s.tap(to);
        assert!(
            s.app.library.move_picker.is_some(),
            "the folders are offered"
        );
        assert!(s.app.library.menu.is_none());
        for _ in 0..40 {
            s.frame(Vec::new());
        }
        // The Library itself is first in the list but is where the drawing already is, so the folder is the first row.
        let work = sheet_row(&s, 0, 0);
        s.tap(work);
        assert!(folder.join("Plan.ink").exists(), "it is in the folder");
        assert!(!s.path("Plan").exists());
    }

    #[test]
    fn rename_in_the_menu_asks_for_a_name_and_enter_keeps_what_was_typed() {
        let mut s = Screen::new("rename", &["Plan"]);
        open_menu(&mut s, "Plan");
        let rename = sheet_row(&s, 1, 0);
        s.tap(rename);
        assert!(s.app.library.renaming.is_some(), "it asks for a name");
        for _ in 0..40 {
            s.frame(Vec::new());
        }
        s.frame(vec![Event::Text("X".to_owned())]);
        s.frame(vec![Event::Key {
            key: eframe::egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        s.settle();
        let names: Vec<String> = std::fs::read_dir(&s.root)
            .expect("dir")
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect();
        assert!(
            names.iter().any(|n| n.contains('X')),
            "renamed to what was typed: {names:?}"
        );
        assert!(s.app.library.renaming.is_none());
    }
}
