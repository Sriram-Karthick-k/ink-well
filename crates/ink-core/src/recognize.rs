//! Shape recognition: a stroke that is clearly a line, an arc, a circle, an ellipse, a triangle or a rectangle, as
//! the clean shape it was meant to be.
//!
//! It runs when the pen is held still at the end of a stroke. The recognizer is kept strict on purpose: a scribble
//! that is not clearly a shape is left alone, because a stroke that turns into something else is a surprise.
//!
//! It works on the points of a stroke in canvas units and gives the shape, from which the points to draw come
//! ([`Shape::polyline`]).

use std::f32::consts::TAU;

use crate::geometry::Point;

/// How many points a stroke is resampled to before it is looked at.
const SAMPLES: usize = 64;
/// A stroke shorter than this, in canvas units, is not a shape.
const MIN_LENGTH: f32 = 24.0;
/// A stroke with fewer points than this is not a shape.
const MIN_POINTS: usize = 6;
/// Points along a curve when a shape is turned into a polyline.
const CURVE_STEPS: usize = 72;
/// A line this close to horizontal or vertical (in degrees) is made exactly so.
const AXIS_SNAP: f32 = 3.0;
/// A stroke whose ends are closer than this share of its length is closed.
const CLOSED: f32 = 0.2;
/// The turn, in degrees, that makes a point of a closed stroke a corner.
const CORNER_ANGLE: f32 = 40.0;
/// How far a closed shape may be from the polygon it is taken for, as a share of its size.
const POLYGON_FIT: f32 = 0.07;
/// How far a closed shape may be from the ellipse it is taken for: the mean gap from it, as a share of the radius.
const ELLIPSE_FIT: f32 = 0.12;
/// How far a line may bow, as a share of its length.
const LINE_FIT: f32 = 0.06;
/// How far an arc may be from its circle, as a share of the radius.
const ARC_FIT: f32 = 0.045;
/// How far an arm of an angle may bow, as a share of its length. Two arms have to be told from one bend, so a
/// shade stricter than a line is allowed.
const ARM_FIT: f32 = 0.05;
/// The turn at the corner of an angle, in degrees, at the least (less is a bend, as of an arc) and at the most
/// (more is a stroke that comes back along itself).
const ANGLE_TURN: (f32, f32) = (40.0, 150.0);
/// An arm shorter than this share of the other is a flick, not an arm.
const ARM_SHARE: f32 = 0.25;
/// The angles between the arms (in degrees) an angle is made exactly when it is within a few degrees of one.
const ANGLE_STEPS: [f32; 6] = [30.0, 45.0, 60.0, 90.0, 120.0, 135.0];
/// How close, in degrees, an angle has to be to one of them.
const ANGLE_WINDOW: f32 = 3.5;

/// What was recognized.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line {
        a: Point,
        b: Point,
    },
    /// Part of a circle: from `start` (radians, counterclockwise with the y axis up) through `sweep`, which is
    /// negative for a clockwise arc.
    Arc {
        center: Point,
        radius: f32,
        start: f32,
        sweep: f32,
    },
    /// An ellipse turned by `angle` (radians); a circle when the radii are equal.
    Ellipse {
        center: Point,
        rx: f32,
        ry: f32,
        angle: f32,
    },
    /// A triangle or a four-sided shape: the corners in order, closed up by the last joining the first.
    Polygon(Vec<Point>),
    /// Two straight arms that meet at a corner: from `a` to `vertex` and on to `b`.
    Angle {
        a: Point,
        vertex: Point,
        b: Point,
    },
}

fn pt(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

impl Shape {
    /// The points to draw the shape with, in order: closed shapes end where they began.
    #[must_use]
    pub fn polyline(&self) -> Vec<Point> {
        match self {
            Self::Line { a, b } => vec![*a, *b],
            Self::Angle { a, vertex, b } => vec![*a, *vertex, *b],
            Self::Arc {
                center,
                radius,
                start,
                sweep,
            } => (0..=CURVE_STEPS)
                .map(|i| {
                    let angle = start + sweep * i as f32 / CURVE_STEPS as f32;
                    pt(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    )
                })
                .collect(),
            Self::Ellipse {
                center,
                rx,
                ry,
                angle,
            } => {
                let (sin, cos) = angle.sin_cos();
                (0..=CURVE_STEPS)
                    .map(|i| {
                        let t = TAU * i as f32 / CURVE_STEPS as f32;
                        let (u, v) = (rx * t.cos(), ry * t.sin());
                        pt(center.x + u * cos - v * sin, center.y + u * sin + v * cos)
                    })
                    .collect()
            }
            Self::Polygon(corners) => {
                let mut points = corners.clone();
                points.extend(corners.first().copied());
                points
            }
        }
    }

    /// A short name for what it is, for messages.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Line { .. } => "line",
            Self::Angle { .. } => "angle",
            Self::Arc { .. } => "arc",
            Self::Ellipse { rx, ry, .. } if (rx - ry).abs() <= 1e-3 * rx.max(*ry) => "circle",
            Self::Ellipse { .. } => "ellipse",
            Self::Polygon(corners) if corners.len() == 3 => "triangle",
            Self::Polygon(_) => "rectangle",
        }
    }
}

/// The path length of `points`.
fn length_of(points: &[Point]) -> f32 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

fn lerp(a: Point, b: Point, t: f32) -> Point {
    pt(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// `points` as `n` points spaced evenly along the path, the first and the last included.
fn resample(points: &[Point], n: usize) -> Vec<Point> {
    let total = length_of(points);
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return Vec::new();
    };
    if n < 2 || total <= f32::EPSILON {
        return points.to_vec();
    }
    let step = total / (n - 1) as f32;
    let mut out = vec![*first];
    // How far the next point is from the last one made, measured along the path.
    let mut need = step;
    for pair in points.windows(2) {
        let segment = pair[0].distance(pair[1]);
        if segment <= f32::EPSILON {
            continue;
        }
        let mut used = 0.0;
        while segment - used >= need - 1e-6 && out.len() < n - 1 {
            used += need;
            out.push(lerp(pair[0], pair[1], used / segment));
            need = step;
        }
        need -= segment - used;
    }
    out.push(*last);
    out
}

/// The corners of the box around `points`.
fn extent(points: &[Point]) -> (Point, Point) {
    let first = points.first().copied().unwrap_or_default();
    points.iter().fold((first, first), |(lo, hi), p| {
        (
            pt(lo.x.min(p.x), lo.y.min(p.y)),
            pt(hi.x.max(p.x), hi.y.max(p.y)),
        )
    })
}

/// The nearest point to `p` on the segment `a` to `b`.
fn on_segment(a: Point, b: Point, p: Point) -> Point {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length2 = dx * dx + dy * dy;
    if length2 <= f32::EPSILON {
        return a;
    }
    lerp(
        a,
        b,
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / length2).clamp(0.0, 1.0),
    )
}

/// The turn at `b` on the way from `a` through `b` to `c`, in degrees.
fn turn(a: Point, b: Point, c: Point) -> f32 {
    let (v, w) = ((b.x - a.x, b.y - a.y), (c.x - b.x, c.y - b.y));
    let norms = v.0.hypot(v.1) * w.0.hypot(w.1);
    if norms <= f32::EPSILON {
        return 0.0;
    }
    ((v.0 * w.0 + v.1 * w.1) / norms)
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees()
}

/// The circle that comes nearest to `points` (least squares, after Kåsa): its center, its radius, and how far the
/// points are from it on average as a share of the radius. `None` if the points are on a line.
fn fit_circle(points: &[Point]) -> Option<(Point, f32, f32)> {
    let n = points.len() as f32;
    if points.len() < 3 {
        return None;
    }
    let (mx, my) = (
        points.iter().map(|p| p.x).sum::<f32>() / n,
        points.iter().map(|p| p.y).sum::<f32>() / n,
    );
    let (mut suu, mut svv, mut suv, mut suuu, mut svvv, mut suvv, mut svuu) = (
        0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32,
    );
    for p in points {
        let (u, v) = (p.x - mx, p.y - my);
        suu += u * u;
        svv += v * v;
        suv += u * v;
        suuu += u * u * u;
        svvv += v * v * v;
        suvv += u * v * v;
        svuu += v * u * u;
    }
    let det = suu * svv - suv * suv;
    if det.abs() <= 1e-6 * (suu * svv).abs().max(1e-12) {
        return None;
    }
    let (b1, b2) = (f32::midpoint(suuu, suvv), f32::midpoint(svvv, svuu));
    let (uc, vc) = ((b1 * svv - b2 * suv) / det, (b2 * suu - b1 * suv) / det);
    let center = pt(mx + uc, my + vc);
    let radius = (uc * uc + vc * vc + (suu + svv) / n).sqrt();
    if !(radius.is_finite() && radius > 1e-3) {
        return None;
    }
    let gap = points
        .iter()
        .map(|p| (p.distance(center) - radius).powi(2))
        .sum::<f32>()
        / n;
    Some((center, radius, gap.sqrt() / radius))
}

/// Recognizes the shape `points` make, if they are clearly one.
#[must_use]
pub fn recognize(points: &[Point]) -> Option<Shape> {
    let mut path: Vec<Point> = Vec::with_capacity(points.len());
    for p in points.iter().filter(|p| p.x.is_finite() && p.y.is_finite()) {
        if path.last() != Some(p) {
            path.push(*p);
        }
    }
    if path.len() < MIN_POINTS || length_of(&path) < MIN_LENGTH {
        return None;
    }
    let (first, last) = (path[0], path[path.len() - 1]);
    let length = length_of(&path);
    if first.distance(last) < CLOSED * length {
        // A stroke that goes past where it started has the part that overlaps taken off, so it closes.
        let closed = close_up(&path);
        let ring = resample(&closed, SAMPLES);
        return recognize_closed(&ring);
    }
    let ring = resample(&path, SAMPLES);
    recognize_line(&ring)
        .or_else(|| recognize_angle(&ring))
        .or_else(|| recognize_arc(&ring))
}

/// `path` cut where it comes back nearest to its start, so that going past the start does not make a tail.
fn close_up(path: &[Point]) -> Vec<Point> {
    let start = path[0];
    let from = path.len() * 6 / 10;
    let nearest = path
        .iter()
        .enumerate()
        .skip(from)
        .min_by(|a, b| a.1.distance(start).total_cmp(&b.1.distance(start)))
        .map_or(path.len() - 1, |(i, _)| i);
    path[..=nearest].to_vec()
}

fn recognize_line(ring: &[Point]) -> Option<Shape> {
    let (a, b) = (*ring.first()?, *ring.last()?);
    let chord = a.distance(b);
    if chord < MIN_LENGTH {
        return None;
    }
    let bow = ring
        .iter()
        .map(|p| on_segment(a, b, *p).distance(*p))
        .fold(0.0, f32::max);
    // A stroke that goes there and back is not a line either: the path is not much longer than the chord.
    if bow > LINE_FIT * chord || length_of(ring) > 1.15 * chord {
        return None;
    }
    Some(straightened(a, b))
}

/// The line from `a` to `b`, made exactly horizontal or vertical when it is nearly so.
fn straightened(a: Point, b: Point) -> Shape {
    let degrees = (b.y - a.y).atan2(b.x - a.x).to_degrees();
    let quarter = degrees.rem_euclid(90.0);
    let off = quarter.min(90.0 - quarter);
    if off > AXIS_SNAP {
        return Shape::Line { a, b };
    }
    let snapped = (degrees / 90.0).round() * 90.0_f32;
    let (mid, half) = (lerp(a, b, 0.5), a.distance(b) / 2.0);
    let (dx, dy) = (
        snapped.to_radians().cos() * half,
        snapped.to_radians().sin() * half,
    );
    Shape::Line {
        a: pt(mid.x - dx, mid.y - dy),
        b: pt(mid.x + dx, mid.y + dy),
    }
}

/// The point where the line through `p` along `d` meets the line through `q` along `e`, and how far along `d`
/// from `p` that is, in lengths of `d`. `None` if they are parallel.
fn meeting(p: Point, d: (f32, f32), q: Point, e: (f32, f32)) -> Option<(Point, f32)> {
    let cross = d.0 * e.1 - d.1 * e.0;
    if cross.abs() <= 1e-6 * d.0.hypot(d.1) * e.0.hypot(e.1) {
        return None;
    }
    let s = ((q.x - p.x) * e.1 - (q.y - p.y) * e.0) / cross;
    Some((pt(p.x + d.0 * s, p.y + d.1 * s), s))
}

/// `v` turned by `radians` counterclockwise.
fn turned(v: (f32, f32), radians: f32) -> (f32, f32) {
    let (sin, cos) = radians.sin_cos();
    (v.0 * cos - v.1 * sin, v.0 * sin + v.1 * cos)
}

/// Two straight arms with one corner between them: the split of the stroke into two that leaves each the
/// straightest, with the corner where the arms' lines meet (a hand rounds a corner off, which the lines do not).
/// An angle close to a common one (a right angle, say) is made exactly that, and an arm close to level or upright
/// is made exactly so.
fn recognize_angle(ring: &[Point]) -> Option<Shape> {
    let n = ring.len();
    if n < 24 {
        return None;
    }
    // How far the points between `lo` and `hi` are from the chord between them, as a share of its length.
    let bow = |lo: usize, hi: usize| -> f32 {
        let (a, b) = (ring[lo], ring[hi]);
        let chord = a.distance(b);
        if chord <= f32::EPSILON {
            return f32::MAX;
        }
        ring[lo..=hi]
            .iter()
            .map(|p| on_segment(a, b, *p).distance(*p))
            .fold(0.0, f32::max)
            / chord
    };
    // The rounding of a corner is left out of the arms.
    let trim = (n / 16).max(2);
    let splits: Vec<(usize, f32)> = (n / 6..=n - 1 - n / 6)
        .map(|k| (k, bow(0, k - trim).max(bow(k + trim, n - 1))))
        .collect();
    let best = splits.iter().map(|s| s.1).fold(f32::MAX, f32::min);
    if best > ARM_FIT {
        return None;
    }
    // Every split near the corner leaves two straight arms; the middle of those is the corner, so that neither
    // arm's line is taken from points that are past it.
    let near: Vec<usize> = splits
        .iter()
        .filter(|s| s.1 <= best * 1.6 + 0.004)
        .map(|s| s.0)
        .collect();
    let k = near[near.len() / 2];
    let (a, b) = (ring[0], ring[n - 1]);
    let first = (ring[k - trim].x - a.x, ring[k - trim].y - a.y);
    let second = (b.x - ring[k + trim].x, b.y - ring[k + trim].y);
    let (vertex, along) = meeting(a, first, ring[k + trim], second)?;
    // The corner is ahead of the start along the first arm, and the lines meet near where the stroke turned.
    let (arm_one, arm_two) = (a.distance(vertex), vertex.distance(b));
    let (short, long) = (arm_one.min(arm_two), arm_one.max(arm_two));
    if along <= 0.0
        || short < MIN_LENGTH / 2.0
        || short < ARM_SHARE * long
        || ring
            .iter()
            .map(|p| p.distance(vertex))
            .fold(f32::MAX, f32::min)
            > 0.15 * short
    {
        return None;
    }
    let (heading_one, heading_two) = (
        (vertex.y - a.y).atan2(vertex.x - a.x),
        (b.y - vertex.y).atan2(b.x - vertex.x),
    );
    let bend =
        (heading_two - heading_one + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI;
    let degrees = bend.to_degrees();
    if !(ANGLE_TURN.0..=ANGLE_TURN.1).contains(&degrees.abs()) {
        return None;
    }
    // Between the arms (a straight line is 180 degrees): a common angle is made exact by turning the second arm.
    let between = 180.0 - degrees.abs();
    let (mut a, mut b) = (a, b);
    if let Some(step) = ANGLE_STEPS
        .iter()
        .find(|s| (between - **s).abs() <= ANGLE_WINDOW)
    {
        let want = (180.0 - step).copysign(degrees).to_radians();
        let arm = turned((b.x - vertex.x, b.y - vertex.y), want - bend);
        b = pt(vertex.x + arm.0, vertex.y + arm.1);
    }
    // An arm nearly level or upright is made so, and the rest turns with it about the corner.
    let heading = (vertex.y - a.y).atan2(vertex.x - a.x).to_degrees();
    let quarter = heading.rem_euclid(90.0);
    if quarter.min(90.0 - quarter) <= AXIS_SNAP {
        let by = ((heading / 90.0).round() * 90.0 - heading).to_radians();
        let about = |p: Point| {
            let v = turned((p.x - vertex.x, p.y - vertex.y), by);
            pt(vertex.x + v.0, vertex.y + v.1)
        };
        (a, b) = (about(a), about(b));
    }
    Some(Shape::Angle { a, vertex, b })
}

fn recognize_arc(ring: &[Point]) -> Option<Shape> {
    let (center, radius, gap) = fit_circle(ring)?;
    if gap > ARC_FIT || radius < 6.0 {
        return None;
    }
    // Which way round it goes, and that it keeps going that way: an S is not an arc.
    let turns: Vec<f32> = ring
        .windows(2)
        .map(|w| {
            let (u, v) = (
                (w[0].x - center.x, w[0].y - center.y),
                (w[1].x - center.x, w[1].y - center.y),
            );
            u.0 * v.1 - u.1 * v.0
        })
        .collect();
    let counterclockwise = turns.iter().sum::<f32>() > 0.0;
    let agree = turns
        .iter()
        .filter(|t| (**t > 0.0) == counterclockwise)
        .count();
    if (agree as f32) < 0.95 * turns.len() as f32 {
        return None;
    }
    let angle = |p: Point| (p.y - center.y).atan2(p.x - center.x);
    let (start, end) = (angle(*ring.first()?), angle(*ring.last()?));
    let sweep = if counterclockwise {
        (end - start).rem_euclid(TAU)
    } else {
        -(start - end).rem_euclid(TAU)
    };
    let span = sweep.abs().to_degrees();
    // Less than this it is nearly a line, and more it is nearly a circle.
    if !(35.0..=330.0).contains(&span) {
        return None;
    }
    Some(Shape::Arc {
        center,
        radius,
        start,
        sweep,
    })
}

/// The indices of the corners of a closed ring of points: where it turns sharply, once for each corner.
fn corners(ring: &[Point]) -> Vec<usize> {
    let n = ring.len();
    let k = (n / 16).max(2);
    let angles: Vec<f32> = (0..n)
        .map(|i| turn(ring[(i + n - k) % n], ring[i], ring[(i + k) % n]))
        .collect();
    (0..n)
        .filter(|&i| {
            angles[i] > CORNER_ANGLE
                && (1..=k).all(|d| {
                    angles[i] >= angles[(i + d) % n] && angles[i] > angles[(i + n - d) % n]
                })
        })
        .collect()
}

fn recognize_closed(ring: &[Point]) -> Option<Shape> {
    // The ring closes on itself, so its last point is its first.
    let ring = &ring[..ring.len().saturating_sub(1)];
    if ring.len() < 8 {
        return None;
    }
    let found = corners(ring);
    match found.len() {
        3 | 4 => recognize_polygon(ring, &found),
        0..=2 => recognize_ellipse(ring),
        _ => None,
    }
}

/// The corners (the points at the indices `at`) as a triangle or a four-sided shape, if the ring keeps close to
/// the lines between them.
fn recognize_polygon(ring: &[Point], at: &[usize]) -> Option<Shape> {
    let corner_points: Vec<Point> = at.iter().map(|&i| ring[i]).collect();
    let (lo, hi) = extent(ring);
    let size = lo.distance(hi);
    let sides = corner_points.len();
    let near_polygon = ring.iter().all(|p| {
        (0..sides)
            .map(|i| on_segment(corner_points[i], corner_points[(i + 1) % sides], *p).distance(*p))
            .fold(f32::MAX, f32::min)
            <= POLYGON_FIT * size
    });
    if !near_polygon {
        return None;
    }
    let perimeter: f32 = (0..sides)
        .map(|i| corner_points[i].distance(corner_points[(i + 1) % sides]))
        .sum();
    if (0..sides)
        .any(|i| corner_points[i].distance(corner_points[(i + 1) % sides]) < 0.1 * perimeter)
    {
        return None;
    }
    if sides == 3 {
        return Some(Shape::Polygon(corner_points));
    }
    Some(Shape::Polygon(squared_up(&corner_points)))
}

/// Four corners as a rectangle if they are nearly one (all four angles nearly right), turned the way the sides
/// run and exactly level when they nearly are; otherwise as they are.
fn squared_up(corners: &[Point]) -> Vec<Point> {
    let n = corners.len();
    let edge = |i: usize| {
        let (a, b) = (corners[i], corners[(i + 1) % n]);
        (b.y - a.y).atan2(b.x - a.x)
    };
    let right = (0..n).all(|i| {
        let (a, b, c) = (corners[(i + n - 1) % n], corners[i], corners[(i + 1) % n]);
        (turn(a, b, c) - 90.0).abs() <= 25.0
    });
    if !right {
        return corners.to_vec();
    }
    // The sides repeat every quarter turn, so their mean direction is found with the angle multiplied by four.
    let (sum_sin, sum_cos) = (0..n).fold((0.0_f32, 0.0_f32), |(s, c), i| {
        (s + (4.0 * edge(i)).sin(), c + (4.0 * edge(i)).cos())
    });
    let mut theta = sum_sin.atan2(sum_cos) / 4.0;
    if theta
        .to_degrees()
        .abs()
        .min((theta.to_degrees().abs() - 90.0).abs())
        <= AXIS_SNAP
    {
        theta = 0.0;
    }
    let (sin, cos) = theta.sin_cos();
    let along = |p: Point| (p.x * cos + p.y * sin, -p.x * sin + p.y * cos);
    let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
    for p in corners {
        let (u, v) = along(*p);
        lo = (lo.0.min(u), lo.1.min(v));
        hi = (hi.0.max(u), hi.1.max(v));
    }
    let back = |u: f32, v: f32| pt(u * cos - v * sin, u * sin + v * cos);
    // In the order the pen went round.
    let ordered = [
        back(lo.0, lo.1),
        back(hi.0, lo.1),
        back(hi.0, hi.1),
        back(lo.0, hi.1),
    ];
    let start = ordered
        .iter()
        .enumerate()
        .min_by(|a, b| {
            a.1.distance(corners[0])
                .total_cmp(&b.1.distance(corners[0]))
        })
        .map_or(0, |(i, _)| i);
    // Which way round the pen went: the sign of the area.
    let area: f32 = (0..n)
        .map(|i| corners[i].x * corners[(i + 1) % n].y - corners[(i + 1) % n].x * corners[i].y)
        .sum();
    let mut out: Vec<Point> = (0..4).map(|i| ordered[(start + i) % 4]).collect();
    if area < 0.0 {
        out[1..].reverse();
    }
    out
}

fn recognize_ellipse(ring: &[Point]) -> Option<Shape> {
    let n = ring.len() as f32;
    let (mx, my) = (
        ring.iter().map(|p| p.x).sum::<f32>() / n,
        ring.iter().map(|p| p.y).sum::<f32>() / n,
    );
    let (mut cxx, mut cyy, mut cxy) = (0.0_f32, 0.0_f32, 0.0_f32);
    for p in ring {
        let (u, v) = (p.x - mx, p.y - my);
        cxx += u * u;
        cyy += v * v;
        cxy += u * v;
    }
    let angle = 0.5 * (2.0 * cxy).atan2(cxx - cyy);
    let (sin, cos) = angle.sin_cos();
    let along: Vec<(f32, f32)> = ring
        .iter()
        .map(|p| {
            let (u, v) = (p.x - mx, p.y - my);
            (u * cos + v * sin, -u * sin + v * cos)
        })
        .collect();
    let spread = |pick: fn(&(f32, f32)) -> f32| {
        let mut values: Vec<f32> = along.iter().map(pick).collect();
        values.sort_by(f32::total_cmp);
        let at = |share: f32| values[((values.len() - 1) as f32 * share).round() as usize];
        (at(0.02), at(0.98))
    };
    let (u_lo, u_hi) = spread(|c| c.0);
    let (v_lo, v_hi) = spread(|c| c.1);
    let (rx, ry) = ((u_hi - u_lo) / 2.0, (v_hi - v_lo) / 2.0);
    if rx < 6.0 || ry < 6.0 {
        return None;
    }
    let (cu, cv) = (f32::midpoint(u_lo, u_hi), f32::midpoint(v_lo, v_hi));
    let gap = along
        .iter()
        .map(|(u, v)| ((((u - cu) / rx).powi(2) + ((v - cv) / ry).powi(2)).sqrt() - 1.0).abs())
        .sum::<f32>()
        / n;
    if gap > ELLIPSE_FIT {
        return None;
    }
    let center = pt(mx + cu * cos - cv * sin, my + cu * sin + cv * cos);
    // Nearly round is round; nearly level is level.
    if (rx - ry).abs() <= 0.12 * rx.max(ry) {
        let r = f32::midpoint(rx, ry);
        return Some(Shape::Ellipse {
            center,
            rx: r,
            ry: r,
            angle: 0.0,
        });
    }
    let degrees = angle.to_degrees();
    let quarter = degrees.rem_euclid(90.0);
    let level = quarter.min(90.0 - quarter) <= 6.0;
    let (rx, ry, angle) = if level {
        let turned = (degrees / 90.0).round() as i32 % 2 != 0;
        if turned { (ry, rx, 0.0) } else { (rx, ry, 0.0) }
    } else {
        (rx, ry, angle)
    };
    Some(Shape::Ellipse {
        center,
        rx,
        ry,
        angle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source of repeatable noise.
    struct Noise(u64);

    impl Noise {
        /// A number from -1 to 1.
        fn next(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
        }
    }

    /// `points` with each moved by up to `amount` either way.
    fn shaky(points: &[Point], amount: f32, seed: u64) -> Vec<Point> {
        let mut noise = Noise(seed);
        points
            .iter()
            .map(|p| pt(p.x + noise.next() * amount, p.y + noise.next() * amount))
            .collect()
    }

    /// Points along a polyline, `per` of them on each side.
    fn along(corners: &[Point], per: usize) -> Vec<Point> {
        let mut out = Vec::new();
        for pair in corners.windows(2) {
            for i in 0..per {
                out.push(lerp(pair[0], pair[1], i as f32 / per as f32));
            }
        }
        out.extend(corners.last().copied());
        out
    }

    fn arc_points(center: Point, rx: f32, ry: f32, from: f32, to: f32, n: usize) -> Vec<Point> {
        (0..=n)
            .map(|i| {
                let a = from + (to - from) * i as f32 / n as f32;
                pt(center.x + rx * a.cos(), center.y + ry * a.sin())
            })
            .collect()
    }

    #[test]
    fn resampling_gives_evenly_spaced_points_with_both_ends() {
        let line = [pt(0.0, 0.0), pt(10.0, 0.0), pt(100.0, 0.0)];
        let r = resample(&line, 11);
        assert_eq!(r.len(), 11);
        assert_eq!(r[0], pt(0.0, 0.0));
        assert!((r[10].x - 100.0).abs() < 1e-3);
        for w in r.windows(2) {
            assert!((w[0].distance(w[1]) - 10.0).abs() < 0.01, "{w:?}");
        }
    }

    #[test]
    fn a_shaky_circle_is_a_circle_of_about_the_same_size_and_place() {
        let circle = arc_points(pt(200.0, 150.0), 80.0, 80.0, 0.3, 0.3 + TAU, 90);
        let Some(Shape::Ellipse { center, rx, ry, .. }) = recognize(&shaky(&circle, 3.0, 1)) else {
            panic!("a circle");
        };
        assert!(center.distance(pt(200.0, 150.0)) < 4.0, "{center:?}");
        assert!(
            (rx - 80.0).abs() < 5.0 && (ry - 80.0).abs() < 5.0,
            "{rx} {ry}"
        );
        assert!((rx - ry).abs() < 1e-3, "a circle has one radius");
    }

    #[test]
    fn a_circle_that_goes_past_its_start_or_stops_short_is_still_a_circle() {
        for (from, to) in [(0.0, TAU + 0.5), (0.0, TAU - 0.35), (1.0, 1.0 + TAU + 0.9)] {
            let circle = arc_points(pt(0.0, 0.0), 60.0, 60.0, from, to, 90);
            let shape = recognize(&shaky(&circle, 2.0, 7));
            assert!(
                matches!(shape, Some(Shape::Ellipse { .. })),
                "{from}..{to}: {shape:?}"
            );
        }
    }

    #[test]
    fn a_turned_ellipse_keeps_its_shape_and_its_turn() {
        let angle = 30.0_f32.to_radians();
        let base = arc_points(pt(0.0, 0.0), 120.0, 50.0, 0.0, TAU, 100);
        let turned: Vec<Point> = base
            .iter()
            .map(|p| {
                pt(
                    p.x * angle.cos() - p.y * angle.sin(),
                    p.x * angle.sin() + p.y * angle.cos(),
                )
            })
            .collect();
        let Some(Shape::Ellipse {
            rx,
            ry,
            angle: found,
            ..
        }) = recognize(&shaky(&turned, 2.0, 3))
        else {
            panic!("an ellipse");
        };
        let (long, short) = (rx.max(ry), rx.min(ry));
        assert!(
            (long - 120.0).abs() < 10.0 && (short - 50.0).abs() < 8.0,
            "{long} {short}"
        );
        let turn = found.to_degrees().rem_euclid(180.0);
        let turn = if rx < ry {
            (turn + 90.0).rem_euclid(180.0)
        } else {
            turn
        };
        assert!((turn - 30.0).abs() < 6.0, "{turn}");
    }

    #[test]
    fn a_level_ellipse_is_made_exactly_level() {
        let base = arc_points(pt(0.0, 0.0), 120.0, 50.0, 0.0, TAU, 100);
        let turned: Vec<Point> = base
            .iter()
            .map(|p| {
                let a = 3.0_f32.to_radians();
                pt(p.x * a.cos() - p.y * a.sin(), p.x * a.sin() + p.y * a.cos())
            })
            .collect();
        let Some(Shape::Ellipse { angle, rx, ry, .. }) = recognize(&shaky(&turned, 1.5, 4)) else {
            panic!("an ellipse");
        };
        assert_eq!(angle, 0.0);
        assert!(rx > ry);
    }

    #[test]
    fn a_shaky_rectangle_is_a_clean_one_with_right_angles() {
        let corners = [
            pt(0.0, 0.0),
            pt(200.0, 0.0),
            pt(200.0, 120.0),
            pt(0.0, 120.0),
            pt(0.0, 0.0),
        ];
        let Some(Shape::Polygon(found)) = recognize(&shaky(&along(&corners, 25), 2.5, 5)) else {
            panic!("a rectangle");
        };
        assert_eq!(found.len(), 4);
        for i in 0..4 {
            let turn = turn(found[(i + 3) % 4], found[i], found[(i + 1) % 4]);
            assert!((turn - 90.0).abs() < 0.01, "a right angle at {i}: {turn}");
        }
        let (lo, hi) = extent(&found);
        assert!((hi.x - lo.x - 200.0).abs() < 8.0 && (hi.y - lo.y - 120.0).abs() < 8.0);
        assert_eq!(Shape::Polygon(found).name(), "rectangle");
    }

    #[test]
    fn a_rectangle_drawn_at_a_slant_stays_at_its_slant() {
        let a = 20.0_f32.to_radians();
        let turn_by = |p: Point| pt(p.x * a.cos() - p.y * a.sin(), p.x * a.sin() + p.y * a.cos());
        let corners: Vec<Point> = [
            pt(0.0, 0.0),
            pt(180.0, 0.0),
            pt(180.0, 90.0),
            pt(0.0, 90.0),
            pt(0.0, 0.0),
        ]
        .into_iter()
        .map(turn_by)
        .collect();
        let Some(Shape::Polygon(found)) = recognize(&shaky(&along(&corners, 25), 2.0, 6)) else {
            panic!("a rectangle");
        };
        assert_eq!(found.len(), 4);
        for i in 0..4 {
            let angle = turn(found[(i + 3) % 4], found[i], found[(i + 1) % 4]);
            assert!((angle - 90.0).abs() < 0.01, "{angle}");
        }
        let side = found[0].distance(found[1]);
        let other = found[1].distance(found[2]);
        assert!((side.max(other) - 180.0).abs() < 10.0 && (side.min(other) - 90.0).abs() < 10.0);
    }

    #[test]
    fn a_shaky_triangle_is_a_triangle() {
        let corners = [pt(0.0, 0.0), pt(220.0, 20.0), pt(90.0, 170.0), pt(0.0, 0.0)];
        let Some(shape) = recognize(&shaky(&along(&corners, 30), 2.5, 8)) else {
            panic!("a triangle");
        };
        let Shape::Polygon(found) = &shape else {
            panic!("a polygon: {shape:?}")
        };
        assert_eq!(found.len(), 3);
        assert_eq!(shape.name(), "triangle");
        for c in &corners[..3] {
            assert!(
                found.iter().any(|f| f.distance(*c) < 12.0),
                "{c:?} in {found:?}"
            );
        }
    }

    #[test]
    fn a_shaky_line_is_a_line_between_where_it_began_and_ended() {
        let line = along(&[pt(10.0, 10.0), pt(250.0, 90.0)], 40);
        let Some(Shape::Line { a, b }) = recognize(&shaky(&line, 3.0, 9)) else {
            panic!("a line");
        };
        assert!(
            a.distance(pt(10.0, 10.0)) < 6.0 && b.distance(pt(250.0, 90.0)) < 6.0,
            "{a:?} {b:?}"
        );
    }

    #[test]
    fn a_line_that_is_nearly_level_or_upright_is_made_exactly_so() {
        let level = along(&[pt(0.0, 0.0), pt(300.0, 6.0)], 40);
        let Some(Shape::Line { a, b }) = recognize(&level) else {
            panic!("a line")
        };
        assert!((a.y - b.y).abs() < 1e-3, "{a:?} {b:?}");
        assert!(
            (a.distance(b) - level[0].distance(level[level.len() - 1])).abs() < 1e-2,
            "same length"
        );
        let upright = along(&[pt(50.0, 0.0), pt(53.0, 200.0)], 40);
        let Some(Shape::Line { a, b }) = recognize(&upright) else {
            panic!("a line")
        };
        assert!((a.x - b.x).abs() < 1e-3);
        // A slant of 10 degrees is not made level.
        let slant = along(&[pt(0.0, 0.0), pt(300.0, 52.0)], 40);
        let Some(Shape::Line { a, b }) = recognize(&slant) else {
            panic!("a line")
        };
        assert!((b.y - a.y).abs() > 40.0);
    }

    /// The angle between the arms of `Angle`, in degrees.
    fn between(shape: &Shape) -> f32 {
        let Shape::Angle { a, vertex, b } = shape else {
            panic!("an angle: {shape:?}");
        };
        180.0 - turn(*a, *vertex, *b)
    }

    #[test]
    fn two_straight_arms_with_a_corner_are_an_angle_with_its_corner_where_the_arms_meet() {
        let path = [pt(0.0, 0.0), pt(150.0, 90.0), pt(40.0, 190.0)];
        let Some(shape) = recognize(&shaky(&along(&path, 30), 2.5, 21)) else {
            panic!("an angle");
        };
        let Shape::Angle { a, vertex, b } = &shape else {
            panic!("an angle: {shape:?}");
        };
        assert!(
            a.distance(path[0]) < 8.0 && b.distance(path[2]) < 8.0,
            "{a:?} {b:?}"
        );
        assert!(vertex.distance(path[1]) < 10.0, "{vertex:?}");
        assert_eq!(shape.name(), "angle");
        assert_eq!(shape.polyline(), vec![*a, *vertex, *b]);
    }

    #[test]
    fn a_corner_a_hand_rounded_off_is_found_where_the_arms_would_meet() {
        // Two arms with a quarter of a circle between them instead of a point.
        let mut path = along(&[pt(0.0, 0.0), pt(160.0, 0.0)], 30);
        path.extend(arc_points(
            pt(160.0, 12.0),
            12.0,
            12.0,
            -std::f32::consts::FRAC_PI_2,
            0.0,
            6,
        ));
        path.extend(along(&[pt(172.0, 12.0), pt(172.0, 140.0)], 30));
        let Some(Shape::Angle { vertex, .. }) = recognize(&path) else {
            panic!("an angle");
        };
        assert!(vertex.distance(pt(172.0, 0.0)) < 6.0, "{vertex:?}");
    }

    #[test]
    fn a_right_angle_is_exactly_one_and_an_arm_that_is_nearly_level_is_level() {
        // Eighty-eight degrees between the arms, the first a little off level.
        let first = 2.0_f32.to_radians();
        let corner = pt(180.0 * first.cos(), 180.0 * first.sin());
        let second = first + 92.0_f32.to_radians();
        let end = pt(
            corner.x + 130.0 * second.cos(),
            corner.y + 130.0 * second.sin(),
        );
        let Some(shape) = recognize(&shaky(&along(&[pt(0.0, 0.0), corner, end], 30), 1.0, 22))
        else {
            panic!("an angle");
        };
        assert!((between(&shape) - 90.0).abs() < 0.01, "{}", between(&shape));
        let Shape::Angle { a, vertex, .. } = shape else {
            unreachable!()
        };
        assert!((a.y - vertex.y).abs() < 1e-3, "level: {a:?} {vertex:?}");
    }

    #[test]
    fn an_angle_that_is_not_a_common_one_keeps_its_own() {
        // A hundred and ten degrees between the arms.
        let bend = 70.0_f32.to_radians();
        let corner = pt(150.0, 0.0);
        let end = pt(corner.x + 120.0 * bend.cos(), corner.y + 120.0 * bend.sin());
        let Some(shape) = recognize(&shaky(&along(&[pt(0.0, 0.0), corner, end], 30), 1.0, 23))
        else {
            panic!("an angle");
        };
        assert!((between(&shape) - 110.0).abs() < 3.0, "{}", between(&shape));
    }

    #[test]
    fn a_check_mark_with_a_short_arm_is_an_angle_and_a_flick_at_the_end_of_a_line_is_not() {
        let tick = [pt(0.0, 40.0), pt(40.0, 0.0), pt(160.0, 120.0)];
        assert!(matches!(
            recognize(&shaky(&along(&tick, 30), 1.0, 24)),
            Some(Shape::Angle { .. })
        ));
        let flick = [pt(0.0, 0.0), pt(200.0, 0.0), pt(210.0, 15.0)];
        assert!(!matches!(
            recognize(&along(&flick, 30)),
            Some(Shape::Angle { .. })
        ));
    }

    #[test]
    fn what_is_not_two_arms_is_not_an_angle() {
        // A hairpin goes back along itself.
        let hairpin = [pt(0.0, 0.0), pt(200.0, 0.0), pt(10.0, 12.0)];
        assert!(!matches!(
            recognize(&along(&hairpin, 30)),
            Some(Shape::Angle { .. })
        ));
        // A Z has two corners.
        let z = [
            pt(0.0, 0.0),
            pt(120.0, 0.0),
            pt(0.0, 120.0),
            pt(120.0, 120.0),
        ];
        assert_eq!(recognize(&along(&z, 30)), None, "Z");
        // A bend of an arc is no corner.
        let arc = arc_points(pt(0.0, 0.0), 100.0, 100.0, 0.0, 0.9, 60);
        assert!(!matches!(recognize(&arc), Some(Shape::Angle { .. })), "arc");
        // A line with a bow in it is neither.
        let bow: Vec<Point> = (0..=60)
            .map(|i| {
                let t = i as f32 / 60.0;
                pt(200.0 * t, 30.0 * (t * std::f32::consts::PI).sin())
            })
            .collect();
        assert!(!matches!(recognize(&bow), Some(Shape::Angle { .. })), "bow");
    }

    #[test]
    fn a_curve_of_a_circle_is_an_arc_that_turns_the_way_it_was_drawn() {
        let radius = 90.0;
        let ccw = arc_points(pt(0.0, 0.0), radius, radius, 0.4, 0.4 + 2.2, 70);
        let Some(Shape::Arc {
            radius: r,
            sweep,
            center,
            ..
        }) = recognize(&shaky(&ccw, 1.5, 10))
        else {
            panic!("an arc");
        };
        assert!(
            (r - radius).abs() < 6.0 && center.distance(pt(0.0, 0.0)) < 8.0,
            "{r} {center:?}"
        );
        assert!(
            (sweep - 2.2).abs() < 0.12,
            "counterclockwise, 2.2 radians: {sweep}"
        );
        let cw = arc_points(pt(0.0, 0.0), radius, radius, 2.0, 2.0 - 1.8, 70);
        let Some(Shape::Arc { sweep, .. }) = recognize(&shaky(&cw, 1.5, 11)) else {
            panic!("an arc");
        };
        assert!((sweep + 1.8).abs() < 0.12, "clockwise: {sweep}");
    }

    #[test]
    fn what_is_drawn_for_a_shape_is_on_the_shape_and_closed_shapes_close() {
        let circle = Shape::Ellipse {
            center: pt(5.0, 5.0),
            rx: 40.0,
            ry: 40.0,
            angle: 0.0,
        };
        let ring = circle.polyline();
        assert!(
            ring.iter()
                .all(|p| (p.distance(pt(5.0, 5.0)) - 40.0).abs() < 1e-3)
        );
        assert!(ring[0].distance(ring[ring.len() - 1]) < 1e-3);
        let triangle = Shape::Polygon(vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(5.0, 8.0)]);
        assert_eq!(triangle.polyline().len(), 4);
        assert_eq!(
            Shape::Line {
                a: pt(0.0, 0.0),
                b: pt(1.0, 1.0)
            }
            .polyline()
            .len(),
            2
        );
        assert_eq!(circle.name(), "circle");
    }

    #[test]
    fn things_that_are_not_clearly_a_shape_are_left_alone() {
        let mut noise = Noise(42);
        // A zigzag.
        let zigzag: Vec<Point> = (0..40)
            .map(|i| pt(i as f32 * 8.0, if i % 2 == 0 { 0.0 } else { 40.0 }))
            .collect();
        assert_eq!(recognize(&zigzag), None, "zigzag");
        // A scribble: a random walk.
        let (mut x, mut y) = (0.0_f32, 0.0_f32);
        let scribble: Vec<Point> = (0..120)
            .map(|_| {
                x += noise.next() * 14.0;
                y += noise.next() * 14.0;
                pt(x, y)
            })
            .collect();
        assert_eq!(recognize(&scribble), None, "scribble");
        // An S.
        let s: Vec<Point> = (0..=60)
            .map(|i| {
                let t = i as f32 / 60.0;
                pt(40.0 * (t * 2.0 * TAU).sin(), 200.0 * t)
            })
            .collect();
        assert_eq!(recognize(&s), None, "wave");
        // A figure of eight.
        let eight: Vec<Point> = (0..=100)
            .map(|i| {
                let t = TAU * i as f32 / 100.0;
                pt(60.0 * t.sin(), 40.0 * (2.0 * t).sin())
            })
            .collect();
        assert_eq!(recognize(&eight), None, "figure of eight");
        // A spiral.
        let spiral: Vec<Point> = (0..120)
            .map(|i| {
                let t = i as f32 * 0.12;
                pt((10.0 + 6.0 * t) * t.cos(), (10.0 + 6.0 * t) * t.sin())
            })
            .collect();
        assert_eq!(recognize(&spiral), None, "spiral");
    }

    #[test]
    fn too_little_is_not_a_shape_and_nonsense_does_not_panic() {
        assert_eq!(recognize(&[]), None);
        assert_eq!(recognize(&[pt(0.0, 0.0); 30]), None, "a dot");
        assert_eq!(
            recognize(&along(&[pt(0.0, 0.0), pt(5.0, 0.0)], 10)),
            None,
            "a tick is too short"
        );
        assert_eq!(
            recognize(&[pt(f32::NAN, 0.0), pt(1.0, f32::INFINITY), pt(2.0, 2.0)]),
            None
        );
        // Many repeated points still do not divide by zero.
        let mut repeated = vec![pt(0.0, 0.0); 50];
        repeated.extend(vec![pt(100.0, 0.0); 50]);
        let _ = recognize(&repeated);
    }
}
