//! The soft eraser: wiping out only the part of a stroke that the eraser touches, and leaving the rest.
//!
//! The hard eraser takes a stroke away whole; the soft one is this. Everything within the eraser's radius of the
//! path it swept is taken out, and the stroke is cut where the eraser's edge crosses it.

use uuid::Uuid;

use crate::document::{RawSample, Stroke};
use crate::geometry::Point;
use crate::nudge::densify;
use crate::slice::lerp_sample;

/// What is left of a stroke after the eraser went over it.
#[derive(Debug, Clone, PartialEq)]
pub enum Wiped {
    /// The eraser did not touch it.
    Untouched,
    /// Nothing is left of it.
    Gone,
    /// What is left, in drawing order: the first keeps the stroke's id and the rest get the nil id, to be given
    /// ids of their own.
    Parts(Vec<Stroke>),
}

/// How far `p` is from the segment `a` to `b`.
fn distance_to(p: Point, a: Point, b: Point) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length2 = dx * dx + dy * dy;
    if length2 <= f32::EPSILON {
        return p.distance(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / length2).clamp(0.0, 1.0);
    p.distance(Point::new(a.x + dx * t, a.y + dy * t))
}

/// Where along the way from `outside` to `inside` (which is on the eraser's side of its edge) the eraser's edge
/// is, as the fraction of the way from `outside`.
fn edge_between(
    outside: &RawSample,
    inside: &RawSample,
    swept: (Point, Point),
    radius: f32,
) -> f32 {
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    for _ in 0..12 {
        let middle = f32::midpoint(low, high);
        let at = lerp_sample(outside, inside, middle).point;
        if distance_to(at, swept.0, swept.1) > radius {
            low = middle;
        } else {
            high = middle;
        }
    }
    f32::midpoint(low, high)
}

/// A run of the stroke that the eraser left, while it is being collected.
#[derive(Default)]
struct Run {
    samples: Vec<RawSample>,
    pressure: Vec<f32>,
}

impl Run {
    fn push(&mut self, sample: RawSample, pressure: Option<f32>) {
        self.samples.push(sample);
        if let Some(p) = pressure {
            self.pressure.push(p);
        }
    }

    /// A run needs two points, in different places, to be a stroke.
    fn is_stroke(&self) -> bool {
        self.samples.len() >= 2
            && self
                .samples
                .windows(2)
                .any(|w| w[0].point.distance(w[1].point) > 1e-4)
    }
}

/// The runs of `stroke` (already with enough points) that are outside the disc swept from `from` to `to`, each
/// ending exactly at the eraser's edge.
fn runs_outside(stroke: &Stroke, from: Point, to: Point, radius: f32) -> Vec<Run> {
    let inside = |s: &RawSample| distance_to(s.point, from, to) <= radius;
    let has_pressure = stroke.pressure.len() == stroke.samples.len();
    let pressure_at = |i: usize| has_pressure.then(|| stroke.pressure[i]);
    let mut runs: Vec<Run> = Vec::new();
    let mut open: Option<Run> = None;
    for (i, here) in stroke.samples.iter().enumerate() {
        let is_inside = inside(here);
        if i > 0 {
            let before = &stroke.samples[i - 1];
            let was_inside = inside(before);
            if was_inside != is_inside {
                // The eraser's edge is between the two: the part that is left ends (or begins) exactly there.
                let (outer, inner, a, b) = if was_inside {
                    (here, before, i, i - 1)
                } else {
                    (before, here, i - 1, i)
                };
                let t = edge_between(outer, inner, (from, to), radius);
                let edge = lerp_sample(outer, inner, t);
                let pressure = pressure_at(a)
                    .zip(pressure_at(b))
                    .map(|(p, q)| p + (q - p) * t);
                if was_inside {
                    // Coming out of the eraser: a new run begins at its edge.
                    let mut run = Run::default();
                    run.push(edge, pressure);
                    open = Some(run);
                } else if let Some(mut run) = open.take() {
                    // Going into the eraser: the run ends at its edge.
                    run.push(edge, pressure);
                    runs.push(run);
                }
            }
        }
        if !is_inside {
            open.get_or_insert_with(Run::default)
                .push(*here, pressure_at(i));
        }
    }
    runs.extend(open);
    runs
}

/// What is left of `stroke` after an eraser of `radius` wiped from `from` to `to`.
///
/// A stroke that has only a stored path and no samples cannot be wiped in part: it is gone if the eraser touches
/// its path, and otherwise left alone.
#[must_use]
pub fn wipe(stroke: &Stroke, from: Point, to: Point, radius: f32) -> Wiped {
    if radius <= 0.0 || !radius.is_finite() {
        return Wiped::Untouched;
    }
    let reaches = stroke.bounds().is_some_and(|b| {
        b[0] <= from.x.max(to.x) + radius
            && b[2] >= from.x.min(to.x) - radius
            && b[1] <= from.y.max(to.y) + radius
            && b[3] >= from.y.min(to.y) - radius
    });
    if !reaches {
        return Wiped::Untouched;
    }
    if stroke.samples.len() < 2 {
        let touched = stroke.points().any(|p| distance_to(p, from, to) <= radius);
        return if touched {
            Wiped::Gone
        } else {
            Wiped::Untouched
        };
    }
    // Extra points first, so that a stroke with a long gap between its points can be wiped in the middle of it.
    let dense = densify(stroke, (radius / 2.5).max(0.5));
    if !dense
        .samples
        .iter()
        .any(|s| distance_to(s.point, from, to) <= radius)
    {
        return Wiped::Untouched;
    }
    let mut parts: Vec<Stroke> = Vec::new();
    for run in runs_outside(&dense, from, to, radius)
        .into_iter()
        .filter(Run::is_stroke)
    {
        let mut part = stroke.clone();
        part.samples = run.samples;
        part.pressure = run.pressure;
        part.path = Vec::new();
        if parts.is_empty() {
            part.id = stroke.id;
        } else {
            part.id = Uuid::nil();
        }
        parts.push(part);
    }
    if parts.is_empty() {
        Wiped::Gone
    } else {
        Wiped::Parts(parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::StrokeBuilder;
    use crate::document::Pen;
    use crate::geometry::Color;

    /// A horizontal line from x = 0 to x = 100 with a point every 10 units, with pressure rising along it if asked.
    fn line(pressure: bool) -> Stroke {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.0)
            .with_id(Uuid::from_u128(1));
        for i in 0..=10 {
            let p = pressure.then_some(i as f32 / 10.0);
            b.push(Point::new(i as f32 * 10.0, 0.0), p, f64::from(i) * 0.01);
        }
        b.finish()
    }

    fn xs(stroke: &Stroke) -> (f32, f32) {
        let first = stroke.samples.first().expect("samples").point.x;
        let last = stroke.samples.last().expect("samples").point.x;
        (first, last)
    }

    fn at(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn an_eraser_that_misses_leaves_the_stroke_alone() {
        let s = line(false);
        assert_eq!(
            wipe(&s, at(50.0, 40.0), at(60.0, 40.0), 10.0),
            Wiped::Untouched
        );
        // Close enough to the box but not to the line.
        assert_eq!(
            wipe(&s, at(50.0, 15.0), at(50.0, 15.0), 10.0),
            Wiped::Untouched
        );
        assert_eq!(
            wipe(&s, at(50.0, 0.0), at(50.0, 0.0), 0.0),
            Wiped::Untouched
        );
        assert_eq!(
            wipe(&s, at(50.0, 0.0), at(50.0, 0.0), f32::NAN),
            Wiped::Untouched
        );
    }

    #[test]
    fn an_eraser_in_the_middle_takes_out_that_part_and_leaves_two_ends_cut_at_its_edge() {
        let s = line(false);
        let Wiped::Parts(parts) = wipe(&s, at(50.0, 0.0), at(50.0, 0.0), 10.0) else {
            panic!("two parts");
        };
        assert_eq!(parts.len(), 2);
        let (a, b) = (xs(&parts[0]), xs(&parts[1]));
        assert!(
            a.0.abs() < 1e-3 && (a.1 - 40.0).abs() < 0.05,
            "left part {a:?}"
        );
        assert!(
            (b.0 - 60.0).abs() < 0.05 && (b.1 - 100.0).abs() < 1e-3,
            "right part {b:?}"
        );
        assert_eq!(parts[0].id, s.id, "the first keeps the id");
        assert_eq!(parts[1].id, Uuid::nil(), "the second is waiting for one");
    }

    #[test]
    fn an_eraser_over_an_end_trims_it_and_leaves_one_part() {
        let s = line(false);
        let Wiped::Parts(parts) = wipe(&s, at(0.0, 0.0), at(20.0, 0.0), 6.0) else {
            panic!("one part");
        };
        assert_eq!(parts.len(), 1);
        let (first, last) = xs(&parts[0]);
        assert!(
            (first - 26.0).abs() < 0.05 && (last - 100.0).abs() < 1e-3,
            "{first} {last}"
        );
        assert_eq!(parts[0].id, s.id);
    }

    #[test]
    fn a_sweep_wipes_everything_along_its_path_and_a_stroke_it_covers_is_gone() {
        let s = line(false);
        let Wiped::Parts(parts) = wipe(&s, at(20.0, 3.0), at(70.0, -3.0), 5.0) else {
            panic!("two parts");
        };
        assert_eq!(parts.len(), 2);
        assert!(xs(&parts[0]).1 < 20.0 && xs(&parts[1]).0 > 70.0);
        assert_eq!(wipe(&s, at(-10.0, 0.0), at(110.0, 0.0), 8.0), Wiped::Gone);
    }

    #[test]
    fn a_stroke_with_a_long_gap_between_its_points_is_still_wiped_in_the_middle_of_the_gap() {
        // Two points 200 apart: a point-by-point test would never see the eraser at 100.
        let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.0)
            .with_id(Uuid::from_u128(2));
        b.push(at(0.0, 0.0), None, 0.0);
        b.push(at(200.0, 0.0), None, 0.01);
        let s = b.finish();
        let Wiped::Parts(parts) = wipe(&s, at(100.0, 0.0), at(100.0, 0.0), 10.0) else {
            panic!("two parts");
        };
        assert_eq!(parts.len(), 2);
        assert!((xs(&parts[0]).1 - 90.0).abs() < 0.05 && (xs(&parts[1]).0 - 110.0).abs() < 0.05);
    }

    #[test]
    fn pressure_stays_with_its_points_and_is_worked_out_at_the_cut() {
        let s = line(true);
        let Wiped::Parts(parts) = wipe(&s, at(50.0, 0.0), at(50.0, 0.0), 10.0) else {
            panic!("two parts");
        };
        for part in &parts {
            assert_eq!(
                part.pressure.len(),
                part.samples.len(),
                "one pressure for each point"
            );
        }
        // Pressure rises from 0 to 1 over 100 units, so at the cut at x = 40 it is 0.4.
        let at_cut = *parts[0].pressure.last().expect("pressure");
        assert!((at_cut - 0.4).abs() < 0.01, "{at_cut}");
        let at_start = parts[1].pressure[0];
        assert!((at_start - 0.6).abs() < 0.01, "{at_start}");
    }

    #[test]
    fn whatever_the_sweep_every_part_left_is_a_real_stroke_and_the_first_keeps_the_id() {
        let s = line(true);
        for from in (-20..=120).step_by(7) {
            for to in (-20..=120).step_by(11) {
                for radius in [0.5_f32, 3.0, 12.0, 40.0] {
                    match wipe(&s, at(from as f32, 4.0), at(to as f32, -4.0), radius) {
                        Wiped::Untouched | Wiped::Gone => {}
                        Wiped::Parts(parts) => {
                            assert_eq!(parts[0].id, s.id);
                            for part in &parts {
                                assert!(part.samples.len() >= 2);
                                assert!(
                                    part.samples
                                        .windows(2)
                                        .any(|w| w[0].point.distance(w[1].point) > 1e-4)
                                );
                                assert_eq!(part.pressure.len(), part.samples.len());
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_stroke_with_only_a_stored_path_goes_whole_or_stays() {
        let mut s = line(false);
        s.path = s.samples.iter().map(|p| p.point).collect();
        s.samples.truncate(1);
        assert_eq!(wipe(&s, at(50.0, 0.0), at(50.0, 0.0), 5.0), Wiped::Gone);
        assert_eq!(
            wipe(&s, at(50.0, 50.0), at(50.0, 50.0), 5.0),
            Wiped::Untouched
        );
    }
}
