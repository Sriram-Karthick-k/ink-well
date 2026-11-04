//! Filling a region: the triangles of the inside of a stroke drawn with the Fill pen.
//!
//! A Fill stroke is a region, not a line: its points are the edge of a polygon that is closed by a line from the last
//! point back to the first, and everything inside is covered. A path that crosses itself is filled where it winds an
//! odd number of times (even and odd, as a loop of rope would be), and a path that goes in along a line and comes back
//! along it (a bridge to a hole) fills the hole, which is how a letter with a hole in it is one stroke.

use ink_core::Point;
use lyon_tessellation::geom::point;
use lyon_tessellation::path::Path;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
};

/// The triangles that fill the polygon `points`, as `x, y` pairs three to a triangle, in the units of `points`. Empty
/// when there is no inside: fewer than three points, or all of them on one line.
#[must_use]
pub fn triangulate(points: &[Point]) -> Vec<f32> {
    if points.len() < 3 {
        return Vec::new();
    }
    let mut path = Path::builder();
    path.begin(point(points[0].x, points[0].y));
    for p in &points[1..] {
        path.line_to(point(p.x, p.y));
    }
    path.end(true);
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let options = FillOptions::default()
        .with_fill_rule(FillRule::EvenOdd)
        // Smooth curves are already many short lines; a tolerance of a hundredth of a unit is far below a pixel.
        .with_tolerance(0.01);
    let result = FillTessellator::new().tessellate_path(
        &path.build(),
        &options,
        &mut BuffersBuilder::new(&mut buffers, |v: FillVertex<'_>| {
            let p = v.position();
            [p.x, p.y]
        }),
    );
    if result.is_err() {
        return Vec::new();
    }
    buffers
        .indices
        .iter()
        .flat_map(|&i| buffers.vertices[i as usize])
        .collect()
}

/// The area the triangles of `triangles` cover, which is the area of what they fill.
#[must_use]
pub fn area(triangles: &[f32]) -> f32 {
    let (triangles, _) = triangles.as_chunks::<6>();
    triangles
        .iter()
        .map(|t| ((t[2] - t[0]) * (t[5] - t[1]) - (t[4] - t[0]) * (t[3] - t[1])).abs() / 2.0)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(side: f32) -> Vec<Point> {
        vec![
            Point::new(0.0, 0.0),
            Point::new(side, 0.0),
            Point::new(side, side),
            Point::new(0.0, side),
        ]
    }

    #[test]
    fn a_square_is_filled_with_triangles_that_add_up_to_its_area() {
        let triangles = triangulate(&square(10.0));
        assert_eq!(triangles.len() % 6, 0, "whole triangles");
        assert!(
            (area(&triangles) - 100.0).abs() < 1e-3,
            "{}",
            area(&triangles)
        );
    }

    #[test]
    fn a_polygon_that_is_not_convex_is_filled_only_inside() {
        // An L: a 10 by 10 square without its top right quarter.
        let l = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 5.0),
            Point::new(5.0, 5.0),
            Point::new(5.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        assert!((area(&triangulate(&l)) - 75.0).abs() < 1e-3);
    }

    #[test]
    fn closing_it_by_hand_makes_no_difference() {
        let open = square(10.0);
        let mut closed = square(10.0);
        closed.push(closed[0]);
        assert!((area(&triangulate(&open)) - area(&triangulate(&closed))).abs() < 1e-3);
    }

    #[test]
    fn a_hole_joined_by_a_bridge_stays_empty() {
        // A 10 square with a 4 square hole in the middle, joined to the outside by a line that is gone along twice.
        let (outer, hole) = (square(10.0), [3.0_f32, 7.0]);
        let ring = vec![
            outer[0],
            outer[1],
            outer[2],
            outer[3],
            outer[0],
            Point::new(hole[0], hole[0]),
            Point::new(hole[0], hole[1]),
            Point::new(hole[1], hole[1]),
            Point::new(hole[1], hole[0]),
            Point::new(hole[0], hole[0]),
        ];
        let filled = area(&triangulate(&ring));
        assert!(
            (filled - 84.0).abs() < 1e-2,
            "100 less a hole of 16: {filled}"
        );
    }

    #[test]
    fn nothing_that_has_no_inside_is_filled() {
        assert_eq!(triangulate(&[]), Vec::<f32>::new());
        assert_eq!(
            triangulate(&[Point::new(0.0, 0.0), Point::new(5.0, 5.0)]),
            Vec::<f32>::new()
        );
        let line = vec![
            Point::new(0.0, 0.0),
            Point::new(5.0, 5.0),
            Point::new(10.0, 10.0),
        ];
        assert!(area(&triangulate(&line)) < 1e-4, "a line has no area");
    }
}
