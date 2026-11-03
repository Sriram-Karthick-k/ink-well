//! The tools that work on strokes already drawn: select, move, scale, turn and slice.
//!
//! The functions that decide things (what a lasso covers, where a blade cuts, how a drag moves the selection)
//! work on an [`Editor`] and plain numbers, so they are tested without a window. The ones at the bottom connect
//! them to pointer events.

use std::collections::HashSet;

use eframe::egui::{self, Event, LayerId, PointerButton, Pos2, Rect};
use ink_core::document::Stroke;
use ink_core::transform::bounds_of;
use ink_core::{Affine, Editor, Point, hit, nudge, slice, snap};
use uuid::Uuid;

use crate::canvas::CanvasEnv;
use crate::config::{NudgeVariant, SelectShape, SliceMode};
use crate::precision::{self, Plan};
use crate::state::{Mode, ViewState};

/// How far, in points, the pointer must travel before a press counts as a drag and not a tap.
pub const TAP_DISTANCE: f32 = 5.0;
/// How far from a stroke, in points, a tap still selects it.
pub const TAP_REACH: f32 = 9.0;
/// How far, in points, the pen must go from where a nudge began before the stroke is changed at all.
const NUDGE_DEAD_ZONE: f32 = 2.0;
/// The size of the handles around a selection, in points.
pub const HANDLE_RADIUS: f32 = 7.0;
/// How far above the selection the turning handle floats, in points.
pub const ROTATE_OFFSET: f32 = 34.0;
/// How much a selection may be scaled by in one drag.
const SCALE_RANGE: (f32, f32) = (0.05, 40.0);
/// The space between the selected strokes and the box drawn around them, in points.
const FRAME_PAD: f32 = 4.0;
/// How near a handle the pointer has to be to grab it. More than the drawn size, so a finger can do it.
const GRAB_REACH: f32 = HANDLE_RADIUS + 7.0;

/// What a drag that started on a selection is doing to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    Move,
    /// Scaling from the corner with this index (clockwise from the top left).
    Scale(usize),
    Rotate,
}

/// What a drag of the selection can snap to, worked out once when the drag begins.
#[derive(Debug, Clone)]
pub struct EditSnap {
    /// The grid and the ends of strokes, when snapping to them is on.
    plan: Option<Plan>,
    /// A turn lands on a multiple of [`ROTATION_STEP`] when it comes near one.
    rotate: bool,
    /// For a move: the points of the selection that can land on something: the corners and middle of its box, and
    /// the ends of its strokes.
    anchors: Vec<Point>,
    /// The ends of the strokes that are not being dragged.
    keys: Vec<Point>,
    /// Where the selection is landing now, for marking it; `None` while nothing is near.
    landing: Option<Point>,
}

/// The most strokes whose ends are tried as places to snap with; more would only slow every move down.
const MOST_ANCHORED_STROKES: usize = 32;
/// A turn of the selection lands on a multiple of this when it is near one, in radians (15 degrees).
const ROTATION_STEP: f32 = std::f32::consts::PI / 12.0;
/// How near a multiple of [`ROTATION_STEP`] a turn has to be to land on it, in radians (4 degrees).
const ROTATION_REACH: f32 = 4.0 * std::f32::consts::PI / 180.0;

/// How snapping goes for a drag of `originals` by `handle`, if it is on. The box that is snapped when moving is
/// the one around the points of the strokes, not the selection box drawn on screen, which is a little bigger than
/// the pen's width.
fn edit_snap(env: &CanvasEnv<'_>, originals: &[Original], handle: Handle) -> Option<EditSnap> {
    let plan = precision::plan(env.config, env.view.zoom, true);
    let precision = &env.config.precision;
    let rotate = handle == Handle::Rotate && precision.snap && precision.snap_while_editing;
    if plan.is_none() && !rotate {
        return None;
    }
    let moving: HashSet<Uuid> = originals.iter().map(|o| o.stroke.id).collect();
    let keys = plan.map_or_else(Vec::new, |plan| {
        precision::keys(&plan, env.editor.document(), &moving)
    });
    let anchors = if handle == Handle::Move {
        move_anchors(originals)
    } else {
        Vec::new()
    };
    Some(EditSnap {
        plan,
        rotate,
        anchors,
        keys,
        landing: None,
    })
}

/// The points of `originals` that a move tries to land on something: the corners and middle of the box around
/// their points, and the ends of the first strokes.
fn move_anchors(originals: &[Original]) -> Vec<Point> {
    let all: Vec<Point> = originals.iter().flat_map(|o| o.stroke.points()).collect();
    let Some(bounds) = bounds_of_points(&all) else {
        return Vec::new();
    };
    let mut anchors: Vec<Point> = vec![
        corner(bounds, 0),
        corner(bounds, 1),
        corner(bounds, 2),
        corner(bounds, 3),
        middle(bounds),
    ];
    for original in originals.iter().take(MOST_ANCHORED_STROKES) {
        let mut ends = original.stroke.points();
        if let Some(first) = ends.next() {
            anchors.push(first);
            if let Some(last) = ends.last() {
                anchors.push(last);
            }
        }
    }
    anchors
}

/// The pointer as it would be for a turn from `start` to `now` about `center` that is the nearest multiple of
/// `step` radians, if the turn is within `reach` of one; `None` if it is not near any.
#[must_use]
fn turn_snapped(start: Point, center: Point, now: Point, step: f32, reach: f32) -> Option<Point> {
    let angle = |p: Point| (p.y - center.y).atan2(p.x - center.x);
    let turn = (angle(now) - angle(start) + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    let snapped = (turn / step).round() * step;
    if (turn - snapped).abs() > reach {
        return None;
    }
    let back = snapped - turn;
    let (sin, cos) = back.sin_cos();
    let (dx, dy) = (now.x - center.x, now.y - center.y);
    Some(Point::new(
        center.x + dx * cos - dy * sin,
        center.y + dx * sin + dy * cos,
    ))
}

/// The pointer as it would be for a scale about `center` that lands the corner being dragged (which started at
/// `start`) on the nearest target, and where the corner then is. A scale keeps the shape, so the corner can only
/// move along the line from the center, and it lands on the point of that line nearest the target.
#[must_use]
fn scale_snapped(
    start: Point,
    center: Point,
    now: Point,
    targets: &snap::Targets<'_>,
) -> Option<(Point, Point)> {
    let (bx, by) = (start.x - center.x, start.y - center.y);
    let base2 = bx * bx + by * by;
    if base2 <= f32::EPSILON {
        return None;
    }
    let factor = (now.distance(center) / base2.sqrt()).clamp(SCALE_RANGE.0, SCALE_RANGE.1);
    let corner_now = Point::new(center.x + bx * factor, center.y + by * factor);
    let target = targets.nearest(corner_now)?.point;
    let along = ((target.x - center.x) * bx + (target.y - center.y) * by) / base2;
    let along = along.clamp(SCALE_RANGE.0, SCALE_RANGE.1);
    let corner = Point::new(center.x + bx * along, center.y + by * along);
    Some((corner, corner))
}

/// Where the selection being dragged is landing on something it snapped to, if anywhere.
#[must_use]
pub fn landing(gesture: &Gesture) -> Option<Point> {
    match gesture {
        Gesture::Transform { snap, .. } => snap.as_ref().and_then(|s| s.landing),
        _ => None,
    }
}

/// A selected stroke as it was when the drag began, and where it sits in the document.
#[derive(Debug, Clone)]
pub struct Original {
    layer: usize,
    index: usize,
    stroke: Stroke,
}

/// What the pointer is doing on the paper while a select or slice tool is in use.
#[derive(Debug, Clone)]
pub enum Gesture {
    /// Drawing a loop or a box around strokes to select them.
    Marquee {
        start: Pos2,
        /// The path of the pointer, in canvas coordinates.
        points: Vec<Point>,
        /// Adds to the selection instead of replacing it.
        additive: bool,
        /// The most the pointer has been from where it started, in points.
        travelled: f32,
    },
    /// Moving, scaling or turning the selection. Like a nudge, it leaves the drawing alone while the pen is down:
    /// the strokes as they look now are `shown`, drawn over the paper, and they go into the drawing once, when the
    /// pen is lifted. (Rewriting them on every move would redraw the whole drawing on every move.)
    Transform {
        handle: Handle,
        /// Where the pointer was when the drag began, in canvas coordinates.
        start: Point,
        /// The point the selection turns or scales around (unused when moving).
        center: Point,
        originals: Vec<Original>,
        /// What the originals look like now, in the same order; empty until the drag has really begun.
        shown: Vec<Stroke>,
        /// Snapping while it is moved, scaled or turned.
        snap: Option<EditSnap>,
        travelled: f32,
    },
    /// Drawing the blade of the slice tool.
    Blade {
        /// The path of the blade, in canvas coordinates.
        points: Vec<Point>,
    },
    /// Pulling or pushing one stroke into a new shape. The drawing is not touched while the pen is down: the
    /// stroke being shaped is `shown`, drawn over the paper like a stroke being written, and it goes into the
    /// drawing once, when the pen is lifted. (Changing the drawing on every move would redraw all of it on every
    /// move.)
    Nudge {
        layer: usize,
        index: usize,
        /// The stroke as it was before the nudge, for undo.
        original: Box<Stroke>,
        /// Pull: the stroke with extra points that every frame is bent from. Push: the stroke as it is now.
        base: Box<Stroke>,
        /// The stroke as it looks now.
        shown: Box<Stroke>,
        /// The pen has moved, so there is something to keep.
        moved: bool,
        /// The sample being dragged (pull).
        held: usize,
        variant: NudgeVariant,
        start: Point,
        last: Point,
        /// How far the nudge reaches, in canvas units.
        reach: f32,
    },
}

/// The box drawn around the selection, in screen coordinates. When the paper is turned the box is turned with it.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    /// Clockwise from the top left, as the paper is turned.
    pub corners: [Pos2; 4],
    /// The middle of the top side, where the stem of the turning handle begins.
    pub stem: Pos2,
    pub rotate: Pos2,
}

/// The box for `bounds` (left, bottom, right, top on the canvas) as the view shows it.
#[must_use]
pub fn frame_of(view: ViewState, bounds: [f32; 4]) -> Frame {
    let origin = view.to_screen(Point::new(0.0, 0.0));
    // Which way the canvas's right and up are on the screen.
    let right = (view.to_screen(Point::new(1.0, 0.0)) - origin).normalized();
    let up = (view.to_screen(Point::new(0.0, 1.0)) - origin).normalized();
    // Each corner is a little way out from the strokes, along the sides of the box.
    let corners = [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)].map(|(sx, sy)| {
        let at = view.to_screen(corner_of(bounds, sx, sy));
        at + (right * sx + up * sy) * FRAME_PAD
    });
    let stem = egui::pos2(
        f32::midpoint(corners[0].x, corners[1].x),
        f32::midpoint(corners[0].y, corners[1].y),
    );
    Frame {
        corners,
        stem,
        rotate: stem + up * ROTATE_OFFSET,
    }
}

/// The corner of `bounds` on the left (`-1`) or right (`1`) and the top (`1`) or bottom (`-1`).
fn corner_of(bounds: [f32; 4], sx: f32, sy: f32) -> Point {
    Point::new(
        if sx < 0.0 { bounds[0] } else { bounds[2] },
        if sy < 0.0 { bounds[1] } else { bounds[3] },
    )
}

/// `true` if `pos` is inside the four-sided shape with these corners, in order round it.
fn inside(corners: &[Pos2; 4], pos: Pos2) -> bool {
    let sides: Vec<f32> = (0..4)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            (b.x - a.x) * (pos.y - a.y) - (b.y - a.y) * (pos.x - a.x)
        })
        .collect();
    sides.iter().all(|s| *s >= 0.0) || sides.iter().all(|s| *s <= 0.0)
}

/// What a press at `pos` grabs: the turning handle, a corner, or the inside of the box.
#[must_use]
pub fn hit_handle(frame: &Frame, pos: Pos2) -> Option<Handle> {
    if frame.rotate.distance(pos) <= GRAB_REACH {
        return Some(Handle::Rotate);
    }
    let corner = frame
        .corners
        .iter()
        .enumerate()
        .map(|(i, c)| (i, c.distance(pos)))
        .filter(|(_, d)| *d <= GRAB_REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((i, _)) = corner {
        return Some(Handle::Scale(i));
    }
    inside(&frame.corners, pos).then_some(Handle::Move)
}

/// A corner of `bounds` (left, bottom, right, top), clockwise from the top left.
fn corner(bounds: [f32; 4], index: usize) -> Point {
    match index % 4 {
        0 => Point::new(bounds[0], bounds[3]),
        1 => Point::new(bounds[2], bounds[3]),
        2 => Point::new(bounds[2], bounds[1]),
        _ => Point::new(bounds[0], bounds[1]),
    }
}

fn middle(bounds: [f32; 4]) -> Point {
    Point::new(
        f32::midpoint(bounds[0], bounds[2]),
        f32::midpoint(bounds[1], bounds[3]),
    )
}

/// What a drag does to the selection when the pointer has gone from `start` to `now`. `center` is what a scale
/// or a turn is around.
#[must_use]
pub fn matrix_for(handle: Handle, start: Point, center: Point, now: Point) -> Affine {
    match handle {
        Handle::Move => Affine::translate(now.x - start.x, now.y - start.y),
        Handle::Scale(_) => {
            let base = start.distance(center).max(1e-3);
            let factor = (now.distance(center) / base).clamp(SCALE_RANGE.0, SCALE_RANGE.1);
            Affine::scale_about(center, factor)
        }
        Handle::Rotate => {
            let before = (start.y - center.y).atan2(start.x - center.x);
            let after = (now.y - center.y).atan2(now.x - center.x);
            Affine::rotate_about(center, after - before)
        }
    }
}

// --- the selection ----------------------------------------------------------------------------

/// The strokes with these ids, with the layer and position of each, in drawing order.
pub(crate) fn selected<'a>(editor: &'a Editor, ids: &[Uuid]) -> Vec<(usize, usize, &'a Stroke)> {
    if ids.is_empty() {
        return Vec::new();
    }
    let wanted: HashSet<Uuid> = ids.iter().copied().collect();
    editor
        .document()
        .layers
        .iter()
        .enumerate()
        .flat_map(|(l, layer)| {
            layer
                .strokes
                .iter()
                .enumerate()
                .filter(|(_, s)| wanted.contains(&s.id))
                .map(move |(i, s)| (l, i, s))
        })
        .collect()
}

fn originals_of(editor: &Editor, ids: &[Uuid]) -> Vec<Original> {
    selected(editor, ids)
        .into_iter()
        .map(|(layer, index, stroke)| Original {
            layer,
            index,
            stroke: stroke.clone(),
        })
        .collect()
}

/// The box around the selected strokes, as left, bottom, right and top; `None` if nothing is selected.
#[must_use]
pub fn selection_bounds(editor: &Editor, ids: &[Uuid]) -> Option<[f32; 4]> {
    bounds_of(selected(editor, ids).into_iter().map(|(_, _, s)| s))
}

/// Forgets selected strokes that are no longer in the drawing (erased, or undone away).
pub fn prune_selection(editor: &Editor, ids: &mut Vec<Uuid>) {
    if ids.is_empty() {
        return;
    }
    let present: HashSet<Uuid> = selected(editor, ids).iter().map(|(_, _, s)| s.id).collect();
    ids.retain(|id| present.contains(id));
}

/// The topmost stroke the pointer would count as touching, on visible layers. A locked stroke is not touched.
fn top_stroke_at(editor: &Editor, at: Point, radius: f32) -> Option<Uuid> {
    editor
        .document()
        .layers
        .iter()
        .rev()
        .filter(|l| l.content && l.visible)
        .flat_map(|l| l.strokes.iter().rev())
        .filter(|s| !editor.is_locked(s.id))
        .find(|s| hit::touches(s, at, radius))
        .map(|s| s.id)
}

/// The strokes on visible layers of which at least `coverage` (a fraction) lies inside `polygon`. A coverage of
/// `0` takes any stroke with a point inside. A locked stroke is not taken.
#[must_use]
pub fn strokes_in(editor: &Editor, polygon: &[Point], coverage: f32) -> Vec<Uuid> {
    editor
        .document()
        .layers
        .iter()
        .filter(|l| l.content && l.visible)
        .flat_map(|l| l.strokes.iter())
        .filter(|s| !editor.is_locked(s.id))
        .filter(|s| {
            let inside = hit::fraction_inside(s, polygon);
            inside > 0.0 && inside >= coverage
        })
        .map(|s| s.id)
        .collect()
}

/// The four corners of the box with opposite corners at the first and last of `points`.
fn rectangle(points: &[Point]) -> Vec<Point> {
    let (Some(a), Some(b)) = (points.first(), points.last()) else {
        return Vec::new();
    };
    vec![
        Point::new(a.x, a.y),
        Point::new(b.x, a.y),
        Point::new(b.x, b.y),
        Point::new(a.x, b.y),
    ]
}

// --- changing the selection -------------------------------------------------------------------

/// Puts the history of a finished drag in one step: the strokes of `originals` are now what they were turned
/// into, and were what the originals say.
pub fn record_transform(editor: &mut Editor, originals: &[Original]) {
    let mark = editor.history_mark();
    let mut layers: Vec<usize> = originals.iter().map(|o| o.layer).collect();
    layers.sort_unstable();
    layers.dedup();
    for layer in layers {
        let before = originals
            .iter()
            .filter(|o| o.layer == layer)
            .map(|o| (o.index, o.stroke.clone()))
            .collect();
        editor.record_replacements(layer, before);
    }
    editor.merge_history_since(mark);
}

/// The strokes a drag of the selection is showing in place of the ones in the drawing, each with its layer, and
/// the id of the stroke it stands in for. Empty when there is no such drag, or it has not begun.
#[must_use]
pub fn moving_strokes(gesture: &Gesture) -> Vec<(usize, Uuid, &Stroke)> {
    match gesture {
        Gesture::Transform {
            originals, shown, ..
        } => originals
            .iter()
            .zip(shown)
            .map(|(o, s)| (o.layer, o.stroke.id, s))
            .collect(),
        _ => Vec::new(),
    }
}

/// The box around the selection as it looks now: while it is being dragged, around the strokes being shown, and
/// otherwise around those in the drawing.
#[must_use]
pub fn current_bounds(
    editor: &Editor,
    ids: &[Uuid],
    gesture: Option<&Gesture>,
) -> Option<[f32; 4]> {
    if let Some(gesture) = gesture {
        let moving = moving_strokes(gesture);
        if !moving.is_empty() {
            return bounds_of(moving.into_iter().map(|(_, _, s)| s));
        }
    }
    selection_bounds(editor, ids)
}

/// The topmost stroke a nudge would take hold of at `at`, with its place in the drawing.
#[must_use]
pub fn stroke_to_nudge(editor: &Editor, at: Point, radius: f32) -> Option<(usize, usize)> {
    let id = top_stroke_at(editor, at, radius)?;
    let place = editor.find_stroke(id)?;
    let stroke = editor
        .document()
        .layers
        .get(place.0)?
        .strokes
        .get(place.1)?;
    (stroke.samples.len() >= 2).then_some(place)
}

/// Bends the stroke being nudged for a pointer that is now at `now`. Returns the stroke to show.
fn nudged(
    variant: NudgeVariant,
    base: &Stroke,
    held: usize,
    (start, last, now): (Point, Point, Point),
    reach: f32,
) -> Stroke {
    match variant {
        NudgeVariant::Pull => nudge::pull(base, held, (now.x - start.x, now.y - start.y), reach),
        NudgeVariant::Push => nudge::push(base, last, (now.x - last.x, now.y - last.y), reach),
    }
}

/// Puts the selected strokes through `m` as one step of history. Returns `false` if nothing was selected.
pub fn transform_selection(editor: &mut Editor, ids: &[Uuid], m: &Affine) -> bool {
    let originals = originals_of(editor, ids);
    if originals.is_empty() {
        return false;
    }
    for o in &originals {
        editor.overwrite_stroke(o.layer, o.index, o.stroke.transformed(m));
    }
    record_transform(editor, &originals);
    true
}

/// Cuts every stroke on a visible layer that the `blade` crosses, as one step of history, and returns how many
/// strokes were cut. The first piece of a cut stroke keeps its id; the others get new ones.
pub fn slice_all(editor: &mut Editor, blade: &[Point]) -> usize {
    let cuts = plan_cuts(editor, blade);
    let count = cuts.len();
    let mark = editor.history_mark();
    // Last first, so the positions of the strokes still to cut do not move.
    for (li, si, mut pieces) in cuts.into_iter().rev() {
        for piece in pieces.iter_mut().skip(1) {
            piece.id = editor.next_id();
        }
        editor.splice(li, si, 1, pieces);
    }
    editor.merge_history_since(mark);
    count
}

/// Where the blade would cut, shown before it is let go: one point for every cut.
#[must_use]
pub fn cut_points(editor: &Editor, blade: &[Point]) -> Vec<Point> {
    plan_cuts(editor, blade)
        .into_iter()
        .flat_map(|(_, _, pieces)| {
            pieces
                .into_iter()
                .skip(1)
                .filter_map(|piece| piece.samples.first().map(|s| s.point))
        })
        .collect()
}

/// Every stroke on a visible layer the blade cuts, with its layer, position and pieces (the pieces after the
/// first still have placeholder ids). In drawing order.
fn plan_cuts(editor: &Editor, blade: &[Point]) -> Vec<(usize, usize, Vec<Stroke>)> {
    let Some(reach) = bounds_of_points(blade) else {
        return Vec::new();
    };
    let mut cuts: Vec<(usize, usize, Vec<Stroke>)> = Vec::new();
    for (li, layer) in editor
        .document()
        .layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.content && l.visible)
    {
        for (si, stroke) in layer.strokes.iter().enumerate() {
            if editor.is_locked(stroke.id) {
                continue;
            }
            let near = stroke.bounds().is_some_and(|b| {
                b[0] <= reach[2] && b[2] >= reach[0] && b[1] <= reach[3] && b[3] >= reach[1]
            });
            if near && let Some(pieces) = slice::slice_stroke(stroke, blade, Uuid::nil) {
                cuts.push((li, si, pieces));
            }
        }
    }
    cuts
}

fn bounds_of_points(points: &[Point]) -> Option<[f32; 4]> {
    let first = points.first()?;
    let mut b = [first.x, first.y, first.x, first.y];
    for p in points {
        b[0] = b[0].min(p.x);
        b[1] = b[1].min(p.y);
        b[2] = b[2].max(p.x);
        b[3] = b[3].max(p.y);
    }
    Some(b)
}

// --- pointer events ---------------------------------------------------------------------------

/// Runs the select or slice tool over the events of one frame.
pub fn handle_events(
    ctx: &egui::Context,
    rect: Rect,
    canvas_layer: LayerId,
    events: &[Event],
    env: &mut CanvasEnv<'_>,
) {
    prune_selection(env.editor, env.selection);
    for event in events {
        match event {
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers,
                ..
            } if env.gesture.is_none()
                && rect.contains(*pos)
                && ctx.layer_id_at(*pos) == Some(canvas_layer) =>
            {
                press(env, *pos, modifiers.shift || modifiers.command);
            }
            Event::PointerMoved(pos) if env.gesture.is_some() => moved(env, *pos),
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: false,
                ..
            } => release(env, *pos),
            _ => {}
        }
    }
}

fn press(env: &mut CanvasEnv<'_>, pos: Pos2, additive: bool) {
    let at = env.view.to_canvas(pos);
    if env.tool.mode == Mode::Slice {
        *env.gesture = Some(Gesture::Blade { points: vec![at] });
        return;
    }
    if env.tool.mode == Mode::Nudge {
        let Some((layer, index)) = stroke_to_nudge(env.editor, at, TAP_REACH / env.view.zoom)
        else {
            return;
        };
        let original = env.editor.document().layers[layer].strokes[index].clone();
        let reach = env.config.tools.nudge_reach / env.view.zoom;
        let base = nudge::densify(&original, reach / 10.0);
        let held = nudge::nearest_sample(&base, at).unwrap_or(0);
        *env.gesture = Some(Gesture::Nudge {
            layer,
            index,
            original: Box::new(original),
            shown: Box::new(base.clone()),
            base: Box::new(base),
            moved: false,
            held,
            variant: env.config.tools.nudge_variant,
            start: at,
            last: at,
            reach,
        });
        return;
    }
    let bounds = selection_bounds(env.editor, env.selection);
    let grabbed = bounds
        .filter(|_| !additive)
        .and_then(|b| hit_handle(&frame_of(*env.view, b), pos).map(|h| (h, b)));
    *env.gesture = Some(match grabbed {
        Some((handle, b)) => {
            let originals = originals_of(env.editor, env.selection);
            let snap = edit_snap(env, &originals, handle);
            Gesture::Transform {
                handle,
                start: at,
                center: match handle {
                    Handle::Move => at,
                    Handle::Scale(i) => corner(b, i + 2),
                    Handle::Rotate => middle(b),
                },
                originals,
                shown: Vec::new(),
                snap,
                travelled: 0.0,
            }
        }
        None => Gesture::Marquee {
            start: pos,
            points: vec![at],
            additive,
            travelled: 0.0,
        },
    });
}

fn moved(env: &mut CanvasEnv<'_>, pos: Pos2) {
    let at = env.view.to_canvas(pos);
    let view = *env.view;
    let slice_mode = env.config.tools.slice_mode;
    match env.gesture.as_mut() {
        Some(Gesture::Marquee {
            start,
            points,
            travelled,
            ..
        }) => {
            *travelled = travelled.max(start.distance(pos));
            points.push(at);
        }
        Some(Gesture::Blade { points }) => {
            if slice_mode == SliceMode::Line {
                points.truncate(1);
            }
            points.push(at);
        }
        Some(Gesture::Nudge {
            base,
            shown,
            moved,
            held,
            variant,
            start,
            last,
            reach,
            ..
        }) => {
            let next = nudged(*variant, base, *held, (*start, *last, at), *reach);
            if *variant == NudgeVariant::Push {
                // A push carries what it has already moved, so the next step starts from here.
                base.as_mut().clone_from(&next);
            }
            **shown = next;
            // A click is never quite still; only a pen that has really gone somewhere changes the stroke.
            *moved |= view.to_screen(*start).distance(pos) >= NUDGE_DEAD_ZONE;
            *last = at;
        }
        Some(Gesture::Transform {
            handle,
            start,
            center,
            originals,
            shown,
            snap,
            travelled,
        }) => {
            *travelled = travelled.max(view.to_screen(*start).distance(pos));
            // A press that has not moved yet is still a tap; leave the strokes where they are.
            if *handle == Handle::Move && *travelled < TAP_DISTANCE {
                return;
            }
            let at = snapped_drag(snap.as_mut(), *handle, (*start, *center), at);
            let m = matrix_for(*handle, *start, *center, at);
            *shown = originals.iter().map(|o| o.stroke.transformed(&m)).collect();
        }
        None => {}
    }
}

/// The pointer of a drag of the selection by `handle` as it counts once snapping has had its say: a move lands its
/// nearest point on a target, a scale lands the corner on one, and a turn lands on a multiple of 15 degrees when
/// it is near. `start` and `center` are where the drag began and what it is about. The strokes that are shown are
/// the ones that are kept, so this is the one place the pointer is changed.
fn snapped_drag(
    snap: Option<&mut EditSnap>,
    handle: Handle,
    (start, center): (Point, Point),
    at: Point,
) -> Point {
    let Some(snap) = snap else { return at };
    snap.landing = None;
    let targets = snap.plan.map(|plan| plan.targets(&snap.keys));
    match handle {
        Handle::Move => {
            let Some(targets) = targets else { return at };
            let delta = (at.x - start.x, at.y - start.y);
            let Some(((dx, dy), found)) = targets.nearest_for_move(&snap.anchors, delta) else {
                return at;
            };
            snap.landing = Some(found.point);
            Point::new(at.x + dx, at.y + dy)
        }
        Handle::Scale(_) => {
            let Some((pointer, corner)) =
                targets.and_then(|targets| scale_snapped(start, center, at, &targets))
            else {
                return at;
            };
            snap.landing = Some(corner);
            pointer
        }
        Handle::Rotate => {
            if !snap.rotate {
                return at;
            }
            let Some(pointer) = turn_snapped(start, center, at, ROTATION_STEP, ROTATION_REACH)
            else {
                return at;
            };
            snap.landing = Some(pointer);
            pointer
        }
    }
}

fn release(env: &mut CanvasEnv<'_>, pos: Pos2) {
    let Some(gesture) = env.gesture.take() else {
        return;
    };
    match gesture {
        Gesture::Marquee {
            points,
            additive,
            travelled,
            ..
        } => {
            if travelled < TAP_DISTANCE {
                tap_select(env, pos, additive);
            } else {
                finish_marquee(env, &points, additive);
            }
        }
        Gesture::Transform {
            handle,
            originals,
            shown,
            travelled,
            ..
        } => {
            if handle == Handle::Move && travelled < TAP_DISTANCE {
                tap_select(env, pos, false);
            } else if !shown.is_empty() {
                // Put in the drawing now, in one go, as one step to undo.
                for (o, stroke) in originals.iter().zip(shown) {
                    env.editor.overwrite_stroke(o.layer, o.index, stroke);
                }
                record_transform(env.editor, &originals);
            }
        }
        Gesture::Blade { points } => {
            slice_all(env.editor, &points);
        }
        Gesture::Nudge {
            layer,
            index,
            original,
            shown,
            moved,
            ..
        } => {
            // Put in the drawing now, in one go, as one step to undo. A press that never moved changes nothing.
            if moved && env.editor.overwrite_stroke(layer, index, *shown) {
                env.editor
                    .record_replacements(layer, vec![(index, *original)]);
            }
        }
    }
}

/// A tap picks the topmost stroke under the pointer; a tap on bare paper clears the selection.
fn tap_select(env: &mut CanvasEnv<'_>, pos: Pos2, additive: bool) {
    let at = env.view.to_canvas(pos);
    let radius = TAP_REACH / env.view.zoom;
    match top_stroke_at(env.editor, at, radius) {
        Some(id) if additive => {
            // A stroke in a group goes in and out with the whole group.
            let mut group = vec![id];
            env.editor.marks().expand(&mut group);
            if env.selection.contains(&id) {
                env.selection.retain(|s| !group.contains(s));
            } else {
                for member in group {
                    if !env.selection.contains(&member) {
                        env.selection.push(member);
                    }
                }
            }
        }
        Some(id) => {
            env.selection.clear();
            env.selection.push(id);
            env.editor.marks().expand(env.selection);
        }
        None if !additive => env.selection.clear(),
        None => {}
    }
}

fn finish_marquee(env: &mut CanvasEnv<'_>, points: &[Point], additive: bool) {
    let polygon = match env.config.tools.select_shape {
        SelectShape::Lasso => points.to_vec(),
        SelectShape::Rectangle => rectangle(points),
    };
    let found = strokes_in(env.editor, &polygon, env.config.tools.select_coverage);
    if additive {
        for id in found {
            if !env.selection.contains(&id) {
                env.selection.push(id);
            }
        }
    } else {
        *env.selection = found;
    }
    // A stroke in a group brings the rest of its group.
    env.editor.marks().expand(env.selection);
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use ink_core::{Color, IdGen, Pen, StrokeBuilder};

    use super::*;

    fn near(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-3
    }

    /// A horizontal line at height `y` from x = 0 to x = 100.
    fn line(editor: &mut Editor, y: f32) -> Uuid {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(2.0), Color::BLACK, 1.0, 0.0)
            .with_id(editor.next_id());
        for i in 0..=10 {
            b.push(Point::new(i as f32 * 10.0, y), None, f64::from(i) * 0.01);
        }
        let stroke = b.finish();
        let id = stroke.id;
        editor.commit_stroke(0, stroke);
        id
    }

    fn square(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Point> {
        vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ]
    }

    #[test]
    fn dragging_moves_scales_and_turns_around_the_right_points() {
        let moved = matrix_for(
            Handle::Move,
            Point::new(1.0, 1.0),
            Point::new(0.0, 0.0),
            Point::new(4.0, -2.0),
        );
        assert!(near(
            moved.apply(Point::new(0.0, 0.0)),
            Point::new(3.0, -3.0)
        ));

        let anchor = Point::new(10.0, 10.0);
        let scaled = matrix_for(
            Handle::Scale(2),
            Point::new(20.0, 10.0),
            anchor,
            Point::new(30.0, 10.0),
        );
        assert!(near(scaled.apply(anchor), anchor), "the far corner stays");
        assert!(near(
            scaled.apply(Point::new(20.0, 10.0)),
            Point::new(30.0, 10.0)
        ));

        let turned = matrix_for(
            Handle::Rotate,
            Point::new(10.0, 0.0),
            Point::new(0.0, 0.0),
            Point::new(0.0, 10.0),
        );
        assert!(near(
            turned.apply(Point::new(10.0, 0.0)),
            Point::new(0.0, 10.0)
        ));
        assert!(
            (matrix_for(
                Handle::Rotate,
                Point::new(1.0, 0.0),
                Point::new(0.0, 0.0),
                Point::new(0.0, 1.0)
            )
            .b - FRAC_PI_2.sin())
            .abs()
                < 1e-5
        );
    }

    #[test]
    fn scaling_cannot_collapse_a_selection_or_blow_it_up() {
        let center = Point::new(0.0, 0.0);
        let start = Point::new(10.0, 0.0);
        let tiny = matrix_for(Handle::Scale(0), start, center, center);
        assert!((tiny.scale_factor() - SCALE_RANGE.0).abs() < 1e-6);
        let huge = matrix_for(Handle::Scale(0), start, center, Point::new(1.0e6, 0.0));
        assert!((huge.scale_factor() - SCALE_RANGE.1).abs() < 1e-4);
    }

    #[test]
    fn the_handles_win_over_the_inside_of_the_box() {
        let frame = frame_of(ViewState::default(), [0.0, 0.0, 100.0, 50.0]);
        assert_eq!(hit_handle(&frame, frame.rotate), Some(Handle::Rotate));
        assert_eq!(hit_handle(&frame, frame.corners[0]), Some(Handle::Scale(0)));
        assert_eq!(hit_handle(&frame, frame.corners[2]), Some(Handle::Scale(2)));
        let middle = egui::pos2(
            f32::midpoint(frame.corners[0].x, frame.corners[2].x),
            f32::midpoint(frame.corners[0].y, frame.corners[2].y),
        );
        assert_eq!(hit_handle(&frame, middle), Some(Handle::Move));
        assert_eq!(hit_handle(&frame, egui::pos2(900.0, 900.0)), None);
        assert!(
            frame.rotate.y < frame.corners[0].y,
            "the turning handle floats above the box"
        );
    }

    #[test]
    fn the_frame_turns_with_the_paper_and_its_handles_and_inside_turn_with_it() {
        let bounds = [0.0, 0.0, 100.0, 50.0];
        let upright = frame_of(ViewState::default(), bounds);
        // A quarter turn counterclockwise about the origin: the box's right side is now up on the screen.
        let view = ViewState {
            angle: std::f32::consts::FRAC_PI_2,
            ..ViewState::default()
        };
        let turned = frame_of(view, bounds);
        for (a, b) in upright.corners.iter().zip(&turned.corners) {
            // Turned about the origin (0, 0): (x, y) on the screen goes to (y, -x).
            assert!(
                (b.x - a.y).abs() < 1e-3 && (b.y + a.x).abs() < 1e-3,
                "{a:?} -> {b:?}"
            );
        }
        // The turning handle is still on the side that is up on the paper, which is now the left of the screen.
        assert!(turned.rotate.x < turned.corners.iter().map(|c| c.x).fold(f32::MAX, f32::min));
        assert_eq!(hit_handle(&turned, turned.rotate), Some(Handle::Rotate));
        assert_eq!(
            hit_handle(&turned, turned.corners[2]),
            Some(Handle::Scale(2))
        );
        let middle = egui::pos2(
            f32::midpoint(turned.corners[0].x, turned.corners[2].x),
            f32::midpoint(turned.corners[0].y, turned.corners[2].y),
        );
        assert_eq!(hit_handle(&turned, middle), Some(Handle::Move));
        // A point that is inside the box's upright rectangle but outside the turned box is not a grab.
        let outside = egui::pos2(middle.x + 80.0, middle.y + 60.0);
        assert_eq!(hit_handle(&turned, outside), None);
        // At any turn, the box has the four corners the strokes' box has, a little out.
        for angle in [-2.0_f32, -0.7, 0.3, 1.9] {
            let view = ViewState {
                angle,
                zoom: 2.0,
                ..ViewState::default()
            };
            let frame = frame_of(view, bounds);
            let (a, b) = (
                frame.corners[0].distance(frame.corners[1]),
                frame.corners[1].distance(frame.corners[2]),
            );
            assert!(
                (a - (200.0 + 2.0 * FRAME_PAD)).abs() < 1e-2
                    && (b - (100.0 + 2.0 * FRAME_PAD)).abs() < 1e-2,
                "{angle}: {a} {b}"
            );
        }
    }

    #[test]
    fn a_tap_picks_the_topmost_stroke_within_reach() {
        let mut editor = Editor::blank(IdGen::new(1));
        let low = line(&mut editor, 0.0);
        let high = line(&mut editor, 0.0);
        assert_eq!(
            top_stroke_at(&editor, Point::new(50.0, 3.0), 9.0),
            Some(high)
        );
        assert_ne!(Some(low), Some(high));
        assert_eq!(top_stroke_at(&editor, Point::new(50.0, 40.0), 9.0), None);
    }

    #[test]
    fn a_lasso_takes_what_is_mostly_inside_and_a_zero_coverage_takes_any_touch() {
        let mut editor = Editor::blank(IdGen::new(1));
        let inside = line(&mut editor, 10.0);
        let half = line(&mut editor, 30.0);
        let outside = line(&mut editor, 90.0);
        // Covers x 0..60 of the second line only: 7 of its 11 points.
        let around_inside = square(-10.0, 0.0, 120.0, 20.0);
        assert_eq!(strokes_in(&editor, &around_inside, 0.5), vec![inside]);
        let left = square(-10.0, 20.0, 45.0, 40.0);
        assert!(
            strokes_in(&editor, &left, 0.8).is_empty(),
            "only 5 of 11 points are inside"
        );
        assert_eq!(strokes_in(&editor, &left, 0.4), vec![half]);
        assert_eq!(strokes_in(&editor, &left, 0.0), vec![half]);
        assert!(
            !strokes_in(&editor, &square(-10.0, 0.0, 120.0, 100.0), 1.0).contains(&Uuid::nil())
        );
        assert_eq!(
            strokes_in(&editor, &square(-10.0, 0.0, 120.0, 100.0), 1.0).len(),
            3
        );
        assert!(
            strokes_in(&editor, &around_inside, 0.5)
                .iter()
                .all(|id| *id != outside)
        );
    }

    #[test]
    fn moving_the_selection_is_one_step_of_history_and_undoes_cleanly() {
        let mut editor = Editor::blank(IdGen::new(1));
        let a = line(&mut editor, 0.0);
        let b = line(&mut editor, 20.0);
        let before = editor.document().layers[0].strokes.clone();
        let steps = editor.history_mark();
        assert!(transform_selection(
            &mut editor,
            &[a, b],
            &Affine::translate(5.0, 7.0)
        ));
        assert_eq!(
            editor.history_mark(),
            steps + 1,
            "both strokes move in one step"
        );
        let moved = &editor.document().layers[0].strokes;
        assert_eq!(moved[0].id, a, "a moved stroke keeps its identity");
        assert!(near(moved[0].samples[0].point, Point::new(5.0, 7.0)));
        assert!(near(moved[1].samples[0].point, Point::new(5.0, 27.0)));
        assert!(editor.undo());
        assert_eq!(editor.document().layers[0].strokes, before);
        assert!(editor.redo());
        assert!(near(
            editor.document().layers[0].strokes[0].samples[0].point,
            Point::new(5.0, 7.0)
        ));
        assert!(!transform_selection(&mut editor, &[], &Affine::IDENTITY));
    }

    #[test]
    fn a_turn_near_a_multiple_of_fifteen_degrees_lands_on_it_and_one_between_stays() {
        let (center, start) = (Point::new(0.0, 0.0), Point::new(100.0, 0.0));
        let at_degrees = |degrees: f32| {
            let a = degrees.to_radians();
            Point::new(100.0 * a.cos(), 100.0 * a.sin())
        };
        // 13 degrees is 2 from 15: it lands on 15, at the same distance from the center.
        let snapped = turn_snapped(
            start,
            center,
            at_degrees(13.0),
            ROTATION_STEP,
            ROTATION_REACH,
        )
        .expect("near 15");
        let found = snapped.y.atan2(snapped.x).to_degrees();
        assert!((found - 15.0).abs() < 1e-3, "{found}");
        assert!((snapped.distance(center) - 100.0).abs() < 1e-3);
        // Further than the reach from every multiple (7.5 is the middle between 0 and 15).
        assert_eq!(
            turn_snapped(
                start,
                center,
                at_degrees(7.5),
                ROTATION_STEP,
                ROTATION_REACH
            ),
            None
        );
        // Backwards, and past a half turn.
        let back = turn_snapped(
            start,
            center,
            at_degrees(-44.0),
            ROTATION_STEP,
            ROTATION_REACH,
        )
        .expect("near -45");
        assert!((back.y.atan2(back.x).to_degrees() + 45.0).abs() < 1e-3);
        let far = turn_snapped(
            start,
            center,
            at_degrees(178.0),
            ROTATION_STEP,
            ROTATION_REACH,
        )
        .expect("near 180");
        assert!((far.y.atan2(far.x).to_degrees().abs() - 180.0).abs() < 1e-3);
    }

    #[test]
    fn the_turn_is_measured_from_where_the_drag_began_not_from_the_horizontal() {
        // The handle began at 90 degrees: a turn of 13 degrees from there is at 103, which lands on 105.
        let (center, start) = (Point::new(10.0, 10.0), Point::new(10.0, 110.0));
        let a = 103.0_f32.to_radians();
        let now = Point::new(10.0 + 100.0 * a.cos(), 10.0 + 100.0 * a.sin());
        let snapped =
            turn_snapped(start, center, now, ROTATION_STEP, ROTATION_REACH).expect("near");
        let found = (snapped.y - 10.0).atan2(snapped.x - 10.0).to_degrees();
        assert!((found - 105.0).abs() < 1e-3, "{found}");
    }

    #[test]
    fn a_scale_lands_the_corner_on_the_nearest_target_along_the_line_from_the_center() {
        let keys = [Point::new(200.0, 200.0)];
        let targets = snap::Targets {
            grid: None,
            lattice: None,
            keys: &keys,
            reach: 12.0,
        };
        let (center, start) = (Point::new(0.0, 0.0), Point::new(100.0, 100.0));
        // The corner is dragged to about (196, 196): near the key point at (200, 200).
        let now = Point::new(196.0, 196.0);
        let (pointer, corner) = scale_snapped(start, center, now, &targets).expect("a snap");
        assert!(
            corner.distance(Point::new(200.0, 200.0)) < 1e-3,
            "{corner:?}"
        );
        assert!((pointer.distance(center) - 200.0 * 2.0_f32.sqrt()).abs() < 1e-2);
        // Too far from it.
        assert_eq!(
            scale_snapped(start, center, Point::new(120.0, 120.0), &targets),
            None
        );
        // A target off the diagonal is reached as near as the diagonal allows.
        let off = [Point::new(204.0, 196.0)];
        let targets = snap::Targets {
            keys: &off,
            ..targets
        };
        let (_, corner) = scale_snapped(start, center, now, &targets).expect("a snap");
        assert!(
            (corner.x - corner.y).abs() < 1e-3,
            "it stays on the diagonal: {corner:?}"
        );
        assert!(corner.distance(Point::new(200.0, 200.0)) < 1e-3);
    }

    #[test]
    fn a_scale_is_never_snapped_to_nothing_or_from_nowhere() {
        let targets = snap::Targets {
            grid: None,
            lattice: None,
            keys: &[],
            reach: 12.0,
        };
        let (center, start) = (Point::new(0.0, 0.0), Point::new(100.0, 100.0));
        assert_eq!(
            scale_snapped(start, center, Point::new(150.0, 150.0), &targets),
            None
        );
        // A handle at the center itself cannot scale.
        let keys = [Point::new(1.0, 1.0)];
        let targets = snap::Targets {
            keys: &keys,
            ..targets
        };
        assert_eq!(
            scale_snapped(center, center, Point::new(1.0, 1.0), &targets),
            None
        );
    }

    #[test]
    fn the_bounds_of_a_selection_follow_the_strokes_after_they_move() {
        let mut editor = Editor::blank(IdGen::new(1));
        let a = line(&mut editor, 0.0);
        let _other = line(&mut editor, 500.0);
        assert_eq!(
            selection_bounds(&editor, &[a]),
            Some([-1.0, -1.0, 101.0, 1.0])
        );
        transform_selection(&mut editor, &[a], &Affine::translate(10.0, 10.0));
        assert_eq!(
            selection_bounds(&editor, &[a]),
            Some([9.0, 9.0, 111.0, 11.0])
        );
        assert_eq!(selection_bounds(&editor, &[]), None);
    }

    #[test]
    fn slicing_cuts_every_stroke_the_blade_crosses_in_one_undoable_step() {
        let mut editor = Editor::blank(IdGen::new(1));
        let a = line(&mut editor, 0.0);
        let _far = line(&mut editor, 500.0);
        let b = line(&mut editor, 20.0);
        let before = editor.document().layers[0].strokes.clone();
        let blade = [Point::new(45.0, -10.0), Point::new(45.0, 30.0)];
        let steps = editor.history_mark();
        assert_eq!(slice_all(&mut editor, &blade), 2);
        assert_eq!(editor.history_mark(), steps + 1);
        let strokes = &editor.document().layers[0].strokes;
        assert_eq!(
            strokes.len(),
            5,
            "two strokes became four pieces beside the one left alone"
        );
        let mut ids: Vec<Uuid> = strokes.iter().map(|s| s.id).collect();
        assert!(
            ids.contains(&a) && ids.contains(&b),
            "the first pieces keep their ids"
        );
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 5, "no two pieces share an id");
        assert!(editor.undo());
        assert_eq!(editor.document().layers[0].strokes, before);
    }

    #[test]
    fn a_blade_that_misses_everything_changes_nothing() {
        let mut editor = Editor::blank(IdGen::new(1));
        line(&mut editor, 0.0);
        let revision = editor.revision();
        let blade = [Point::new(500.0, -10.0), Point::new(500.0, 30.0)];
        assert_eq!(slice_all(&mut editor, &blade), 0);
        assert_eq!(editor.revision(), revision, "nothing was touched");
        assert_eq!(slice_all(&mut editor, &[]), 0);
    }

    #[test]
    fn selected_strokes_that_are_gone_drop_out_of_the_selection() {
        let mut editor = Editor::blank(IdGen::new(1));
        let a = line(&mut editor, 0.0);
        let b = line(&mut editor, 20.0);
        let mut selection = vec![a, b];
        editor.remove_stroke(0, 0);
        prune_selection(&editor, &mut selection);
        assert_eq!(selection, vec![b]);
    }

    #[test]
    fn the_two_corner_rectangle_is_the_box_between_the_first_and_last_point() {
        let path = [
            Point::new(0.0, 0.0),
            Point::new(5.0, 9.0),
            Point::new(10.0, 4.0),
        ];
        let polygon = rectangle(&path);
        assert_eq!(polygon.len(), 4);
        assert!(hit::point_in_polygon(Point::new(5.0, 2.0), &polygon));
        assert!(!hit::point_in_polygon(Point::new(5.0, 9.0), &polygon));
        assert_eq!(rectangle(&[]), Vec::<Point>::new());
    }
}
