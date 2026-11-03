//! The selection: how it is drawn over the paper, and what can be done with it.
//!
//! The commands work on an [`Editor`] and the list of selected ids, so they are the same whether a key, a button
//! or a menu runs them, and each is one step to undo.

use eframe::egui::{self, Color32, CursorIcon, Painter, Pos2, Rect, Response, Shape};
use ink_core::document::Stroke;
use ink_core::{Affine, Editor, Point};
use uuid::Uuid;

use crate::canvas::CanvasEnv;
use crate::config::SelectShape;
use crate::precision;
use crate::state::{Mode, ViewState};
use crate::theme;
use crate::tools::{self, Gesture, HANDLE_RADIUS, Handle};

// --- drawing ----------------------------------------------------------------------------------

/// A line that shows on paper of any color: a dark edge under a bright line.
fn outlined(color: Color32, width: f32) -> [egui::Stroke; 2] {
    [
        egui::Stroke::new(width + 2.0, Color32::from_black_alpha(60)),
        egui::Stroke::new(width, color),
    ]
}

fn dashed(painter: &Painter, path: &[Pos2], color: Color32, width: f32) {
    for stroke in outlined(color, width) {
        painter.extend(Shape::dashed_line(path, stroke, 7.0, 5.0));
    }
}

fn closed(points: [Pos2; 4]) -> [Pos2; 5] {
    [points[0], points[1], points[2], points[3], points[0]]
}

fn handle_dot(painter: &Painter, at: Pos2, round: bool) {
    let rect = Rect::from_center_size(at, egui::Vec2::splat(HANDLE_RADIUS * 2.0));
    let rounding = if round { HANDLE_RADIUS } else { 3.0 };
    painter.rect_filled(
        rect.translate(egui::vec2(0.0, 1.0)).expand(1.5),
        rounding,
        Color32::from_black_alpha(55),
    );
    painter.rect_filled(rect, rounding, Color32::WHITE);
    painter.rect_stroke(
        rect,
        rounding,
        egui::Stroke::new(1.8, theme::ACCENT),
        egui::StrokeKind::Inside,
    );
}

fn paint_frame(painter: &Painter, frame: &tools::Frame) {
    painter.add(Shape::convex_polygon(
        frame.corners.to_vec(),
        theme::ACCENT.gamma_multiply(0.07),
        egui::Stroke::NONE,
    ));
    dashed(painter, &closed(frame.corners), theme::ACCENT, 1.4);
    painter.line_segment(
        [frame.stem, frame.rotate],
        egui::Stroke::new(1.4, theme::ACCENT),
    );
    for corner in frame.corners {
        handle_dot(painter, corner, false);
    }
    handle_dot(painter, frame.rotate, true);
}

fn paint_marquee(painter: &Painter, env: &CanvasEnv<'_>, points: &[Point]) {
    let screen: Vec<Pos2> = points.iter().map(|p| env.view.to_screen(*p)).collect();
    let (Some(first), Some(last)) = (screen.first().copied(), screen.last().copied()) else {
        return;
    };
    match env.config.tools.select_shape {
        SelectShape::Rectangle => {
            let rect = Rect::from_two_pos(first, last);
            painter.rect_filled(rect, 1.0, theme::ACCENT.gamma_multiply(0.12));
            dashed(
                painter,
                &closed([
                    rect.left_top(),
                    rect.right_top(),
                    rect.right_bottom(),
                    rect.left_bottom(),
                ]),
                theme::ACCENT,
                1.4,
            );
        }
        SelectShape::Lasso => {
            let mut path = screen;
            path.push(first);
            dashed(painter, &path, theme::ACCENT, 1.4);
        }
    }
}

/// Marks, before the blade is lifted, each place it is going to cut.
fn paint_cut_points(painter: &Painter, env: &CanvasEnv<'_>, blade: &[Point]) {
    for cut in tools::cut_points(env.editor, blade) {
        let at = env.view.to_screen(cut);
        painter.circle_filled(at, 6.5, theme::ACCENT.gamma_multiply(0.35));
        painter.circle_filled(at, 3.6, Color32::WHITE);
        painter.circle_stroke(at, 3.6, egui::Stroke::new(1.6, theme::ACCENT));
    }
}

fn paint_blade(painter: &Painter, view: ViewState, points: &[Point]) {
    let screen: Vec<Pos2> = points.iter().map(|p| view.to_screen(*p)).collect();
    if screen.len() < 2 {
        return;
    }
    for stroke in outlined(Color32::WHITE, 1.6) {
        painter.add(Shape::line(screen.clone(), stroke));
    }
    if let (Some(first), Some(last)) = (screen.first(), screen.last()) {
        for end in [first, last] {
            painter.circle_filled(*end, 3.2, Color32::WHITE);
            painter.circle_stroke(*end, 3.2, egui::Stroke::new(1.2, theme::ACCENT));
        }
    }
}

/// Outlines the stroke the nudge tool has hold of, or would take hold of where the pointer is.
fn paint_nudge_target(painter: &Painter, env: &CanvasEnv<'_>) {
    let stroke: Option<&Stroke> = match &*env.gesture {
        // The stroke as it looks while it is being shaped.
        Some(Gesture::Nudge { shown, .. }) => Some(shown),
        Some(_) => None,
        None => painter
            .ctx()
            .pointer_hover_pos()
            .and_then(|p| {
                tools::stroke_to_nudge(
                    env.editor,
                    env.view.to_canvas(p),
                    tools::TAP_REACH / env.view.zoom,
                )
            })
            .and_then(|(l, i)| env.editor.document().layers.get(l)?.strokes.get(i)),
    };
    let Some(stroke) = stroke else {
        return;
    };
    let path: Vec<Pos2> = stroke.points().map(|p| env.view.to_screen(p)).collect();
    if path.len() >= 2 {
        for line in outlined(theme::ACCENT, 1.8) {
            painter.add(Shape::line(path.clone(), line));
        }
    }
}

/// Draws the selection box with its handles, and the loop or blade being drawn.
pub fn paint_overlay(painter: &Painter, env: &CanvasEnv<'_>) {
    if env.tool.mode == Mode::Nudge {
        paint_nudge_target(painter, env);
    }
    if env.tool.mode == Mode::Select
        && let Some(bounds) = tools::current_bounds(env.editor, env.selection, env.gesture.as_ref())
    {
        paint_frame(painter, &tools::frame_of(*env.view, bounds));
    }
    match &*env.gesture {
        Some(Gesture::Marquee { points, .. }) => paint_marquee(painter, env, points),
        Some(Gesture::Blade { points }) => {
            paint_cut_points(painter, env, points);
            paint_blade(painter, *env.view, points);
        }
        _ => {}
    }
    if let Some(at) = env.gesture.as_ref().and_then(tools::landing) {
        precision::paint_marker(painter, env.view.to_screen(at));
    }
}

/// The pointer over the paper while the select or slice tool is in use: a cross to aim with, and over the
/// selection what grabbing it would do.
pub fn set_cursor(response: &Response, env: &CanvasEnv<'_>) {
    let icon_for = |handle: Handle| match handle {
        Handle::Move => CursorIcon::Move,
        Handle::Rotate => CursorIcon::Grab,
        Handle::Scale(i) if i % 2 == 0 => CursorIcon::ResizeNwSe,
        Handle::Scale(_) => CursorIcon::ResizeNeSw,
    };
    let icon = match (&*env.gesture, env.tool.mode) {
        (Some(Gesture::Transform { handle, .. }), _) => match handle {
            Handle::Move | Handle::Rotate => CursorIcon::Grabbing,
            scale @ Handle::Scale(_) => icon_for(*scale),
        },
        (_, Mode::Select) => response
            .hover_pos()
            .zip(tools::selection_bounds(env.editor, env.selection))
            .and_then(|(at, bounds)| tools::hit_handle(&tools::frame_of(*env.view, bounds), at))
            .map_or(CursorIcon::Crosshair, icon_for),
        _ => CursorIcon::Crosshair,
    };
    response.ctx.set_cursor_icon(icon);
}

// --- commands ---------------------------------------------------------------------------------

/// Selects every stroke on the visible layers that is not locked. Returns how many.
pub fn select_all(editor: &Editor, selection: &mut Vec<Uuid>) -> usize {
    selection.clear();
    selection.extend(
        editor
            .document()
            .layers
            .iter()
            .filter(|l| l.content && l.visible)
            .flat_map(|l| l.strokes.iter().map(|s| s.id))
            .filter(|id| !editor.is_locked(*id)),
    );
    selection.len()
}

/// Makes the selection one group, so that picking up any of it picks up all of it. Returns `false` if the
/// selection is fewer than two strokes.
pub fn group(editor: &mut Editor, selection: &[Uuid]) -> bool {
    editor.edit_marks(|m| m.group(selection))
}

/// Breaks up the groups the selection is in. Returns how many groups there were.
pub fn ungroup(editor: &mut Editor, selection: &[Uuid]) -> usize {
    editor.edit_marks(|m| m.ungroup(selection))
}

/// Locks the selected strokes and lets them go from the selection: a locked stroke cannot be selected, erased,
/// nudged or sliced. Returns how many.
pub fn lock(editor: &mut Editor, selection: &mut Vec<Uuid>) -> usize {
    let ids: Vec<Uuid> = tools::selected(editor, selection)
        .iter()
        .map(|(_, _, s)| s.id)
        .collect();
    let count = ids.len();
    editor.edit_marks(|m| m.lock(ids));
    selection.clear();
    count
}

/// Unlocks every stroke of the drawing. Returns how many were locked.
pub fn unlock_all(editor: &mut Editor) -> usize {
    editor.edit_marks(ink_core::marks::Marks::unlock_all)
}

/// Moves the selected strokes to the layer `by` places up (`1`) or down (`-1`) from the layer each is on, as one
/// step. A stroke keeps its place in the drawing order of the layer it goes to: it goes on top of it. Strokes that
/// have no layer that way (or one that cannot hold strokes) stay where they are. Returns how many moved.
pub fn move_to_layer(editor: &mut Editor, selection: &[Uuid], by: isize) -> usize {
    let layers = editor.document().layers.len();
    let holds = |editor: &Editor, i: usize| {
        editor
            .document()
            .layers
            .get(i)
            .is_some_and(|l| l.content && l.visible)
    };
    let mut moves: Vec<(usize, usize, usize)> = Vec::new();
    for (layer, index, _) in tools::selected(editor, selection) {
        // The nearest layer that way that holds strokes.
        let mut to = layer as isize + by.signum();
        while (0..layers as isize).contains(&to) && !holds(editor, to as usize) {
            to += by.signum();
        }
        if (0..layers as isize).contains(&to) && by != 0 {
            moves.push((layer, index, to as usize));
        }
    }
    if moves.is_empty() {
        return 0;
    }
    // Last first, so taking one out does not move the places of the rest.
    moves.sort_unstable_by(|a, b| b.cmp(a));
    let mark = editor.history_mark();
    let mut taken: Vec<(usize, Stroke)> = Vec::with_capacity(moves.len());
    for (layer, index, to) in &moves {
        if let Some(stroke) = editor
            .document()
            .layers
            .get(*layer)
            .and_then(|l| l.strokes.get(*index))
            .cloned()
        {
            editor.remove_stroke(*layer, *index);
            taken.push((*to, stroke));
        }
    }
    // In the order they were drawn in.
    taken.reverse();
    let count = taken.len();
    for (to, stroke) in taken {
        editor.commit_stroke(to, stroke);
    }
    editor.merge_history_since(mark);
    count
}

/// Removes the selected strokes as one step. Returns how many there were.
pub fn delete(editor: &mut Editor, selection: &mut Vec<Uuid>) -> usize {
    let mut places: Vec<(usize, usize)> = tools::selected(editor, selection)
        .iter()
        .map(|(l, i, _)| (*l, *i))
        .collect();
    // Last first, so removing one does not move the others.
    places.sort_unstable_by(|a, b| b.cmp(a));
    let mark = editor.history_mark();
    for (layer, index) in &places {
        editor.remove_stroke(*layer, *index);
    }
    editor.merge_history_since(mark);
    selection.clear();
    places.len()
}

/// The selected strokes, in drawing order, to paste later.
#[must_use]
pub fn copy(editor: &Editor, selection: &[Uuid]) -> Vec<Stroke> {
    tools::selected(editor, selection)
        .into_iter()
        .map(|(_, _, s)| s.clone())
        .collect()
}

/// Adds copies of `items` (each with the layer it goes on), moved by `m` and with ids of their own, as one step.
fn add_copies(editor: &mut Editor, items: Vec<(usize, Stroke)>, m: &Affine) -> Vec<Uuid> {
    let mark = editor.history_mark();
    let mut ids = Vec::with_capacity(items.len());
    for (layer, stroke) in items {
        let mut copy = stroke.transformed(m);
        copy.id = editor.next_id();
        if editor.commit_stroke(layer, copy.clone()) {
            ids.push(copy.id);
        }
    }
    editor.merge_history_since(mark);
    ids
}

/// Copies the selected strokes onto the layers they are on, `offset` canvas units down and to the right, and
/// selects the copies. Returns how many.
pub fn duplicate(editor: &mut Editor, selection: &mut Vec<Uuid>, offset: f32) -> usize {
    let items: Vec<(usize, Stroke)> = tools::selected(editor, selection)
        .into_iter()
        .map(|(layer, _, s)| (layer, s.clone()))
        .collect();
    let ids = add_copies(editor, items, &Affine::translate(offset, -offset));
    *selection = ids;
    selection.len()
}

/// Puts the strokes of `clip` on `layer`, `offset` canvas units down and to the right of where they were copied
/// from, and selects them. Returns how many; none if `layer` cannot hold strokes.
pub fn paste(
    editor: &mut Editor,
    layer: usize,
    clip: &[Stroke],
    offset: f32,
    selection: &mut Vec<Uuid>,
) -> usize {
    let holds_strokes = editor
        .document()
        .layers
        .get(layer)
        .is_some_and(|l| l.content && l.visible);
    if !holds_strokes || clip.is_empty() {
        return 0;
    }
    let items = clip.iter().map(|s| (layer, s.clone())).collect();
    *selection = add_copies(editor, items, &Affine::translate(offset, -offset));
    selection.len()
}

/// Moves the selection by `dx` and `dy` canvas units as one step (`dy` is positive upward).
pub fn move_by(editor: &mut Editor, selection: &[Uuid], dx: f32, dy: f32) -> bool {
    tools::transform_selection(editor, selection, &Affine::translate(dx, dy))
}

/// Mirrors the selection about its own center, left to right or top to bottom.
pub fn flip(editor: &mut Editor, selection: &[Uuid], horizontal: bool) -> bool {
    let Some(b) = tools::selection_bounds(editor, selection) else {
        return false;
    };
    let m = if horizontal {
        Affine::flip_horizontal(f32::midpoint(b[0], b[2]))
    } else {
        Affine::flip_vertical(f32::midpoint(b[1], b[3]))
    };
    tools::transform_selection(editor, selection, &m)
}

#[cfg(test)]
mod tests {
    use ink_core::{Color, IdGen, Pen, StrokeBuilder};

    use super::*;

    /// A horizontal line at height `y` from x = 0 to x = 100, on layer `layer`.
    fn line(editor: &mut Editor, layer: usize, y: f32) -> Uuid {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(2.0), Color::BLACK, 1.0, 0.0)
            .with_id(editor.next_id());
        for i in 0..=10 {
            b.push(Point::new(i as f32 * 10.0, y), None, f64::from(i) * 0.01);
        }
        let stroke = b.finish();
        let id = stroke.id;
        editor.commit_stroke(layer, stroke);
        id
    }

    fn count(editor: &Editor) -> usize {
        editor
            .document()
            .layers
            .iter()
            .map(|l| l.strokes.len())
            .sum()
    }

    #[test]
    fn select_all_takes_what_is_visible_and_nothing_else() {
        let mut editor = Editor::blank(IdGen::new(1));
        let second = editor.add_layer(Some(0));
        line(&mut editor, 0, 0.0);
        line(&mut editor, second, 10.0);
        let mut selection = Vec::new();
        assert_eq!(select_all(&editor, &mut selection), 2);
        editor.set_layer_visible(second, false);
        assert_eq!(select_all(&editor, &mut selection), 1);
    }

    #[test]
    fn locking_takes_the_strokes_out_of_the_selection_and_select_all_leaves_them_out() {
        let mut editor = Editor::blank(IdGen::new(1));
        let a = line(&mut editor, 0, 0.0);
        let b = line(&mut editor, 0, 10.0);
        line(&mut editor, 0, 20.0);
        let mut selection = vec![a, b];
        assert_eq!(lock(&mut editor, &mut selection), 2);
        assert!(selection.is_empty(), "they cannot stay selected");
        assert!(editor.is_locked(a) && editor.is_locked(b));
        assert_eq!(
            select_all(&editor, &mut selection),
            1,
            "only the one that is not locked"
        );
        assert_eq!(unlock_all(&mut editor), 2);
        assert_eq!(select_all(&editor, &mut selection), 3);
    }

    #[test]
    fn grouping_needs_two_and_ungrouping_breaks_the_group_up() {
        let mut editor = Editor::blank(IdGen::new(1));
        let a = line(&mut editor, 0, 0.0);
        let b = line(&mut editor, 0, 10.0);
        assert!(!group(&mut editor, &[a]));
        assert!(group(&mut editor, &[a, b]));
        assert_eq!(editor.marks().groups().len(), 1);
        assert_eq!(ungroup(&mut editor, &[b]), 1);
        assert_eq!(editor.marks().groups().len(), 0);
    }

    /// The layer and the y height of each stroke of the document, in order, layer by layer.
    fn layout(editor: &Editor) -> Vec<(usize, i32)> {
        editor
            .document()
            .layers
            .iter()
            .enumerate()
            .flat_map(|(l, layer)| {
                layer
                    .strokes
                    .iter()
                    .map(move |s| (l, s.points().next().map_or(0, |p| p.y as i32)))
            })
            .collect()
    }

    #[test]
    fn strokes_move_to_the_layer_above_or_below_as_one_step_keeping_their_order_and_ids() {
        let mut editor = Editor::blank(IdGen::new(1));
        let upper = editor.add_layer(Some(0));
        let a = line(&mut editor, 0, 0.0);
        let b = line(&mut editor, 0, 10.0);
        line(&mut editor, 0, 20.0);
        line(&mut editor, upper, 99.0);
        let before = layout(&editor);
        let steps = editor.history_mark();
        assert_eq!(move_to_layer(&mut editor, &[a, b], 1), 2);
        assert_eq!(editor.history_mark(), steps + 1, "one step");
        // Up in the drawing order too: on top of what the layer above already had.
        assert_eq!(layout(&editor), vec![(0, 20), (1, 99), (1, 0), (1, 10)]);
        assert_eq!(
            editor.find_stroke(a).map(|p| p.0),
            Some(1),
            "the same stroke, with its id"
        );
        // And back down.
        assert_eq!(move_to_layer(&mut editor, &[a, b], -1), 2);
        assert!(editor.undo() && editor.undo());
        assert_eq!(layout(&editor), before, "undone to how it began");
    }

    #[test]
    fn a_stroke_with_no_layer_that_way_stays_and_a_layer_that_cannot_hold_strokes_is_skipped() {
        let mut editor = Editor::blank(IdGen::new(1));
        let a = line(&mut editor, 0, 0.0);
        let steps = editor.history_mark();
        assert_eq!(
            move_to_layer(&mut editor, &[a], -1),
            0,
            "nothing below the bottom layer"
        );
        assert_eq!(
            move_to_layer(&mut editor, &[a], 1),
            0,
            "nor above the only one"
        );
        assert_eq!(
            move_to_layer(&mut editor, &[a], 0),
            0,
            "a move of nothing is not a move"
        );
        assert_eq!(editor.history_mark(), steps, "and none of these is a step");
        let hidden = editor.add_layer(Some(0));
        let top = editor.add_layer(Some(hidden));
        editor.set_layer_visible(hidden, false);
        assert_eq!(move_to_layer(&mut editor, &[a], 1), 1);
        assert_eq!(
            editor.find_stroke(a).map(|p| p.0),
            Some(top),
            "it went past the hidden layer"
        );
    }

    #[test]
    fn deleting_removes_the_selection_in_one_step_and_undo_brings_it_back() {
        let mut editor = Editor::blank(IdGen::new(1));
        let a = line(&mut editor, 0, 0.0);
        line(&mut editor, 0, 10.0);
        let c = line(&mut editor, 0, 20.0);
        let before = editor.document().layers[0].strokes.clone();
        let mut selection = vec![a, c];
        let steps = editor.history_mark();
        assert_eq!(delete(&mut editor, &mut selection), 2);
        assert_eq!(count(&editor), 1);
        assert_eq!(selection, Vec::<Uuid>::new());
        assert_eq!(editor.history_mark(), steps + 1);
        assert!(editor.undo());
        assert_eq!(editor.document().layers[0].strokes, before);
    }

    #[test]
    fn duplicating_adds_offset_copies_with_new_ids_and_selects_them() {
        let mut editor = Editor::blank(IdGen::new(1));
        let a = line(&mut editor, 0, 0.0);
        let mut selection = vec![a];
        let steps = editor.history_mark();
        assert_eq!(duplicate(&mut editor, &mut selection, 24.0), 1);
        assert_eq!(count(&editor), 2);
        assert_eq!(editor.history_mark(), steps + 1);
        assert_ne!(selection[0], a, "the copy is selected, not the original");
        let strokes = &editor.document().layers[0].strokes;
        assert_eq!(strokes[0].id, a);
        let (from, to) = (strokes[0].samples[0].point, strokes[1].samples[0].point);
        assert!((to.x - from.x - 24.0).abs() < 1e-3 && (from.y - to.y - 24.0).abs() < 1e-3);
        assert!(editor.undo());
        assert_eq!(count(&editor), 1);
    }

    #[test]
    fn copy_and_paste_work_across_layers_and_each_paste_is_selected() {
        let mut editor = Editor::blank(IdGen::new(1));
        let other = editor.add_layer(Some(0));
        let a = line(&mut editor, 0, 0.0);
        let clip = copy(&editor, &[a]);
        assert_eq!(clip.len(), 1);
        let mut selection = Vec::new();
        assert_eq!(paste(&mut editor, other, &clip, 24.0, &mut selection), 1);
        assert_eq!(editor.document().layers[other].strokes.len(), 1);
        assert_eq!(
            selection,
            vec![editor.document().layers[other].strokes[0].id]
        );
        assert_eq!(paste(&mut editor, other, &clip, 24.0, &mut selection), 1);
        assert_eq!(
            editor.document().layers[other].strokes.len(),
            2,
            "again, and again"
        );
        editor.set_layer_visible(other, false);
        assert_eq!(
            paste(&mut editor, other, &clip, 24.0, &mut selection),
            0,
            "not onto a hidden layer"
        );
        assert_eq!(paste(&mut editor, 99, &clip, 24.0, &mut selection), 0);
        assert_eq!(paste(&mut editor, 0, &[], 24.0, &mut selection), 0);
    }

    #[test]
    fn moving_by_hand_moves_the_selection_and_undo_puts_it_back() {
        let mut editor = Editor::blank(IdGen::new(1));
        let a = line(&mut editor, 0, 0.0);
        let before = editor.document().layers[0].strokes.clone();
        assert!(move_by(&mut editor, &[a], 8.0, -4.0));
        let moved = editor.document().layers[0].strokes[0].samples[0].point;
        assert!((moved.x - 8.0).abs() < 1e-3 && (moved.y + 4.0).abs() < 1e-3);
        assert!(editor.undo());
        assert_eq!(editor.document().layers[0].strokes, before);
        assert!(!move_by(&mut editor, &[], 1.0, 1.0));
    }

    #[test]
    fn flipping_mirrors_about_the_middle_of_the_selection_and_keeps_its_box() {
        let mut editor = Editor::blank(IdGen::new(1));
        let a = line(&mut editor, 0, 0.0);
        let b = line(&mut editor, 0, 40.0);
        let ids = [a, b];
        // Make the two lines different, so a mirror image is not the same picture.
        assert!(move_by(&mut editor, &[a], 30.0, 0.0));
        let before = tools::selection_bounds(&editor, &ids).expect("bounds");
        let first_before = editor.document().layers[0].strokes[0].samples[0].point;
        assert!(flip(&mut editor, &ids, true));
        let after = tools::selection_bounds(&editor, &ids).expect("bounds");
        for (x, y) in before.iter().zip(&after) {
            assert!((x - y).abs() < 1e-3, "{before:?} {after:?}");
        }
        let first_after = editor.document().layers[0].strokes[0].samples[0].point;
        assert!(
            (first_after.x - (before[0] + before[2] - first_before.x)).abs() < 1e-3,
            "a point goes to its mirror image"
        );
        assert!((first_after.y - first_before.y).abs() < 1e-3);
        assert!(flip(&mut editor, &ids, false));
        assert!(!flip(&mut editor, &[], true));
    }
}
