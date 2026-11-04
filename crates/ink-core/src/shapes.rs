//! Shapes: ready-made figures that can be put on a drawing, and the figures a person makes themselves.
//!
//! A shape is a few paths in a box whose longer side is `1.0`, centered on the origin, with the y axis up like the
//! canvas. Putting one on a drawing scales it to a size, moves it to a place and turns each path into an ordinary
//! stroke, so it can be selected, moved, resized, colored and undone like anything drawn by hand.
//!
//! The built-in shapes take the pen, size and color in use. A shape made from strokes (see [`Shape::from_strokes`])
//! keeps the pen, color and relative width of each of them, so what was drawn comes back as it was drawn.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::builder::StrokeBuilder;
use crate::document::{Pen, Stroke};
use crate::geometry::{Color, Point};

/// How the strokes of a built-in shape are drawn: the pen, size and color the person is using, and whether the
/// inside is filled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub pen: Pen,
    pub width: Option<f32>,
    pub color: Color,
    /// The color of the inside of a closed path, or `None` to leave it empty.
    pub fill: Option<Color>,
    /// Whether the outline is drawn. A filled shape may have none.
    pub outline: bool,
}

/// The look a path of a shape made by hand keeps.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ink {
    pub pen: Pen,
    /// Red, green, blue and alpha, each `0.0..=1.0`.
    pub color: [f32; 4],
    /// The width as a part of the shape's longer side, so the shape keeps its look at any size.
    pub width: Option<f32>,
}

/// One line of a shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapePath {
    pub points: Vec<[f32; 2]>,
    /// How it was drawn, for a shape made by hand; `None` for a built-in shape, which is drawn in the pen in use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ink: Option<Ink>,
}

/// A figure: its paths and what it is called.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    /// A name that does not change when the shape is renamed, to find it by.
    pub id: String,
    pub name: String,
    pub paths: Vec<ShapePath>,
}

impl ShapePath {
    fn plain(points: Vec<[f32; 2]>) -> Self {
        Self { points, ink: None }
    }
}

impl Shape {
    fn built_in(id: &str, name: &str, paths: Vec<Vec<[f32; 2]>>) -> Self {
        Self {
            id: id.to_owned(),
            name: name.to_owned(),
            paths: paths.into_iter().map(ShapePath::plain).collect(),
        }
    }

    /// `true` for a shape the app provides, which a person cannot change.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        self.paths.iter().all(|p| p.ink.is_none())
    }

    /// The width and height of the box the shape fills, the longer of them being `1.0`.
    #[must_use]
    pub fn extent(&self) -> (f32, f32) {
        let mut bounds: Option<[f32; 4]> = None;
        for [x, y] in self.paths.iter().flat_map(|p| p.points.iter().copied()) {
            bounds = Some(match bounds {
                None => [x, y, x, y],
                Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
            });
        }
        bounds.map_or((0.0, 0.0), |[x0, y0, x1, y1]| (x1 - x0, y1 - y0))
    }

    /// The strokes that draw the shape with its center at `center` and its longer side `size` long. `style` is what the
    /// built-in paths are drawn with; `next_id` gives each stroke its id.
    ///
    /// A closed built-in path that is to be filled gives a stroke of the filling pen under the one of its outline, so
    /// the outline is on top. A path of a shape made by hand is drawn as it was.
    pub fn to_strokes(
        &self,
        center: Point,
        size: f32,
        style: &Style,
        next_id: &mut dyn FnMut() -> Uuid,
    ) -> Vec<Stroke> {
        let place = |path: &ShapePath| -> Vec<Point> {
            path.points
                .iter()
                .map(|&[x, y]| Point::new(center.x + x * size, center.y + y * size))
                .collect()
        };
        let mut strokes = Vec::new();
        for path in self.paths.iter().filter(|path| path.points.len() >= 2) {
            if let Some(ink) = path.ink {
                let color = Color::new(ink.color[0], ink.color[1], ink.color[2], ink.color[3]);
                strokes.push(stroke_of(
                    ink.pen,
                    ink.width.map(|w| w * size),
                    color,
                    place(path),
                    next_id,
                ));
                continue;
            }
            if let Some(fill) = style.fill
                && is_closed(&path.points)
            {
                strokes.push(stroke_of(Pen::Fill, None, fill, place(path), next_id));
            }
            if style.outline || style.fill.is_none() || !is_closed(&path.points) {
                strokes.push(stroke_of(
                    style.pen,
                    style.width,
                    style.color,
                    place(path),
                    next_id,
                ));
            }
        }
        strokes
    }

    /// A shape made of `strokes`, as they look now, or `None` when there is nothing to make it of.
    #[must_use]
    pub fn from_strokes(id: &str, name: &str, strokes: &[&Stroke]) -> Option<Self> {
        let [x0, y0, x1, y1] = geometry_bounds(strokes)?;
        let longer = (x1 - x0).max(y1 - y0);
        if longer <= f32::EPSILON {
            return None;
        }
        let (cx, cy) = (f32::midpoint(x0, x1), f32::midpoint(y0, y1));
        let paths: Vec<ShapePath> = strokes
            .iter()
            .filter_map(|stroke| {
                let normalized: Vec<[f32; 2]> = stroke
                    .geometry()
                    .into_iter()
                    .map(|p| [(p.x - cx) / longer, (p.y - cy) / longer])
                    .collect();
                let points = simplify(&normalized, SIMPLIFY);
                (points.len() >= 2).then(|| ShapePath {
                    points,
                    ink: Some(Ink {
                        pen: stroke.pen,
                        color: [
                            stroke.color.r,
                            stroke.color.g,
                            stroke.color.b,
                            stroke.color.a,
                        ],
                        width: stroke.width.map(|w| w / longer),
                    }),
                })
            })
            .collect();
        (!paths.is_empty()).then(|| Self {
            id: id.to_owned(),
            name: name.to_owned(),
            paths,
        })
    }
}

/// A stroke that draws exactly `points`.
fn stroke_of(
    pen: Pen,
    width: Option<f32>,
    color: Color,
    points: Vec<Point>,
    next_id: &mut dyn FnMut() -> Uuid,
) -> Stroke {
    let mut builder = StrokeBuilder::new(pen, width, color, 1.0, 0.0);
    builder.replace_with(points);
    builder.with_id(next_id()).finish()
}

/// `true` when a path ends where it begins, so it has an inside.
#[must_use]
pub fn is_closed(points: &[[f32; 2]]) -> bool {
    match (points.first(), points.last()) {
        (Some(a), Some(b)) if points.len() >= 4 => {
            (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
        }
        _ => false,
    }
}

/// The smallest box round the points of `strokes`, as left, bottom, right and top, without the width of the pens.
fn geometry_bounds(strokes: &[&Stroke]) -> Option<[f32; 4]> {
    strokes
        .iter()
        .flat_map(|s| s.points())
        .fold(None, |acc: Option<[f32; 4]>, p| {
            Some(match acc {
                None => [p.x, p.y, p.x, p.y],
                Some([x0, y0, x1, y1]) => [x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)],
            })
        })
}

/// How far, as a part of the shape's longer side, a point may be from the line that replaces it.
const SIMPLIFY: f32 = 0.0015;

/// The points of `line` without the ones that lie, to within `tolerance`, on the line between their neighbours
/// (Douglas and Peucker's way). The first and last are always kept.
#[must_use]
pub fn simplify(line: &[[f32; 2]], tolerance: f32) -> Vec<[f32; 2]> {
    if line.len() < 3 {
        return line.to_vec();
    }
    let mut keep = vec![false; line.len()];
    keep[0] = true;
    keep[line.len() - 1] = true;
    let mut stack = vec![(0, line.len() - 1)];
    while let Some((first, last)) = stack.pop() {
        let (mut far, mut farthest) = (0, 0.0_f32);
        for (i, p) in line.iter().enumerate().take(last).skip(first + 1) {
            let d = distance_to_segment(*p, line[first], line[last]);
            if d > farthest {
                (far, farthest) = (i, d);
            }
        }
        if farthest > tolerance {
            keep[far] = true;
            stack.push((first, far));
            stack.push((far, last));
        }
    }
    line.iter()
        .zip(keep)
        .filter_map(|(p, keep)| keep.then_some(*p))
        .collect()
}

fn distance_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length_squared = dx * dx + dy * dy;
    let t = if length_squared <= f32::EPSILON {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length_squared).clamp(0.0, 1.0)
    };
    let (nx, ny) = (a[0] + t * dx, a[1] + t * dy);
    ((p[0] - nx).powi(2) + (p[1] - ny).powi(2)).sqrt()
}

// --- the built-in shapes ------------------------------------------------------------------------------------------

/// A regular polygon with `n` corners, the first at the top, closed, filling a box `1.0` across.
fn polygon(n: usize, turn: f32) -> Vec<[f32; 2]> {
    let mut points: Vec<[f32; 2]> = (0..n)
        .map(|i| {
            let a =
                std::f32::consts::FRAC_PI_2 + turn + std::f32::consts::TAU * i as f32 / n as f32;
            [0.5 * a.cos(), 0.5 * a.sin()]
        })
        .collect();
    points.push(points[0]);
    fit(points)
}

/// `points` scaled and moved so that the box they fill is centered and its longer side is `1.0`.
fn fit(points: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for [x, y] in &points {
        (x0, y0, x1, y1) = (x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y));
    }
    let longer = (x1 - x0).max(y1 - y0).max(f32::EPSILON);
    let (cx, cy) = (f32::midpoint(x0, x1), f32::midpoint(y0, y1));
    points
        .into_iter()
        .map(|[x, y]| [(x - cx) / longer, (y - cy) / longer])
        .collect()
}

fn ellipse(rx: f32, ry: f32) -> Vec<[f32; 2]> {
    const STEPS: usize = 64;
    let mut points: Vec<[f32; 2]> = (0..STEPS)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / STEPS as f32;
            [rx * a.cos(), ry * a.sin()]
        })
        .collect();
    points.push(points[0]);
    points
}

fn rectangle(half_w: f32, half_h: f32) -> Vec<[f32; 2]> {
    vec![
        [-half_w, half_h],
        [half_w, half_h],
        [half_w, -half_h],
        [-half_w, -half_h],
        [-half_w, half_h],
    ]
}

fn rounded_rectangle(half_w: f32, half_h: f32, radius: f32) -> Vec<[f32; 2]> {
    const ARC: usize = 8;
    let mut points = Vec::new();
    // Each corner as a quarter of a circle, going round from the top right.
    for (cx, cy, from) in [
        (half_w - radius, half_h - radius, 0.0_f32),
        (-half_w + radius, half_h - radius, 90.0),
        (-half_w + radius, -half_h + radius, 180.0),
        (half_w - radius, -half_h + radius, 270.0),
    ] {
        for i in 0..=ARC {
            let a = (from + 90.0 * i as f32 / ARC as f32).to_radians();
            points.push([cx + radius * a.cos(), cy + radius * a.sin()]);
        }
    }
    points.push(points[0]);
    points
}

fn star(points_on_it: usize) -> Vec<[f32; 2]> {
    let n = points_on_it * 2;
    let mut points: Vec<[f32; 2]> = (0..n)
        .map(|i| {
            let r = if i % 2 == 0 { 0.5 } else { 0.2 };
            let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / n as f32;
            [r * a.cos(), r * a.sin()]
        })
        .collect();
    points.push(points[0]);
    fit(points)
}

fn heart() -> Vec<[f32; 2]> {
    let points: Vec<[f32; 2]> = (0..=64)
        .map(|i| {
            let t = std::f32::consts::TAU * i as f32 / 64.0;
            [
                16.0 * t.sin().powi(3),
                13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos(),
            ]
        })
        .collect();
    fit(points)
}

fn wave() -> Vec<[f32; 2]> {
    fit((0..=48)
        .map(|i| {
            let t = i as f32 / 48.0;
            [t, 0.12 * (t * std::f32::consts::TAU * 2.0).sin()]
        })
        .collect())
}

fn arrow_head(tip: [f32; 2], toward: f32) -> Vec<[f32; 2]> {
    vec![
        [tip[0] - toward * 0.2, 0.14],
        tip,
        [tip[0] - toward * 0.2, -0.14],
    ]
}

/// The shapes the app provides, in the order they are listed.
#[must_use]
pub fn built_in() -> Vec<Shape> {
    let mut shapes = lines_and_arrows();
    shapes.extend(boxes_and_rounds());
    shapes.extend(polygons());
    shapes.extend(symbols());
    shapes
}

fn lines_and_arrows() -> Vec<Shape> {
    vec![
        Shape::built_in("line", "Line", vec![vec![[-0.5, 0.0], [0.5, 0.0]]]),
        Shape::built_in(
            "arrow",
            "Arrow",
            vec![vec![[-0.5, 0.0], [0.5, 0.0]], arrow_head([0.5, 0.0], 1.0)],
        ),
        Shape::built_in(
            "double_arrow",
            "Double arrow",
            vec![
                vec![[-0.5, 0.0], [0.5, 0.0]],
                arrow_head([0.5, 0.0], 1.0),
                arrow_head([-0.5, 0.0], -1.0),
            ],
        ),
        Shape::built_in("wave", "Wave", vec![wave()]),
    ]
}

fn boxes_and_rounds() -> Vec<Shape> {
    vec![
        Shape::built_in("rectangle", "Rectangle", vec![rectangle(0.5, 0.375)]),
        Shape::built_in("square", "Square", vec![rectangle(0.5, 0.5)]),
        Shape::built_in(
            "rounded_rectangle",
            "Rounded rectangle",
            vec![rounded_rectangle(0.5, 0.375, 0.12)],
        ),
        Shape::built_in("circle", "Circle", vec![ellipse(0.5, 0.5)]),
        Shape::built_in("ellipse", "Ellipse", vec![ellipse(0.5, 0.3)]),
    ]
}

fn polygons() -> Vec<Shape> {
    vec![
        Shape::built_in("triangle", "Triangle", vec![polygon(3, 0.0)]),
        Shape::built_in(
            "right_triangle",
            "Right triangle",
            vec![vec![[-0.5, -0.5], [-0.5, 0.5], [0.5, -0.5], [-0.5, -0.5]]],
        ),
        Shape::built_in(
            "diamond",
            "Diamond",
            vec![vec![
                [0.0, 0.5],
                [0.35, 0.0],
                [0.0, -0.5],
                [-0.35, 0.0],
                [0.0, 0.5],
            ]],
        ),
        Shape::built_in(
            "parallelogram",
            "Parallelogram",
            vec![vec![
                [-0.3, 0.25],
                [0.5, 0.25],
                [0.3, -0.25],
                [-0.5, -0.25],
                [-0.3, 0.25],
            ]],
        ),
        Shape::built_in(
            "trapezoid",
            "Trapezoid",
            vec![vec![
                [-0.25, 0.25],
                [0.25, 0.25],
                [0.5, -0.25],
                [-0.5, -0.25],
                [-0.25, 0.25],
            ]],
        ),
        Shape::built_in("pentagon", "Pentagon", vec![polygon(5, 0.0)]),
        Shape::built_in(
            "hexagon",
            "Hexagon",
            vec![polygon(6, std::f32::consts::FRAC_PI_6)],
        ),
        Shape::built_in(
            "octagon",
            "Octagon",
            vec![polygon(8, std::f32::consts::PI / 8.0)],
        ),
        Shape::built_in("star", "Star", vec![star(5)]),
    ]
}

fn symbols() -> Vec<Shape> {
    vec![
        Shape::built_in(
            "plus",
            "Plus",
            vec![vec![[0.0, 0.5], [0.0, -0.5]], vec![[-0.5, 0.0], [0.5, 0.0]]],
        ),
        Shape::built_in(
            "cross",
            "Cross",
            vec![
                vec![[-0.5, 0.5], [0.5, -0.5]],
                vec![[-0.5, -0.5], [0.5, 0.5]],
            ],
        ),
        Shape::built_in(
            "check",
            "Check mark",
            vec![fit(vec![[-0.4, 0.0], [-0.12, -0.3], [0.4, 0.3]])],
        ),
        Shape::built_in("heart", "Heart", vec![heart()]),
        Shape::built_in(
            "speech",
            "Speech bubble",
            vec![vec![
                [-0.5, 0.3],
                [0.5, 0.3],
                [0.5, -0.2],
                [-0.1, -0.2],
                [-0.3, -0.45],
                [-0.3, -0.2],
                [-0.5, -0.2],
                [-0.5, 0.3],
            ]],
        ),
        Shape::built_in(
            "bolt",
            "Lightning bolt",
            vec![fit(vec![
                [0.1, 0.5],
                [-0.25, 0.0],
                [0.0, 0.0],
                [-0.1, -0.5],
                [0.25, 0.05],
                [0.0, 0.05],
                [0.1, 0.5],
            ])],
        ),
        Shape::built_in("cube", "Cube", cube()),
    ]
}

/// A cube seen corner on: the hexagon of its outline and the three edges that meet in the middle.
fn cube() -> Vec<Vec<[f32; 2]>> {
    let outline = polygon(6, 0.0);
    let reach = outline.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
    let mut paths = vec![outline];
    for degrees in [30.0_f32, 150.0, 270.0] {
        let a = degrees.to_radians();
        paths.push(vec![[0.0, 0.0], [reach * a.cos(), reach * a.sin()]]);
    }
    paths
}

/// The built-in shape called `id`, if there is one.
#[must_use]
pub fn built_in_named(id: &str) -> Option<Shape> {
    built_in().into_iter().find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::IdGen;

    fn style() -> Style {
        Style {
            pen: Pen::Pen,
            width: Some(3.0),
            color: Color::BLACK,
            fill: None,
            outline: true,
        }
    }

    /// The box round the points of `strokes`, without the width of the pens.
    fn spread(strokes: &[Stroke]) -> [f32; 4] {
        let refs: Vec<&Stroke> = strokes.iter().collect();
        geometry_bounds(&refs).expect("bounds")
    }

    fn put(shape: &Shape, center: Point, size: f32, style: &Style) -> Vec<Stroke> {
        let mut ids = IdGen::new(7);
        shape.to_strokes(center, size, style, &mut || ids.next_id())
    }

    #[test]
    fn there_are_at_least_twenty_built_in_shapes_each_with_its_own_name_and_id() {
        let shapes = built_in();
        assert!(shapes.len() >= 20, "{}", shapes.len());
        let mut ids: Vec<&str> = shapes.iter().map(|s| s.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), shapes.len(), "ids are unique");
        let mut names: Vec<&str> = shapes.iter().map(|s| s.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), shapes.len(), "names are unique");
        assert!(shapes.iter().all(Shape::is_built_in));
    }

    #[test]
    fn every_built_in_shape_fits_a_box_with_a_longer_side_of_one_around_the_origin() {
        for shape in built_in() {
            let (w, h) = shape.extent();
            let longer = w.max(h);
            assert!(
                (longer - 1.0).abs() < 0.02,
                "{} has a longer side of {longer}",
                shape.name
            );
            for path in &shape.paths {
                assert!(path.points.len() >= 2, "{}", shape.name);
                for [x, y] in &path.points {
                    assert!(
                        x.abs() <= 0.52 && y.abs() <= 0.52,
                        "{}: {x},{y}",
                        shape.name
                    );
                }
            }
        }
    }

    #[test]
    fn the_closed_shapes_end_where_they_begin() {
        for id in [
            "rectangle",
            "square",
            "circle",
            "ellipse",
            "triangle",
            "right_triangle",
            "diamond",
            "pentagon",
            "hexagon",
            "octagon",
            "star",
            "heart",
            "rounded_rectangle",
        ] {
            let shape = built_in_named(id).expect(id);
            assert!(is_closed(&shape.paths[0].points), "{id} is open");
        }
    }

    #[test]
    fn putting_a_shape_on_a_drawing_scales_it_and_centers_it_where_asked() {
        let square = built_in_named("square").expect("square");
        let strokes = put(&square, Point::new(100.0, 200.0), 80.0, &style());
        assert_eq!(strokes.len(), 1);
        let [x0, y0, x1, y1] = spread(&strokes);
        assert!(
            (x0 - 60.0).abs() < 1e-3 && (x1 - 140.0).abs() < 1e-3,
            "{x0} {x1}"
        );
        assert!(
            (y0 - 160.0).abs() < 1e-3 && (y1 - 240.0).abs() < 1e-3,
            "{y0} {y1}"
        );
    }

    #[test]
    fn a_built_in_shape_is_drawn_in_the_pen_size_and_color_in_use() {
        let circle = built_in_named("circle").expect("circle");
        let red = Style {
            pen: Pen::Fountain,
            width: Some(9.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
            ..style()
        };
        let strokes = put(&circle, Point::new(0.0, 0.0), 50.0, &red);
        assert_eq!(strokes[0].pen, Pen::Fountain);
        assert_eq!(strokes[0].width, Some(9.0));
        assert!((strokes[0].color.r - 1.0).abs() < 1e-6);
    }

    #[test]
    fn every_stroke_of_a_shape_has_an_id_of_its_own() {
        let cube = built_in_named("cube").expect("cube");
        let strokes = put(&cube, Point::new(0.0, 0.0), 100.0, &style());
        assert_eq!(strokes.len(), 4, "the outline and three inner lines");
        let mut ids: Vec<_> = strokes.iter().map(|s| s.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn a_shape_made_from_strokes_comes_back_where_and_as_big_as_it_was_drawn() {
        let original = built_in_named("star").expect("star");
        let drawn = put(&original, Point::new(500.0, -300.0), 240.0, &style());
        let refs: Vec<&Stroke> = drawn.iter().collect();
        let mine = Shape::from_strokes("mine", "My star", &refs).expect("a shape");
        assert!(!mine.is_built_in(), "it keeps how it was drawn");
        let again = put(&mine, Point::new(500.0, -300.0), 240.0, &style());
        let (a, b) = (spread(&drawn), spread(&again));
        for i in 0..4 {
            assert!((a[i] - b[i]).abs() < 0.5, "{a:?} against {b:?}");
        }
        assert_eq!(again[0].pen, Pen::Pen);
        assert_eq!(
            again[0].width,
            Some(3.0),
            "the width scales with the size, here the same size"
        );
    }

    #[test]
    fn a_shape_made_by_hand_keeps_its_own_colors_whatever_the_pen_in_use() {
        let mut ids = IdGen::new(3);
        let blue = Color::new(0.0, 0.0, 1.0, 1.0);
        let mut builder = StrokeBuilder::new(Pen::Pen, Some(4.0), blue, 1.0, 0.0);
        builder.replace_with(vec![Point::new(0.0, 0.0), Point::new(100.0, 50.0)]);
        let stroke = builder.with_id(ids.next_id()).finish();
        let mine = Shape::from_strokes("a", "A", &[&stroke]).expect("a shape");
        let red = Style {
            pen: Pen::Fountain,
            width: Some(20.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
            ..style()
        };
        let placed = put(&mine, Point::new(0.0, 0.0), 100.0, &red);
        assert!(
            (placed[0].color.b - 1.0).abs() < 1e-6 && placed[0].color.r.abs() < 1e-6,
            "still blue"
        );
        assert_eq!(placed[0].pen, Pen::Pen);
    }

    #[test]
    fn a_closed_built_in_shape_can_be_filled_under_its_outline() {
        let square = built_in_named("square").expect("square");
        let filled = Style {
            fill: Some(Color::new(1.0, 0.9, 0.0, 1.0)),
            ..style()
        };
        let strokes = put(&square, Point::new(0.0, 0.0), 100.0, &filled);
        assert_eq!(strokes.len(), 2, "the fill and the outline");
        assert_eq!(strokes[0].pen, Pen::Fill, "the fill is first, so under");
        assert!((strokes[0].color.g - 0.9).abs() < 1e-6);
        assert_eq!(strokes[1].pen, Pen::Pen, "and the outline on top");
        let only_fill = Style {
            outline: false,
            ..filled
        };
        let strokes = put(&square, Point::new(0.0, 0.0), 100.0, &only_fill);
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].pen, Pen::Fill);
    }

    #[test]
    fn an_open_shape_has_no_inside_so_it_is_only_an_outline_even_when_fill_is_asked_for() {
        let arrow = built_in_named("arrow").expect("arrow");
        let filled = Style {
            fill: Some(Color::new(1.0, 0.0, 0.0, 1.0)),
            outline: false,
            ..style()
        };
        let strokes = put(&arrow, Point::new(0.0, 0.0), 100.0, &filled);
        assert!(strokes.iter().all(|s| s.pen == Pen::Pen), "{strokes:?}");
        assert_eq!(strokes.len(), 2, "the shaft and the head");
    }

    #[test]
    fn which_paths_are_closed() {
        assert!(is_closed(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]));
        assert!(!is_closed(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]));
        assert!(
            !is_closed(&[[0.0, 0.0], [1.0, 0.0]]),
            "two points are a line"
        );
        assert!(!is_closed(&[]));
    }

    #[test]
    fn nothing_to_make_a_shape_of_gives_none() {
        assert!(Shape::from_strokes("a", "A", &[]).is_none());
        let mut ids = IdGen::new(3);
        let mut builder = StrokeBuilder::new(Pen::Pen, Some(4.0), Color::BLACK, 1.0, 0.0);
        builder.replace_with(vec![Point::new(5.0, 5.0), Point::new(5.0, 5.0)]);
        let dot = builder.with_id(ids.next_id()).finish();
        assert!(
            Shape::from_strokes("a", "A", &[&dot]).is_none(),
            "a point has no size to scale from"
        );
    }

    #[test]
    fn a_shape_goes_through_json_and_back() {
        for shape in built_in() {
            let text = serde_json::to_string(&shape).expect("written");
            let back: Shape = serde_json::from_str(&text).expect("read");
            assert_eq!(back, shape, "{}", shape.name);
        }
    }

    #[test]
    fn simplifying_drops_points_that_lie_on_a_line_and_keeps_the_corners() {
        let line: Vec<[f32; 2]> = (0..=10).map(|i| [i as f32 / 10.0, 0.0]).collect();
        assert_eq!(
            simplify(&line, 0.001).len(),
            2,
            "a straight line is its two ends"
        );
        let corner = vec![[0.0, 0.0], [0.5, 0.0], [1.0, 0.0], [1.0, 0.5], [1.0, 1.0]];
        let kept = simplify(&corner, 0.001);
        assert_eq!(kept, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]);
    }
}
