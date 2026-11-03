//! Shape guides over the paper: the guides a drawing has, grabbing their handles, holding a stroke to one, and
//! drawing them.
//!
//! The geometry is `ink_core::guide`. A guide belongs to the drawing it was put on (it is parked with the drawing
//! when the tab changes) and is not saved in the file. Guides work with the pen: with another tool they are put away.

use eframe::egui::{self, Color32, Painter, Pos2, Shape};
use ink_core::Point;
use ink_core::guide::{self, Guide, Kind};

use crate::canvas::CanvasEnv;
use crate::precision;
use crate::state::{Mode, ViewState};
use crate::theme;

/// The size of a handle, in points.
const HANDLE_RADIUS: f32 = 7.0;
/// How near, in points, a handle is grabbed: a little more than it is drawn, but not so much that a stroke cannot
/// be started near the end of a guide, which is where a ruler is wanted most.
const GRAB_REACH: f32 = HANDLE_RADIUS + 5.0;
/// How near, in points, to a guide a stroke has to start to be held to it.
const HOLD_REACH: f32 = 20.0;

/// A handle that is being dragged.
#[derive(Debug, Clone)]
pub struct Drag {
    guide: usize,
    handle: usize,
    /// Where the pointer was when the drag began, in canvas coordinates.
    grab: Point,
    /// Where the handle was then. The handle follows the pointer by the distance it has gone, so it does not jump
    /// to the pointer when it is grabbed off its middle.
    start: Point,
    /// The guide as it was then.
    before: Guide,
    /// Where it can snap to, when it can: the ends of the strokes.
    keys: Vec<Point>,
}

/// The guides of the drawing on the screen and what the pointer is doing with them.
#[derive(Debug, Clone, Default)]
pub struct GuideState {
    pub list: Vec<Guide>,
    pub drag: Option<Drag>,
    /// The guide the stroke being drawn is held to.
    pub lock: Option<Guide>,
    /// The last point put in the stroke that is held to a guide, so what the guide has between it and the next is
    /// put in too.
    pub last: Option<Point>,
}

impl GuideState {
    #[must_use]
    pub fn has(&self, kind: Kind) -> bool {
        self.list.iter().any(|g| g.kind() == kind)
    }

    /// Puts a guide of `kind` centered on `center`, `size` across, or takes it away if there is one. Returns
    /// whether there is one now.
    pub fn toggle(&mut self, kind: Kind, center: Point, size: f32) -> bool {
        self.drag = None;
        if let Some(i) = self.list.iter().position(|g| g.kind() == kind) {
            self.list.remove(i);
            return false;
        }
        self.list.push(Guide::new(kind, center, size));
        true
    }

    /// The point `p` as it is drawn: held to the guide the stroke is locked to, if it is.
    #[must_use]
    pub fn held(&self, p: Point) -> Point {
        self.lock.map_or(p, |g| g.project(p))
    }
}

/// Lets the guides take the pointer events of a frame that are theirs: grabbing a handle, dragging it, and letting
/// go. Returns `true` for an event that was used, which the pen must not also draw with.
pub fn take_event(
    env: &mut CanvasEnv<'_>,
    event: &egui::Event,
    can_press: impl Fn(Pos2) -> bool,
) -> bool {
    use egui::{Event, PointerButton};
    if env.tool.mode != Mode::Draw {
        return false;
    }
    match *event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            ..
        } if env.guides.drag.is_none() && env.builder.is_none() && can_press(pos) => {
            let at = env.view.to_canvas(pos);
            let reach = GRAB_REACH / env.view.zoom;
            let Some((g, h)) = guide::nearest_handle(&env.guides.list, at, reach) else {
                return false;
            };
            let before = env.guides.list[g];
            let keys =
                precision::plan(env.config, env.view.zoom, true).map_or_else(Vec::new, |plan| {
                    precision::keys(
                        &plan,
                        env.editor.document(),
                        &std::collections::HashSet::new(),
                    )
                });
            env.guides.drag = Some(Drag {
                guide: g,
                handle: h,
                grab: at,
                start: before.handles()[h].point,
                before,
                keys,
            });
            true
        }
        Event::PointerMoved(pos) if env.guides.drag.is_some() => {
            let at = env.view.to_canvas(pos);
            drag_to(env, at);
            true
        }
        Event::PointerButton {
            button: PointerButton::Primary,
            pressed: false,
            ..
        } if env.guides.drag.is_some() => {
            env.guides.drag = None;
            true
        }
        _ => false,
    }
}

/// Moves the guide being dragged to follow a pointer that is now at `at`.
fn drag_to(env: &mut CanvasEnv<'_>, at: Point) {
    let Some(drag) = env.guides.drag.as_ref() else {
        return;
    };
    let mut delta = (at.x - drag.grab.x, at.y - drag.grab.y);
    // The handle, where it would be, lands on the grid or on the end of a stroke if it is near one.
    if let Some(plan) = precision::plan(env.config, env.view.zoom, true) {
        let handle = Point::new(drag.start.x + delta.0, drag.start.y + delta.1);
        let mut keys = drag.keys.clone();
        keys.extend(precision::crossings(
            &plan,
            env.editor.document(),
            &std::collections::HashSet::new(),
            handle,
        ));
        if let Some(found) = plan.targets(&keys).nearest(handle) {
            delta = (found.point.x - drag.start.x, found.point.y - drag.start.y);
        }
    }
    let next = drag.before.dragged(drag.handle, delta);
    let index = drag.guide;
    if let Some(slot) = env.guides.list.get_mut(index) {
        *slot = next;
    }
}

/// Picks the guide a stroke that starts at `at` is held to, if it starts near one.
#[must_use]
pub fn guide_for_start(list: &[Guide], at: Point, zoom: f32) -> Option<Guide> {
    guide::nearest(list, at, HOLD_REACH / zoom).map(|i| list[i])
}

/// A line that shows on paper of any color: a dark edge under a bright line.
fn outlined(color: Color32, width: f32) -> [egui::Stroke; 2] {
    [
        egui::Stroke::new(width + 2.0, Color32::from_black_alpha(60)),
        egui::Stroke::new(width, color),
    ]
}

fn paint_handle(painter: &Painter, at: Pos2, moves: bool) {
    painter.circle_filled(
        at + egui::vec2(0.0, 1.0),
        HANDLE_RADIUS + 1.5,
        Color32::from_black_alpha(55),
    );
    painter.circle_filled(
        at,
        HANDLE_RADIUS,
        if moves { theme::ACCENT } else { Color32::WHITE },
    );
    painter.circle_stroke(
        at,
        HANDLE_RADIUS,
        egui::Stroke::new(1.8, if moves { Color32::WHITE } else { theme::ACCENT }),
    );
}

/// Draws the guides, their handles and what they measure.
pub fn paint(painter: &Painter, env: &CanvasEnv<'_>) {
    if env.tool.mode != Mode::Draw {
        return;
    }
    let view: &ViewState = env.view;
    for guide in &env.guides.list {
        let held = env.guides.lock.is_some_and(|g| g == *guide);
        for path in guide.paths() {
            let screen: Vec<Pos2> = path.iter().map(|p| view.to_screen(*p)).collect();
            // The accent shows on paper of any color, as the handles do; a stroke held to the guide lights it up.
            let color = if held { Color32::WHITE } else { theme::ACCENT };
            for stroke in outlined(color, if held { 2.2 } else { 1.5 }) {
                painter.add(Shape::line(screen.clone(), stroke));
            }
        }
        for handle in guide.handles() {
            paint_handle(painter, view.to_screen(handle.point), handle.moves);
        }
        paint_label(
            painter,
            view.to_screen(guide.label_at()),
            &guide.label(&env.config.measurement),
        );
    }
}

fn paint_label(painter: &Painter, at: Pos2, text: &str) {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::monospace(12.0),
        Color32::WHITE,
    );
    let corner = at + egui::vec2(14.0, -30.0);
    let pill = egui::Rect::from_min_size(corner, galley.size()).expand2(egui::vec2(8.0, 4.0));
    painter.rect_filled(
        pill,
        egui::CornerRadius::same(10),
        Color32::from_black_alpha(190),
    );
    painter.galley(corner, galley, Color32::WHITE);
}
