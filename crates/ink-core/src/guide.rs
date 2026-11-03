//! Shape guides: a line, an arc, an angle, a circle or a square put on the paper, that a stroke drawn along follows.
//!
//! Each has handles to drag, a label (its length, angle, size or diameter) and a way of holding the points of a
//! stroke to it.
//!
//! A guide is a value: dragging a handle gives a new guide from the one before the drag ([`Guide::dragged`]), and
//! a point is held to a guide by [`Guide::project`]. Both are plain geometry, tested without a window.

use std::f32::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::geometry::Point;
use crate::units::Measurement;

/// The smallest a guide can be made by a handle, in canvas units.
const MIN_SIZE: f32 = 4.0;
/// Points along a curve when it is turned into a polyline to draw.
const CURVE_STEPS: usize = 96;

/// Which shape a guide is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Line,
    Arc,
    Angle,
    Circle,
    Square,
}

impl Kind {
    pub const ALL: [Self; 5] = [
        Self::Line,
        Self::Arc,
        Self::Angle,
        Self::Circle,
        Self::Square,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Line => "Line",
            Self::Arc => "Arc",
            Self::Angle => "Angle",
            Self::Circle => "Circle",
            Self::Square => "Square",
        }
    }
}

/// A handle of a guide: where it is, and whether dragging it moves the whole guide or reshapes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Handle {
    pub point: Point,
    pub moves: bool,
}

/// A guide, in canvas coordinates (the y axis points up).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Guide {
    Line {
        a: Point,
        b: Point,
    },
    /// An ellipse with its axes along the canvas axes; a circle when the radii are equal.
    Circle {
        center: Point,
        rx: f32,
        ry: f32,
    },
    /// A rectangle with its sides along the canvas axes; a square when the sides are equal.
    Square {
        min: Point,
        max: Point,
    },
    /// The arc of the circle through three points, from `a` to `b` by way of `through`.
    Arc {
        a: Point,
        b: Point,
        through: Point,
    },
    /// Two arms from a corner.
    Angle {
        vertex: Point,
        a: Point,
        b: Point,
    },
}

fn pt(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

fn mid(a: Point, b: Point) -> Point {
    pt(f32::midpoint(a.x, b.x), f32::midpoint(a.y, b.y))
}

/// The nearest point to `p` on the segment `a` to `b`.
fn on_segment(a: Point, b: Point, p: Point) -> Point {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length2 = dx * dx + dy * dy;
    if length2 <= f32::EPSILON {
        return a;
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / length2).clamp(0.0, 1.0);
    pt(a.x + dx * t, a.y + dy * t)
}

/// How far along `path` (as a distance from its start; `along` has the distance of each of its points) the point
/// nearest to `p` is.
fn position_on(path: &[Point], along: &[f32], p: Point) -> f32 {
    let mut best = (f32::MAX, 0.0);
    for (i, pair) in path.windows(2).enumerate() {
        let nearest = on_segment(pair[0], pair[1], p);
        let gap = nearest.distance(p);
        if gap < best.0 {
            best = (gap, along[i] + pair[0].distance(nearest));
        }
    }
    best.1
}

/// The circle through three points, as its center and radius; `None` if they are on one line.
fn circumcircle(a: Point, b: Point, c: Point) -> Option<(Point, f32)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-4 {
        return None;
    }
    let (a2, b2, c2) = (
        a.x * a.x + a.y * a.y,
        b.x * b.x + b.y * b.y,
        c.x * c.x + c.y * c.y,
    );
    let x = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let y = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let center = pt(x, y);
    Some((center, center.distance(a)))
}

/// An arc as its circle and the angles it runs between: from `start` through `sweep` (positive is
/// counterclockwise, with the y axis up).
struct Curve {
    center: Point,
    radius: f32,
    start: f32,
    sweep: f32,
}

impl Curve {
    fn at(&self, fraction: f32) -> Point {
        let angle = self.start + self.sweep * fraction;
        pt(
            self.center.x + self.radius * angle.cos(),
            self.center.y + self.radius * angle.sin(),
        )
    }
}

/// The arc from `a` to `b` through `through`, if the three are not on one line.
fn arc_sweep(a: Point, b: Point, through: Point) -> Option<Curve> {
    let (center, radius) = circumcircle(a, through, b)?;
    let angle = |p: Point| (p.y - center.y).atan2(p.x - center.x);
    let (start, end) = (angle(a), angle(b));
    let cross = (through.x - a.x) * (b.y - through.y) - (through.y - a.y) * (b.x - through.x);
    let counterclockwise = cross > 0.0;
    let mut sweep = if counterclockwise {
        end - start
    } else {
        start - end
    };
    sweep = sweep.rem_euclid(TAU);
    Some(Curve {
        center,
        radius,
        start,
        sweep: if counterclockwise { sweep } else { -sweep },
    })
}

impl Guide {
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::Line { .. } => Kind::Line,
            Self::Circle { .. } => Kind::Circle,
            Self::Square { .. } => Kind::Square,
            Self::Arc { .. } => Kind::Arc,
            Self::Angle { .. } => Kind::Angle,
        }
    }

    /// A guide of `kind` centered on `center`, about `size` canvas units across.
    #[must_use]
    pub fn new(kind: Kind, center: Point, size: f32) -> Self {
        let half = size / 2.0;
        let third = size / 3.0;
        match kind {
            Kind::Line => Self::Line {
                a: pt(center.x - half, center.y),
                b: pt(center.x + half, center.y),
            },
            Kind::Circle => Self::Circle {
                center,
                rx: third,
                ry: third,
            },
            Kind::Square => Self::Square {
                min: pt(center.x - third, center.y - third),
                max: pt(center.x + third, center.y + third),
            },
            Kind::Arc => Self::Arc {
                a: pt(center.x - half, center.y - third / 2.0),
                b: pt(center.x + half, center.y - third / 2.0),
                through: pt(center.x, center.y + third),
            },
            Kind::Angle => {
                let vertex = pt(center.x - third, center.y - third / 2.0);
                let arm = size * 0.7;
                let slant = std::f32::consts::FRAC_1_SQRT_2;
                Self::Angle {
                    vertex,
                    a: pt(vertex.x + arm, vertex.y),
                    b: pt(vertex.x + arm * slant, vertex.y + arm * slant),
                }
            }
        }
    }

    /// The places to grab it. Dragging the last one that `moves` carries the whole guide; the rest reshape it.
    #[must_use]
    pub fn handles(&self) -> Vec<Handle> {
        let reshape = |point| Handle {
            point,
            moves: false,
        };
        let carry = |point| Handle { point, moves: true };
        match *self {
            Self::Line { a, b } => vec![reshape(a), reshape(b), carry(mid(a, b))],
            Self::Circle { center, rx, ry } => vec![
                reshape(pt(center.x + rx, center.y)),
                reshape(pt(center.x, center.y + ry)),
                carry(center),
            ],
            Self::Square { min, max } => vec![
                reshape(min),
                reshape(pt(max.x, min.y)),
                reshape(max),
                reshape(pt(min.x, max.y)),
                carry(mid(min, max)),
            ],
            Self::Arc { a, b, through } => {
                vec![reshape(a), reshape(b), reshape(through), carry(mid(a, b))]
            }
            Self::Angle { vertex, a, b } => vec![carry(vertex), reshape(a), reshape(b)],
        }
    }

    /// The guide as it is after handle number `handle` of this one is dragged by `delta`. A handle that does not
    /// exist leaves it as it is.
    #[must_use]
    pub fn dragged(&self, handle: usize, delta: (f32, f32)) -> Self {
        let (dx, dy) = delta;
        let by = |p: Point| pt(p.x + dx, p.y + dy);
        let Some(grabbed) = self.handles().get(handle).copied() else {
            return *self;
        };
        if grabbed.moves {
            return self.translated(delta);
        }
        match (*self, handle) {
            (Self::Line { a, b }, 0) => Self::Line { a: by(a), b },
            (Self::Line { a, b }, _) => Self::Line { a, b: by(b) },
            (Self::Circle { center, rx, ry }, 0) => Self::Circle {
                center,
                rx: (rx + dx).abs().max(MIN_SIZE),
                ry,
            },
            (Self::Circle { center, rx, ry }, _) => Self::Circle {
                center,
                rx,
                ry: (ry + dy).abs().max(MIN_SIZE),
            },
            (Self::Square { min, max }, i) => {
                // The corner that is dragged moves, and the one opposite it stays.
                let (mut lo, mut hi) = (min, max);
                if i == 0 || i == 3 {
                    lo.x += dx;
                } else {
                    hi.x += dx;
                }
                if i == 0 || i == 1 {
                    lo.y += dy;
                } else {
                    hi.y += dy;
                }
                Self::Square {
                    min: pt(lo.x.min(hi.x - MIN_SIZE), lo.y.min(hi.y - MIN_SIZE)),
                    max: pt(hi.x.max(lo.x + MIN_SIZE), hi.y.max(lo.y + MIN_SIZE)),
                }
            }
            (Self::Arc { a, b, through }, 0) => Self::Arc {
                a: by(a),
                b,
                through,
            },
            (Self::Arc { a, b, through }, 1) => Self::Arc {
                a,
                b: by(b),
                through,
            },
            (Self::Arc { a, b, through }, _) => Self::Arc {
                a,
                b,
                through: by(through),
            },
            (Self::Angle { vertex, a, b }, 1) => Self::Angle {
                vertex,
                a: by(a),
                b,
            },
            (Self::Angle { vertex, a, b }, _) => Self::Angle {
                vertex,
                a,
                b: by(b),
            },
        }
    }

    /// The same guide moved by `delta`.
    #[must_use]
    pub fn translated(&self, delta: (f32, f32)) -> Self {
        let by = |p: Point| pt(p.x + delta.0, p.y + delta.1);
        match *self {
            Self::Line { a, b } => Self::Line { a: by(a), b: by(b) },
            Self::Circle { center, rx, ry } => Self::Circle {
                center: by(center),
                rx,
                ry,
            },
            Self::Square { min, max } => Self::Square {
                min: by(min),
                max: by(max),
            },
            Self::Arc { a, b, through } => Self::Arc {
                a: by(a),
                b: by(b),
                through: by(through),
            },
            Self::Angle { vertex, a, b } => Self::Angle {
                vertex: by(vertex),
                a: by(a),
                b: by(b),
            },
        }
    }

    /// The nearest point to `p` on the guide. A stroke that is drawn along the guide has each of its points put
    /// here, so it follows the guide however the pen wanders off it.
    #[must_use]
    pub fn project(&self, p: Point) -> Point {
        match *self {
            Self::Line { a, b } => on_segment(a, b, p),
            Self::Circle { center, rx, ry } => {
                let (rx, ry) = (rx.max(MIN_SIZE), ry.max(MIN_SIZE));
                let (dx, dy) = (p.x - center.x, p.y - center.y);
                let scale = ((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt();
                if scale < 1e-6 {
                    return pt(center.x + rx, center.y);
                }
                pt(center.x + dx / scale, center.y + dy / scale)
            }
            Self::Square { min, max } => {
                let inside = p.x > min.x && p.x < max.x && p.y > min.y && p.y < max.y;
                if !inside {
                    return pt(p.x.clamp(min.x, max.x), p.y.clamp(min.y, max.y));
                }
                // Inside, to the nearest side.
                let gaps = [
                    (p.x - min.x, pt(min.x, p.y)),
                    (max.x - p.x, pt(max.x, p.y)),
                    (p.y - min.y, pt(p.x, min.y)),
                    (max.y - p.y, pt(p.x, max.y)),
                ];
                gaps.into_iter()
                    .min_by(|x, y| x.0.total_cmp(&y.0))
                    .map_or(p, |(_, point)| point)
            }
            Self::Arc { a, b, through } => {
                let Some(arc) = arc_sweep(a, b, through) else {
                    return on_segment(a, b, p);
                };
                let angle = (p.y - arc.center.y).atan2(p.x - arc.center.x);
                let along = if arc.sweep >= 0.0 {
                    (angle - arc.start).rem_euclid(TAU)
                } else {
                    (arc.start - angle).rem_euclid(TAU)
                };
                if along <= arc.sweep.abs() {
                    let fraction = along / arc.sweep.abs();
                    return arc.at(fraction);
                }
                if p.distance(a) <= p.distance(b) { a } else { b }
            }
            Self::Angle { vertex, a, b } => {
                let (on_a, on_b) = (on_segment(vertex, a, p), on_segment(vertex, b, p));
                if p.distance(on_a) <= p.distance(on_b) {
                    on_a
                } else {
                    on_b
                }
            }
        }
    }

    /// How far `p` is from the guide.
    #[must_use]
    pub fn distance(&self, p: Point) -> f32 {
        self.project(p).distance(p)
    }

    /// The corners and curve of the guide between two points that are on it, in the order a pen going from `a` to
    /// `b` the shorter way along it passes them. A pen that cuts a corner has samples on both sides of it and none
    /// on it, and joining those up would cut the corner off the stroke; these points keep it.
    #[must_use]
    pub fn between(&self, a: Point, b: Point) -> Vec<Point> {
        let Some(path) = self.paths().into_iter().next().filter(|p| p.len() >= 3) else {
            return Vec::new();
        };
        let closed = path
            .first()
            .zip(path.last())
            .is_some_and(|(f, l)| f.distance(*l) < 1e-3);
        // The vertices (without the repeated one that closes a ring) and how far along each is.
        let vertices = if closed {
            &path[..path.len() - 1]
        } else {
            &path[..]
        };
        let mut along = vec![0.0_f32];
        for pair in path.windows(2) {
            along.push(along.last().copied().unwrap_or(0.0) + pair[0].distance(pair[1]));
        }
        let total = along.last().copied().unwrap_or(0.0);
        if total <= f32::EPSILON {
            return Vec::new();
        }
        let (from, to) = (position_on(&path, &along, a), position_on(&path, &along, b));
        let eps = 1e-3;
        // How far each vertex is ahead of `from`, going the way the pen goes.
        let (forward, ring) = if closed {
            let ahead = (to - from).rem_euclid(total);
            (ahead <= total - ahead, true)
        } else {
            (to >= from, false)
        };
        let reach = if closed {
            if forward {
                (to - from).rem_euclid(total)
            } else {
                (from - to).rem_euclid(total)
            }
        } else {
            (to - from).abs()
        };
        let mut found: Vec<(f32, Point)> = vertices
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let step = if forward {
                    along[i] - from
                } else {
                    from - along[i]
                };
                let step = if ring { step.rem_euclid(total) } else { step };
                (step > eps && step < reach - eps).then_some((step, *v))
            })
            .collect();
        found.sort_by(|x, y| x.0.total_cmp(&y.0));
        found.into_iter().map(|(_, v)| v).collect()
    }

    /// The guide as lines to draw: every path is a list of points to join up.
    #[must_use]
    pub fn paths(&self) -> Vec<Vec<Point>> {
        match *self {
            Self::Line { a, b } => vec![vec![a, b]],
            Self::Circle { center, rx, ry } => {
                let ring = (0..=CURVE_STEPS)
                    .map(|i| {
                        let angle = TAU * i as f32 / CURVE_STEPS as f32;
                        pt(center.x + rx * angle.cos(), center.y + ry * angle.sin())
                    })
                    .collect();
                vec![ring]
            }
            Self::Square { min, max } => {
                vec![vec![min, pt(max.x, min.y), max, pt(min.x, max.y), min]]
            }
            Self::Arc { a, b, through } => match arc_sweep(a, b, through) {
                Some(arc) => vec![
                    (0..=CURVE_STEPS)
                        .map(|i| arc.at(i as f32 / CURVE_STEPS as f32))
                        .collect(),
                ],
                None => vec![vec![a, b]],
            },
            Self::Angle { vertex, a, b } => vec![vec![a, vertex, b]],
        }
    }

    /// Where the label goes, in canvas units.
    #[must_use]
    pub fn label_at(&self) -> Point {
        match *self {
            Self::Line { a, b } => mid(a, b),
            Self::Circle { center, .. } => center,
            Self::Square { min, max } => mid(min, max),
            Self::Arc { a, b, through } => mid(mid(a, b), through),
            Self::Angle { vertex, .. } => vertex,
        }
    }

    /// What the guide measures, written in the unit the settings choose.
    #[must_use]
    pub fn label(&self, measurement: &Measurement) -> String {
        match *self {
            Self::Line { a, b } => {
                let degrees = (b.y - a.y).atan2(b.x - a.x).to_degrees().rem_euclid(180.0);
                format!(
                    "{} Â· {}",
                    measurement.length(a.distance(b)),
                    measurement.angle(degrees)
                )
            }
            Self::Circle { rx, ry, .. } => {
                if (rx - ry).abs() <= 0.01 * rx.max(ry) {
                    format!("\u{d8} {}", measurement.length(rx * 2.0))
                } else {
                    format!(
                        "{} Ã— {}",
                        measurement.length(rx * 2.0),
                        measurement.length(ry * 2.0)
                    )
                }
            }
            Self::Square { min, max } => {
                let (w, h) = (max.x - min.x, max.y - min.y);
                if (w - h).abs() <= 0.01 * w.max(h) {
                    measurement.length(w)
                } else {
                    format!("{} Ã— {}", measurement.length(w), measurement.length(h))
                }
            }
            Self::Arc { a, b, through } => match arc_sweep(a, b, through) {
                Some(arc) => format!(
                    "{} Â· {}",
                    measurement.length(arc.radius * arc.sweep.abs()),
                    measurement.angle(arc.sweep.abs().to_degrees())
                ),
                None => measurement.length(a.distance(b)),
            },
            Self::Angle { vertex, a, b } => {
                let (u, v) = (
                    (a.x - vertex.x, a.y - vertex.y),
                    (b.x - vertex.x, b.y - vertex.y),
                );
                let dot = u.0 * v.0 + u.1 * v.1;
                let norms = u.0.hypot(u.1) * v.0.hypot(v.1);
                let degrees = if norms <= f32::EPSILON {
                    0.0
                } else {
                    (dot / norms).clamp(-1.0, 1.0).acos().to_degrees()
                };
                measurement.angle(degrees)
            }
        }
    }
}

/// The nearest of `guides` to `p` that is within `reach`, as its place in the list.
#[must_use]
pub fn nearest(guides: &[Guide], p: Point, reach: f32) -> Option<usize> {
    guides
        .iter()
        .enumerate()
        .map(|(i, g)| (i, g.distance(p)))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The handle of any of `guides` within `reach` of `p`, nearest first, as the guide's place and the handle's.
#[must_use]
pub fn nearest_handle(guides: &[Guide], p: Point, reach: f32) -> Option<(usize, usize)> {
    guides
        .iter()
        .enumerate()
        .flat_map(|(g, guide)| {
            guide
                .handles()
                .into_iter()
                .enumerate()
                .map(move |(h, handle)| ((g, h), handle.point.distance(p)))
        })
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(place, _)| place)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-3
    }

    #[test]
    fn a_point_is_held_to_the_nearest_place_on_a_line_and_stops_at_its_ends() {
        let line = Guide::Line {
            a: pt(0.0, 0.0),
            b: pt(100.0, 0.0),
        };
        assert!(near(line.project(pt(40.0, 25.0)), pt(40.0, 0.0)));
        assert!(
            near(line.project(pt(-30.0, 5.0)), pt(0.0, 0.0)),
            "not past the end"
        );
        assert!(near(line.project(pt(180.0, -9.0)), pt(100.0, 0.0)));
        assert!((line.distance(pt(40.0, 25.0)) - 25.0).abs() < 1e-3);
        // Two ends in one place do not divide by zero.
        let dot = Guide::Line {
            a: pt(3.0, 4.0),
            b: pt(3.0, 4.0),
        };
        assert!(near(dot.project(pt(10.0, 10.0)), pt(3.0, 4.0)));
    }

    #[test]
    fn a_circle_holds_a_point_to_its_ring_from_inside_or_outside_and_from_the_middle() {
        let circle = Guide::Circle {
            center: pt(10.0, 20.0),
            rx: 50.0,
            ry: 50.0,
        };
        assert!(near(circle.project(pt(10.0 + 150.0, 20.0)), pt(60.0, 20.0)));
        assert!(near(circle.project(pt(10.0, 20.0 - 5.0)), pt(10.0, -30.0)));
        let diagonal = circle.project(pt(10.0 + 30.0, 20.0 + 30.0));
        assert!((diagonal.distance(pt(10.0, 20.0)) - 50.0).abs() < 1e-3);
        let centre = circle.project(pt(10.0, 20.0));
        assert!(centre.x.is_finite() && (centre.distance(pt(10.0, 20.0)) - 50.0).abs() < 1e-3);
        // An ellipse keeps to its own radii.
        let ellipse = Guide::Circle {
            center: pt(0.0, 0.0),
            rx: 80.0,
            ry: 20.0,
        };
        assert!(near(ellipse.project(pt(500.0, 0.0)), pt(80.0, 0.0)));
        assert!(near(ellipse.project(pt(0.0, -500.0)), pt(0.0, -20.0)));
    }

    #[test]
    fn a_square_holds_a_point_to_its_sides() {
        let square = Guide::Square {
            min: pt(0.0, 0.0),
            max: pt(100.0, 60.0),
        };
        assert!(
            near(square.project(pt(50.0, 90.0)), pt(50.0, 60.0)),
            "above"
        );
        assert!(
            near(square.project(pt(150.0, 90.0)), pt(100.0, 60.0)),
            "off a corner"
        );
        assert!(
            near(square.project(pt(10.0, 30.0)), pt(0.0, 30.0)),
            "inside, near the left"
        );
        assert!(
            near(square.project(pt(50.0, 5.0)), pt(50.0, 0.0)),
            "inside, near the bottom"
        );
        assert!(square.distance(pt(100.0, 30.0)) < 1e-3, "on a side");
    }

    #[test]
    fn an_arc_runs_through_its_three_points_and_stops_at_its_ends() {
        // The upper half of the circle of radius 50 around the origin, from (50, 0) to (-50, 0) by way of (0, 50).
        let arc = Guide::Arc {
            a: pt(50.0, 0.0),
            b: pt(-50.0, 0.0),
            through: pt(0.0, 50.0),
        };
        assert!(
            near(arc.project(pt(0.0, 90.0)), pt(0.0, 50.0)),
            "outside the top"
        );
        assert!(
            near(arc.project(pt(0.0, 10.0)), pt(0.0, 50.0)),
            "inside the top"
        );
        let diagonal = arc.project(pt(60.0, 60.0));
        assert!((diagonal.distance(pt(0.0, 0.0)) - 50.0).abs() < 1e-3 && diagonal.y > 0.0);
        // The lower half is not part of it: the nearest end is.
        assert!(near(arc.project(pt(40.0, -60.0)), pt(50.0, 0.0)));
        assert!(near(arc.project(pt(-40.0, -60.0)), pt(-50.0, 0.0)));
        // Going the other way round, by way of the bottom, it is the lower half.
        let lower = Guide::Arc {
            a: pt(50.0, 0.0),
            b: pt(-50.0, 0.0),
            through: pt(0.0, -50.0),
        };
        assert!(near(lower.project(pt(0.0, -90.0)), pt(0.0, -50.0)));
        // And a point above it, equally far from both ends, goes to the end the arc starts at.
        assert!(near(lower.project(pt(0.0, 70.0)), pt(50.0, 0.0)));
    }

    #[test]
    fn an_arc_through_three_points_on_a_line_is_that_line() {
        let flat = Guide::Arc {
            a: pt(0.0, 0.0),
            b: pt(100.0, 0.0),
            through: pt(50.0, 0.0),
        };
        assert!(near(flat.project(pt(30.0, 12.0)), pt(30.0, 0.0)));
        assert_eq!(flat.paths().len(), 1);
        assert_eq!(flat.paths()[0].len(), 2);
    }

    #[test]
    fn an_angle_holds_a_point_to_the_nearer_arm() {
        let angle = Guide::Angle {
            vertex: pt(0.0, 0.0),
            a: pt(100.0, 0.0),
            b: pt(0.0, 100.0),
        };
        assert!(near(angle.project(pt(60.0, 10.0)), pt(60.0, 0.0)));
        assert!(near(angle.project(pt(8.0, 70.0)), pt(0.0, 70.0)));
        assert!(
            near(angle.project(pt(-20.0, -20.0)), pt(0.0, 0.0)),
            "behind the corner"
        );
    }

    #[test]
    fn dragging_a_handle_reshapes_and_dragging_the_middle_one_moves_the_whole_guide() {
        let line = Guide::Line {
            a: pt(0.0, 0.0),
            b: pt(100.0, 0.0),
        };
        assert_eq!(
            line.dragged(1, (10.0, 20.0)),
            Guide::Line {
                a: pt(0.0, 0.0),
                b: pt(110.0, 20.0)
            }
        );
        assert_eq!(
            line.dragged(0, (-5.0, 5.0)),
            Guide::Line {
                a: pt(-5.0, 5.0),
                b: pt(100.0, 0.0)
            }
        );
        assert_eq!(
            line.dragged(2, (7.0, 9.0)),
            Guide::Line {
                a: pt(7.0, 9.0),
                b: pt(107.0, 9.0)
            }
        );
        assert_eq!(line.dragged(9, (7.0, 9.0)), line, "no such handle");
    }

    #[test]
    fn a_circle_is_resized_from_its_side_and_top_and_cannot_vanish() {
        let circle = Guide::Circle {
            center: pt(0.0, 0.0),
            rx: 50.0,
            ry: 50.0,
        };
        assert_eq!(
            circle.dragged(0, (30.0, 99.0)),
            Guide::Circle {
                center: pt(0.0, 0.0),
                rx: 80.0,
                ry: 50.0
            }
        );
        assert_eq!(
            circle.dragged(1, (99.0, 10.0)),
            Guide::Circle {
                center: pt(0.0, 0.0),
                rx: 50.0,
                ry: 60.0
            }
        );
        let Guide::Circle { rx, .. } = circle.dragged(0, (-50.0, 0.0)) else {
            panic!("still a circle")
        };
        assert!(rx >= MIN_SIZE, "{rx}");
        assert_eq!(
            circle.dragged(2, (5.0, 6.0)),
            Guide::Circle {
                center: pt(5.0, 6.0),
                rx: 50.0,
                ry: 50.0
            }
        );
    }

    #[test]
    fn a_square_keeps_the_corner_opposite_the_one_dragged_and_cannot_turn_inside_out() {
        let square = Guide::Square {
            min: pt(0.0, 0.0),
            max: pt(100.0, 100.0),
        };
        // The top right corner out.
        assert_eq!(
            square.dragged(2, (20.0, 10.0)),
            Guide::Square {
                min: pt(0.0, 0.0),
                max: pt(120.0, 110.0)
            }
        );
        // The bottom left corner in.
        assert_eq!(
            square.dragged(0, (30.0, 40.0)),
            Guide::Square {
                min: pt(30.0, 40.0),
                max: pt(100.0, 100.0)
            }
        );
        // Dragged right across the opposite side: stays a rectangle with a size.
        let Guide::Square { min, max } = square.dragged(0, (500.0, 500.0)) else {
            panic!("still a square")
        };
        assert!(max.x - min.x >= MIN_SIZE && max.y - min.y >= MIN_SIZE);
        assert_eq!(square.handles().len(), 5);
    }

    #[test]
    fn every_kind_made_for_a_place_is_a_real_shape_around_it() {
        for kind in Kind::ALL {
            let guide = Guide::new(kind, pt(100.0, 200.0), 300.0);
            assert_eq!(guide.kind(), kind);
            assert_ne!(guide.handles().len(), 0);
            assert_eq!(
                guide.handles().iter().filter(|h| h.moves).count(),
                1,
                "{kind:?} has one handle that moves it"
            );
            for path in guide.paths() {
                assert!(path.len() >= 2);
                assert!(
                    path.iter()
                        .all(|p| (p.x - 100.0).abs() < 300.0 && (p.y - 200.0).abs() < 300.0),
                    "{kind:?}"
                );
                // Every point of what is drawn is on the guide.
                assert!(path.iter().all(|p| guide.distance(*p) < 0.5), "{kind:?}");
            }
            let at = guide.label_at();
            assert!((at.x - 100.0).abs() < 300.0 && (at.y - 200.0).abs() < 300.0);
        }
    }

    #[test]
    fn the_labels_say_what_each_guide_measures() {
        let m = Measurement::default();
        let line = Guide::Line {
            a: pt(0.0, 0.0),
            b: pt(30.0, 40.0),
        };
        assert_eq!(line.label(&m), "50 px Â· 53\u{b0}");
        let circle = Guide::Circle {
            center: pt(0.0, 0.0),
            rx: 50.0,
            ry: 50.0,
        };
        assert_eq!(circle.label(&m), "\u{d8} 100 px");
        let ellipse = Guide::Circle {
            center: pt(0.0, 0.0),
            rx: 50.0,
            ry: 25.0,
        };
        assert_eq!(ellipse.label(&m), "100 px Ã— 50 px");
        let square = Guide::Square {
            min: pt(0.0, 0.0),
            max: pt(80.0, 80.0),
        };
        assert_eq!(square.label(&m), "80 px");
        let right = Guide::Angle {
            vertex: pt(0.0, 0.0),
            a: pt(100.0, 0.0),
            b: pt(0.0, 100.0),
        };
        assert_eq!(right.label(&m), "90\u{b0}");
        let half = Guide::Arc {
            a: pt(50.0, 0.0),
            b: pt(-50.0, 0.0),
            through: pt(0.0, 50.0),
        };
        assert_eq!(
            half.label(&m),
            "157 px Â· 180\u{b0}",
            "half of a circle of radius 50"
        );
    }

    #[test]
    fn between_two_points_on_a_square_are_the_corners_the_pen_passes() {
        let square = Guide::Square {
            min: pt(0.0, 0.0),
            max: pt(100.0, 100.0),
        };
        // From the bottom side to the right side: round the corner between them.
        assert_eq!(
            square.between(pt(50.0, 0.0), pt(100.0, 50.0)),
            vec![pt(100.0, 0.0)]
        );
        // And back the same way.
        assert_eq!(
            square.between(pt(100.0, 50.0), pt(50.0, 0.0)),
            vec![pt(100.0, 0.0)]
        );
        // Two on one side have none between them.
        assert_eq!(square.between(pt(20.0, 0.0), pt(80.0, 0.0)).len(), 0);
        // The shorter way: from near the end of the bottom side to near the start of the left side is one corner,
        // not three the long way round.
        assert_eq!(
            square.between(pt(10.0, 0.0), pt(0.0, 10.0)),
            vec![pt(0.0, 0.0)]
        );
        // Across two corners.
        assert_eq!(
            square.between(pt(90.0, 0.0), pt(90.0, 100.0)),
            vec![pt(100.0, 0.0), pt(100.0, 100.0)]
        );
    }

    #[test]
    fn between_two_points_on_a_circle_are_points_of_the_circle_in_order() {
        let circle = Guide::Circle {
            center: pt(0.0, 0.0),
            rx: 100.0,
            ry: 100.0,
        };
        let between = circle.between(pt(100.0, 0.0), pt(0.0, 100.0));
        assert!(
            between.len() >= 20,
            "a quarter of the ring is a smooth curve: {}",
            between.len()
        );
        assert!(
            between
                .iter()
                .all(|p| (p.distance(pt(0.0, 0.0)) - 100.0).abs() < 0.01)
        );
        let angles: Vec<f32> = between.iter().map(|p| p.y.atan2(p.x)).collect();
        assert!(
            angles.windows(2).all(|w| w[0] < w[1]),
            "counterclockwise, in order"
        );
        // The other way round is the short way too.
        let back = circle.between(pt(0.0, 100.0), pt(100.0, 0.0));
        assert_eq!(back.len(), between.len());
        assert!(
            back.windows(2)
                .all(|w| w[0].y.atan2(w[0].x) > w[1].y.atan2(w[1].x))
        );
    }

    #[test]
    fn between_on_a_line_is_nothing_and_on_an_angle_is_the_corner() {
        let line = Guide::Line {
            a: pt(0.0, 0.0),
            b: pt(100.0, 0.0),
        };
        assert_eq!(line.between(pt(10.0, 0.0), pt(90.0, 0.0)).len(), 0);
        let angle = Guide::Angle {
            vertex: pt(0.0, 0.0),
            a: pt(100.0, 0.0),
            b: pt(0.0, 100.0),
        };
        assert_eq!(
            angle.between(pt(60.0, 0.0), pt(0.0, 40.0)),
            vec![pt(0.0, 0.0)]
        );
        assert_eq!(angle.between(pt(60.0, 0.0), pt(20.0, 0.0)).len(), 0);
    }

    #[test]
    fn between_on_an_arc_stays_on_the_arc_and_never_goes_the_long_way() {
        let arc = Guide::Arc {
            a: pt(50.0, 0.0),
            b: pt(-50.0, 0.0),
            through: pt(0.0, 50.0),
        };
        let between = arc.between(pt(50.0, 0.0), pt(-50.0, 0.0));
        assert!(between.len() > 50);
        assert!(
            between
                .iter()
                .all(|p| p.y > -1e-3 && (p.distance(pt(0.0, 0.0)) - 50.0).abs() < 0.01)
        );
    }

    #[test]
    fn the_nearest_guide_and_handle_are_found_within_reach() {
        let guides = [
            Guide::Line {
                a: pt(0.0, 0.0),
                b: pt(100.0, 0.0),
            },
            Guide::Line {
                a: pt(0.0, 50.0),
                b: pt(100.0, 50.0),
            },
        ];
        assert_eq!(nearest(&guides, pt(40.0, 8.0), 10.0), Some(0));
        assert_eq!(nearest(&guides, pt(40.0, 46.0), 10.0), Some(1));
        assert_eq!(nearest(&guides, pt(40.0, 25.0), 10.0), None);
        assert_eq!(nearest_handle(&guides, pt(98.0, 52.0), 10.0), Some((1, 1)));
        assert_eq!(
            nearest_handle(&guides, pt(50.0, 3.0), 10.0),
            Some((0, 2)),
            "the middle of the first"
        );
        assert_eq!(nearest_handle(&guides, pt(30.0, 25.0), 10.0), None);
    }

    #[test]
    fn nothing_is_nan_for_nonsense_input() {
        for guide in [
            Guide::new(Kind::Line, pt(0.0, 0.0), 0.0),
            Guide::new(Kind::Circle, pt(0.0, 0.0), 0.0),
            Guide::new(Kind::Arc, pt(0.0, 0.0), 0.0),
            Guide::new(Kind::Angle, pt(0.0, 0.0), 0.0),
            Guide::new(Kind::Square, pt(0.0, 0.0), 0.0),
        ] {
            let p = guide.project(pt(1.0e6, -1.0e6));
            assert!(p.x.is_finite() && p.y.is_finite(), "{guide:?}");
            assert_ne!(guide.label(&Measurement::default()), "");
        }
    }
}
