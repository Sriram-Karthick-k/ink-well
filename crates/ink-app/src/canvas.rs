//! The drawing surface: paper, grid, pan and zoom gestures, pen input and the live stroke.

use std::collections::HashSet;

use eframe::egui::{
    self, Color32, CursorIcon, Event, LayerId, PointerButton, Pos2, Rect, Sense, Stroke,
    TouchPhase, Ui,
};
use ink_core::artboard::Artboard;
use ink_core::erase::{self, Wiped};
use ink_core::{Document, Editor, Point, StrokeBuilder, recognize};
use uuid::Uuid;

use crate::config::{Config, EraserKind};
use crate::gpu::GpuScene;
use crate::guides::{self, GuideState};
use crate::paper::{self, Ruling};
use crate::precision;
use crate::render::{self, StrokeCache};
use crate::selection;
use crate::state::{Mode, ToolState, ViewState};
use crate::theme;
use crate::tools::{self, Gesture};

/// Everything the canvas needs from the app for one frame.
pub struct CanvasEnv<'a> {
    pub editor: &'a mut Editor,
    pub view: &'a mut ViewState,
    pub tool: &'a ToolState,
    pub config: &'a Config,
    /// Index into `document.layers` of the layer strokes are added to.
    pub active_layer: usize,
    pub builder: &'a mut Option<StrokeBuilder>,
    /// While the eraser is down, what it is doing.
    pub eraser: &'a mut Option<Erasing>,
    pub cache: &'a mut StrokeCache,
    /// Identifies the drawing, so cached GPU batches of one drawing are never used for another.
    pub epoch: u64,
    /// The OpenGL stroke renderer, when it is available and still working.
    pub gpu: Option<&'a mut GpuScene>,
    /// The pressure last reported by a touch or pen, if the device reports it.
    pub force: &'a mut Option<f32>,
    /// When set, the view is fitted to the drawing before it is drawn.
    pub fit: &'a mut bool,
    /// The strokes picked with the select tool.
    pub selection: &'a mut Vec<Uuid>,
    /// What the select or slice tool is doing while the pointer is down.
    pub gesture: &'a mut Option<Gesture>,
    /// The key for moving the canvas is held, so dragging pans whatever the tool is.
    pub pan_held: bool,
    /// The shape guides of the drawing, and what the pointer is doing with them.
    pub guides: &'a mut GuideState,
    /// The page the drawing is on, if it has one.
    pub artboard: Option<Artboard>,
    /// How long the pen has been still, for shape recognition.
    pub hold: &'a mut Hold,
}

/// How far, in points, the pen may move and still be held still.
const HOLD_SLOP: f32 = 4.0;

/// How long the pen has been still, for recognizing the shape of a stroke when it is held at its end.
#[derive(Debug, Clone, Copy, Default)]
pub struct Hold {
    /// Where the pen stopped.
    at: Option<Pos2>,
    /// When it did.
    since: f64,
    /// The stroke has been made into a shape.
    recognized: bool,
    /// How many points the stroke had when a shape was last looked for, so it is not looked for again until the
    /// pen has drawn more.
    tried: usize,
}

/// A color written as `#RRGGBB`; anything else is the default paper.
pub(crate) fn parse_hex(text: &str) -> Color32 {
    let hex = text.trim_start_matches('#');
    let channel = |i: usize| {
        hex.get(i..i + 2)
            .and_then(|s| u8::from_str_radix(s, 16).ok())
    };
    match (channel(0), channel(2), channel(4)) {
        (Some(r), Some(g), Some(b)) => Color32::from_rgb(r, g, b),
        _ => theme::PAPER,
    }
}

/// Fits the artboard, if the drawing has one, or else the drawing into `rect` with a margin. An empty drawing is
/// centered at zoom 1.
pub fn fit_view(
    view: &mut ViewState,
    rect: Rect,
    document: &Document,
    artboard: Option<&Artboard>,
) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    if let Some(board) = artboard {
        [x0, y0, x1, y1] = board.bounds();
    } else {
        for layer in document.layers.iter().filter(|l| l.content && l.visible) {
            for stroke in &layer.strokes {
                let pad = stroke.width.unwrap_or(1.0) / 2.0;
                for p in stroke.geometry() {
                    x0 = x0.min(p.x - pad);
                    y0 = y0.min(p.y - pad);
                    x1 = x1.max(p.x + pad);
                    y1 = y1.max(p.y + pad);
                }
            }
        }
    }
    if x0 > x1 {
        *view = ViewState {
            zoom: 1.0,
            origin: rect.center(),
            ..ViewState::default()
        };
        return;
    }
    let margin = 56.0;
    let zoom = ((rect.width() - 2.0 * margin) / (x1 - x0).max(1.0))
        .min((rect.height() - 2.0 * margin) / (y1 - y0).max(1.0))
        .clamp(ViewState::MIN_ZOOM, 4.0);
    let center = egui::pos2(f32::midpoint(x0, x1), f32::midpoint(y0, y1));
    // Fitting shows the drawing upright.
    view.angle = 0.0;
    view.zoom = zoom;
    view.origin = egui::pos2(
        rect.center().x - center.x * zoom,
        rect.center().y + center.y * zoom, // canvas y points up
    );
}

/// The paper round the artboard: shaded, so the page stands out, or, when the pattern is only for the page, bare.
/// Then the page's edge and what it measures.
fn paint_artboard(painter: &egui::Painter, env: &CanvasEnv<'_>, paper: Color32) {
    let Some(board) = env.artboard else { return };
    let view = *env.view;
    let corner = |x: f32, y: f32| view.to_screen(Point::new(x, y));
    let inner = [
        corner(board.min.x, board.max.y),
        corner(board.max.x, board.max.y),
        corner(board.max.x, board.min.y),
        corner(board.min.x, board.min.y),
    ];
    // Each corner of the page, pushed out along the line from the middle of the page through it, far enough that
    // what is between covers the whole window: a ring that reaches only as far as it needs to.
    let middle = inner
        .iter()
        .fold(egui::Vec2::ZERO, |sum, p| sum + p.to_vec2())
        / 4.0;
    let middle = middle.to_pos2();
    let window = painter.clip_rect();
    let reach = 2.0
        * [
            window.left_top(),
            window.right_top(),
            window.right_bottom(),
            window.left_bottom(),
        ]
        .iter()
        .map(|c| c.distance(middle))
        .fold(0.0, f32::max)
        + 200.0;
    let outer = inner.map(|p| {
        let away = (p - middle).normalized();
        let away = if away.is_finite() {
            away
        } else {
            egui::vec2(1.0, 1.0).normalized()
        };
        p + away * reach
    });
    // The ring between those corners and the page's corners, as triangles that do not overlap.
    let shade = if board.grid_inside {
        paper
    } else {
        Color32::from_black_alpha(46)
    };
    let mut ring = egui::Mesh::default();
    for p in outer.iter().chain(&inner) {
        ring.colored_vertex(*p, shade);
    }
    for i in 0..4u32 {
        let next = (i + 1) % 4;
        ring.add_triangle(i, next, 4 + next);
        ring.add_triangle(i, 4 + next, 4 + i);
    }
    painter.add(egui::Shape::mesh(ring));
    let outline = |width: f32, color: Color32| {
        painter.add(egui::Shape::closed_line(
            inner.to_vec(),
            Stroke::new(width, color),
        ));
    };
    outline(3.0, Color32::from_black_alpha(50));
    outline(1.2, theme::ACCENT);
    let m = &env.config.measurement;
    let size = format!("{} × {}", m.length(board.width()), m.length(board.height()));
    let text = match ink_core::artboard::matching(board.width(), board.height(), m.pixels_per_inch)
    {
        Some(preset) => format!("{} · {size}", preset.name),
        None => size,
    };
    let galley = painter.layout_no_wrap(text, egui::FontId::monospace(12.0), Color32::WHITE);
    let at = inner[0] + egui::vec2(0.0, -30.0);
    let pill = Rect::from_min_size(at, galley.size()).expand2(egui::vec2(8.0, 4.0));
    painter.rect_filled(
        pill,
        egui::CornerRadius::same(10),
        Color32::from_black_alpha(190),
    );
    painter.galley(at, galley, Color32::WHITE);
}

/// Shows the canvas filling the rest of `ui` and handles its input. Returns the canvas rectangle.
pub fn show(ui: &mut Ui, env: &mut CanvasEnv<'_>) -> Rect {
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    if std::mem::take(env.fit) {
        fit_view(env.view, rect, env.editor.document(), env.artboard.as_ref());
    }
    let paper = parse_hex(&env.editor.document().metadata.background_solid_color);
    painter.rect_filled(rect, 0.0, paper);
    let pattern = env.config.canvas.pattern;
    paper::paint(
        &painter,
        rect,
        env.view.origin,
        &Ruling {
            pattern,
            spacing: env.config.canvas.grid_size * env.view.zoom,
            strength: env.config.canvas.grid_strength,
            ink: env.config.canvas.grid_color.map_or_else(
                || paper::ink_for(paper),
                |[r, g, b]| Color32::from_rgb(r, g, b),
            ),
            min_spacing: paper::min_spacing(pattern),
            major_every: i64::from(env.config.canvas.grid_divisions),
            zoom: env.view.zoom,
            weight: env.config.canvas.line_weight,
            // The view's angle is counterclockwise as seen; a screen angle turns the other way.
            turn: -env.view.angle,
        },
    );
    paint_artboard(&painter, env, paper);

    handle_input(ui, rect, &response, env);
    watch_hold(ui.ctx(), env);
    show_cursor(&painter, &response, env);

    let (live, hide) = live_strokes(env);
    // Leaving strokes out of the committed picture is a different picture, so it has its own key; the key then
    // stays the same for as long as they are held, and only the live strokes are redrawn.
    let hidden_key = hidden_key(&hide);
    if let Some(gpu) = env.gpu.as_deref_mut().filter(|g| g.is_working()) {
        gpu.paint(
            &painter,
            rect,
            env.view,
            env.editor.document(),
            (env.epoch << 32) | (env.editor.revision() & 0xFFFF_FFFF) | hidden_key,
            &live,
            &hide,
            env.cache,
        );
    } else {
        render::draw_document(&painter, env.view, env.editor.document(), env.cache, &hide);
        for (stroke, opacity) in &live {
            render::draw_stroke(
                &painter,
                env.view,
                stroke,
                &render::smoothed(stroke),
                *opacity,
            );
        }
    }
    selection::paint_overlay(&painter, env);
    guides::paint(&painter, env);
    paint_turning(ui.ctx(), &painter, rect, env.view);
    if let Some(end) = env.builder.as_ref().and_then(StrokeBuilder::end) {
        precision::paint_marker(&painter, env.view.to_screen(end));
    }
    if env.config.workspace.show_stroke_length
        && env.builder.is_some()
        && let Some((stroke, _)) = live.first()
    {
        paint_stroke_length(&painter, env.view, stroke, &env.config.measurement);
    }
    rect
}

/// A number for which strokes are left out of the committed picture, to add to the key of that picture: zero for
/// none, and otherwise one that differs from the key with nothing left out (and, almost always, from another set).
fn hidden_key(hide: &HashSet<Uuid>) -> u64 {
    if hide.is_empty() {
        return 0;
    }
    let mixed = hide.iter().fold(0_u128, |all, id| all ^ id.as_u128());
    let folded = (mixed ^ (mixed >> 64)) as u64;
    (1 << 63) | ((folded & 0x7FFF) << 48)
}

/// The length of the stroke being drawn, in a small label beside the pen, in the unit the settings choose.
fn paint_stroke_length(
    painter: &egui::Painter,
    view: &ViewState,
    stroke: &ink_core::Stroke,
    measurement: &ink_core::units::Measurement,
) {
    let Some(last) = stroke.points().last() else {
        return;
    };
    let at = view.to_screen(last) + egui::vec2(18.0, -24.0);
    let galley = painter.layout_no_wrap(
        measurement.length(stroke.length()),
        egui::FontId::monospace(12.0),
        Color32::WHITE,
    );
    let pill = Rect::from_min_size(at, galley.size()).expand2(egui::vec2(8.0, 4.0));
    painter.rect_filled(
        pill,
        egui::CornerRadius::same(10),
        Color32::from_black_alpha(190),
    );
    painter.galley(at, galley, Color32::WHITE);
}

/// The radius, in points, of the ring that shows how big a stroke of `size` canvas units would be at `zoom`. It
/// never gets so small that it cannot be seen, or so big that it covers the paper.
#[must_use]
fn preview_radius(size: f32, zoom: f32) -> f32 {
    (size * zoom / 2.0).clamp(2.5, 160.0)
}

/// The strokes to draw over the paper, each with its layer's opacity (the one being written, the one the nudge
/// tool is shaping, or the selection being dragged), and the strokes of the drawing they stand in for, which are
/// then left out.
fn live_strokes(env: &CanvasEnv<'_>) -> (Vec<(ink_core::Stroke, f32)>, HashSet<Uuid>) {
    let opacity_of = |layer: usize| {
        env.editor
            .document()
            .layers
            .get(layer)
            .map_or(1.0, |l| l.opacity)
    };
    if let Some(builder) = env.builder.as_ref() {
        return (
            vec![(builder.snapshot(), opacity_of(env.active_layer))],
            HashSet::new(),
        );
    }
    match &*env.gesture {
        Some(Gesture::Nudge {
            layer,
            original,
            shown,
            ..
        }) => (
            vec![((**shown).clone(), opacity_of(*layer))],
            HashSet::from([original.id]),
        ),
        Some(gesture @ Gesture::Transform { .. }) => {
            let moving = tools::moving_strokes(gesture);
            (
                moving
                    .iter()
                    .map(|(layer, _, shown)| ((*shown).clone(), opacity_of(*layer)))
                    .collect(),
                moving.iter().map(|(_, id, _)| *id).collect(),
            )
        }
        _ => (Vec::new(), HashSet::new()),
    }
}

/// A time for every event of a frame. Events arrive in batches with no timestamps of their own, and the pen
/// speed needs times, so the pen events are spread evenly over the time since the last frame, the last one
/// landing at the frame time.
fn event_times(events: &[Event], now: f64, frame_dt: f64) -> Vec<f64> {
    let is_pen_event = |e: &Event| {
        matches!(
            e,
            Event::PointerMoved(_)
                | Event::PointerButton {
                    button: PointerButton::Primary,
                    ..
                }
        )
    };
    let batch = events.iter().filter(|e| is_pen_event(e)).count().max(1);
    let mut seen = 0_usize;
    events
        .iter()
        .map(|e| {
            if is_pen_event(e) {
                seen += 1;
                now - frame_dt * (batch - seen) as f64 / batch as f64
            } else {
                now
            }
        })
        .collect()
}

/// An eraser stroke in progress.
#[derive(Debug, Clone, Copy)]
pub struct Erasing {
    /// Where the history stood when the eraser touched the paper, so the whole sweep undoes at once.
    mark: usize,
    /// Where the pointer was last, so a quick move erases everything along the way and not only where it landed.
    last: Pos2,
}

/// The pointer on the paper: a ring for the eraser, an open hand for moving the canvas.
fn show_cursor(painter: &egui::Painter, response: &egui::Response, env: &CanvasEnv<'_>) {
    if !response.hovered() {
        return;
    }
    if env.pan_held {
        let icon = if response.dragged() {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Grab
        };
        response.ctx.set_cursor_icon(icon);
        return;
    }
    match env.tool.mode {
        Mode::Erase => {
            if let Some(at) = response.hover_pos() {
                let radius = env.config.tools.eraser_radius;
                response.ctx.set_cursor_icon(CursorIcon::None);
                painter.circle_stroke(at, radius, Stroke::new(3.0, Color32::from_black_alpha(70)));
                painter.circle_stroke(at, radius, Stroke::new(1.4, Color32::WHITE));
            }
        }
        Mode::Pan => response.ctx.set_cursor_icon(CursorIcon::Grab),
        Mode::Select | Mode::Slice => selection::set_cursor(response, env),
        Mode::Nudge => {
            if let Some(at) = response.hover_pos() {
                let reach = env.config.tools.nudge_reach;
                response.ctx.set_cursor_icon(CursorIcon::None);
                painter.circle_stroke(at, reach, Stroke::new(3.0, Color32::from_black_alpha(55)));
                painter.circle_stroke(at, reach, Stroke::new(1.2, Color32::WHITE));
                painter.circle_filled(at, 3.0, Color32::WHITE);
                painter.circle_stroke(at, 3.0, Stroke::new(1.0, Color32::from_black_alpha(90)));
            }
        }
        Mode::Draw => {
            // A ring the size of the brush, so what a stroke will be like can be seen before it is made.
            if env.config.workspace.hover_brush_preview
                && env.builder.is_none()
                && let Some(at) = response.hover_pos()
            {
                let size = env.config.brush(env.tool.pen).size;
                let radius = preview_radius(size, env.view.zoom);
                response.ctx.set_cursor_icon(CursorIcon::None);
                painter.circle_stroke(at, radius, Stroke::new(2.6, Color32::from_black_alpha(60)));
                painter.circle_stroke(at, radius, Stroke::new(1.1, Color32::WHITE));
                painter.circle_filled(at, 1.6, Color32::WHITE);
            }
        }
    }
}

/// The eraser: everything it touches goes, and one sweep is one step to undo.
fn erase(
    ctx: &egui::Context,
    rect: Rect,
    canvas_layer: LayerId,
    events: &[Event],
    env: &mut CanvasEnv<'_>,
) {
    for event in events {
        match event {
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                ..
            } if env.eraser.is_none()
                && rect.contains(*pos)
                && ctx.layer_id_at(*pos) == Some(canvas_layer) =>
            {
                *env.eraser = Some(Erasing {
                    mark: env.editor.history_mark(),
                    last: *pos,
                });
                erase_along(env, *pos, *pos);
            }
            Event::PointerMoved(pos) if env.eraser.is_some() => {
                if let Some(state) = env.eraser.as_mut() {
                    let from = std::mem::replace(&mut state.last, *pos);
                    erase_along(env, from, *pos);
                }
            }
            Event::PointerButton {
                button: PointerButton::Primary,
                pressed: false,
                ..
            } => {
                if let Some(state) = env.eraser.take() {
                    env.editor.merge_history_since(state.mark);
                }
            }
            _ => {}
        }
    }
}

/// The soft eraser, from `from` to `to`: wipes out what is within its reach of the path between them and leaves
/// the rest of each stroke it went over, cut where its edge crossed. A locked stroke is not touched.
fn wipe_along(env: &mut CanvasEnv<'_>, from: Pos2, to: Pos2) {
    let (a, b) = (env.view.to_canvas(from), env.view.to_canvas(to));
    let radius = env.config.tools.eraser_radius / env.view.zoom;
    let mut changes: Vec<(usize, usize, Wiped)> = Vec::new();
    for (layer_index, layer) in env
        .editor
        .document()
        .layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.content && l.visible)
    {
        for (stroke_index, stroke) in layer.strokes.iter().enumerate() {
            if env.editor.is_locked(stroke.id) {
                continue;
            }
            let wiped = erase::wipe(stroke, a, b, radius);
            if wiped != Wiped::Untouched {
                changes.push((layer_index, stroke_index, wiped));
            }
        }
    }
    // Last first, so that a stroke that becomes several does not move the places of those still to do.
    for (layer_index, stroke_index, wiped) in changes.into_iter().rev() {
        let parts = match wiped {
            Wiped::Parts(mut parts) => {
                for part in parts.iter_mut().skip(1) {
                    part.id = env.editor.next_id();
                }
                parts
            }
            Wiped::Gone => Vec::new(),
            Wiped::Untouched => continue,
        };
        env.editor.splice(layer_index, stroke_index, 1, parts);
    }
}

/// Erases at every point from `from` to `to`, close enough together that nothing in between is skipped.
fn erase_along(env: &mut CanvasEnv<'_>, from: Pos2, to: Pos2) {
    if env.config.tools.eraser_kind == EraserKind::Soft {
        wipe_along(env, from, to);
        return;
    }
    let steps = ((to - from).length() / (env.config.tools.eraser_radius / 2.0))
        .ceil()
        .max(1.0);
    for i in 1..=steps as u32 {
        erase_at(env, from.lerp(to, i as f32 / steps));
    }
}

/// Removes every stroke on a visible layer that the eraser touches at `pos`.
fn erase_at(env: &mut CanvasEnv<'_>, pos: Pos2) {
    let at = env.view.to_canvas(pos);
    let radius = env.config.tools.eraser_radius / env.view.zoom;
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for (layer_index, layer) in env
        .editor
        .document()
        .layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.content && l.visible)
    {
        for (stroke_index, stroke) in layer.strokes.iter().enumerate() {
            if !env.editor.is_locked(stroke.id) && ink_core::hit::touches(stroke, at, radius) {
                hits.push((layer_index, stroke_index));
            }
        }
    }
    // Highest first, so removing one does not move the others.
    for (layer_index, stroke_index) in hits.into_iter().rev() {
        env.editor.remove_stroke(layer_index, stroke_index);
    }
}

/// Where a point that is being drawn at `at` goes instead, if snapping is on and something is near it.
fn snapped(env: &CanvasEnv<'_>, at: Point) -> Option<Point> {
    let plan = precision::plan(env.config, env.view.zoom, false)?;
    let none = HashSet::new();
    let mut keys = precision::keys(&plan, env.editor.document(), &none);
    keys.extend(precision::crossings(
        &plan,
        env.editor.document(),
        &none,
        at,
    ));
    plan.targets(&keys).nearest(at).map(|s| s.point)
}

/// While the pen is held still, makes the stroke into the clean shape it is once it has been held long enough, if
/// it is clearly one. The stroke then is the shape, in what is shown and in what is kept.
fn watch_hold(ctx: &egui::Context, env: &mut CanvasEnv<'_>) {
    let precision = &env.config.precision;
    if !precision.recognize_shapes || env.hold.recognized || env.guides.lock.is_some() {
        return;
    }
    let Some(builder) = env.builder.as_mut() else {
        return;
    };
    let delay = f64::from(precision.hold_delay);
    let waited = ctx.input(|i| i.time) - env.hold.since;
    if waited < delay {
        // Nothing arrives while the pen is still, so ask to be called when it has been still long enough.
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay - waited + 0.02));
        return;
    }
    if builder.len() == env.hold.tried {
        return;
    }
    env.hold.tried = builder.len();
    if let Some(shape) = recognize::recognize(&builder.points()) {
        builder.replace_with(shape.polyline());
        env.hold.recognized = true;
    }
}

/// `p`, or the nearest place on the artboard if the drawing is kept to one and `p` is outside it.
fn kept_in(env: &CanvasEnv<'_>, p: Point) -> Point {
    match env.artboard {
        Some(board) if board.confine => board.clamp(p),
        _ => p,
    }
}

/// A stroke can begin where the pen went down: anywhere, unless the drawing is kept to an artboard and the pen
/// went down outside it.
fn may_begin(env: &CanvasEnv<'_>, pos: egui::Pos2) -> bool {
    env.artboard
        .is_none_or(|board| !board.confine || board.contains(env.view.to_canvas(pos)))
}

/// Where a point of the stroke being drawn goes when the pen is at `at`, and where the end of the stroke is drawn
/// instead if it snaps. A stroke held to a shape guide follows it exactly (and does not snap, as the guide is the
/// shape it is after); any other can snap its end to the grid or to the end of another stroke.
fn drawn_at(env: &mut CanvasEnv<'_>, at: Point) -> (Vec<Point>, Option<Point>) {
    // A drawing that is kept to an artboard keeps to its edge where the pen goes past it.
    let at = kept_in(env, at);
    if let Some(guide) = env.guides.lock {
        let now = guide.project(at);
        // A pen that cuts a corner of the guide has no point on it; the guide's own points are put in so the
        // corner is kept.
        let mut points = env
            .guides
            .last
            .map_or_else(Vec::new, |previous| guide.between(previous, now));
        points.push(now);
        env.guides.last = Some(now);
        return (points, None);
    }
    (vec![at], snapped(env, at).map(|end| kept_in(env, end)))
}

/// Begins a stroke with the current brush at `pos`.
fn start_stroke(env: &mut CanvasEnv<'_>, pos: egui::Pos2, time: f64) {
    let tool = env.tool;
    let brush = env.config.brush(tool.pen);
    let at = env.view.to_canvas(pos);
    *env.hold = Hold {
        at: Some(pos),
        since: time,
        ..Hold::default()
    };
    // A stroke that starts on a guide is held to it. The pen's smoothing would round the corners of a square and
    // pull a line off its guide, so such a stroke is not smoothed.
    env.guides.lock = guides::guide_for_start(&env.guides.list, at, env.view.zoom);
    let smoothness = if env.guides.lock.is_some() {
        0.0
    } else {
        brush.smoothness
    };
    // The stroke has its identity from the first point, so what is drawn is exactly what is kept.
    let mut builder = StrokeBuilder::new(
        tool.pen,
        Some(brush.size),
        tool.stroke_color(brush.opacity),
        env.view.zoom,
        smoothness,
    )
    .with_id(env.editor.next_id());
    // The first point is put where it snaps to for good: nothing after it changes it.
    let first = if env.guides.lock.is_some() {
        env.guides.held(at)
    } else {
        snapped(env, at).unwrap_or(at)
    };
    let first = kept_in(env, first);
    env.guides.last = env.guides.lock.map(|_| first);
    builder.push(first, *env.force, time);
    *env.builder = Some(builder);
}

fn handle_input(ui: &Ui, rect: Rect, response: &egui::Response, env: &mut CanvasEnv<'_>) {
    let ctx = ui.ctx().clone();
    let canvas_layer = ui.layer_id();
    let input = &env.config.input;
    let (wheel_zooms, gestures, zoom_enabled, zoom_snap) = (
        input.wheel_zooms,
        input.two_finger_gestures,
        input.zoom_enabled,
        input.zoom_snap,
    );
    let (multi_touch, zoom_delta, scroll, latest, events, now, frame_dt) = ui.input(|i| {
        (
            i.multi_touch().filter(|_| gestures),
            i.zoom_delta(),
            i.smooth_scroll_delta,
            i.pointer.latest_pos(),
            i.events.clone(),
            i.time,
            f64::from(i.unstable_dt),
        )
    });
    let hovering =
        latest.is_some_and(|p| rect.contains(p) && ctx.layer_id_at(p) == Some(canvas_layer));

    // Two fingers pan and pinch-zoom. A stroke in progress is a palm or a mistake, so drop it.
    // Two fingers also turn the paper when they twist. egui measures a twist clockwise, and the view's angle
    // counterclockwise.
    let mut turned_at = None;
    if let Some(touch) = multi_touch {
        *env.builder = None;
        env.view.pan(touch.translation_delta);
        if env.config.input.rotation_enabled && touch.rotation_delta.abs() > 1e-5 {
            let at = latest.unwrap_or_else(|| rect.center());
            env.view.rotate_at(at, -touch.rotation_delta);
            turned_at = Some(at);
            note_turning(&ctx);
        }
    }
    let mut zoomed_at = None;
    if hovering {
        if zoom_enabled
            && (zoom_delta - 1.0).abs() > 1e-4
            && let Some(at) = latest
        {
            env.view.zoom_at(at, zoom_delta);
            zoomed_at = Some(at);
        }
        if scroll != egui::Vec2::ZERO && multi_touch.is_none() {
            match latest {
                Some(at) if wheel_zooms && zoom_enabled => {
                    env.view.zoom_at(at, (scroll.y / 200.0).exp());
                    zoomed_at = Some(at);
                }
                _ => env.view.pan(scroll),
            }
        }
    }
    settle_zoom(&ctx, env.view, zoomed_at, zoom_snap, now);
    settle_turn(
        &ctx,
        env.view,
        turned_at,
        env.config.input.rotation_snap,
        now,
    );
    let pan_drag = response.dragged_by(PointerButton::Secondary)
        || response.dragged_by(PointerButton::Middle)
        || ((env.tool.mode == Mode::Pan || env.pan_held)
            && response.dragged_by(PointerButton::Primary));
    if pan_drag {
        env.view.pan(response.drag_delta());
    }
    if multi_touch.is_some() || env.pan_held {
        return;
    }
    match env.tool.mode {
        Mode::Pan => return,
        Mode::Erase => {
            erase(&ctx, rect, canvas_layer, &events, env);
            return;
        }
        Mode::Select | Mode::Slice | Mode::Nudge => {
            tools::handle_events(&ctx, rect, canvas_layer, &events, env);
            return;
        }
        Mode::Draw => {}
    }

    let times = event_times(&events, now, frame_dt);
    draw_events(&ctx, rect, canvas_layer, env, events.into_iter().zip(times));
}

/// Once the zoom has stopped changing for a moment, settles it on a round level if it is close to one. (Doing it
/// on every frame of a zoom would hold the zoom still at the level and never let it pass.)
fn settle_zoom(
    ctx: &egui::Context,
    view: &mut ViewState,
    zoomed_at: Option<Pos2>,
    snap: bool,
    now: f64,
) {
    const PAUSE: f64 = 0.22;
    let id = egui::Id::unique("zoom_settle");
    if let Some(at) = zoomed_at {
        ctx.data_mut(|d| d.insert_temp(id, (now, at)));
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(PAUSE + 0.03));
        return;
    }
    let Some((since, at)) = ctx.data(|d| d.get_temp::<(f64, Pos2)>(id)) else {
        return;
    };
    if !snap {
        ctx.data_mut(|d| d.remove_temp::<(f64, Pos2)>(id));
    } else if now - since >= PAUSE {
        view.snap_zoom_at(at);
        ctx.data_mut(|d| d.remove_temp::<(f64, Pos2)>(id));
    } else {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
            PAUSE - (now - since) + 0.01,
        ));
    }
}

/// How long, in seconds, the horizon line and the angle stay on the paper after the paper was last turned.
const TURNING_SHOWN: f64 = 0.9;

/// Says the paper has just been turned, so that the horizon line and the angle show for a moment.
pub fn note_turning(ctx: &egui::Context) {
    let until = ctx.input(|i| i.time) + TURNING_SHOWN;
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("turning_until"), until));
    ctx.request_repaint();
}

/// Once the paper has stopped turning for a moment, settles it on a multiple of fifteen degrees if it is close to
/// one. (Doing it on every frame of a turn would hold the paper still at the angle and never let it pass.)
fn settle_turn(
    ctx: &egui::Context,
    view: &mut ViewState,
    turned_at: Option<Pos2>,
    snap: bool,
    now: f64,
) {
    const PAUSE: f64 = 0.22;
    let id = egui::Id::unique("turn_settle");
    if let Some(at) = turned_at {
        ctx.data_mut(|d| d.insert_temp(id, (now, at)));
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(PAUSE + 0.03));
        return;
    }
    let Some((since, at)) = ctx.data(|d| d.get_temp::<(f64, Pos2)>(id)) else {
        return;
    };
    if !snap {
        ctx.data_mut(|d| d.remove_temp::<(f64, Pos2)>(id));
    } else if now - since >= PAUSE {
        view.snap_angle_at(at);
        ctx.data_mut(|d| d.remove_temp::<(f64, Pos2)>(id));
        note_turning(ctx);
    } else {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
            PAUSE - (now - since) + 0.01,
        ));
    }
}

/// While the paper is being turned, and for a moment after: a line through the middle along the paper's own
/// level (its horizon) and the angle, lit when the paper is exactly on a step.
fn paint_turning(ctx: &egui::Context, painter: &egui::Painter, rect: Rect, view: &ViewState) {
    let now = ctx.input(|i| i.time);
    let until = ctx
        .data(|d| d.get_temp::<f64>(egui::Id::unique("turning_until")))
        .unwrap_or(0.0);
    if until <= now {
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(until - now + 0.02));
    let fade = ((until - now) / 0.3).clamp(0.0, 1.0) as f32;
    let step = ViewState::ANGLE_STEP;
    let on_step = ((view.degrees() / step).round() * step - view.degrees()).abs() < 0.05;
    let color = if on_step {
        theme::ACCENT
    } else {
        Color32::WHITE
    };
    let (center, along) = (
        rect.center(),
        view.to_screen(Point::new(1.0, 0.0)) - view.to_screen(Point::new(0.0, 0.0)),
    );
    let along = along.normalized() * rect.size().length();
    for stroke in [
        Stroke::new(3.5, Color32::from_black_alpha((60.0 * fade) as u8)),
        Stroke::new(1.5, color.gamma_multiply(fade)),
    ] {
        painter.line_segment([center - along, center + along], stroke);
    }
    let text = format!("{:.0}\u{b0}", view.degrees());
    let galley = painter.layout_no_wrap(text, egui::FontId::monospace(13.0), Color32::WHITE);
    let corner = center + egui::vec2(12.0, -34.0);
    let pill = Rect::from_min_size(corner, galley.size()).expand2(egui::vec2(8.0, 4.0));
    painter.rect_filled(
        pill,
        egui::CornerRadius::same(10),
        Color32::from_black_alpha((190.0 * fade) as u8),
    );
    painter.galley(corner, galley, Color32::WHITE.gamma_multiply(fade));
}

/// Turns pen events into strokes: a press starts one, moves extend it, a release commits it.
fn draw_events(
    ctx: &egui::Context,
    rect: Rect,
    canvas_layer: LayerId,
    env: &mut CanvasEnv<'_>,
    events: impl Iterator<Item = (Event, f64)>,
) {
    let use_pressure = env.config.input.use_pressure;
    if env.builder.is_none() {
        env.guides.lock = None;
    }
    for (event, time) in events {
        let on_paper = |pos: Pos2| rect.contains(pos) && ctx.layer_id_at(pos) == Some(canvas_layer);
        if guides::take_event(env, &event, on_paper) {
            continue;
        }
        match event {
            Event::Touch { phase, force, .. } => {
                *env.force = match phase {
                    TouchPhase::End | TouchPhase::Cancel => None,
                    _ => force
                        .filter(|_| use_pressure)
                        .map(|f| env.config.input.pressure(f)),
                };
            }
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                ..
            } if env.builder.is_none()
                && rect.contains(pos)
                && may_begin(env, pos)
                && ctx.layer_id_at(pos) == Some(canvas_layer)
                && env
                    .editor
                    .document()
                    .layers
                    .get(env.active_layer)
                    .is_some_and(|l| l.content && l.visible) =>
            {
                start_stroke(env, pos, time);
            }
            Event::PointerMoved(pos) if env.builder.is_some() => {
                if env
                    .hold
                    .at
                    .is_none_or(|stopped| stopped.distance(pos) > HOLD_SLOP)
                {
                    env.hold.at = Some(pos);
                    env.hold.since = time;
                }
                let (points, end) = drawn_at(env, env.view.to_canvas(pos));
                // The last point is drawn where it would snap to, and goes back to the pen as it moves away; the
                // points before it are never touched.
                if let Some(builder) = env.builder.as_mut() {
                    for at in points {
                        builder.push(at, *env.force, time);
                    }
                    builder.set_end(end);
                }
            }
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: false,
                ..
            } if env.builder.is_some() => {
                let (points, end) = drawn_at(env, env.view.to_canvas(pos));
                if let Some(mut builder) = env.builder.take() {
                    for at in points {
                        builder.push(at, *env.force, time);
                    }
                    builder.set_end(end);
                    let stroke = builder.finish();
                    env.editor.commit_stroke(env.active_layer, stroke);
                }
                env.guides.lock = None;
                env.guides.last = None;
                *env.hold = Hold::default();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_that_stops_near_a_step_settles_on_it_after_a_pause_and_not_before() {
        let ctx = egui::Context::default();
        let at = egui::pos2(300.0, 200.0);
        let mut view = ViewState {
            angle: 28.0_f32.to_radians(),
            ..ViewState::default()
        };
        // Still turning: nothing is snapped.
        settle_turn(&ctx, &mut view, Some(at), true, 10.0);
        settle_turn(&ctx, &mut view, Some(at), true, 10.1);
        assert!((view.degrees() - 28.0).abs() < 1e-3);
        // Stopped, but not for long enough yet.
        settle_turn(&ctx, &mut view, None, true, 10.2);
        assert!((view.degrees() - 28.0).abs() < 1e-3);
        // After the pause, it lands on 30.
        settle_turn(&ctx, &mut view, None, true, 10.4);
        assert!((view.degrees() - 30.0).abs() < 1e-3, "{}", view.degrees());
    }

    #[test]
    fn a_turn_between_steps_stays_and_nothing_snaps_when_snapping_is_off() {
        let ctx = egui::Context::default();
        let at = egui::pos2(300.0, 200.0);
        let mut between = ViewState {
            angle: 22.0_f32.to_radians(),
            ..ViewState::default()
        };
        settle_turn(&ctx, &mut between, Some(at), true, 1.0);
        settle_turn(&ctx, &mut between, None, true, 1.5);
        assert!(
            (between.degrees() - 22.0).abs() < 1e-3,
            "7 from one step and 8 from the next"
        );
        let mut off = ViewState {
            angle: 28.0_f32.to_radians(),
            ..ViewState::default()
        };
        settle_turn(&ctx, &mut off, Some(at), false, 1.0);
        settle_turn(&ctx, &mut off, None, false, 1.5);
        assert!((off.degrees() - 28.0).abs() < 1e-3);
    }

    #[test]
    fn the_hover_ring_is_the_size_of_the_brush_but_always_visible_and_never_huge() {
        assert!((preview_radius(20.0, 1.0) - 10.0).abs() < 1e-6);
        assert!(
            (preview_radius(20.0, 2.0) - 20.0).abs() < 1e-6,
            "it follows the zoom"
        );
        assert!(
            (preview_radius(0.5, 1.0) - 2.5).abs() < 1e-6,
            "a hairline still shows"
        );
        assert!(
            (preview_radius(300.0, 64.0) - 160.0).abs() < 1e-6,
            "and a huge one is held in"
        );
    }
}
