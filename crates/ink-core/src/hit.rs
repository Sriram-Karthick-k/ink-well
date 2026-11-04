//! Which strokes a point touches, for erasing and selecting.

use crate::document::{Pen, Stroke};
use crate::geometry::Point;

/// The shortest distance from `p` to the segment `a`..`b`.
fn segment_distance(p: Point, a: Point, b: Point) -> f32 {
    let (abx, aby) = (b.x - a.x, b.y - a.y);
    let length_squared = abx * abx + aby * aby;
    if length_squared <= f32::EPSILON {
        return p.distance(a);
    }
    let t = (((p.x - a.x) * abx + (p.y - a.y) * aby) / length_squared).clamp(0.0, 1.0);
    p.distance(Point::new(a.x + abx * t, a.y + aby * t))
}

/// `true` when a disc of `radius` around `p` touches `stroke`, counting half the stroke's own width. A region (a
/// stroke of the filling pen) is touched anywhere inside it as well as along its edge.
#[must_use]
pub fn touches(stroke: &Stroke, p: Point, radius: f32) -> bool {
    let reach = radius + stroke.width.unwrap_or(1.0) / 2.0;
    let points: Vec<Point> = stroke.points().collect();
    let Some((&first, rest)) = points.split_first() else {
        return false;
    };
    if rest.is_empty() {
        return first.distance(p) <= reach;
    }
    let region = stroke.pen == Pen::Fill && points.len() >= 3;
    if region && point_in_polygon(p, &points) {
        return true;
    }
    let mut previous = first;
    for &next in rest {
        if segment_distance(p, previous, next) <= reach {
            return true;
        }
        previous = next;
    }
    // A region is closed by a line from its last point back to its first.
    region && segment_distance(p, previous, first) <= reach
}

/// `true` if `p` is inside the closed `polygon` (the last point joins the first).
#[must_use]
pub fn point_in_polygon(p: Point, polygon: &[Point]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &current in polygon {
        if (current.y > p.y) != (previous.y > p.y)
            && p.x
                < (previous.x - current.x) * (p.y - current.y) / (previous.y - current.y)
                    + current.x
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

/// How much of the stroke is inside `polygon`, from `0.0` to `1.0`, counted in points.
#[must_use]
pub fn fraction_inside(stroke: &Stroke, polygon: &[Point]) -> f32 {
    let (mut total, mut inside) = (0_usize, 0_usize);
    for p in stroke.points() {
        total += 1;
        if point_in_polygon(p, polygon) {
            inside += 1;
        }
    }
    if total == 0 {
        return 0.0;
    }
    inside as f32 / total as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::StrokeBuilder;
    use crate::document::Pen;
    use crate::geometry::Color;

    fn line(width: f32) -> Stroke {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(width), Color::BLACK, 1.0, 0.0);
        for i in 0..=10 {
            b.push(Point::new(i as f32 * 10.0, 0.0), None, f64::from(i) * 0.01);
        }
        b.finish()
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
    fn points_are_inside_or_outside_a_polygon() {
        let poly = square(0.0, 0.0, 10.0, 10.0);
        assert!(point_in_polygon(Point::new(5.0, 5.0), &poly));
        assert!(!point_in_polygon(Point::new(15.0, 5.0), &poly));
        assert!(!point_in_polygon(Point::new(5.0, -1.0), &poly));
        assert!(
            !point_in_polygon(Point::new(5.0, 5.0), &poly[..2]),
            "two points enclose nothing"
        );
        let notch = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(5.0, 4.0),
            Point::new(0.0, 10.0),
        ];
        assert!(
            !point_in_polygon(Point::new(5.0, 8.0), &notch),
            "in the notch"
        );
        assert!(point_in_polygon(Point::new(5.0, 2.0), &notch));
    }

    #[test]
    fn a_stroke_is_selected_by_how_much_of_it_is_inside() {
        let stroke = line(2.0);
        assert!((fraction_inside(&stroke, &square(-5.0, -5.0, 105.0, 5.0)) - 1.0).abs() < 1e-6);
        let half = fraction_inside(&stroke, &square(-5.0, -5.0, 45.0, 5.0));
        assert!(half > 0.4 && half < 0.6, "{half}");
        assert!(fraction_inside(&stroke, &square(200.0, 0.0, 300.0, 10.0)).abs() < 1e-6);
    }

    #[test]
    fn a_disc_on_the_line_touches_it() {
        assert!(touches(&line(2.0), Point::new(50.0, 0.0), 1.0));
    }

    #[test]
    fn the_reach_is_the_radius_plus_half_the_width() {
        let stroke = line(10.0);
        assert!(touches(&stroke, Point::new(50.0, 14.0), 10.0));
        assert!(!touches(&stroke, Point::new(50.0, 16.0), 10.0));
    }

    #[test]
    fn beyond_the_ends_does_not_touch() {
        assert!(!touches(&line(2.0), Point::new(130.0, 0.0), 5.0));
    }

    fn region(points: &[(f32, f32)]) -> Stroke {
        let mut b = StrokeBuilder::new(Pen::Fill, None, Color::BLACK, 1.0, 0.0);
        b.replace_with(points.iter().map(|&(x, y)| Point::new(x, y)).collect());
        b.finish()
    }

    #[test]
    fn a_region_is_touched_anywhere_inside_it_and_not_outside() {
        let square = region(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]);
        assert!(touches(&square, Point::new(50.0, 50.0), 1.0), "the middle");
        assert!(!touches(&square, Point::new(150.0, 50.0), 1.0), "outside");
        assert!(
            touches(&square, Point::new(50.0, -0.5), 1.0),
            "its edge, a touch outside it"
        );
    }

    #[test]
    fn the_edge_a_region_is_closed_by_is_part_of_it() {
        let square = region(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]);
        assert!(touches(&square, Point::new(-0.5, 50.0), 1.0));
    }

    #[test]
    fn a_hole_in_a_region_is_not_touched() {
        let ring = region(&[
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 100.0),
            (0.0, 100.0),
            (0.0, 0.0),
            (30.0, 30.0),
            (30.0, 70.0),
            (70.0, 70.0),
            (70.0, 30.0),
            (30.0, 30.0),
        ]);
        assert!(touches(&ring, Point::new(10.0, 50.0), 1.0), "in the ring");
        assert!(
            !touches(&ring, Point::new(50.0, 50.0), 1.0),
            "in the hole, away from the edges"
        );
    }

    #[test]
    fn a_stroke_that_is_not_a_region_is_not_touched_inside_its_outline() {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(2.0), Color::BLACK, 1.0, 0.0);
        b.replace_with(vec![
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            Point::new(100.0, 100.0),
            Point::new(0.0, 100.0),
            Point::new(0.0, 0.0),
        ]);
        assert!(!touches(&b.finish(), Point::new(50.0, 50.0), 1.0));
    }

    #[test]
    fn a_stroke_with_no_points_is_never_touched() {
        let mut stroke = line(2.0);
        stroke.samples.clear();
        stroke.path.clear();
        assert!(!touches(&stroke, Point::new(0.0, 0.0), 100.0));
    }
}
