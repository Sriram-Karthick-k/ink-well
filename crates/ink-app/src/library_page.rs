//! The Library page: every drawing and folder, laid out as cards. At the top of the library it starts with the
//! drawings edited most recently; folders open like a file manager's and are shown in a breadcrumb trail; a
//! drawing or folder can be dragged onto a folder (or onto a step of the trail) to organize it.
//!
//! The page covers the paper, below the toolbar with the tabs. It is what the app starts on.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{
    self, Align, Align2, Area, Color32, Context, CornerRadius, CursorIcon, FontId, Frame, Id, Key,
    Layout, Margin, Mesh, Modal, Order, Painter, Pos2, Rect, Response, RichText, ScrollArea, Sense,
    Shape, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, epaint::Shadow, pos2, vec2,
};

use crate::app::InkApp;
use crate::library::{self, FolderCard, ItemMenu, MovePicker, Note, Sort};
use crate::library_ops::name_of;
use crate::theme;
use crate::thumbnail::{paint_folder_glyph, paint_preview};
use crate::widgets;

const CARD_WIDTH: f32 = 236.0;
const CARD_HEIGHT: f32 = 226.0;
const FOLDER_HEIGHT: f32 = 172.0;
const THUMB_HEIGHT: f32 = 150.0;
const GAP: f32 = 22.0;
const MARGIN_X: f32 = 56.0;
/// How long the "Deleted" message with its undo stays.
const UNDO_SECONDS: f64 = 8.0;
/// How many drawings the Recent row shows at most.
const RECENT_SHOWN: usize = 5;
const FOLDER_BACK: Color32 = Color32::from_rgb(0x3A, 0x36, 0x30);
const FOLDER_FRONT: Color32 = Color32::from_rgb(0x4B, 0x46, 0x3D);

/// Something done on the page, applied once it has been drawn.
enum Action {
    New,
    NewFolder,
    Import,
    GoTo(PathBuf),
    Open(PathBuf),
    ToggleMenu(ItemMenu),
    AskRename(PathBuf),
    Rename(PathBuf, String),
    Duplicate(PathBuf),
    Delete(PathBuf),
    #[cfg_attr(
        target_os = "android",
        allow(
            dead_code,
            reason = "there is no file manager to show a drawing in on Android"
        )
    )]
    Reveal(PathBuf),
    OpenMovePicker(MovePicker),
    MoveInto(PathBuf, PathBuf),
    BeginDrag(PathBuf),
    Undo,
}

/// What the page collects while it is drawn: what was done, and where a dragged item can be dropped.
#[derive(Default)]
struct Page {
    actions: Vec<Action>,
    drops: Vec<(Rect, PathBuf)>,
}

pub fn show(ctx: &Context, app: &mut InkApp) {
    let t = theme::pop(ctx, Id::unique("library_pop"), app.ui.library_open);
    if t <= 0.001 {
        return;
    }
    app.library.poll();
    demo_state(ctx, app);
    let whole = ctx.content_rect();
    let screen = Rect::from_min_max(
        pos2(whole.left(), whole.top() + crate::density::bar_height(ctx)),
        whole.max,
    );
    let columns =
        (((screen.width() - 2.0 * MARGIN_X + GAP) / (CARD_WIDTH + GAP)).floor() as usize).max(1);
    let width = columns as f32 * (CARD_WIDTH + GAP) - GAP;
    let mut page = Page::default();

    Area::new(Id::unique("library"))
        .fixed_pos(pos2(screen.left(), screen.top() + (1.0 - t) * 18.0))
        .order(Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_opacity(t);
            // One surface that swallows every press, so nothing reaches the paper underneath.
            let (backdrop, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            paint_backdrop(ui, backdrop);
            let column = Rect::from_min_max(
                pos2(backdrop.center().x - width / 2.0, backdrop.top() + 30.0),
                pos2(backdrop.center().x + width / 2.0, backdrop.bottom()),
            );
            let mut inner = ui.new_child(UiBuilder::new().max_rect(column));
            header(&mut inner, app, &mut page);
            inner.add_space(18.0);
            toolbar(&mut inner, &mut page);
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(&mut inner, |ui| {
                    sections(ui, app, &mut page, columns);
                    ui.add_space(60.0);
                });
        });

    item_menu(ctx, app, &mut page);
    move_picker(ctx, app, &mut page);
    rename_dialog(ctx, app, &mut page);
    undo_bar(ctx, app, &mut page);
    dragging(ctx, app, &mut page);
    for action in std::mem::take(&mut page.actions) {
        apply(ctx, app, action);
    }
}

/// What Escape does on the page: put away what is open, then the search, and only then leave the page, if
/// there is a drawing to go back to.
pub fn escape(app: &mut InkApp) {
    let library = &mut app.library;
    if library.menu.is_some() || library.move_picker.is_some() {
        library.menu = None;
        library.move_picker = None;
    } else if library.drag.is_some() {
        library.drag = None;
    } else if library.is_searching() {
        library.query.clear();
    } else if !app.tabs.is_empty() {
        app.close_library();
    }
}

/// For screenshots: puts the page in the state named by `--library-demo` once the drawings have loaded.
fn demo_state(ctx: &Context, app: &mut InkApp) {
    let Some(state) = app.options.library_demo.clone() else {
        return;
    };
    if app.library.is_scanning() || app.library.count() == 0 {
        return;
    }
    app.options.library_demo = None;
    let first = app.library.notes_here().first().map(|n| n.path.clone());
    let folder = app.library.folders_here().first().map(|f| f.path.clone());
    match (state.as_str(), first, folder) {
        ("menu", Some(path), _) => {
            app.library.menu = Some(ItemMenu {
                path,
                at: [594.0, 330.0],
            });
        }
        ("move", Some(path), _) => {
            app.library.move_picker = Some(MovePicker {
                path,
                at: [594.0, 330.0],
            });
        }
        ("rename", Some(path), _) => {
            let name = name_of(&path);
            app.library.renaming = Some((path, name));
        }
        ("undo", Some(path), _) => {
            if let Ok(copy) = library::duplicate_note(&path) {
                let now = ctx.input(|i| i.time);
                let root = app.library.root.clone();
                app.library.deleted = library::delete_item(&copy, &root, now).ok();
            }
        }
        ("search", ..) => "zzz".clone_into(&mut app.library.query),
        ("folder", _, Some(path)) => app.library.go_to(path),
        (other, ..) => eprintln!("unknown --library-demo state {other}"),
    }
}

/// A deep warm background, a little lighter in the top left corner where the light would come from.
fn paint_backdrop(ui: &Ui, rect: Rect) {
    let mut mesh = Mesh::default();
    let corners = [
        (rect.left_top(), Color32::from_rgb(0x27, 0x24, 0x20)),
        (rect.right_top(), Color32::from_rgb(0x1B, 0x1A, 0x18)),
        (rect.right_bottom(), Color32::from_rgb(0x12, 0x11, 0x0F)),
        (rect.left_bottom(), Color32::from_rgb(0x17, 0x16, 0x13)),
    ];
    for (at, color) in corners {
        mesh.colored_vertex(at, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(Shape::mesh(mesh));
}

fn header(ui: &mut Ui, app: &mut InkApp, page: &mut Page) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            breadcrumbs(ui, app, page);
            ui.label(RichText::new(app.library.title()).size(38.0).strong());
            widgets::caption(ui, subtitle(app));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let mut sort = app.library.sort;
            if widgets::text_tabs(
                ui,
                &[(Sort::Name, "Name"), (Sort::Recent, "Recent")],
                &mut sort,
            ) {
                app.library.sort = sort;
            }
            search_box(ui, &mut app.library.query);
        });
    });
}

/// What the page says under its title.
fn subtitle(app: &InkApp) -> String {
    let library = &app.library;
    if library.is_searching() {
        return "Searching every folder".to_owned();
    }
    if library.is_scanning() && library.count() == 0 {
        return "Looking for your drawings\u{2026}".to_owned();
    }
    let drawings = if library.at_root() {
        library.count()
    } else {
        library.notes_here().len()
    };
    let folders = library.folders_here().len();
    let drawings = format!("{drawings} drawing{}", if drawings == 1 { "" } else { "s" });
    if folders == 0 {
        format!("{drawings} \u{b7} saved automatically")
    } else {
        format!(
            "{drawings} \u{b7} {folders} folder{}",
            if folders == 1 { "" } else { "s" }
        )
    }
}

/// The way down to the folder shown: every step but the last is a link, and also a place to drop things.
fn breadcrumbs(ui: &mut Ui, app: &InkApp, page: &mut Page) {
    let steps = app.library.breadcrumbs();
    if steps.len() < 2 {
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (name, path) in &steps[..steps.len() - 1] {
            let response = ui.add(
                egui::Label::new(RichText::new(name).size(15.0).color(theme::TEXT_DIM))
                    .sense(Sense::click()),
            );
            let target = response.rect.expand(6.0);
            page.drops.push((target, path.clone()));
            if accepts_drop(app, path) && pointer_in(ui, target) {
                ui.painter().rect_stroke(
                    response.rect.expand(4.0),
                    CornerRadius::same(8),
                    Stroke::new(1.5, theme::ACCENT),
                    StrokeKind::Outside,
                );
            }
            if response.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if response.clicked() {
                page.actions.push(Action::GoTo(path.clone()));
            }
            ui.label(RichText::new("\u{203a}").size(15.0).color(theme::TEXT_DIM));
        }
    });
}

/// `true` if the pointer is inside `rect`.
fn pointer_in(ui: &Ui, rect: Rect) -> bool {
    ui.ctx()
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|p| rect.contains(p))
}

/// A rounded search field with a magnifier.
fn search_box(ui: &mut Ui, query: &mut String) {
    let (rect, _) = ui.allocate_exact_size(vec2(260.0, 44.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(22), theme::CHROME_HI);
    painter.rect_stroke(
        rect,
        CornerRadius::same(22),
        Stroke::new(1.0, Color32::from_white_alpha(16)),
        StrokeKind::Inside,
    );
    let lens = pos2(rect.left() + 24.0, rect.center().y - 1.0);
    let stroke = Stroke::new(1.7, theme::TEXT_DIM);
    painter.circle_stroke(lens, 6.0, stroke);
    painter.line_segment([lens + vec2(4.5, 4.5), lens + vec2(9.0, 9.0)], stroke);
    let field = Rect::from_min_max(
        pos2(rect.left() + 44.0, rect.top() + 6.0),
        pos2(rect.right() - 16.0, rect.bottom() - 6.0),
    );
    ui.put(
        field,
        TextEdit::singleline(query)
            .frame(egui::Frame::NONE)
            .hint_text("Search all folders")
            .align(Align2::LEFT_CENTER)
            .font(FontId::proportional(15.0))
            .desired_width(f32::INFINITY),
    );
}

/// The little picture on a button.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Glyph {
    None,
    Plus,
    Folder,
}

/// The row of things to do: start a drawing, make a folder, import files.
fn toolbar(ui: &mut Ui, page: &mut Page) {
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if pill(ui, "New drawing", true, Glyph::Plus).clicked() {
            page.actions.push(Action::New);
        }
        if pill(ui, "New folder", false, Glyph::Folder).clicked() {
            page.actions.push(Action::NewFolder);
        }
        if pill(ui, "Import", false, Glyph::None)
            .on_hover_text("Copy .ink files in here")
            .clicked()
        {
            page.actions.push(Action::Import);
        }
    });
}

/// A pill-shaped button. The primary one is filled with the accent.
fn pill(ui: &mut Ui, label: &str, primary: bool, glyph: Glyph) -> Response {
    let ink = if primary {
        Color32::from_rgb(0x21, 0x17, 0x06)
    } else {
        theme::TEXT
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), FontId::proportional(15.0), ink);
    let icon = if glyph == Glyph::None { 0.0 } else { 26.0 };
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 44.0 + icon, 44.0), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.12);
    let painter = ui.painter();
    let round = CornerRadius::same(22);
    if primary {
        painter.add(
            Shadow {
                offset: [0, 6],
                blur: 18,
                spread: 0,
                color: theme::ACCENT.gamma_multiply(0.28),
            }
            .as_shape(rect, round),
        );
    }
    let fill = if primary {
        theme::ACCENT.lerp_to_gamma(Color32::from_rgb(0xFF, 0xCB, 0x6B), hover)
    } else {
        theme::CHROME_HI.lerp_to_gamma(theme::CHROME_HOVER, hover)
    };
    painter.rect_filled(rect, round, fill);
    if !primary {
        painter.rect_stroke(
            rect,
            round,
            Stroke::new(1.0, Color32::from_white_alpha(18)),
            StrokeKind::Inside,
        );
    }
    let mut x = rect.left() + 22.0;
    let c = pos2(x + 7.0, rect.center().y);
    match glyph {
        Glyph::Plus => {
            let plus = Stroke::new(2.0, ink);
            painter.line_segment([c - vec2(6.0, 0.0), c + vec2(6.0, 0.0)], plus);
            painter.line_segment([c - vec2(0.0, 6.0), c + vec2(0.0, 6.0)], plus);
        }
        Glyph::Folder => paint_folder_glyph(painter, c, 15.0, ink),
        Glyph::None => {}
    }
    x += icon;
    painter.galley(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response
}

/// A caption above a group of cards.
fn heading(ui: &mut Ui, text: &str, count: Option<usize>) {
    ui.add_space(26.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(
            RichText::new(text.to_uppercase())
                .size(12.0)
                .strong()
                .color(theme::TEXT_DIM),
        );
        if let Some(count) = count {
            ui.label(
                RichText::new(count.to_string())
                    .size(12.0)
                    .color(theme::TEXT_DIM.gamma_multiply(0.7)),
            );
        }
    });
    ui.add_space(2.0);
}

/// Lays `items` out in rows of `columns`.
fn rows<T>(ui: &mut Ui, items: &[T], columns: usize, mut each: impl FnMut(&mut Ui, &T)) {
    ui.spacing_mut().item_spacing = vec2(GAP, GAP);
    for row in items.chunks(columns) {
        ui.horizontal(|ui| {
            for item in row {
                each(ui, item);
            }
        });
    }
}

fn sections(ui: &mut Ui, app: &InkApp, page: &mut Page, columns: usize) {
    let library = &app.library;
    let now = SystemTime::now();
    if library.is_searching() {
        let (folders, notes) = library.search_results();
        heading(ui, "Results", Some(folders.len() + notes.len()));
        if folders.is_empty() && notes.is_empty() {
            ui.label(
                RichText::new(format!(
                    "Nothing is called \u{201c}{}\u{201d}",
                    library.query.trim()
                ))
                .size(18.0)
                .color(theme::TEXT_DIM),
            );
            return;
        }
        rows(ui, &folders, columns, |ui, folder| {
            folder_card(ui, app, page, folder);
        });
        rows(ui, &notes, columns, |ui, note| {
            note_card(ui, app, page, note, now, true);
        });
        return;
    }
    if library.at_root() {
        let recent = library.recent(columns.min(RECENT_SHOWN));
        if !recent.is_empty() {
            heading(ui, "Recent", None);
            rows(ui, &recent, columns, |ui, note| {
                note_card(ui, app, page, note, now, true);
            });
        }
    }
    let folders = library.folders_here();
    if !folders.is_empty() {
        heading(ui, "Folders", Some(folders.len()));
        rows(ui, &folders, columns, |ui, folder| {
            folder_card(ui, app, page, folder);
        });
    }
    let notes = library.notes_here();
    heading(ui, "Drawings", Some(notes.len()));
    let mut tiles: Vec<Option<&Note>> = vec![None];
    tiles.extend(notes.into_iter().map(Some));
    rows(ui, &tiles, columns, |ui, tile| match tile {
        None => new_tile(ui, page),
        Some(note) => note_card(ui, app, page, note, now, false),
    });
    if library.count() == 0 && !library.is_scanning() {
        ui.add_space(14.0);
        widgets::caption(
            ui,
            "Drawings you make are saved here by themselves. Start one, make folders to organize them, or import files you already have.",
        );
    }
}

/// The dashed tile that starts a new drawing in the folder shown.
fn new_tile(ui: &mut Ui, page: &mut Page) {
    let (rect, response) = ui.allocate_exact_size(vec2(CARD_WIDTH, CARD_HEIGHT), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.15);
    let painter = ui.painter();
    let outline = {
        let mut points = theme::rounded_outline(rect.shrink(1.0), [22.0; 4]);
        points.push(points[0]);
        points
    };
    painter.extend(Shape::dashed_line(
        &outline,
        Stroke::new(1.6, theme::TEXT_DIM.lerp_to_gamma(theme::ACCENT, hover)),
        8.0,
        7.0,
    ));
    if hover > 0.0 {
        painter.rect_filled(
            rect.shrink(2.0),
            CornerRadius::same(22),
            theme::ACCENT.gamma_multiply(0.06 * hover),
        );
    }
    let center = pos2(rect.center().x, rect.center().y - 14.0);
    painter.circle_filled(
        center,
        25.0,
        theme::CHROME_HI.lerp_to_gamma(theme::ACCENT, hover),
    );
    let ink = Color32::from_rgb(0x21, 0x17, 0x06).lerp_to_gamma(theme::TEXT, 1.0 - hover);
    let plus = Stroke::new(2.4, ink);
    painter.line_segment([center - vec2(9.0, 0.0), center + vec2(9.0, 0.0)], plus);
    painter.line_segment([center - vec2(0.0, 9.0), center + vec2(0.0, 9.0)], plus);
    painter.text(
        pos2(rect.center().x, rect.center().y + 46.0),
        Align2::CENTER_CENTER,
        "New drawing",
        FontId::proportional(16.0),
        theme::TEXT,
    );
    painter.text(
        pos2(rect.center().x, rect.center().y + 68.0),
        Align2::CENTER_CENTER,
        "in this folder",
        FontId::proportional(12.5),
        theme::TEXT_DIM,
    );
    if response.clicked() {
        page.actions.push(Action::New);
    }
}

/// `true` while something is being dragged that may be dropped on `target`.
fn accepts_drop(app: &InkApp, target: &Path) -> bool {
    app.library
        .drag
        .as_deref()
        .is_some_and(|dragged| library::can_move_into(dragged, target))
}

/// The menu button of a card: three dots on a dark disc, shown while the pointer is on the card.
fn menu_button(ui: &mut Ui, id: Id, center: Pos2, visible: f32, active: bool) -> Response {
    let button = Rect::from_center_size(center, vec2(36.0, 36.0));
    let response = ui.interact(button, id, Sense::click());
    if visible > 0.01 || active {
        let painter = ui.painter();
        painter.circle_filled(
            center,
            16.0,
            Color32::from_black_alpha((170.0 * visible.max(0.4)) as u8),
        );
        for dx in [-6.0, 0.0, 6.0] {
            painter.circle_filled(center + vec2(dx, 0.0), 1.9, Color32::WHITE);
        }
    }
    response
}

fn note_card(
    ui: &mut Ui,
    app: &InkApp,
    page: &mut Page,
    note: &Note,
    now: SystemTime,
    show_location: bool,
) {
    let (slot, response) =
        ui.allocate_exact_size(vec2(CARD_WIDTH, CARD_HEIGHT), Sense::click_and_drag());
    let menu_open = app
        .library
        .menu
        .as_ref()
        .is_some_and(|m| m.path == note.path);
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered() || menu_open, 0.15);
    let lifted = app.library.drag.as_deref() == Some(note.path.as_path());
    let card = slot.translate(vec2(0.0, -5.0 * hover));
    let painter = ui.painter();
    painter.extend(theme::surface(card, [22.0; 4]));

    let thumb = Rect::from_min_size(
        card.min + vec2(10.0, 10.0),
        vec2(CARD_WIDTH - 20.0, THUMB_HEIGHT),
    );
    let [r, g, b] = note.preview.paper;
    painter.rect_filled(thumb, CornerRadius::same(15), Color32::from_rgb(r, g, b));
    if note.unreadable {
        painter.text(
            thumb.center(),
            Align2::CENTER_CENTER,
            "Can\u{2019}t be read",
            FontId::proportional(13.0),
            Color32::from_black_alpha(110),
        );
    } else {
        paint_preview(painter, thumb, &note.preview);
    }
    painter.rect_stroke(
        thumb,
        CornerRadius::same(15),
        Stroke::new(1.0, Color32::from_black_alpha(40)),
        StrokeKind::Inside,
    );

    let text_x = card.left() + 18.0;
    let name = widgets::elided(
        painter,
        &note.name,
        FontId::proportional(15.5),
        theme::TEXT,
        CARD_WIDTH - 36.0,
    );
    painter.galley(pos2(text_x, thumb.bottom() + 12.0), name, theme::TEXT);
    let open = app.is_open(&note.path);
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
        CARD_WIDTH - 36.0 - if open { 52.0 } else { 0.0 },
    );
    painter.galley(pos2(text_x, thumb.bottom() + 35.0), line, theme::TEXT_DIM);
    if open {
        painter.circle_filled(
            pos2(card.right() - 62.0, thumb.bottom() + 42.0),
            3.5,
            theme::ACCENT,
        );
        painter.text(
            pos2(card.right() - 54.0, thumb.bottom() + 42.0),
            Align2::LEFT_CENTER,
            "Open",
            FontId::proportional(12.5),
            theme::ACCENT,
        );
    }
    if lifted {
        painter.rect_filled(card, CornerRadius::same(22), Color32::from_black_alpha(110));
    }

    let center = pos2(thumb.right() - 24.0, thumb.top() + 24.0);
    let more = menu_button(ui, response.id.with("more"), center, hover, menu_open);
    if more.clicked() {
        page.actions.push(Action::ToggleMenu(ItemMenu {
            path: note.path.clone(),
            at: [center.x, center.y],
        }));
    } else if response.clicked() {
        page.actions.push(Action::Open(note.path.clone()));
    }
    if response.drag_started() {
        page.actions.push(Action::BeginDrag(note.path.clone()));
    }
}

/// "5 min ago · 12 strokes": when it was last edited and how much is on it.
fn age_line(now: SystemTime, note: &Note) -> String {
    let age = library::format_age(now, note.modified);
    match note.strokes {
        0 => format!("{age} \u{b7} empty"),
        1 => format!("{age} \u{b7} 1 stroke"),
        n => format!("{age} \u{b7} {n} strokes"),
    }
}

/// A folder as a real one: a back panel with a tab, the newest drawings peeking out, a front panel with the
/// name. It opens when tapped, and takes whatever is dropped on it.
/// Paints a folder: shadow, back panel with its tab, the drawings peeking out, and the front panel with the
/// name and what is inside. Returns the front panel.
fn paint_folder(painter: &Painter, rect: Rect, folder: &FolderCard, hover: f32) -> Rect {
    painter.add(
        Shadow {
            offset: [0, (10.0 + 6.0 * hover) as i8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha((80.0 + 30.0 * hover) as u8),
        }
        .as_shape(rect, CornerRadius::same(20)),
    );
    // The back panel with its tab.
    let tab = Rect::from_min_size(rect.min, vec2(96.0, 30.0));
    painter.rect_filled(
        tab,
        CornerRadius {
            nw: 14,
            ne: 14,
            sw: 0,
            se: 0,
        },
        FOLDER_BACK,
    );
    let back = Rect::from_min_max(rect.min + vec2(0.0, 16.0), rect.max);
    painter.rect_filled(
        back,
        CornerRadius {
            nw: 0,
            ne: 20,
            sw: 20,
            se: 20,
        },
        FOLDER_BACK,
    );

    // What is inside peeks out between the panels.
    let inside = painter.with_clip_rect(back);
    for (i, preview) in folder.previews.iter().enumerate() {
        let sheet = Rect::from_min_size(
            back.min + vec2(16.0 + 66.0 * i as f32, 12.0 + 4.0 * (i % 2) as f32),
            vec2(92.0, 70.0),
        );
        let [r, g, b] = preview.paper;
        inside.rect_filled(sheet, CornerRadius::same(8), Color32::from_rgb(r, g, b));
        paint_preview(&inside, sheet, preview);
        inside.rect_stroke(
            sheet,
            CornerRadius::same(8),
            Stroke::new(1.0, Color32::from_black_alpha(50)),
            StrokeKind::Inside,
        );
    }
    let front = Rect::from_min_max(pos2(rect.left(), back.top() + 58.0), rect.max);
    painter.rect_filled(front, CornerRadius::same(20), FOLDER_FRONT);
    painter.line_segment(
        [
            pos2(front.left() + 20.0, front.top() + 1.0),
            pos2(front.right() - 20.0, front.top() + 1.0),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(40)),
    );
    let name = widgets::elided(
        painter,
        &folder.name,
        FontId::proportional(15.5),
        theme::TEXT,
        CARD_WIDTH - 72.0,
    );
    painter.galley(
        pos2(front.left() + 18.0, front.top() + 14.0),
        name,
        theme::TEXT,
    );
    let count = match folder.drawings {
        0 => "Empty".to_owned(),
        1 => "1 drawing".to_owned(),
        n => format!("{n} drawings"),
    };
    painter.text(
        pos2(front.left() + 18.0, front.top() + 42.0),
        Align2::LEFT_CENTER,
        count,
        FontId::proportional(12.5),
        theme::TEXT.gamma_multiply(0.62),
    );
    front
}

fn folder_card(ui: &mut Ui, app: &InkApp, page: &mut Page, folder: &FolderCard) {
    let (slot, response) =
        ui.allocate_exact_size(vec2(CARD_WIDTH, FOLDER_HEIGHT), Sense::click_and_drag());
    page.drops.push((slot, folder.path.clone()));
    let menu_open = app
        .library
        .menu
        .as_ref()
        .is_some_and(|m| m.path == folder.path);
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered() || menu_open, 0.15);
    let drop_over = accepts_drop(app, &folder.path) && pointer_in(ui, slot);
    let lifted = app.library.drag.as_deref() == Some(folder.path.as_path());
    let rect = slot.translate(vec2(0.0, -4.0 * hover));
    let painter = ui.painter();
    let front = paint_folder(painter, rect, folder, hover);
    if drop_over {
        painter.rect_stroke(
            rect.expand(3.0),
            CornerRadius::same(23),
            Stroke::new(2.4, theme::ACCENT),
            StrokeKind::Outside,
        );
        painter.rect_filled(
            rect,
            CornerRadius::same(20),
            theme::ACCENT.gamma_multiply(0.12),
        );
    }
    if lifted {
        painter.rect_filled(rect, CornerRadius::same(20), Color32::from_black_alpha(110));
    }

    let center = pos2(rect.right() - 26.0, front.top() + 26.0);
    let more = menu_button(ui, response.id.with("more"), center, hover, menu_open);
    if more.clicked() {
        page.actions.push(Action::ToggleMenu(ItemMenu {
            path: folder.path.clone(),
            at: [center.x, center.y],
        }));
    } else if response.clicked() {
        page.actions.push(Action::GoTo(folder.path.clone()));
    }
    if response.drag_started() {
        page.actions.push(Action::BeginDrag(folder.path.clone()));
    }
}

/// The menu of a drawing or folder.
fn item_menu(ctx: &Context, app: &mut InkApp, page: &mut Page) {
    let Some(menu) = app.library.menu.clone() else {
        return;
    };
    let is_folder = menu.path.is_dir();
    let button = Rect::from_center_size(pos2(menu.at[0], menu.at[1]), vec2(40.0, 40.0));
    let area = Area::new(Id::unique("library_item_menu"))
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(pos2(menu.at[0] + 18.0, menu.at[1] + 22.0))
        .order(Order::Tooltip)
        .show(ctx, |ui| {
            theme::floating(ui, [20.0; 4], Margin::same(8), |ui| {
                ui.set_width(204.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                if widgets::menu_row(ui, "Rename", "").clicked() {
                    page.actions.push(Action::AskRename(menu.path.clone()));
                }
                if !is_folder && widgets::menu_row(ui, "Duplicate", "").clicked() {
                    page.actions.push(Action::Duplicate(menu.path.clone()));
                }
                if widgets::menu_row(ui, "Move to\u{2026}", "").clicked() {
                    page.actions.push(Action::OpenMovePicker(MovePicker {
                        path: menu.path.clone(),
                        at: menu.at,
                    }));
                }
                #[cfg(not(target_os = "android"))]
                if widgets::menu_row(ui, "Show in folder", "").clicked() {
                    page.actions.push(Action::Reveal(menu.path.clone()));
                }
                ui.add_space(4.0);
                widgets::divider(ui);
                ui.add_space(4.0);
                let danger = Color32::from_rgb(0xF0, 0x7F, 0x6B);
                if widgets::menu_row_colored(ui, "Delete", "", danger).clicked() {
                    page.actions.push(Action::Delete(menu.path.clone()));
                }
            })
        });
    if pressed_outside(ctx, area.response.rect, button) {
        app.library.menu = None;
    }
}

/// `true` when a press just landed outside both `popup` and `button`.
fn pressed_outside(ctx: &Context, popup: Rect, button: Rect) -> bool {
    ctx.input(|i| i.pointer.any_pressed())
        && ctx
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| !popup.contains(p) && !button.contains(p))
}

/// The folders an item can be moved to, as an indented list.
fn move_picker(ctx: &Context, app: &mut InkApp, page: &mut Page) {
    let Some(picker) = app.library.move_picker.clone() else {
        return;
    };
    let button = Rect::from_center_size(pos2(picker.at[0], picker.at[1]), vec2(40.0, 40.0));
    let choices = app.library.folder_choices();
    let root = app.library.root.clone();
    let area = Area::new(Id::unique("library_move_picker"))
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(pos2(picker.at[0] + 18.0, picker.at[1] + 22.0))
        .order(Order::Tooltip)
        .show(ctx, |ui| {
            theme::floating(ui, [20.0; 4], Margin::same(10), |ui| {
                ui.set_width(268.0);
                ui.label(RichText::new("Move to").size(16.0).strong());
                ui.add_space(6.0);
                ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (folder, depth) in &choices {
                        let enabled = library::can_move_into(&picker.path, folder);
                        let name = if *folder == root {
                            "Library".to_owned()
                        } else {
                            name_of(folder)
                        };
                        if folder_row(ui, &name, *depth, enabled).clicked() && enabled {
                            page.actions
                                .push(Action::MoveInto(picker.path.clone(), folder.clone()));
                        }
                    }
                });
            })
        });
    if pressed_outside(ctx, area.response.rect, button) {
        app.library.move_picker = None;
    }
}

/// One row of the move picker: a folder, indented by how deep it is.
fn folder_row(ui: &mut Ui, name: &str, depth: usize, enabled: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), 38.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let painter = ui.painter();
    if enabled && response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(12), theme::CHROME_HI);
    }
    let color = if enabled {
        theme::TEXT
    } else {
        theme::TEXT_DIM.gamma_multiply(0.55)
    };
    let x = rect.left() + 12.0 + 18.0 * depth as f32;
    paint_folder_glyph(painter, pos2(x + 8.0, rect.center().y), 15.0, color);
    painter.text(
        pos2(x + 26.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(14.5),
        color,
    );
    if !enabled {
        painter.text(
            pos2(rect.right() - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            "here",
            FontId::proportional(12.0),
            theme::TEXT_DIM.gamma_multiply(0.55),
        );
    }
    response
}

/// A small dialog to type a new name.
fn rename_dialog(ctx: &Context, app: &mut InkApp, page: &mut Page) {
    let Some((path, mut text)) = app.library.renaming.clone() else {
        return;
    };
    let title = if path.is_dir() {
        "Rename folder"
    } else {
        "Rename drawing"
    };
    let mut confirm = false;
    let mut cancel = false;
    let modal = Modal::new(Id::unique("library_rename"))
        .backdrop_color(Color32::from_black_alpha(150))
        .frame(
            Frame::new()
                .fill(theme::CHROME)
                .corner_radius(CornerRadius::same(24))
                .inner_margin(Margin::same(24))
                .stroke(theme::hairline())
                .shadow(theme::shadow()),
        )
        .show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(title).size(21.0).strong());
            ui.add_space(12.0);
            let edit = ui.add(
                TextEdit::singleline(&mut text)
                    .font(FontId::proportional(16.0))
                    .desired_width(f32::INFINITY)
                    .margin(Margin::symmetric(12, 10)),
            );
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                confirm = true;
            }
            ui.add_space(16.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if pill(ui, "Rename", true, Glyph::None).clicked() {
                    confirm = true;
                }
                if pill(ui, "Cancel", false, Glyph::None).clicked() {
                    cancel = true;
                }
            });
        });
    if confirm {
        app.library.renaming = None;
        page.actions.push(Action::Rename(path, text));
    } else if cancel || modal.should_close() {
        app.library.renaming = None;
    } else {
        app.library.renaming = Some((path, text));
    }
}

/// A message at the bottom after a delete, with the way back.
fn undo_bar(ctx: &Context, app: &mut InkApp, page: &mut Page) {
    let now = ctx.input(|i| i.time);
    let Some(deleted) = &app.library.deleted else {
        return;
    };
    if now - deleted.at > UNDO_SECONDS {
        app.library.deleted = None;
        return;
    }
    let name = deleted.name.clone();
    Area::new(Id::unique("library_undo"))
        .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -30.0))
        .order(Order::Tooltip)
        .show(ctx, |ui| {
            theme::floating(ui, [22.0; 4], Margin::symmetric(22, 12), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 18.0;
                    ui.label(RichText::new(format!("Deleted \u{201c}{name}\u{201d}")).size(14.5));
                    let undo = ui.add(
                        egui::Label::new(
                            RichText::new("Undo")
                                .size(14.5)
                                .strong()
                                .color(theme::ACCENT),
                        )
                        .sense(Sense::click()),
                    );
                    if undo.clicked() {
                        page.actions.push(Action::Undo);
                    }
                });
            });
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
}

/// While something is dragged: a label that follows the pointer, and the drop when the button is let go.
fn dragging(ctx: &Context, app: &mut InkApp, page: &mut Page) {
    let Some(path) = app.library.drag.clone() else {
        return;
    };
    ctx.set_cursor_icon(CursorIcon::Grabbing);
    let pointer = ctx.input(|i| i.pointer.latest_pos());
    if let Some(at) = pointer {
        let name = name_of(&path);
        let folder = path.is_dir();
        Area::new(Id::unique("library_drag"))
            .fixed_pos(at + vec2(16.0, 12.0))
            .order(Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                theme::floating(ui, [18.0; 4], Margin::symmetric(16, 9), |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if folder {
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(18.0, 16.0), Sense::hover());
                            paint_folder_glyph(ui.painter(), rect.center(), 15.0, theme::TEXT);
                        }
                        ui.label(RichText::new(name).size(14.0));
                    });
                });
            });
    }
    if !ctx.input(|i| i.pointer.any_down()) {
        if let Some(at) = pointer
            && let Some((_, target)) = page
                .drops
                .iter()
                .find(|(area, target)| area.contains(at) && library::can_move_into(&path, target))
        {
            page.actions.push(Action::MoveInto(path, target.clone()));
        }
        app.library.drag = None;
    }
}

fn apply(ctx: &Context, app: &mut InkApp, action: Action) {
    match action {
        Action::New => app.new_tab(ctx),
        Action::NewFolder => app.library_new_folder(ctx),
        Action::Import => app.import_notes(ctx),
        Action::GoTo(folder) => {
            app.library.menu = None;
            app.library.query.clear();
            app.library.go_to(folder);
        }
        Action::Open(path) => app.open_note(ctx, &path),
        Action::ToggleMenu(menu) => {
            let same = app
                .library
                .menu
                .as_ref()
                .is_some_and(|m| m.path == menu.path);
            app.library.move_picker = None;
            app.library.menu = (!same).then_some(menu);
        }
        Action::AskRename(path) => app.library_ask_rename(path),
        Action::Rename(path, name) => app.library_rename(ctx, &path, name),
        Action::Duplicate(path) => app.library_duplicate(ctx, &path),
        Action::Delete(path) => app.library_delete(ctx, &path),
        Action::Reveal(path) => {
            app.library.menu = None;
            reveal(&path);
        }
        Action::OpenMovePicker(picker) => {
            app.library.menu = None;
            app.library.move_picker = Some(picker);
        }
        Action::MoveInto(path, target) => app.library_move_into(ctx, &path, &target),
        Action::BeginDrag(path) => {
            app.library.menu = None;
            app.library.drag = Some(path);
        }
        Action::Undo => app.library_undo_delete(ctx),
    }
}

/// Shows the file in the system's file manager.
#[cfg(not(target_os = "android"))]
fn reveal(path: &Path) {
    #[cfg(windows)]
    let result = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .spawn();
    if let Err(e) = result {
        eprintln!("could not open the file manager: {e}");
    }
}

#[cfg(target_os = "android")]
fn reveal(_path: &Path) {}

#[cfg(test)]
mod tests {
    use ink_core::{Color, Pen, Point, StrokeBuilder};

    use crate::Options;

    use super::*;

    fn temp_folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("inkwell-page-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp folder");
        dir
    }

    fn app_in(root: &Path) -> InkApp {
        let mut app = InkApp::blank(Options::default());
        app.config.files.library_folder = Some(root.display().to_string());
        app.library.root = root.to_owned();
        app.library.current = root.to_owned();
        app.start_note();
        app
    }

    /// Draws a line on the open drawing and saves it.
    fn write_something(app: &mut InkApp, ctx: &Context) -> PathBuf {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.1);
        for i in 0..10 {
            b.push(Point::new(i as f32 * 5.0, 0.0), None, f64::from(i) * 0.01);
        }
        let stroke = b.with_id(app.editor.next_id()).finish();
        app.editor.commit_stroke(app.active_layer, stroke);
        app.flush_autosave(ctx);
        app.path.clone().expect("the drawing has a place")
    }

    #[test]
    fn renaming_the_open_drawing_keeps_its_tab_under_the_new_name() {
        let root = temp_folder("rename");
        let ctx = Context::default();
        let mut app = app_in(&root);
        let old = write_something(&mut app, &ctx);
        apply(
            &ctx,
            &mut app,
            Action::Rename(old.clone(), "Plans".to_owned()),
        );
        assert_eq!(app.path, Some(root.join("Plans.ink")));
        assert!(!old.exists() && root.join("Plans.ink").exists());
        assert!(!app.is_dirty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_refused_rename_keeps_the_dialog_open_with_what_was_typed() {
        let root = temp_folder("refused");
        let ctx = Context::default();
        let mut app = app_in(&root);
        let first = write_something(&mut app, &ctx);
        std::fs::write(root.join("Taken.ink"), "x").expect("file");
        apply(
            &ctx,
            &mut app,
            Action::Rename(first.clone(), "Taken".to_owned()),
        );
        assert!(first.exists(), "nothing was renamed");
        assert_eq!(
            app.library.renaming,
            Some((first, "Taken".to_owned())),
            "the dialog stays so the name can be fixed"
        );
        assert!(app.message.is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deleting_the_open_drawing_closes_its_tab_and_undo_brings_the_file_back() {
        let root = temp_folder("delete");
        let ctx = Context::default();
        let mut app = app_in(&root);
        let path = write_something(&mut app, &ctx);
        apply(&ctx, &mut app, Action::Delete(path.clone()));
        assert!(!path.exists());
        assert!(app.library.deleted.is_some());
        assert!(app.tabs.is_empty(), "its tab went with it");
        assert!(app.ui.library_open);
        apply(&ctx, &mut app, Action::Undo);
        assert!(path.exists());
        assert!(app.library.deleted.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_folder_asks_for_its_name_and_creates_it_in_the_folder_shown() {
        let root = temp_folder("new-folder");
        let ctx = Context::default();
        let mut app = app_in(&root);
        apply(&ctx, &mut app, Action::NewFolder);
        assert!(root.join("New folder").is_dir());
        assert_eq!(
            app.library.renaming,
            Some((root.join("New folder"), "New folder".to_owned()))
        );
        apply(
            &ctx,
            &mut app,
            Action::Rename(root.join("New folder"), "Work".to_owned()),
        );
        assert!(root.join("Work").is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn moving_the_open_drawing_into_a_folder_keeps_it_open_and_saving_there() {
        let root = temp_folder("move");
        let ctx = Context::default();
        let mut app = app_in(&root);
        let path = write_something(&mut app, &ctx);
        let work = library::create_folder(&root, "Work").expect("folder");
        apply(&ctx, &mut app, Action::MoveInto(path.clone(), work.clone()));
        let moved = work.join("Untitled.ink");
        assert!(moved.exists() && !path.exists());
        assert_eq!(app.path, Some(moved.clone()));
        assert!(app.autosaves(), "still inside the library");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_cannot_be_dropped_into_itself() {
        let root = temp_folder("loop");
        let ctx = Context::default();
        let mut app = app_in(&root);
        let work = library::create_folder(&root, "Work").expect("folder");
        let inner = library::create_folder(&work, "Inner").expect("folder");
        apply(&ctx, &mut app, Action::MoveInto(work.clone(), inner));
        assert!(work.is_dir());
        assert!(app.message.is_some(), "the user is told");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deleting_a_folder_closes_the_tabs_inside_it() {
        let root = temp_folder("delete-folder");
        let ctx = Context::default();
        let mut app = app_in(&root);
        let work = library::create_folder(&root, "Work").expect("folder");
        app.library.current = work.clone();
        app.new_tab(&ctx);
        let inside = write_something(&mut app, &ctx);
        assert_eq!(inside.parent(), Some(work.as_path()));
        let tabs = app.tabs.len();
        apply(&ctx, &mut app, Action::Delete(work.clone()));
        assert!(!work.exists());
        assert_eq!(app.tabs.len(), tabs - 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_menu_of_an_item_opens_and_closes_with_the_same_button() {
        let root = temp_folder("menu");
        let ctx = Context::default();
        let mut app = app_in(&root);
        let path = root.join("A.ink");
        let menu = || ItemMenu {
            path: path.clone(),
            at: [10.0, 10.0],
        };
        apply(&ctx, &mut app, Action::ToggleMenu(menu()));
        assert!(app.library.menu.is_some());
        apply(&ctx, &mut app, Action::ToggleMenu(menu()));
        assert!(app.library.menu.is_none());
        apply(&ctx, &mut app, Action::AskRename(path.clone()));
        assert_eq!(
            app.library.renaming.as_ref().map(|(_, name)| name.as_str()),
            Some("A")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn escape_puts_away_one_thing_at_a_time_and_only_then_leaves_the_page() {
        let root = temp_folder("escape");
        let mut app = app_in(&root);
        app.ui.library_open = true;
        app.library.query = "abc".to_owned();
        app.library.move_picker = Some(MovePicker {
            path: root.join("A.ink"),
            at: [0.0, 0.0],
        });
        escape(&mut app);
        assert!(app.library.move_picker.is_none() && app.library.is_searching());
        escape(&mut app);
        assert!(!app.library.is_searching() && app.ui.library_open);
        escape(&mut app);
        assert!(!app.ui.library_open);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn with_no_drawing_open_escape_does_not_leave_the_page() {
        let root = temp_folder("escape-none");
        let mut app = app_in(&root);
        app.clear_tabs();
        app.ui.library_open = true;
        escape(&mut app);
        assert!(app.ui.library_open, "there is nothing to go back to");
        let _ = std::fs::remove_dir_all(&root);
    }
}
