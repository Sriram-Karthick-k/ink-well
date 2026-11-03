//! Slicing: cutting a stroke in two (or more) where a blade crosses it.

use uuid::Uuid;

use crate::document::{RawSample, Stroke};
use crate::geometry::Point;

/// Where the segment `a` to `b` crosses the segment `c` to `d`, as how far along `a` to `b` it is (`0.0..=1.0`).
fn crossing(a: Point, b: Point, c: Point, d: Point) -> Option<f32> {
    let (r, s) = ((b.x - a.x, b.y - a.y), (d.x - c.x, d.y - c.y));
    let denominator = r.0 * s.1 - r.1 * s.0;
    if denominator.abs() < 1e-9 {
        return None;
    }
    let (qp_x, qp_y) = (c.x - a.x, c.y - a.y);
    let t = (qp_x * s.1 - qp_y * s.0) / denominator;
    let u = (qp_x * r.1 - qp_y * r.0) / denominator;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

pub(crate) fn lerp_sample(a: &RawSample, b: &RawSample, t: f32) -> RawSample {
    let mut attrs = [0_u16; 4];
    for (i, attr) in attrs.iter_mut().enumerate() {
        let (x, y) = (f32::from(a.attrs[i]), f32::from(b.attrs[i]));
        *attr = (x + (y - x) * t).round().clamp(0.0, 65_535.0) as u16;
    }
    RawSample {
        point: Point::new(
            a.point.x + (b.point.x - a.point.x) * t,
            a.point.y + (b.point.y - a.point.y) * t,
        ),
        attrs,
    }
}

/// How far along `a` to `b` the point on that segment nearest to `p` is (`0.0..=1.0`), and how far `p` is from it.
fn nearest_on(a: Point, b: Point, p: Point) -> (f32, f32) {
    let (abx, aby) = (b.x - a.x, b.y - a.y);
    let length_squared = abx * abx + aby * aby;
    let t = if length_squared <= f32::EPSILON {
        0.0
    } else {
        (((p.x - a.x) * abx + (p.y - a.y) * aby) / length_squared).clamp(0.0, 1.0)
    };
    (t, p.distance(Point::new(a.x + abx * t, a.y + aby * t)))
}

/// For two segments that do not cross: how close they come, and how far along `a` to `b` the point of `a` to `b`
/// that is closest is. The closest points of two segments that do not cross are always at an end of one of them.
fn closest_approach(a: Point, b: Point, c: Point, d: Point) -> (f32, f32) {
    let mut best = (f32::INFINITY, 0.0);
    for end in [c, d] {
        let (t, distance) = nearest_on(a, b, end);
        if distance < best.0 {
            best = (distance, t);
        }
    }
    for (end, t) in [(a, 0.0), (b, 1.0)] {
        let (_, distance) = nearest_on(c, d, end);
        if distance < best.0 {
            best = (distance, t);
        }
    }
    best
}

/// A place where the blade cuts the stroke: in segment `segment` (from sample `segment` to the next), `t` of the
/// way along it, and `along` the stroke's length from its start.
#[derive(Debug, Clone, Copy)]
struct Cut {
    segment: usize,
    t: f32,
    along: f32,
}

/// Every place the `blade` cuts `stroke`: where it crosses the stroke's line, and where it comes within `reach`
/// of the line without crossing it (it ends inside the thickness of the stroke, or only grazes it), at the nearest
/// point. Of the latter only the nearest of those within two reaches of each other, and none within two reaches of
/// a crossing, so a blade through the middle of a thick stroke cuts it once.
fn cuts_of(stroke: &Stroke, blade: &[Point], reach: f32) -> Vec<Cut> {
    let points: Vec<Point> = stroke.samples.iter().map(|s| s.point).collect();
    let mut start = Vec::with_capacity(points.len());
    let mut total = 0.0;
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            total += points[i - 1].distance(*p);
        }
        start.push(total);
    }
    let mut cuts = Vec::new();
    let mut grazes: Vec<(f32, Cut)> = Vec::new();
    for (i, pair) in points.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        let length = a.distance(b);
        for edge in blade.windows(2) {
            if let Some(t) = crossing(a, b, edge[0], edge[1]) {
                cuts.push(Cut {
                    segment: i,
                    t,
                    along: start[i] + t * length,
                });
            } else if reach > 0.0 {
                let (distance, t) = closest_approach(a, b, edge[0], edge[1]);
                if distance <= reach {
                    grazes.push((
                        distance,
                        Cut {
                            segment: i,
                            t,
                            along: start[i] + t * length,
                        },
                    ));
                }
            }
        }
    }
    grazes.sort_by(|x, y| x.0.total_cmp(&y.0));
    for (_, graze) in grazes {
        if cuts
            .iter()
            .all(|c| (c.along - graze.along).abs() > 2.0 * reach)
        {
            cuts.push(graze);
        }
    }
    cuts
}

/// A piece of a stroke while it is being cut: its samples, and their pressures if the stroke has them.
struct Piece {
    samples: Vec<RawSample>,
    pressure: Vec<f32>,
}

impl Piece {
    fn new() -> Self {
        Self {
            samples: Vec::new(),
            pressure: Vec::new(),
        }
    }

    fn push(&mut self, sample: RawSample, pressure: Option<f32>) {
        self.samples.push(sample);
        if let Some(p) = pressure {
            self.pressure.push(p);
        }
    }

    /// A piece needs at least two points, in different places, to be a stroke.
    fn is_stroke(&self) -> bool {
        self.samples.len() >= 2
            && self
                .samples
                .windows(2)
                .any(|w| w[0].point.distance(w[1].point) > 1e-4)
    }
}

/// Cuts `stroke` wherever the `blade` (a path of at least two points) crosses it, or reaches into it: a blade
/// that stops inside the thickness of a thick stroke, or only touches its edge, cuts it at the nearest point of
/// its line, as it looks as if it did. The pieces are in drawing order; the first keeps the stroke's id and the
/// others get new ones from `new_id`. Returns `None` if the blade does not cut the stroke into at least two
/// pieces (it misses, or only clips a speck off an end).
///
/// A stroke that only has a stored path and no samples cannot be cut.
pub fn slice_stroke(
    stroke: &Stroke,
    blade: &[Point],
    mut new_id: impl FnMut() -> Uuid,
) -> Option<Vec<Stroke>> {
    if stroke.samples.len() < 2 || blade.len() < 2 {
        return None;
    }
    let reach = stroke.width.unwrap_or(1.0) / 2.0;
    let mut cuts_in: Vec<Vec<f32>> = vec![Vec::new(); stroke.samples.len() - 1];
    for cut in cuts_of(stroke, blade, reach) {
        cuts_in[cut.segment].push(cut.t);
    }
    let has_pressure = stroke.pressure.len() == stroke.samples.len();
    let pressure_at = |i: usize| has_pressure.then(|| stroke.pressure[i]);
    let mut pieces = vec![Piece::new()];
    let mut cut = false;
    for (i, pair) in stroke.samples.windows(2).enumerate() {
        let (from, to) = (&pair[0], &pair[1]);
        let current = pieces.len() - 1;
        pieces[current].push(*from, pressure_at(i));
        let mut hits = std::mem::take(&mut cuts_in[i]);
        hits.sort_by(f32::total_cmp);
        hits.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        for t in hits {
            let at = lerp_sample(from, to, t);
            let pressure = pressure_at(i)
                .zip(pressure_at(i + 1))
                .map(|(a, b)| a + (b - a) * t);
            let end = pieces.len() - 1;
            pieces[end].push(at, pressure);
            let mut next = Piece::new();
            next.push(at, pressure);
            pieces.push(next);
            cut = true;
        }
    }
    let last = stroke.samples.len() - 1;
    let end = pieces.len() - 1;
    pieces[end].push(stroke.samples[last], pressure_at(last));
    if !cut {
        return None;
    }

    let mut result = Vec::new();
    for piece in pieces.into_iter().filter(Piece::is_stroke) {
        let mut part = stroke.clone();
        part.samples = piece.samples;
        part.pressure = piece.pressure;
        part.path = Vec::new();
        part.id = if result.is_empty() {
            stroke.id
        } else {
            new_id()
        };
        result.push(part);
    }
    (result.len() >= 2).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::StrokeBuilder;
    use crate::document::Pen;
    use crate::geometry::Color;

    /// A horizontal line from x = 0 to x = 100 with a point every 10 units.
    fn line(pressure: bool) -> Stroke {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.0)
            .with_id(Uuid::from_u128(1));
        for i in 0..=10 {
            let p = pressure.then_some(i as f32 / 10.0);
            b.push(Point::new(i as f32 * 10.0, 0.0), p, f64::from(i) * 0.01);
        }
        b.finish()
    }

    fn ids() -> impl FnMut() -> Uuid {
        let mut n = 100;
        move || {
            n += 1;
            Uuid::from_u128(n)
        }
    }

    fn vertical(x: f32) -> [Point; 2] {
        [Point::new(x, -10.0), Point::new(x, 10.0)]
    }

    #[test]
    fn a_blade_across_a_line_cuts_it_in_two_at_the_crossing() {
        let pieces = slice_stroke(&line(false), &vertical(45.0), ids()).expect("cut");
        assert_eq!(pieces.len(), 2);
        let end_of_first = pieces[0].samples.last().expect("sample").point;
        let start_of_second = pieces[1].samples[0].point;
        assert!((end_of_first.x - 45.0).abs() < 1e-3);
        assert_eq!(end_of_first, start_of_second, "the cut leaves no gap");
        assert_eq!(pieces[0].samples[0].point, Point::new(0.0, 0.0));
        assert_eq!(
            pieces[1].samples.last().expect("sample").point,
            Point::new(100.0, 0.0)
        );
    }

    #[test]
    fn the_first_piece_keeps_the_id_and_the_others_get_new_ones() {
        let pieces = slice_stroke(&line(false), &vertical(45.0), ids()).expect("cut");
        assert_eq!(pieces[0].id, Uuid::from_u128(1));
        assert_eq!(pieces[1].id, Uuid::from_u128(101));
        assert_eq!(pieces[0].pen, Pen::Pen);
        assert_eq!(pieces[1].width, Some(3.0));
    }

    #[test]
    fn speed_and_pressure_are_interpolated_at_the_cut() {
        let stroke = line(true);
        let pieces = slice_stroke(&stroke, &vertical(45.0), ids()).expect("cut");
        let at_cut = pieces[0].pressure.last().copied().expect("pressure");
        assert!((at_cut - 0.45).abs() < 1e-3, "{at_cut}");
        assert_eq!(pieces[0].pressure.len(), pieces[0].samples.len());
        assert_eq!(pieces[1].pressure.len(), pieces[1].samples.len());
        let between = pieces[1].samples[0].attrs[1];
        assert!(between > stroke.samples[4].attrs[1] || stroke.samples[4].attrs[1] == between);
    }

    #[test]
    fn a_blade_that_misses_changes_nothing() {
        assert!(slice_stroke(&line(false), &vertical(250.0), ids()).is_none());
        let above = [Point::new(0.0, 20.0), Point::new(100.0, 20.0)];
        assert!(slice_stroke(&line(false), &above, ids()).is_none());
    }

    #[test]
    fn a_blade_that_crosses_twice_makes_three_pieces() {
        let blade = [
            Point::new(25.0, -10.0),
            Point::new(25.0, 10.0),
            Point::new(75.0, 10.0),
            Point::new(75.0, -10.0),
        ];
        let pieces = slice_stroke(&line(false), &blade, ids()).expect("cut");
        assert_eq!(pieces.len(), 3);
        let xs: Vec<f32> = pieces.iter().map(|p| p.samples[0].point.x).collect();
        assert!((xs[1] - 25.0).abs() < 1e-3 && (xs[2] - 75.0).abs() < 1e-3);
    }

    #[test]
    fn a_cut_exactly_through_a_point_does_not_make_an_empty_piece() {
        let pieces = slice_stroke(&line(false), &vertical(50.0), ids()).expect("cut");
        assert_eq!(pieces.len(), 2);
        assert!(pieces.iter().all(|p| p.samples.len() >= 2));
    }

    #[test]
    fn a_cut_at_the_very_end_cuts_nothing() {
        assert!(
            slice_stroke(&line(false), &vertical(100.0), ids()).is_none(),
            "only a speck would have been cut off"
        );
        assert!(slice_stroke(&line(false), &vertical(0.0), ids()).is_none());
    }

    #[test]
    fn a_stroke_without_samples_cannot_be_cut() {
        let mut stroke = line(false);
        stroke.samples.clear();
        stroke.path = vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0)];
        assert!(slice_stroke(&stroke, &vertical(50.0), ids()).is_none());
    }

    /// The same line, drawn with a pen 20 wide: it reaches 10 either side of its line.
    fn thick() -> Stroke {
        let mut stroke = line(false);
        stroke.width = Some(20.0);
        stroke
    }

    #[test]
    fn a_blade_that_ends_inside_the_thickness_of_a_stroke_cuts_it_at_the_nearest_point() {
        let blade = [Point::new(45.0, 40.0), Point::new(45.0, 6.0)];
        assert!(
            slice_stroke(&line(false), &blade, ids()).is_none(),
            "a thin line is not reached from 6 away"
        );
        let pieces = slice_stroke(&thick(), &blade, ids()).expect("cut");
        assert_eq!(pieces.len(), 2);
        let end_of_first = pieces[0].samples.last().expect("sample").point;
        assert!((end_of_first.x - 45.0).abs() < 1e-3 && end_of_first.y.abs() < 1e-3);
    }

    #[test]
    fn a_blade_that_stops_short_of_the_edge_does_not_cut() {
        let blade = [Point::new(45.0, 40.0), Point::new(45.0, 14.0)];
        assert!(slice_stroke(&thick(), &blade, ids()).is_none());
    }

    #[test]
    fn a_blade_through_the_middle_of_a_thick_stroke_cuts_it_once() {
        // Across a point of the line, and so near the two segments either side of it as well.
        for x in [45.0, 50.0] {
            let pieces = slice_stroke(&thick(), &vertical(x), ids()).expect("cut");
            assert_eq!(pieces.len(), 2, "x = {x}");
        }
    }

    #[test]
    fn a_blade_reaching_into_a_stroke_in_two_places_far_apart_cuts_it_twice() {
        let blade = [
            Point::new(20.0, 40.0),
            Point::new(20.0, 5.0),
            Point::new(20.0, 40.0),
            Point::new(80.0, 40.0),
            Point::new(80.0, 5.0),
        ];
        let pieces = slice_stroke(&thick(), &blade, ids()).expect("cut");
        assert_eq!(pieces.len(), 3);
    }

    #[test]
    fn a_blade_that_touches_the_cap_beyond_the_end_cuts_nothing() {
        let blade = [Point::new(105.0, 20.0), Point::new(105.0, 0.0)];
        assert!(slice_stroke(&thick(), &blade, ids()).is_none());
    }

    #[test]
    fn a_diagonal_blade_works_too() {
        let blade = [Point::new(30.0, -30.0), Point::new(60.0, 30.0)];
        let pieces = slice_stroke(&line(false), &blade, ids()).expect("cut");
        assert_eq!(pieces.len(), 2);
        assert!((pieces[0].samples.last().expect("sample").point.x - 45.0).abs() < 1e-3);
    }
}
