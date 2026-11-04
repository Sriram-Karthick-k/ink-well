//! The tools along the bottom of a screen held upright, on a device used by touch.
//!
//! One floating panel in two rows. The top row is the pens, each a big chip with its tip showing, the one in use
//! ringed. The row under it is the person's own: whatever they chose to have on the bar, in the order they chose, each a
//! big button with its name under its icon (the row scrolls sideways where the screen is too narrow for all of them),
//! and at its end a button to arrange it. Everything is at least a fingertip across.
//!
//! The panel and everything in it is placed from sizes that are known (see `Geometry`), not from what was measured
//! last frame, so nothing is ever in the wrong place for a frame.

use eframe::egui::ScrollArea;
use eframe::egui::scroll_area::ScrollBarVisibility;
use eframe::egui::{
    Align2, Area, Color32, Context, CornerRadius, FontId, Id, Margin, Order, Rect, Response, Sense,
    Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2,
};
use ink_core::Pen;

use super::bar_items;
use crate::app::InkApp;
use crate::icons::{self, Icon};
use crate::pen_art::{self, Art, Placement};
use crate::state::Mode;
use crate::theme;
use crate::toolbar::Item;

/// How far the panel is from the edges of the window.
const GAP: f32 = 12.0;
const MAX_WIDTH: f32 = 720.0;
/// The space between the edge of the panel and what is in it.
const MARGIN: f32 = 12.0;
/// The height of the row of pens and of the row of tools, and the space between them.
const PEN_ROW: f32 = 64.0;
const TOOL_ROW: f32 = 60.0;
const ROW_GAP: f32 = 8.0;
/// The space between two pens.
const PEN_GAP: f32 = 8.0;
/// The height of the whole panel; the density gives the same number to what has to clear it.
pub const HEIGHT: f32 = 2.0 * MARGIN + PEN_ROW + ROW_GAP + TOOL_ROW;
/// How wide a button of the row of tools is, at the least and at the most the room allows.
const CELL: (f32, f32) = (46.0, 64.0);

/// What an item of the bar puts the canvas in, for the items that are a tool.
fn mode_of(item: &Item) -> Option<Mode> {
    match item {
        Item::Eraser => Some(Mode::Erase),
        Item::Select => Some(Mode::Select),
        Item::Slice => Some(Mode::Slice),
        Item::Nudge => Some(Mode::Nudge),
        Item::Move => Some(Mode::Pan),
        _ => None,
    }
}

/// Whether the item is lit: the tool in use, or the panel it opened.
fn is_active(app: &InkApp, item: &Item) -> bool {
    match item {
        Item::Pens => app.ui.case_open,
        Item::Color => app.ui.color_open,
        Item::Shapes => app.ui.shapes_open,
        Item::Text => app.ui.text.is_some(),
        Item::Move => app.tool.mode == Mode::Pan || app.pan_held,
        other => mode_of(other) == Some(app.tool.mode),
    }
}

/// Where everything is, worked out from the part of the window the panel may use and how many buttons the row has.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    /// The whole panel.
    panel: Rect,
    /// Where its contents go.
    inner: Rect,
    /// How wide a button of the row of tools is.
    cell: f32,
    /// How many buttons the row has.
    cells: usize,
}

impl Geometry {
    fn of(screen: Rect, cells: usize) -> Self {
        let width = (screen.width() - 2.0 * GAP).min(MAX_WIDTH);
        let panel = Rect::from_min_size(
            pos2(
                screen.center().x - width / 2.0,
                screen.bottom() - GAP - HEIGHT,
            ),
            vec2(width, HEIGHT),
        );
        let inner = panel.shrink(MARGIN);
        Self {
            panel,
            inner,
            cell: (inner.width() / cells.max(1) as f32).clamp(CELL.0, CELL.1),
            cells,
        }
    }

    /// The `i`th of `count` pens.
    fn pen(&self, i: usize, count: usize) -> Rect {
        let n = count.max(1) as f32;
        let width = (self.inner.width() - PEN_GAP * (n - 1.0)) / n;
        Rect::from_min_size(
            pos2(
                self.inner.left() + i as f32 * (width + PEN_GAP),
                self.inner.top(),
            ),
            vec2(width, PEN_ROW),
        )
    }

    fn row_top(&self) -> f32 {
        self.inner.bottom() - TOOL_ROW
    }

    /// The room the row of buttons has.
    fn row(&self) -> Rect {
        Rect::from_min_max(pos2(self.inner.left(), self.row_top()), self.inner.max)
    }

    /// Where the buttons start across: centered in the room when they all fit.
    fn start(&self) -> f32 {
        ((self.inner.width() - self.cells as f32 * self.cell) / 2.0).max(0.0)
    }

    /// The `i`th button, when they all fit (the row is a scroll area, which lays them out as this says).
    #[cfg(test)]
    fn button(&self, i: usize) -> Rect {
        Rect::from_min_size(
            pos2(
                self.inner.left() + self.start() + i as f32 * self.cell,
                self.row_top(),
            ),
            vec2(self.cell, TOOL_ROW),
        )
    }
}

/// Shows the panel. It sets where flyouts open from (above it), and it is a panel, so a touch on it is a press and
/// not a stroke.
pub fn show(ctx: &Context, app: &mut InkApp) {
    let items = app.config.toolbar_items();
    let g = Geometry::of(ctx.content_rect(), items.len() + 1);
    app.ui.anchors.dock_top = g.panel.top();
    let pens = app.config.rail_pens();
    let alpha = app.chrome_alpha;
    let area = Area::new(Id::unique("touch_tools"))
        .fixed_pos(g.panel.min)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            theme::floating(ui, [24.0; 4], Margin::same(MARGIN as i8), |ui| {
                ui.set_width(g.inner.width());
                ui.set_height(g.inner.height());
                pen_row(ui, app, &g, &pens);
                tool_row(ctx, ui, app, &g, &items);
            })
        });
    app.chrome.push(area.inner.response.rect);
}

fn pen_row(ui: &mut Ui, app: &mut InkApp, g: &Geometry, pens: &[Pen]) {
    let mut chosen = None;
    for (i, pen) in pens.iter().enumerate() {
        if pen_chip(ui, app, g.pen(i, pens.len()), *pen, i) {
            chosen = Some(*pen);
        }
    }
    if let Some(pen) = chosen {
        app.choose_pen(pen);
    }
}

/// One pen: the front of it lying in a slot with its tip to the right. The one in use is lifted and ringed. Returns
/// `true` when it was tapped.
fn pen_chip(ui: &mut Ui, app: &InkApp, rect: Rect, pen: Pen, index: usize) -> bool {
    let response = ui.interact(rect, Id::unique(("touch_pen", index)), Sense::click());
    let selected = app.tool.mode == Mode::Draw && app.tool.pen == pen;
    let lift = ui.ctx().animate_value_with_time(
        response.id.with("lift"),
        if selected { 1.0 } else { 0.0 },
        0.16,
    );
    let painter = ui.painter().with_clip_rect(rect);
    let corners = CornerRadius::same(18);
    let rest = Color32::from_black_alpha(70);
    painter.rect_filled(
        rect,
        corners,
        if response.is_pointer_button_down_on() {
            rest.lerp_to_gamma(Color32::from_white_alpha(34), 1.0)
        } else {
            rest
        },
    );
    if lift > 0.01 {
        painter.rect_filled(rect, corners, theme::ACCENT.gamma_multiply(0.14 * lift));
        painter.rect_stroke(
            rect,
            corners,
            Stroke::new(2.0, theme::ACCENT.gamma_multiply(lift)),
            StrokeKind::Inside,
        );
    }
    let [r, g, b] = app.tool.color.to_srgb();
    pen_art::paint(
        &painter,
        Art::of(pen),
        Placement {
            tip: pos2(rect.right() - 12.0, rect.center().y - 3.0 * lift),
            scale: 1.25,
            direction: 1.0,
            lift,
        },
        Color32::from_rgb(r, g, b),
    );
    response.clicked()
}

fn tool_row(ctx: &Context, ui: &mut Ui, app: &mut InkApp, g: &Geometry, items: &[Item]) {
    let (can_undo, can_redo) = (app.editor.can_undo(), app.editor.can_redo());
    // The buttons go in a child of their own and the panel's size is set from what is known, so the scroll area never
    // makes the panel taller.
    let mut tapped = None;
    let mut arrange = false;
    let mut row = ui.new_child(UiBuilder::new().max_rect(g.row()));
    ScrollArea::horizontal()
        .id_salt("touch_tools_row")
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
        .auto_shrink([false, false])
        .show(&mut row, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(g.start());
                for item in items {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(g.cell, TOOL_ROW), Sense::click());
                    let enabled = match item {
                        Item::Undo => can_undo,
                        Item::Redo => can_redo,
                        _ => true,
                    };
                    paint_cell(
                        ui,
                        rect,
                        &response,
                        app,
                        item,
                        is_active(app, item),
                        enabled,
                    );
                    if item == &Item::Color {
                        app.ui.anchors.color = rect.center().y;
                    }
                    if response.clicked() && enabled {
                        tapped = Some(item.clone());
                    }
                }
                let (rect, response) =
                    ui.allocate_exact_size(vec2(g.cell, TOOL_ROW), Sense::click());
                paint_arrange(ui, rect, &response);
                arrange = response.clicked();
            });
        });
    if arrange {
        app.ui.open_toolbar_editor();
    }
    if let Some(item) = tapped {
        press(ctx, app, &item);
    }
}

/// What a tap on an item does.
fn press(ctx: &Context, app: &mut InkApp, item: &Item) {
    match item {
        Item::Undo => {
            app.undo();
        }
        Item::Redo => {
            app.redo();
        }
        Item::Pens => app.ui.toggle_case(),
        Item::Color => app.ui.toggle_color(),
        Item::Shapes => app.ui.toggle_shapes(),
        Item::Text => app.toggle_text(ctx),
        Item::Shape(id) => {
            app.insert_shape(ctx, id);
        }
        other => {
            if let Some(mode) = mode_of(other) {
                app.choose_tool(mode);
            }
        }
    }
}

/// A picture with its name under it, in `rect`. `active` lights it; not `enabled` dims it.
fn paint_cell(
    ui: &Ui,
    rect: Rect,
    response: &Response,
    app: &InkApp,
    item: &Item,
    active: bool,
    enabled: bool,
) {
    let painter = ui.painter();
    let body = rect.shrink(2.0);
    if active {
        painter.rect_filled(
            body,
            CornerRadius::same(16),
            theme::ACCENT.gamma_multiply(0.22),
        );
        painter.rect_stroke(
            body,
            CornerRadius::same(16),
            Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.6)),
            StrokeKind::Inside,
        );
    } else if enabled && response.is_pointer_button_down_on() {
        painter.rect_filled(body, CornerRadius::same(16), Color32::from_white_alpha(22));
    }
    let (icon_color, text_color) = if !enabled {
        let dim = theme::TEXT_DIM.gamma_multiply(0.45);
        (dim, dim)
    } else if active {
        (theme::ACCENT, theme::ACCENT)
    } else {
        (theme::TEXT, theme::TEXT_DIM)
    };
    bar_items::paint(
        painter,
        Rect::from_center_size(pos2(rect.center().x, rect.top() + 22.0), vec2(22.0, 22.0)),
        app,
        item,
        icon_color,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 12.0),
        Align2::CENTER_CENTER,
        short(&bar_items::label(app, item)),
        FontId::proportional(11.0),
        text_color,
    );
}

/// The last button of the row: it opens the sheet where the row is arranged. It is dim, for it is not a tool.
fn paint_arrange(ui: &Ui, rect: Rect, response: &Response) {
    let painter = ui.painter();
    if response.is_pointer_button_down_on() {
        painter.rect_filled(
            rect.shrink(2.0),
            CornerRadius::same(16),
            Color32::from_white_alpha(22),
        );
    }
    let dim = theme::TEXT_DIM.gamma_multiply(0.8);
    icons::paint(
        painter,
        Rect::from_center_size(pos2(rect.center().x, rect.top() + 22.0), vec2(22.0, 22.0)),
        Icon::Sliders,
        dim,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 12.0),
        Align2::CENTER_CENTER,
        "Arrange",
        FontId::proportional(11.0),
        dim,
    );
}

/// A name cut to what fits under a button.
fn short(name: &str) -> String {
    const MOST: usize = 9;
    if name.chars().count() <= MOST {
        return name.to_owned();
    }
    let cut: String = name.chars().take(MOST - 1).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Vec2};

    use super::*;
    use crate::Options;
    use crate::density::Density;

    /// The number of buttons the usual bar has: its items and the one that arranges them.
    const USUAL: usize = crate::toolbar::DEFAULT.len() + 1;

    #[test]
    fn the_panel_is_as_tall_as_the_density_says_so_what_must_clear_it_does() {
        assert!((HEIGHT - Density::TOUCH.dock_height).abs() < 1e-4);
    }

    #[test]
    fn the_panel_is_inside_the_screen_on_a_tablet_and_on_a_phone() {
        for (w, h) in [(923.0, 1305.0), (360.0, 780.0), (320.0, 568.0)] {
            for cells in [2, USUAL, 30] {
                let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h));
                let g = Geometry::of(screen, cells);
                assert!(screen.contains_rect(g.panel), "{w}x{h}: {:?}", g.panel);
                assert!(g.row().width() > 0.0, "{w}x{h}: there is room for the row");
            }
        }
    }

    #[test]
    fn the_pens_share_the_width_without_overlapping() {
        let g = Geometry::of(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(360.0, 780.0)),
            USUAL,
        );
        let rects: Vec<Rect> = (0..5).map(|i| g.pen(i, 5)).collect();
        for pair in rects.windows(2) {
            assert!(pair[0].right() <= pair[1].left(), "{pair:?}");
        }
        assert!(g.inner.contains_rect(rects[0]) && g.inner.contains_rect(rects[4]));
        assert!(rects.iter().all(|r| r.width() >= 40.0), "{rects:?}");
    }

    #[test]
    fn on_a_wide_screen_the_usual_bar_fits_with_buttons_big_enough_to_hit() {
        let g = Geometry::of(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(923.0, 1305.0)),
            USUAL,
        );
        assert!(g.button(USUAL - 1).right() <= g.row().right() + 1e-3);
        assert!(g.cell >= 44.0);
    }

    #[test]
    fn a_bar_with_room_to_spare_is_centered() {
        let g = Geometry::of(Rect::from_min_size(pos2(0.0, 0.0), vec2(923.0, 1305.0)), 6);
        assert!(g.start() > 0.0, "centered");
        assert!(g.button(5).right() <= g.row().right());
    }

    #[test]
    fn a_bar_with_more_than_fits_keeps_its_buttons_big_and_scrolls() {
        let g = Geometry::of(
            Rect::from_min_size(pos2(0.0, 0.0), vec2(360.0, 780.0)),
            USUAL + 6,
        );
        assert!(g.cell >= 44.0, "never squeezed below a fingertip");
        assert!(g.start().abs() < 1e-4, "it starts at the edge");
        assert!(
            g.button(USUAL + 5).right() > g.row().right(),
            "the end is past the edge, to scroll to"
        );
    }

    #[test]
    fn a_bar_of_few_buttons_does_not_stretch_them_across_the_panel() {
        let g = Geometry::of(Rect::from_min_size(pos2(0.0, 0.0), vec2(923.0, 1305.0)), 3);
        assert!((g.cell - CELL.1).abs() < 1e-4);
        assert!(g.start() > 100.0);
    }

    /// The app, a frame at a time, showing only the tool panel.
    struct Screen {
        ctx: Context,
        app: InkApp,
        time: f64,
        size: Vec2,
    }

    impl Screen {
        fn new() -> Self {
            let mut s = Self {
                ctx: Context::default(),
                app: InkApp::blank(Options::default()),
                time: 0.0,
                size: vec2(923.0, 1305.0),
            };
            s.app.chrome_alpha = 1.0;
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
            crate::density::set(&self.ctx, Density::TOUCH);
            let app = &mut self.app;
            let mut output = self.ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                show(&ctx, app);
            });
            output.textures_delta.clear(); // nothing is rendered in tests
        }

        fn settle(&mut self) {
            for _ in 0..3 {
                self.frame(Vec::new());
            }
        }

        fn geometry(&self) -> Geometry {
            Geometry::of(
                Rect::from_min_size(pos2(0.0, 0.0), self.size),
                self.app.config.toolbar_items().len() + 1,
            )
        }

        /// The center of the button for the item kept under `key`.
        fn at(&self, key: &str) -> Pos2 {
            let at = self
                .app
                .config
                .toolbar_items()
                .iter()
                .position(|item| item.key() == key)
                .unwrap_or_else(|| panic!("{key} is not on the bar"));
            self.geometry().button(at).center()
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
    }

    #[test]
    fn tapping_a_pen_makes_it_the_pen_in_use() {
        let mut s = Screen::new();
        let pens = s.app.config.rail_pens();
        let target = pens[2];
        assert_ne!(s.app.tool.pen, target, "it is not the pen in use yet");
        let at = s.geometry().pen(2, pens.len()).center();
        s.tap(at);
        assert_eq!(s.app.tool.pen, target);
        assert_eq!(s.app.tool.mode, Mode::Draw);
    }

    #[test]
    fn the_eraser_and_the_hand_are_tools_of_the_row_and_the_hand_switches_off_again() {
        let mut s = Screen::new();
        let eraser = s.at("eraser");
        s.tap(eraser);
        assert_eq!(s.app.tool.mode, Mode::Erase);
        let hand = s.at("move");
        s.tap(hand);
        assert_eq!(s.app.tool.mode, Mode::Pan);
        s.tap(hand);
        assert_eq!(s.app.tool.mode, Mode::Draw, "the hand goes back to drawing");
    }

    #[test]
    fn the_ink_opens_the_color_panel_and_all_pens_opens_the_case() {
        let mut s = Screen::new();
        let ink = s.at("color");
        s.tap(ink);
        assert!(s.app.ui.color_open);
        let pens = s.at("pens");
        s.tap(pens);
        assert!(s.app.ui.case_open && !s.app.ui.color_open);
    }

    #[test]
    fn the_shapes_button_opens_the_shapes_menu_and_the_last_button_arranges_the_bar() {
        let mut s = Screen::new();
        let shapes = s.at("shapes");
        s.tap(shapes);
        assert!(s.app.ui.shapes_open);
        s.app.ui.close_popovers();
        let arrange = s.geometry().button(USUAL - 1).center();
        s.tap(arrange);
        assert!(s.app.ui.toolbar_open && !s.app.ui.shapes_open);
    }

    #[test]
    fn undo_does_nothing_while_there_is_nothing_to_undo() {
        let mut s = Screen::new();
        assert!(!s.app.editor.can_undo());
        let undo = s.at("undo");
        s.tap(undo);
        assert!(!s.app.editor.can_undo());
    }

    #[test]
    fn the_bar_is_what_the_person_made_of_it() {
        let mut s = Screen::new();
        s.app.config.interface.toolbar = vec!["pens".to_owned(), "eraser".to_owned()];
        s.settle();
        let first = s.geometry().button(0).center();
        s.tap(first);
        assert!(
            s.app.ui.case_open,
            "the first button is now the case of pens"
        );
        let second = s.geometry().button(1).center();
        s.tap(second);
        assert_eq!(s.app.tool.mode, Mode::Erase);
    }

    #[test]
    fn a_shape_pinned_to_the_bar_is_put_on_the_paper_with_one_tap() {
        let mut s = Screen::new();
        s.app.config.toolbar_add(Item::Shape("star".to_owned()));
        s.settle();
        let before: usize = s
            .app
            .editor
            .document()
            .layers
            .iter()
            .map(|l| l.strokes.len())
            .sum();
        let star = s.at("shape:star");
        s.tap(star);
        let after: usize = s
            .app
            .editor
            .document()
            .layers
            .iter()
            .map(|l| l.strokes.len())
            .sum();
        assert_eq!(after, before + 1);
        assert_eq!(s.app.tool.mode, Mode::Select, "ready to move and resize");
        assert_eq!(s.app.selection.len(), 1);
    }

    #[test]
    fn a_pinned_shape_that_is_gone_is_a_button_that_does_nothing() {
        let mut s = Screen::new();
        s.app
            .config
            .toolbar_add(Item::Shape("no_such_shape".to_owned()));
        s.settle();
        let gone = s.at("shape:no_such_shape");
        s.tap(gone);
        assert_eq!(s.app.selection.len(), 0);
    }

    #[test]
    fn the_frame_is_exactly_the_panel_and_does_not_grow_past_its_contents() {
        let s = Screen::new();
        let frame = *s.app.chrome.last().expect("the panel");
        let panel = s.geometry().panel;
        assert!(
            (frame.height() - panel.height()).abs() < 0.5,
            "{frame:?} against {panel:?}"
        );
        assert!(
            (frame.top() - panel.top()).abs() < 0.5,
            "{frame:?} against {panel:?}"
        );
    }

    #[test]
    fn the_panel_says_where_flyouts_open_from() {
        let s = Screen::new();
        assert!((s.app.ui.anchors.dock_top - s.geometry().panel.top()).abs() < 1e-3);
    }

    #[test]
    fn a_long_name_is_cut_to_fit_under_a_button() {
        assert_eq!(short("Eraser"), "Eraser");
        assert_eq!(short("Rounded rectangle"), "Rounded…");
        assert!(short("Rounded rectangle").chars().count() <= 9);
    }
}
