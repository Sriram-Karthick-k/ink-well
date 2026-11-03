//! Moving, scaling and turning strokes, and measuring them.

use crate::document::Stroke;
use crate::geometry::Point;

/// A flat transformation of the canvas: `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Affine {
    /// Leaves everything where it is.
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Moves everything by `dx` and `dy`.
    #[must_use]
    pub const fn translate(dx: f32, dy: f32) -> Self {
        Self {
            e: dx,
            f: dy,
            ..Self::IDENTITY
        }
    }

    /// Makes everything `factor` times as large, around `center`.
    #[must_use]
    pub fn scale_about(center: Point, factor: f32) -> Self {
        Self {
            a: factor,
            b: 0.0,
            c: 0.0,
            d: factor,
            e: center.x - center.x * factor,
            f: center.y - center.y * factor,
        }
    }

    /// Mirrors everything left to right about the vertical line at `center_x`.
    #[must_use]
    pub fn flip_horizontal(center_x: f32) -> Self {
        Self {
            a: -1.0,
            e: 2.0 * center_x,
            ..Self::IDENTITY
        }
    }

    /// Mirrors everything top to bottom about the horizontal line at `center_y`.
    #[must_use]
    pub fn flip_vertical(center_y: f32) -> Self {
        Self {
            d: -1.0,
            f: 2.0 * center_y,
            ..Self::IDENTITY
        }
    }

    /// Turns everything by `radians` counterclockwise (the canvas y-axis points up), around `center`.
    #[must_use]
    pub fn rotate_about(center: Point, radians: f32) -> Self {
        let (sin, cos) = radians.sin_cos();
        Self {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: center.x - (center.x * cos - center.y * sin),
            f: center.y - (center.x * sin + center.y * cos),
        }
    }

    /// The transformation that does `self` first and then `next`.
    #[must_use]
    pub fn then(self, next: Self) -> Self {
        Self {
            a: next.a * self.a + next.c * self.b,
            b: next.b * self.a + next.d * self.b,
            c: next.a * self.c + next.c * self.d,
            d: next.b * self.c + next.d * self.d,
            e: next.a * self.e + next.c * self.f + next.e,
            f: next.b * self.e + next.d * self.f + next.f,
        }
    }

    #[must_use]
    pub fn apply(self, p: Point) -> Point {
        Point::new(
            self.a * p.x + self.c * p.y + self.e,
            self.b * p.x + self.d * p.y + self.f,
        )
    }

    /// By how much lengths grow: `1.0` for moving and turning, the factor for scaling.
    #[must_use]
    pub fn scale_factor(self) -> f32 {
        (self.a * self.d - self.b * self.c).abs().sqrt()
    }
}

impl Stroke {
    /// A copy of the stroke put through `m`. Its width grows or shrinks with the transformation, so a scaled
    /// stroke keeps its proportions. Everything else about it, including its id, is kept: a stroke that is
    /// moved looks exactly as it did, only somewhere else.
    #[must_use]
    pub fn transformed(&self, m: &Affine) -> Self {
        let mut stroke = self.clone();
        for sample in &mut stroke.samples {
            sample.point = m.apply(sample.point);
        }
        for point in &mut stroke.path {
            *point = m.apply(*point);
        }
        let scale = m.scale_factor();
        stroke.width = stroke.width.map(|w| w * scale);
        stroke
    }

    /// A number that changes whenever the stroke's shape, size or look does, for caches that hold what was worked
    /// out from a stroke. It takes in every point, so a change anywhere along a stroke (a bend pulled into the
    /// middle of it, say) is noticed; that is one pass over the points, without copying them.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |bits: u32| {
            hash ^= u64::from(bits);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        };
        mix(self.pen.id());
        mix(self.width.unwrap_or(-1.0).to_bits());
        for channel in [self.color.r, self.color.g, self.color.b, self.color.a] {
            mix(channel.to_bits());
        }
        let mut count = 0_u32;
        for p in self.points() {
            mix(p.x.to_bits());
            mix(p.y.to_bits());
            count += 1;
        }
        mix(count);
        hash
    }

    /// How long the stroke is along its path, in canvas units.
    #[must_use]
    pub fn length(&self) -> f32 {
        let mut points = self.points();
        let Some(mut previous) = points.next() else {
            return 0.0;
        };
        points
            .map(|p| {
                let step = previous.distance(p);
                previous = p;
                step
            })
            .sum()
    }

    /// Left, bottom, right and top of the stroke, with half its width added all round; `None` if it has no points.
    #[must_use]
    pub fn bounds(&self) -> Option<[f32; 4]> {
        let mut points = self.points();
        let first = points.next()?;
        let pad = self.width.unwrap_or(1.0) / 2.0;
        let mut b = [first.x, first.y, first.x, first.y];
        for p in points {
            b[0] = b[0].min(p.x);
            b[1] = b[1].min(p.y);
            b[2] = b[2].max(p.x);
            b[3] = b[3].max(p.y);
        }
        Some([b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad])
    }
}

/// The smallest rectangle around all of `strokes`, as left, bottom, right and top.
#[must_use]
pub fn bounds_of<'a>(strokes: impl IntoIterator<Item = &'a Stroke>) -> Option<[f32; 4]> {
    strokes
        .into_iter()
        .filter_map(Stroke::bounds)
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use uuid::Uuid;

    use super::*;
    use crate::builder::StrokeBuilder;
    use crate::document::Pen;
    use crate::geometry::Color;

    fn near(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-3
    }

    fn line() -> Stroke {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(4.0), Color::BLACK, 1.0, 0.0)
            .with_id(Uuid::from_u128(9));
        for i in 0..=10 {
            b.push(Point::new(i as f32 * 10.0, 5.0), None, f64::from(i) * 0.01);
        }
        b.finish()
    }

    #[test]
    fn translating_moves_every_point_by_the_same_amount() {
        let m = Affine::translate(3.0, -2.0);
        assert!(near(m.apply(Point::new(1.0, 1.0)), Point::new(4.0, -1.0)));
    }

    #[test]
    fn scaling_keeps_the_center_where_it_is() {
        let center = Point::new(10.0, 20.0);
        let m = Affine::scale_about(center, 2.0);
        assert!(near(m.apply(center), center));
        assert!(near(
            m.apply(Point::new(11.0, 20.0)),
            Point::new(12.0, 20.0)
        ));
        assert!((m.scale_factor() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn rotating_a_quarter_turn_counterclockwise_about_a_center() {
        let center = Point::new(5.0, 5.0);
        let m = Affine::rotate_about(center, FRAC_PI_2);
        assert!(near(m.apply(center), center));
        assert!(near(m.apply(Point::new(6.0, 5.0)), Point::new(5.0, 6.0)));
        assert!(
            (m.scale_factor() - 1.0).abs() < 1e-6,
            "turning does not scale"
        );
    }

    #[test]
    fn then_does_the_first_transformation_first() {
        let m = Affine::translate(10.0, 0.0).then(Affine::scale_about(Point::new(0.0, 0.0), 2.0));
        assert!(near(m.apply(Point::new(1.0, 1.0)), Point::new(22.0, 2.0)));
        let turned_then_moved =
            Affine::rotate_about(Point::new(0.0, 0.0), FRAC_PI_2).then(Affine::translate(1.0, 0.0));
        assert!(near(
            turned_then_moved.apply(Point::new(1.0, 0.0)),
            Point::new(1.0, 1.0)
        ));
    }

    #[test]
    fn a_moved_stroke_is_the_same_stroke_somewhere_else() {
        let original = line();
        let moved = original.transformed(&Affine::translate(7.0, -3.0));
        assert_eq!(
            moved.id, original.id,
            "its identity, and so its texture, is kept"
        );
        assert_eq!(moved.pen, original.pen);
        assert_eq!(moved.width, original.width);
        assert_eq!(moved.samples.len(), original.samples.len());
        for (m, o) in moved.samples.iter().zip(&original.samples) {
            assert!(near(m.point, Point::new(o.point.x + 7.0, o.point.y - 3.0)));
            assert_eq!(m.attrs, o.attrs, "speed and pressure are untouched");
        }
        let back = moved.transformed(&Affine::translate(-7.0, 3.0));
        for (b, o) in back.samples.iter().zip(&original.samples) {
            assert!(near(b.point, o.point));
        }
    }

    #[test]
    fn a_scaled_stroke_keeps_its_proportions_including_its_width() {
        let stroke = line().transformed(&Affine::scale_about(Point::new(0.0, 0.0), 3.0));
        assert_eq!(stroke.width, Some(12.0));
        let last = stroke.samples.last().expect("sample").point;
        assert!(near(last, Point::new(300.0, 15.0)));
    }

    #[test]
    fn mirroring_swaps_sides_about_the_axis_and_keeps_sizes() {
        let m = Affine::flip_horizontal(10.0);
        assert!(near(m.apply(Point::new(14.0, 3.0)), Point::new(6.0, 3.0)));
        let v = Affine::flip_vertical(5.0);
        assert!(near(v.apply(Point::new(2.0, 8.0)), Point::new(2.0, 2.0)));
        assert!((m.scale_factor() - 1.0).abs() < 1e-6);
        let flipped = line().transformed(&m);
        assert_eq!(flipped.width, Some(4.0));
    }

    #[test]
    fn the_fingerprint_changes_when_a_stroke_moves_or_changes_but_not_otherwise() {
        let stroke = line();
        assert_eq!(stroke.fingerprint(), line().fingerprint());
        let moved = stroke.transformed(&Affine::translate(0.5, 0.0));
        assert_ne!(stroke.fingerprint(), moved.fingerprint());
        let scaled = stroke.transformed(&Affine::scale_about(Point::new(0.0, 0.0), 1.01));
        assert_ne!(stroke.fingerprint(), scaled.fingerprint());
        let mut thicker = stroke.clone();
        thicker.width = Some(5.0);
        assert_ne!(stroke.fingerprint(), thicker.fingerprint());
        let mut shorter = stroke.clone();
        shorter.samples.pop();
        assert_ne!(stroke.fingerprint(), shorter.fingerprint());
        let mut other_id = stroke.clone();
        other_id.id = Uuid::from_u128(1234);
        assert_eq!(
            stroke.fingerprint(),
            other_id.fingerprint(),
            "identity is not part of the look, the cache is keyed by it separately"
        );
    }

    #[test]
    fn the_fingerprint_notices_a_change_anywhere_along_a_stroke() {
        let stroke = line();
        // Every point in turn, not only the ones a coarse key might have looked at.
        for i in 0..stroke.samples.len() {
            let mut bent = stroke.clone();
            bent.samples[i].point.y += 0.01;
            assert_ne!(
                stroke.fingerprint(),
                bent.fingerprint(),
                "a change to point {i} went unnoticed"
            );
        }
        let mut longer = stroke.clone();
        longer.samples.push(*longer.samples.last().expect("sample"));
        assert_ne!(stroke.fingerprint(), longer.fingerprint());
    }

    #[test]
    fn a_stroke_is_as_long_as_its_path() {
        assert!((line().length() - 100.0).abs() < 1e-3);
        let mut empty = line();
        empty.samples.clear();
        empty.path.clear();
        assert_eq!(empty.length(), 0.0);
        let mut one = line();
        one.samples.clear();
        one.path = vec![Point::new(3.0, 4.0)];
        assert_eq!(one.length(), 0.0, "a dot has no length");
    }

    #[test]
    fn bounds_include_half_the_width() {
        let b = line().bounds().expect("bounds");
        assert_eq!(b, [-2.0, 3.0, 102.0, 7.0]);
        let mut empty = line();
        empty.samples.clear();
        empty.path.clear();
        assert_eq!(empty.bounds(), None);
    }

    #[test]
    fn bounds_of_several_strokes_cover_them_all() {
        let a = line();
        let b = line().transformed(&Affine::translate(0.0, 50.0));
        let all = bounds_of([&a, &b]).expect("bounds");
        assert_eq!(all, [-2.0, 3.0, 102.0, 57.0]);
        assert_eq!(bounds_of(std::iter::empty::<&Stroke>()), None);
    }
}
