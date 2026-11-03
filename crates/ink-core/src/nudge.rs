//! Nudging: pulling or pushing part of a stroke into a new shape, the way a fingertip moves wet paint.
//!
//! Two ways to do it:
//!
//! * **Pull** takes hold of one point and drags it; the points along the stroke on either side follow, the
//!   less the further along the stroke they are from the one held.
//! * **Push** is a soft round tip: every point under it is carried along with it, the less the further it is
//!   from the tip.
//!
//! Both fade smoothly to nothing at the edge of their reach, so a nudge never leaves a kink. Strokes get extra
//! points first ([`densify`]), or a long straight stroke could only bend at its ends.

use crate::document::Stroke;
use crate::geometry::Point;
use crate::slice::lerp_sample;

/// How strongly a point `distance` away from the nudge follows it, from `1.0` at the center to `0.0` at `reach`.
#[must_use]
pub fn falloff(distance: f32, reach: f32) -> f32 {
    if reach <= 0.0 || distance >= reach {
        return 0.0;
    }
    f32::midpoint(1.0, (std::f32::consts::PI * distance / reach).cos())
}

/// A copy of `stroke` with points added so that no two neighbors are further apart than `max_gap`. A stroke
/// with no samples (only a stored path) is returned as it is.
#[must_use]
pub fn densify(stroke: &Stroke, max_gap: f32) -> Stroke {
    if stroke.samples.len() < 2 || max_gap <= 0.0 {
        return stroke.clone();
    }
    let has_pressure = stroke.pressure.len() == stroke.samples.len();
    let mut samples = Vec::with_capacity(stroke.samples.len());
    let mut pressure = Vec::new();
    for (i, pair) in stroke.samples.windows(2).enumerate() {
        samples.push(pair[0]);
        if has_pressure {
            pressure.push(stroke.pressure[i]);
        }
        let gap = pair[0].point.distance(pair[1].point);
        let pieces = (gap / max_gap).ceil() as usize;
        for k in 1..pieces {
            let t = k as f32 / pieces as f32;
            samples.push(lerp_sample(&pair[0], &pair[1], t));
            if has_pressure {
                let (a, b) = (stroke.pressure[i], stroke.pressure[i + 1]);
                pressure.push(a + (b - a) * t);
            }
        }
    }
    if let Some(last) = stroke.samples.last() {
        samples.push(*last);
        if has_pressure && let Some(p) = stroke.pressure.last() {
            pressure.push(*p);
        }
    }
    let mut dense = stroke.clone();
    dense.samples = samples;
    dense.pressure = pressure;
    dense
}

/// The position along the stroke (its length so far) at each sample.
fn lengths_along(stroke: &Stroke) -> Vec<f32> {
    let mut at = 0.0;
    let mut previous = None;
    stroke
        .samples
        .iter()
        .map(|s| {
            if let Some(p) = previous {
                at += s.point.distance(p);
            }
            previous = Some(s.point);
            at
        })
        .collect()
}

/// The sample of `stroke` nearest to `p`, if it has any.
#[must_use]
pub fn nearest_sample(stroke: &Stroke, p: Point) -> Option<usize> {
    stroke
        .samples
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.point.distance(p).total_cmp(&b.1.point.distance(p)))
        .map(|(i, _)| i)
}

/// `stroke` with sample `held` dragged by `delta` and the samples along the stroke following it, out to `reach`
/// (measured along the stroke).
#[must_use]
pub fn pull(stroke: &Stroke, held: usize, delta: (f32, f32), reach: f32) -> Stroke {
    let mut out = stroke.clone();
    let lengths = lengths_along(stroke);
    let Some(&anchor) = lengths.get(held) else {
        return out;
    };
    for (sample, along) in out.samples.iter_mut().zip(&lengths) {
        let w = falloff((along - anchor).abs(), reach);
        sample.point = Point::new(sample.point.x + delta.0 * w, sample.point.y + delta.1 * w);
    }
    out
}

/// `stroke` with every point within `reach` of the tip at `tip` carried along by `delta`.
#[must_use]
pub fn push(stroke: &Stroke, tip: Point, delta: (f32, f32), reach: f32) -> Stroke {
    let mut out = stroke.clone();
    for sample in &mut out.samples {
        let w = falloff(sample.point.distance(tip), reach);
        sample.point = Point::new(sample.point.x + delta.0 * w, sample.point.y + delta.1 * w);
    }
    out
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::builder::StrokeBuilder;
    use crate::document::Pen;
    use crate::geometry::Color;

    /// A horizontal line along y = 0 from x = 0 to x = 100 with a point every 50 units, so it needs densifying.
    fn sparse() -> Stroke {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(2.0), Color::BLACK, 1.0, 0.0)
            .with_id(Uuid::from_u128(5));
        for i in 0..=2 {
            b.push(
                Point::new(i as f32 * 50.0, 0.0),
                Some(i as f32 / 2.0),
                f64::from(i) * 0.1,
            );
        }
        b.finish()
    }

    fn near(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-3
    }

    #[test]
    fn the_follow_fades_from_everything_at_the_center_to_nothing_at_the_edge() {
        assert!((falloff(0.0, 10.0) - 1.0).abs() < 1e-6);
        assert!((falloff(5.0, 10.0) - 0.5).abs() < 1e-6);
        assert_eq!(falloff(10.0, 10.0), 0.0);
        assert_eq!(falloff(99.0, 10.0), 0.0);
        assert_eq!(falloff(1.0, 0.0), 0.0, "no reach, no follow");
        let mut last = 1.0;
        for step in 1..=10 {
            let w = falloff(step as f32, 10.0);
            assert!(w <= last + 1e-6, "never grows with distance");
            last = w;
        }
    }

    #[test]
    fn densifying_adds_points_without_changing_the_line() {
        let dense = densify(&sparse(), 10.0);
        assert!(dense.samples.len() >= 11, "{}", dense.samples.len());
        assert_eq!(dense.pressure.len(), dense.samples.len());
        for pair in dense.samples.windows(2) {
            assert!(pair[0].point.distance(pair[1].point) <= 10.0 + 1e-3);
        }
        assert!(dense.samples.iter().all(|s| s.point.y.abs() < 1e-5));
        assert!(near(dense.samples[0].point, Point::new(0.0, 0.0)));
        assert!(near(
            dense.samples.last().expect("sample").point,
            Point::new(100.0, 0.0)
        ));
        assert_eq!(dense.id, Uuid::from_u128(5), "still the same stroke");
        let mid = dense.pressure[dense.samples.len() / 2];
        assert!(
            (mid - 0.5).abs() < 0.06,
            "pressure is carried through: {mid}"
        );
    }

    #[test]
    fn a_stroke_with_nothing_to_densify_comes_back_as_it_was() {
        let mut path_only = sparse();
        path_only.samples.clear();
        assert_eq!(densify(&path_only, 5.0), path_only);
        assert_eq!(densify(&sparse(), 0.0), sparse());
    }

    #[test]
    fn pulling_moves_the_held_point_all_the_way_and_the_ends_not_at_all() {
        let dense = densify(&sparse(), 5.0);
        let held = nearest_sample(&dense, Point::new(50.0, 0.0)).expect("sample");
        let pulled = pull(&dense, held, (0.0, 20.0), 30.0);
        assert!(near(pulled.samples[held].point, Point::new(50.0, 20.0)));
        assert!(
            near(pulled.samples[0].point, Point::new(0.0, 0.0)),
            "outside the reach"
        );
        assert!(near(
            pulled.samples.last().expect("sample").point,
            Point::new(100.0, 0.0)
        ));
        let before = pulled.samples[held - 3].point.y;
        let after = pulled.samples[held - 6].point.y;
        assert!(
            before > after && after >= 0.0,
            "it tapers away: {before} {after}"
        );
        assert_eq!(pulled.pressure, dense.pressure, "only the shape changes");
        assert_eq!(pulled.id, dense.id);
    }

    #[test]
    fn pulling_follows_the_stroke_not_the_straight_distance() {
        // An out and back stroke: the two ends are close in space but far apart along the stroke.
        let mut b = StrokeBuilder::new(Pen::Pen, Some(2.0), Color::BLACK, 1.0, 0.0);
        for (i, p) in [(0.0, 0.0), (100.0, 0.0), (100.0, 4.0), (0.0, 4.0)]
            .iter()
            .enumerate()
        {
            b.push(Point::new(p.0, p.1), None, i as f64 * 0.1);
        }
        let hairpin = densify(&b.finish(), 4.0);
        let held = nearest_sample(&hairpin, Point::new(0.0, 0.0)).expect("sample");
        let pulled = pull(&hairpin, held, (0.0, -30.0), 20.0);
        let last = pulled.samples.last().expect("sample").point;
        assert!(
            near(last, Point::new(0.0, 4.0)),
            "the other end is 200 along the stroke, so it stays"
        );
    }

    #[test]
    fn pushing_carries_what_is_under_the_tip_and_leaves_the_rest() {
        let dense = densify(&sparse(), 5.0);
        let pushed = push(&dense, Point::new(50.0, 0.0), (0.0, 10.0), 20.0);
        let center = nearest_sample(&dense, Point::new(50.0, 0.0)).expect("sample");
        assert!(near(pushed.samples[center].point, Point::new(50.0, 10.0)));
        assert!(near(pushed.samples[0].point, Point::new(0.0, 0.0)));
        for (a, b) in pushed.samples.iter().zip(&dense.samples) {
            assert!((a.point.x - b.point.x).abs() < 1e-5, "only moved sideways");
            assert!(a.point.y >= -1e-5 && a.point.y <= 10.0 + 1e-5);
        }
        let none = push(&dense, Point::new(500.0, 500.0), (9.0, 9.0), 20.0);
        assert_eq!(none, dense, "a tip nowhere near it changes nothing");
    }

    #[test]
    fn nearest_sample_of_an_empty_stroke_is_nothing() {
        let mut empty = sparse();
        empty.samples.clear();
        assert_eq!(nearest_sample(&empty, Point::new(0.0, 0.0)), None);
        assert_eq!(pull(&empty, 0, (1.0, 1.0), 10.0), empty);
    }
}
