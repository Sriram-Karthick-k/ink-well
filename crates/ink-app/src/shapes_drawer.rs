//! The shapes drawer: every shape and object, kept open at the side of the paper to drag from.
//!
//! The menu of shapes (see `shapes_panel` and `touch_ui::shapes_sheet`) closes after every shape put down, which is slow
//! for a diagram that needs a dozen. The drawer stays where it is, on whichever side the person likes: a shape is
//! dragged out of it and let go where it should be, or tapped to land in the middle of the paper. It is one thing for the
//! computer and for a device used by touch; only the size of what is in it differs.

use eframe::egui::{
    Area, Color32, Context, CornerRadius, Id, LayerId, Margin, Order, Rect, ScrollArea, Sense, Ui,
    pos2,
    scroll_area::{DragScroll, ScrollSource},
    vec2,
};
use ink_core::shapes::Shape;

use crate::app::InkApp;
use crate::canvas;
use crate::config::DrawerSide;
use crate::icons::Icon;
use crate::platform;
use crate::rail;
use crate::shape_art::{self, Look, Tile};
use crate::theme;
use crate::widgets;

/// How many shapes go across the drawer.
const COLUMNS: usize = 2;
/// The space between two shapes, and between the edge of the drawer and what is in it.
const GAP: f32 = 8.0;
const MARGIN: f32 = 12.0;
/// How far the drawer is from the edges of the paper.
const EDGE: f32 = 10.0;
/// How far a shape is dragged before it is being carried, in points.
const CARRY: f32 = 10.0;

/// How wide a shape in the drawer is: a fingertip and more on a device used by touch.
fn tile_side() -> f32 {
    if platform::touch_first() { 72.0 } else { 60.0 }
}

/// How wide the drawer is.
fn width() -> f32 {
    COLUMNS as f32 * tile_side() + (COLUMNS as f32 - 1.0) * GAP + 2.0 * MARGIN
}

/// Where the drawer is on `paper`, when it is on `side`: the full height of what is free there, clear of the tools along
/// the bottom (`dock_top`, when there are some) and of the rail on the edge of a window used with a mouse.
#[must_use]
pub(crate) fn place(
    paper: Rect,
    side: DrawerSide,
    dock_top: f32,
    rail_on_left: Option<bool>,
) -> Option<Rect> {
    let bottom = if dock_top > paper.top() && dock_top < paper.bottom() {
        dock_top - EDGE
    } else {
        paper.bottom() - EDGE
    };
    let rail = rail::flyout_offset();
    let (left, right) = match (side, rail_on_left) {
        (DrawerSide::Off, _) => return None,
        (DrawerSide::Left, Some(true)) => (paper.left() + rail, paper.left() + rail + width()),
        (DrawerSide::Left, _) => (paper.left() + EDGE, paper.left() + EDGE + width()),
        (DrawerSide::Right, Some(false)) => (paper.right() - rail - width(), paper.right() - rail),
        (DrawerSide::Right, _) => (paper.right() - EDGE - width(), paper.right() - EDGE),
    };
    let rect = Rect::from_min_max(pos2(left, paper.top() + EDGE), pos2(right, bottom));
    (rect.height() > 120.0).then_some(rect)
}

/// Shows the drawer, while it is open.
pub fn show(ctx: &Context, app: &mut InkApp) {
    let side = app.config.interface.shapes_drawer;
    let rail_on_left = (!platform::touch_first()).then_some(!app.config.interface.left_handed);
    let Some(paper) = app.canvas_rect else {
        return;
    };
    let Some(rect) = place(paper, side, app.ui.anchors.dock_top, rail_on_left) else {
        app.ui.anchors.drawer = None;
        return;
    };
    app.ui.anchors.drawer = Some(rect);
    let alpha = app.chrome_alpha;
    let mut action = None;
    let area = Area::new(Id::unique("shapes_drawer"))
        .fixed_pos(rect.min)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            theme::floating(ui, [20.0; 4], Margin::same(MARGIN as i8), |ui| {
                ui.set_width(rect.width() - 2.0 * MARGIN);
                ui.set_height(rect.height() - 2.0 * MARGIN);
                action = contents(ui, app);
            });
        });
    // A press on the drawer is not a press outside the panels.
    app.chrome.push(area.response.rect);
    carry(ctx, app, rect);
    match action {
        Some(Act::Close) => {
            app.config.interface.shapes_drawer = DrawerSide::Off;
            app.mark_config_changed();
        }
        Some(Act::Swap) => {
            app.config.interface.shapes_drawer = match side {
                DrawerSide::Left => DrawerSide::Right,
                _ => DrawerSide::Left,
            };
            app.mark_config_changed();
        }
        Some(Act::Options) => app.ui.toggle_shapes(),
        None => {}
    }
}

/// Something done in the header of the drawer.
enum Act {
    Close,
    Swap,
    Options,
}

fn contents(ui: &mut Ui, app: &mut InkApp) -> Option<Act> {
    let mut act = None;
    // Three buttons and no title: the drawer is narrow, and what is in it says what it is.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if widgets::round_button(ui, 28.0, Icon::Sliders, true, false)
            .on_hover_text("Fill, outline and size")
            .clicked()
        {
            act = Some(Act::Options);
        }
        let across = if app.config.interface.shapes_drawer == DrawerSide::Left {
            Icon::Right
        } else {
            Icon::Left
        };
        if widgets::round_button(ui, 28.0, across, true, false)
            .on_hover_text("Move to the other side")
            .clicked()
        {
            act = Some(Act::Swap);
        }
        if widgets::round_button(ui, 28.0, Icon::Close, true, false)
            .on_hover_text("Close the drawer")
            .clicked()
        {
            act = Some(Act::Close);
        }
    });
    ui.add_space(6.0);
    let built_in = app.objects.built_in().to_vec();
    let custom = app.objects.custom().to_vec();
    ScrollArea::vertical()
        .id_salt("drawer_scroll")
        .scroll_source(ScrollSource {
            drag: DragScroll::Never,
            ..ScrollSource::default()
        })
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(GAP, GAP);
            grid(ui, app, &built_in);
            if !custom.is_empty() {
                ui.add_space(4.0);
                widgets::heading(ui, "MY OBJECTS");
                grid(ui, app, &custom);
            }
        });
    act
}

/// `shapes` as tiles, two to a row. A tap puts one on the paper; dragging one out carries it.
fn grid(ui: &mut Ui, app: &mut InkApp, shapes: &[Shape]) {
    let sheet = canvas::parse_hex(&app.editor.document().metadata.background_solid_color);
    let look = look_of(app);
    for row in shapes.chunks(COLUMNS) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for shape in row {
                let tile = Tile {
                    shape,
                    look,
                    paper: sheet,
                    selected: app.ui.objects.dragging.as_deref() == Some(shape.id.as_str()),
                    pinned: false,
                };
                let response =
                    shape_art::tile_with(ui, tile_side(), &tile, Sense::click_and_drag())
                        .on_hover_text(&shape.name);
                touched(ui, app, &response, &shape.id);
            }
        });
    }
}

/// What a touch of the tile of the shape `id` does: a tap puts the shape on the paper; moving it out of the drawer starts
/// to carry it, and moving it up or down (on a device used by touch) scrolls the drawer instead.
fn touched(ui: &mut Ui, app: &mut InkApp, response: &eframe::egui::Response, id: &str) {
    let ctx = ui.ctx().clone();
    if response.dragged() {
        let moved = ui.input(|i| {
            i.pointer
                .press_origin()
                .zip(i.pointer.latest_pos())
                .map_or(vec2(0.0, 0.0), |(from, now)| now - from)
        });
        let carrying = app.ui.objects.dragging.as_deref() == Some(id);
        let sideways = moved.x.abs() > 1.2 * moved.y.abs();
        if !carrying && moved.length() > CARRY && (sideways || !platform::touch_first()) {
            app.ui.objects.dragging = Some(id.to_owned());
        } else if !carrying && app.ui.objects.dragging.is_none() {
            ui.scroll_with_delta(vec2(0.0, response.drag_delta().y));
        }
    }
    if response.clicked() {
        app.insert_shape_at(&ctx, id, None);
    }
}

/// How the shapes look in the drawer and while carried: as they will on the paper.
fn look_of(app: &InkApp) -> Look {
    let [r, g, b] = app.tool.color.to_srgb();
    Look {
        line: Color32::from_rgb(r, g, b),
        fill: app
            .config
            .shapes
            .fill
            .map(|[r, g, b]| Color32::from_rgb(r, g, b)),
        outline: app.config.shapes.outline,
    }
}

/// Shows the shape being carried under the pointer, as big as it will land, and puts it on the paper where the pointer is
/// let go, if that is not on the drawer.
fn carry(ctx: &Context, app: &mut InkApp, drawer: Rect) {
    let Some(id) = app.ui.objects.dragging.clone() else {
        return;
    };
    let (latest, down) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.any_down()));
    if latest.is_some() {
        app.ui.objects.carried_at = latest;
    }
    let Some(shape) = app.objects.get(&id).cloned() else {
        app.ui.objects.dragging = None;
        return;
    };
    let Some(at) = app.ui.objects.carried_at else {
        return;
    };
    let over_paper = app.canvas_rect.is_some_and(|p| p.contains(at)) && !drawer.contains(at);
    if down {
        // How big it will be on the paper, on the screen.
        let side = app.shape_screen_size();
        let rect = Rect::from_center_size(at, vec2(side, side));
        let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("shape_carried")));
        let mut look = look_of(app);
        let fade = if over_paper { 0.85 } else { 0.35 };
        look.line = look.line.gamma_multiply(fade);
        look.fill = look.fill.map(|c| c.gamma_multiply(fade));
        painter.rect_stroke(
            rect,
            CornerRadius::same(6),
            eframe::egui::Stroke::new(1.0, theme::ACCENT.gamma_multiply(fade)),
            eframe::egui::StrokeKind::Outside,
        );
        shape_art::paint(
            &painter,
            rect.shrink(rect.width() * 0.04),
            &shape,
            look,
            2.0,
        );
        ctx.request_repaint();
        return;
    }
    // The pointer was let go.
    app.ui.objects.dragging = None;
    app.ui.objects.carried_at = None;
    if over_paper {
        let spot = app.view.to_canvas(at);
        app.insert_shape_at(ctx, &id, Some(spot));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paper() -> Rect {
        Rect::from_min_max(pos2(0.0, 50.0), pos2(1200.0, 800.0))
    }

    #[test]
    fn the_drawer_is_as_tall_as_what_is_free_and_on_the_side_it_was_given() {
        let right = place(paper(), DrawerSide::Right, 0.0, None).expect("open");
        assert!((right.right() - (1200.0 - EDGE)).abs() < 1e-3);
        assert!((right.width() - width()).abs() < 1e-3);
        assert!(right.top() > 50.0 && right.bottom() < 800.0);
        let left = place(paper(), DrawerSide::Left, 0.0, None).expect("open");
        assert!((left.left() - EDGE).abs() < 1e-3);
    }

    #[test]
    fn a_closed_drawer_has_no_place() {
        assert!(place(paper(), DrawerSide::Off, 0.0, None).is_none());
    }

    #[test]
    fn the_drawer_keeps_clear_of_the_tools_along_the_bottom() {
        let drawer = place(paper(), DrawerSide::Right, 600.0, None).expect("open");
        assert!(drawer.bottom() <= 600.0 - EDGE + 1e-3);
    }

    #[test]
    fn the_drawer_keeps_clear_of_the_rail_on_the_edge_of_a_window_used_with_a_mouse() {
        let rail = rail::flyout_offset();
        let left = place(paper(), DrawerSide::Left, 0.0, Some(true)).expect("open");
        assert!(left.left() >= rail - 1e-3, "the rail is on the left");
        let right = place(paper(), DrawerSide::Right, 0.0, Some(false)).expect("open");
        assert!(
            right.right() <= 1200.0 - rail + 1e-3,
            "the rail is on the right"
        );
        let free = place(paper(), DrawerSide::Right, 0.0, Some(true)).expect("open");
        assert!(
            (free.right() - (1200.0 - EDGE)).abs() < 1e-3,
            "the other side is free"
        );
    }

    #[test]
    fn there_is_no_drawer_where_there_is_no_room_for_one() {
        let low = Rect::from_min_max(pos2(0.0, 0.0), pos2(600.0, 100.0));
        assert!(place(low, DrawerSide::Right, 0.0, None).is_none());
    }

    #[test]
    fn a_shape_is_as_wide_as_a_fingertip_and_more_where_that_is_the_input() {
        assert!(tile_side() >= 60.0);
    }
}
