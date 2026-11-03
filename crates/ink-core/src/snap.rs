//! Snapping: drawing a point where something worth lining up with is, when it is close to it.
//!
//! Points snap to the grid and to key points, separately while drawing and while editing. A key point is the two
//! ends of a stroke, so a line can be started or ended exactly on another. The middle of a stroke, the place two
//! strokes cross and the nodes of the isometric and triangle papers are further places of that kind.
//!
//! Everything works on plain points and a [`Document`], so it is tested without a window. Distances are in canvas
//! units; the caller turns a reach in screen points into canvas units with the zoom.

use std::collections::HashSet;
use std::hash::BuildHasher;

use uuid::Uuid;

use crate::document::{Document, Stroke};
use crate::geometry::Point;

/// What a point was drawn to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Grid,
    KeyPoint,
}

/// Where a point goes, and what it was drawn to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub point: Point,
    pub kind: Kind,
}

/// Lines `spacing` apart through the origin of the canvas, running either way. A pattern of rules like a ruled
/// page has only the rows; dots and squares have both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub spacing: f32,
    /// There are vertical lines, so x snaps to them.
    pub columns: bool,
    /// There are horizontal lines, so y snaps to them.
    pub rows: bool,
}

impl Grid {
    /// The place on the grid nearest to `at`: where the lines cross, or the nearest line where there is only one
    /// way of them.
    #[must_use]
    fn nearest(self, at: Point) -> Option<Point> {
        if !(self.spacing.is_finite() && self.spacing > 0.0) || !(self.columns || self.rows) {
            return None;
        }
        let round = |v: f32| (v / self.spacing).round() * self.spacing;
        Some(Point::new(
            if self.columns { round(at.x) } else { at.x },
            if self.rows { round(at.y) } else { at.y },
        ))
    }
}

/// The points where the three families of lines of a lattice paper (the isometric and the triangle) meet. Two of
/// the families are enough to find them, as the third passes through the same points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lattice {
    /// How far apart the parallel lines of a family are, in canvas units.
    pub spacing: f32,
    /// The directions of two of the families, in radians counterclockwise with the y axis up. Each has a line
    /// through the origin.
    pub families: [f32; 2],
}

impl Lattice {
    /// The paper of upright lines and lines at 30 degrees either side of level.
    pub const ISOMETRIC: [f32; 2] = [-std::f32::consts::FRAC_PI_2, -std::f32::consts::FRAC_PI_6];
    /// The paper of level lines and lines at 60 and 120 degrees.
    pub const TRIANGLE: [f32; 2] = [0.0, -std::f32::consts::FRAC_PI_3];

    /// The node nearest to `at`.
    #[must_use]
    fn nearest(self, at: Point) -> Option<Point> {
        if !(self.spacing.is_finite() && self.spacing > 0.0) {
            return None;
        }
        let normal = |a: f32| (-a.sin(), a.cos());
        let (n1, n2) = (normal(self.families[0]), normal(self.families[1]));
        let det = n1.0 * n2.1 - n1.1 * n2.0;
        if det.abs() < 1e-3 {
            return None;
        }
        // How many lines of each family from the origin `at` is.
        let u = (at.x * n1.0 + at.y * n1.1) / self.spacing;
        let v = (at.x * n2.0 + at.y * n2.1) / self.spacing;
        let mut best: Option<(f32, Point)> = None;
        for i in [u.floor(), u.ceil()] {
            for j in [v.floor(), v.ceil()] {
                let (a, b) = (i * self.spacing, j * self.spacing);
                let node = Point::new((a * n2.1 - b * n1.1) / det, (n1.0 * b - n2.0 * a) / det);
                let d = node.distance(at);
                if best.is_none_or(|(least, _)| d < least) {
                    best = Some((d, node));
                }
            }
        }
        best.map(|(_, node)| node)
    }
}

/// The middle of every stroke on a visible layer (halfway along its length), except the strokes in `leave`.
#[must_use]
pub fn middle_points<S: BuildHasher>(document: &Document, leave: &HashSet<Uuid, S>) -> Vec<Point> {
    let mut found = Vec::new();
    for layer in document.layers.iter().filter(|l| l.content && l.visible) {
        for stroke in layer.strokes.iter().filter(|s| !leave.contains(&s.id)) {
            let points: Vec<Point> = stroke.points().collect();
            if let Some(middle) = halfway(&points) {
                found.push(middle);
            }
        }
    }
    found
}

/// The point halfway along the path through `points`; `None` for fewer than two points or no length.
fn halfway(points: &[Point]) -> Option<Point> {
    let total: f32 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
    if points.len() < 2 || !(total.is_finite() && total > 1e-3) {
        return None;
    }
    let mut left = total / 2.0;
    for pair in points.windows(2) {
        let length = pair[0].distance(pair[1]);
        if length >= left && length > 0.0 {
            let t = left / length;
            return Some(Point::new(
                pair[0].x + (pair[1].x - pair[0].x) * t,
                pair[0].y + (pair[1].y - pair[0].y) * t,
            ));
        }
        left -= length;
    }
    points.last().copied()
}

/// The most segments looked at for crossings near a point; a scribble denser than this has no crossing worth
/// aiming at.
const MOST_SEGMENTS: usize = 300;

/// Where two segments cross, if they do.
fn intersection(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let (r, s) = ((b.x - a.x, b.y - a.y), (d.x - c.x, d.y - c.y));
    let denominator = r.0 * s.1 - r.1 * s.0;
    if denominator.abs() < 1e-9 {
        return None;
    }
    let (qp_x, qp_y) = (c.x - a.x, c.y - a.y);
    let t = (qp_x * s.1 - qp_y * s.0) / denominator;
    let u = (qp_x * r.1 - qp_y * r.0) / denominator;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u))
        .then(|| Point::new(a.x + r.0 * t, a.y + r.1 * t))
}

/// The places strokes cross each other (or themselves, in a loop) within `radius` of `at`, on visible layers,
/// leaving out the strokes in `leave`. Only the part of the drawing near `at` is looked at, so this is cheap to
/// ask for at every move of the pen.
#[must_use]
pub fn crossings_near<S: BuildHasher>(
    document: &Document,
    leave: &HashSet<Uuid, S>,
    at: Point,
    radius: f32,
) -> Vec<Point> {
    // (which stroke, which segment of it, its two ends)
    let mut segments: Vec<(usize, usize, Point, Point)> = Vec::new();
    let mut stroke_number = 0;
    'strokes: for layer in document.layers.iter().filter(|l| l.content && l.visible) {
        for stroke in layer.strokes.iter().filter(|s| !leave.contains(&s.id)) {
            stroke_number += 1;
            let near = stroke.bounds().is_some_and(|b| {
                at.x >= b[0] - radius
                    && at.x <= b[2] + radius
                    && at.y >= b[1] - radius
                    && at.y <= b[3] + radius
            });
            if !near {
                continue;
            }
            let points: Vec<Point> = stroke.points().collect();
            for (i, pair) in points.windows(2).enumerate() {
                let (lo_x, hi_x) = (pair[0].x.min(pair[1].x), pair[0].x.max(pair[1].x));
                let (lo_y, hi_y) = (pair[0].y.min(pair[1].y), pair[0].y.max(pair[1].y));
                if hi_x >= at.x - radius
                    && lo_x <= at.x + radius
                    && hi_y >= at.y - radius
                    && lo_y <= at.y + radius
                {
                    segments.push((stroke_number, i, pair[0], pair[1]));
                    if segments.len() >= MOST_SEGMENTS {
                        break 'strokes;
                    }
                }
            }
        }
    }
    let mut found = Vec::new();
    for (n, first) in segments.iter().enumerate() {
        for second in &segments[n + 1..] {
            // Neighbours in one stroke share an end; that is a bend, not a crossing.
            if first.0 == second.0 && first.1.abs_diff(second.1) <= 1 {
                continue;
            }
            if let Some(p) = intersection(first.2, first.3, second.2, second.3)
                && p.distance(at) <= radius
                // Through a point of both strokes it is found for each of the segments that meet there.
                && found.iter().all(|q: &Point| q.distance(p) > 1e-3)
            {
                found.push(p);
            }
        }
    }
    found
}

/// The two ends of every stroke on a visible layer, except the strokes in `leave`: where a new line can join an
/// old one.
#[must_use]
pub fn key_points<S: BuildHasher>(document: &Document, leave: &HashSet<Uuid, S>) -> Vec<Point> {
    let mut found = Vec::new();
    for layer in document.layers.iter().filter(|l| l.content && l.visible) {
        for stroke in layer.strokes.iter().filter(|s| !leave.contains(&s.id)) {
            if let Some((first, last)) = ends(stroke) {
                found.push(first);
                if last != first {
                    found.push(last);
                }
            }
        }
    }
    found
}

/// The first and last point of a stroke.
fn ends(stroke: &Stroke) -> Option<(Point, Point)> {
    if stroke.samples.len() >= 2 {
        let first = stroke.samples.first()?.point;
        let last = stroke.samples.last()?.point;
        return Some((first, last));
    }
    Some((*stroke.path.first()?, *stroke.path.last()?))
}

/// What to snap to, and how far away it still counts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Targets<'a> {
    pub grid: Option<Grid>,
    /// The nodes of a lattice paper; a paper has a grid or a lattice, not both.
    pub lattice: Option<Lattice>,
    pub keys: &'a [Point],
    /// In canvas units.
    pub reach: f32,
}

impl Targets<'_> {
    /// The nearest target to `at` within reach, if there is one. A key point wins a tie with the grid, as it is
    /// the more deliberate place.
    #[must_use]
    pub fn nearest(&self, at: Point) -> Option<Snap> {
        let key = self
            .keys
            .iter()
            .map(|k| (k.distance(at), *k))
            .filter(|(d, _)| *d <= self.reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(d, point)| {
                (
                    d,
                    Snap {
                        point,
                        kind: Kind::KeyPoint,
                    },
                )
            });
        let grid = self
            .grid
            .and_then(|g| g.nearest(at))
            .or_else(|| self.lattice.and_then(|l| l.nearest(at)))
            .map(|point| {
                (
                    point.distance(at),
                    Snap {
                        point,
                        kind: Kind::Grid,
                    },
                )
            })
            .filter(|(d, _)| *d <= self.reach);
        match (key, grid) {
            (Some(k), Some(g)) => Some(if g.0 < k.0 { g.1 } else { k.1 }),
            (Some((_, s)), None) | (None, Some((_, s))) => Some(s),
            (None, None) => None,
        }
    }

    /// How far to move `moving` (points that go with something being dragged by `delta`) for the one that comes
    /// nearest a target to land on it, and what it lands on. `None` if none is within reach.
    #[must_use]
    pub fn nearest_for_move(
        &self,
        moving: &[Point],
        delta: (f32, f32),
    ) -> Option<((f32, f32), Snap)> {
        // Only the targets near where the points are going can be reached, so most of the key points are left out
        // before they are measured against every point.
        let (min, max) = corners(moving)?;
        let (lo_x, hi_x) = (min.x + delta.0 - self.reach, max.x + delta.0 + self.reach);
        let (lo_y, hi_y) = (min.y + delta.1 - self.reach, max.y + delta.1 + self.reach);
        let near: Vec<Point> = self
            .keys
            .iter()
            .copied()
            .filter(|k| k.x >= lo_x && k.x <= hi_x && k.y >= lo_y && k.y <= hi_y)
            .collect();
        let narrowed = Targets {
            keys: &near,
            ..*self
        };
        moving
            .iter()
            .filter_map(|m| {
                let at = Point::new(m.x + delta.0, m.y + delta.1);
                let snap = narrowed.nearest(at)?;
                let offset = (snap.point.x - at.x, snap.point.y - at.y);
                Some((offset, snap))
            })
            .min_by(|a, b| a.0.0.hypot(a.0.1).total_cmp(&b.0.0.hypot(b.0.1)))
    }
}

/// The corners of the box around `points`.
fn corners(points: &[Point]) -> Option<(Point, Point)> {
    let first = *points.first()?;
    Some(points.iter().fold((first, first), |(lo, hi), p| {
        (
            Point::new(lo.x.min(p.x), lo.y.min(p.y)),
            Point::new(hi.x.max(p.x), hi.y.max(p.y)),
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Pen, RawSample};
    use crate::geometry::Color;
    use crate::{Editor, IdGen, StrokeBuilder};

    fn grid() -> Grid {
        Grid {
            spacing: 20.0,
            columns: true,
            rows: true,
        }
    }

    fn targets(keys: &[Point], grid: Option<Grid>) -> Targets<'_> {
        Targets {
            grid,
            lattice: None,
            keys,
            reach: 6.0,
        }
    }

    #[test]
    fn a_point_near_a_crossing_of_the_grid_goes_to_it_and_one_far_from_any_stays() {
        let t = targets(&[], Some(grid()));
        let snap = t.nearest(Point::new(41.0, 57.5)).expect("near 40, 60");
        assert_eq!(snap.point, Point::new(40.0, 60.0));
        assert_eq!(snap.kind, Kind::Grid);
        assert_eq!(
            t.nearest(Point::new(50.0, 50.0)),
            None,
            "the middle of a square is further than the reach from every crossing"
        );
    }

    #[test]
    fn ruled_paper_snaps_only_the_height() {
        let rows = Grid {
            columns: false,
            ..grid()
        };
        let t = targets(&[], Some(rows));
        let snap = t.nearest(Point::new(133.7, 38.0)).expect("near row 40");
        assert_eq!(snap.point, Point::new(133.7, 40.0), "x is left alone");
    }

    #[test]
    fn a_key_point_wins_over_the_grid_when_it_is_as_near_or_nearer() {
        let keys = [Point::new(42.0, 61.0)];
        let t = targets(&keys, Some(grid()));
        let snap = t.nearest(Point::new(41.5, 60.5)).expect("a snap");
        assert_eq!(snap.kind, Kind::KeyPoint);
        assert_eq!(snap.point, Point::new(42.0, 61.0));
        // Nearer the grid crossing than the key point: the crossing.
        let snap = t.nearest(Point::new(40.2, 60.1)).expect("a snap");
        assert_eq!(snap.kind, Kind::Grid);
    }

    #[test]
    fn nothing_snaps_when_there_is_nothing_to_snap_to_or_the_grid_is_unusable() {
        assert_eq!(targets(&[], None).nearest(Point::new(1.0, 1.0)), None);
        for spacing in [0.0, -5.0, f32::NAN, f32::INFINITY] {
            let broken = Grid { spacing, ..grid() };
            assert_eq!(
                targets(&[], Some(broken)).nearest(Point::new(1.0, 1.0)),
                None
            );
        }
        let neither = Grid {
            columns: false,
            rows: false,
            ..grid()
        };
        assert_eq!(
            targets(&[], Some(neither)).nearest(Point::new(1.0, 1.0)),
            None
        );
    }

    /// A straight line from one place to another.
    type Line = ((f32, f32), (f32, f32));

    fn editor_with_lines(lines: &[Line]) -> Editor {
        let mut editor = Editor::blank(IdGen::new(7));
        for (from, to) in lines {
            let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.0);
            for i in 0..=10 {
                let t = i as f32 / 10.0;
                b.push(
                    Point::new(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t),
                    None,
                    f64::from(i) * 0.01,
                );
            }
            let stroke = b.with_id(editor.next_id()).finish();
            editor.commit_stroke(0, stroke);
        }
        editor
    }

    #[test]
    fn the_key_points_are_the_ends_of_the_strokes_on_visible_layers() {
        let mut editor =
            editor_with_lines(&[((0.0, 0.0), (100.0, 0.0)), ((0.0, 50.0), (0.0, 90.0))]);
        let keys = key_points(editor.document(), &HashSet::new());
        assert_eq!(keys.len(), 4);
        assert!(
            keys.iter()
                .any(|k| k.distance(Point::new(100.0, 0.0)) < 1.0)
        );
        assert!(keys.iter().any(|k| k.distance(Point::new(0.0, 90.0)) < 1.0));
        let second = editor.document().layers[0].strokes[1].id;
        assert_eq!(
            key_points(editor.document(), &HashSet::from([second])).len(),
            2,
            "a stroke that is being moved is not something to snap to"
        );
        editor.set_layer_visible(0, false);
        assert_eq!(key_points(editor.document(), &HashSet::new()).len(), 0);
    }

    #[test]
    fn a_stroke_with_only_a_stored_path_has_ends_too() {
        let stroke = Stroke {
            id: Uuid::nil(),
            pen: Pen::Pen,
            width: Some(3.0),
            color: Color::BLACK,
            path: vec![
                Point::new(1.0, 2.0),
                Point::new(5.0, 6.0),
                Point::new(9.0, 3.0),
            ],
            samples: vec![RawSample {
                point: Point::new(100.0, 100.0),
                attrs: [0; 4],
            }],
            pressure: Vec::new(),
        };
        assert_eq!(
            ends(&stroke),
            Some((Point::new(1.0, 2.0), Point::new(9.0, 3.0)))
        );
    }

    #[test]
    fn a_selection_being_dragged_lands_its_nearest_point_on_a_target() {
        let keys = [Point::new(200.0, 100.0)];
        let t = targets(&keys, None);
        // The selection has an end at (100, 100); dragged 97 to the right it is 3 short of the key point.
        let moving = [Point::new(100.0, 100.0), Point::new(0.0, 0.0)];
        let ((dx, dy), snap) = t.nearest_for_move(&moving, (97.0, 1.0)).expect("a snap");
        assert!(
            (dx - 3.0).abs() < 1e-4 && (dy + 1.0).abs() < 1e-4,
            "{dx} {dy}"
        );
        assert_eq!(snap.point, Point::new(200.0, 100.0));
        // Too far from everything.
        assert_eq!(t.nearest_for_move(&moving, (40.0, 40.0)), None);
        assert_eq!(t.nearest_for_move(&[], (0.0, 0.0)), None);
    }

    fn lattice(families: [f32; 2]) -> Targets<'static> {
        Targets {
            grid: None,
            lattice: Some(Lattice {
                spacing: 20.0,
                families,
            }),
            keys: &[],
            reach: 6.0,
        }
    }

    #[test]
    fn the_nodes_of_the_triangle_paper_are_where_its_three_lines_meet() {
        let t = lattice(Lattice::TRIANGLE);
        // Lines 20 apart at 0, 60 and 120 degrees meet 23.09 apart along each line: nodes at
        // (0, 0), (23.09, 0), (11.55, 20), ...
        let side = 20.0 / 60_f32.to_radians().sin();
        for (near, node) in [
            (Point::new(1.0, -2.0), Point::new(0.0, 0.0)),
            (Point::new(side + 2.0, 1.5), Point::new(side, 0.0)),
            (
                Point::new(side / 2.0 - 1.0, 21.0),
                Point::new(side / 2.0, 20.0),
            ),
            (
                Point::new(-side / 2.0, -20.0),
                Point::new(-side / 2.0, -20.0),
            ),
        ] {
            let found = t.nearest(near).expect("a node").point;
            assert!(
                found.distance(node) < 1e-3,
                "{near:?} -> {found:?}, not {node:?}"
            );
        }
        // Half way between nodes is further than the reach from all of them.
        assert_eq!(t.nearest(Point::new(side / 2.0, 0.0)), None);
    }

    #[test]
    fn the_nodes_of_the_isometric_paper_are_on_the_upright_lines() {
        let t = lattice(Lattice::ISOMETRIC);
        // Upright lines 20 apart, and the slanting ones cross each every 23.09 up, those on the next line over
        // half a step higher.
        let step = 20.0 / 60_f32.to_radians().sin();
        for (near, node) in [
            (Point::new(0.5, step - 1.0), Point::new(0.0, step)),
            (Point::new(40.5, 1.0), Point::new(40.0, 0.0)),
            (
                Point::new(20.5, step / 2.0 + 1.0),
                Point::new(20.0, step / 2.0),
            ),
            (
                Point::new(19.0, -step / 2.0 - 1.0),
                Point::new(20.0, -step / 2.0),
            ),
        ] {
            let found = t.nearest(near).expect("a node").point;
            assert!(
                found.distance(node) < 1e-3,
                "{near:?} -> {found:?}, not {node:?}"
            );
        }
    }

    #[test]
    fn a_lattice_with_no_spacing_or_two_families_that_are_one_snaps_to_nothing() {
        for spacing in [0.0, -1.0, f32::NAN] {
            let mut t = lattice(Lattice::TRIANGLE);
            t.lattice = Some(Lattice {
                spacing,
                families: Lattice::TRIANGLE,
            });
            assert_eq!(t.nearest(Point::new(1.0, 1.0)), None);
        }
        let same = lattice([0.3, 0.3]);
        assert_eq!(same.nearest(Point::new(1.0, 1.0)), None);
    }

    #[test]
    fn the_middle_of_a_stroke_is_halfway_along_it_not_halfway_between_its_ends() {
        // An L: 100 across and then 100 up. Halfway along is the corner.
        let l = [
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            Point::new(100.0, 100.0),
        ];
        let middle = halfway(&l).expect("a middle");
        assert!(middle.distance(Point::new(100.0, 0.0)) < 1e-3, "{middle:?}");
        assert_eq!(halfway(&[Point::new(1.0, 1.0)]), None);
        assert_eq!(
            halfway(&[Point::new(1.0, 1.0); 5]),
            None,
            "a dot has no middle"
        );
    }

    #[test]
    fn the_middle_points_are_one_for_each_stroke_not_being_moved() {
        let editor = editor_with_lines(&[((0.0, 0.0), (100.0, 0.0)), ((0.0, 50.0), (0.0, 90.0))]);
        let middles = middle_points(editor.document(), &HashSet::new());
        assert_eq!(middles.len(), 2);
        assert!(
            middles
                .iter()
                .any(|m| m.distance(Point::new(50.0, 0.0)) < 1e-3)
        );
        assert!(
            middles
                .iter()
                .any(|m| m.distance(Point::new(0.0, 70.0)) < 1e-3)
        );
        let second = editor.document().layers[0].strokes[1].id;
        assert_eq!(
            middle_points(editor.document(), &HashSet::from([second])).len(),
            1
        );
    }

    #[test]
    fn two_strokes_that_cross_have_a_crossing_where_they_meet_and_only_near_the_asked_for_place() {
        let editor =
            editor_with_lines(&[((0.0, 0.0), (100.0, 100.0)), ((0.0, 100.0), (100.0, 0.0))]);
        let doc = editor.document();
        let near = crossings_near(doc, &HashSet::new(), Point::new(55.0, 48.0), 10.0);
        assert_eq!(near.len(), 1, "{near:?}");
        assert!(near[0].distance(Point::new(50.0, 50.0)) < 1e-2, "{near:?}");
        assert!(
            crossings_near(doc, &HashSet::new(), Point::new(90.0, 20.0), 10.0).is_empty(),
            "the crossing is further than the radius"
        );
        let second = doc.layers[0].strokes[1].id;
        assert!(
            crossings_near(doc, &HashSet::from([second]), Point::new(50.0, 50.0), 10.0).is_empty(),
            "one stroke alone crosses nothing"
        );
    }

    #[test]
    fn a_bend_is_not_a_crossing_and_a_loop_crossing_itself_is() {
        // A V: its two segments meet at the point of the V, which is a bend.
        let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.0);
        for (i, p) in [(0.0, 100.0), (50.0, 0.0), (100.0, 100.0)]
            .iter()
            .enumerate()
        {
            b.push(Point::new(p.0, p.1), None, i as f64 * 0.01);
        }
        let mut editor = Editor::blank(IdGen::new(9));
        let v = b.with_id(editor.next_id()).finish();
        editor.commit_stroke(0, v);
        let bend = crossings_near(
            editor.document(),
            &HashSet::new(),
            Point::new(50.0, 0.0),
            20.0,
        );
        assert_eq!(bend.len(), 0, "a bend is not a crossing: {bend:?}");
        // A figure of eight in a single stroke: its middle crosses itself.
        let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.0);
        for (i, p) in [(0.0, 0.0), (100.0, 100.0), (100.0, 0.0), (0.0, 100.0)]
            .iter()
            .enumerate()
        {
            b.push(Point::new(p.0, p.1), None, i as f64 * 0.01);
        }
        let mut editor = Editor::blank(IdGen::new(10));
        let eight = b.with_id(editor.next_id()).finish();
        editor.commit_stroke(0, eight);
        let found = crossings_near(
            editor.document(),
            &HashSet::new(),
            Point::new(50.0, 50.0),
            15.0,
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].distance(Point::new(50.0, 50.0)) < 1e-2);
    }

    #[test]
    fn a_drag_takes_the_closest_of_several_possible_snaps() {
        let t = targets(&[], Some(grid()));
        // One point is 4 from a crossing, the other 1: the nearer one decides.
        let moving = [Point::new(4.0, 0.0), Point::new(101.0, 0.0)];
        let ((dx, dy), _) = t.nearest_for_move(&moving, (0.0, 0.0)).expect("a snap");
        assert!((dx + 1.0).abs() < 1e-4 && dy.abs() < 1e-4, "{dx} {dy}");
    }
}
