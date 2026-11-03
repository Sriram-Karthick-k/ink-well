//! Turns live pointer input into a finished [`Stroke`].
//!
//! What is drawn is what is kept. Points go to a [`LiveSmoother`], which appends smoothed output as they arrive
//! without ever changing what it has already produced, and the stroke that is stored when the pen lifts is
//! exactly that output: nothing is recomputed, thinned or re-smoothed at the end, so the stroke cannot change
//! shape the moment the pen is lifted. The stroke also keeps one id from the first point, because the textured
//! pens seed their scatter from it; a stroke that changed id when finished would change its texture.
//!
//! Each point also carries the pen speed, measured by a [`SpeedFilter`] from the times the points arrived.
//!
//! Nothing is trimmed from the ends, and the stroke is not smoothed again when the pen lifts, on purpose: what was
//! drawn is what is kept.

use uuid::Uuid;

use crate::document::{Pen, RawSample, Stroke};
use crate::geometry::{Color, Point};
use crate::smoothing::{LiveSmoother, SmoothParams, Smoothed};
use crate::speed::SpeedFilter;

/// Collects points while the pen is down.
#[derive(Debug, Clone)]
pub struct StrokeBuilder {
    id: Uuid,
    pen: Pen,
    width: Option<f32>,
    color: Color,
    zoom: f32,
    smoother: LiveSmoother,
    speed: SpeedFilter,
    /// Where the last point is drawn instead of where the smoother put it (see [`StrokeBuilder::set_end`]).
    end: Option<Point>,
    /// A clean shape drawn in place of the stroke (see [`StrokeBuilder::replace_with`]).
    replaced: Option<Vec<Point>>,
}

fn quantize(value: Option<f32>, default: u16) -> u16 {
    value.map_or(default, |v| (v.clamp(0.0, 1.0) * 65_535.0).round() as u16)
}

impl StrokeBuilder {
    /// `zoom` is the view zoom (screen pixels per canvas unit); `smoothness` is the pen's smoothness setting.
    #[must_use]
    pub fn new(pen: Pen, width: Option<f32>, color: Color, zoom: f32, smoothness: f32) -> Self {
        let zoom = zoom.max(1e-6);
        Self {
            id: Uuid::nil(),
            pen,
            width,
            color,
            zoom,
            smoother: LiveSmoother::new(SmoothParams { smoothness, zoom }),
            speed: SpeedFilter::default(),
            end: None,
            replaced: None,
        }
    }

    /// Draws `points` (a clean shape the stroke was recognized as) in place of everything drawn so far, and
    /// ignores the pen from then on. It is the same in what is shown while the pen is down and in what is stored
    /// when it lifts. The shape has the same width and pen as the stroke, and an even pressure.
    pub fn replace_with(&mut self, points: Vec<Point>) {
        if points.len() >= 2 {
            self.replaced = Some(points);
        }
    }

    /// `true` once the stroke has been replaced by a shape.
    #[must_use]
    pub fn is_replaced(&self) -> bool {
        self.replaced.is_some()
    }

    /// The points of the stroke as they are so far, to look at while it is drawn (what a shape is recognized
    /// from). Not the stroke itself: it has no pressure or speed.
    #[must_use]
    pub fn points(&self) -> Vec<Point> {
        match &self.replaced {
            Some(points) => points.clone(),
            None => self.smoother.output().points,
        }
    }

    /// Draws the last point of the stroke at `end` instead of where the pen is, or at the pen again with `None`:
    /// for a pen that is close to something it snaps to. Only the last point moves, and it is the same in what is
    /// shown while the pen is down and in what is stored when it lifts, so the stroke does not change on pen-up.
    pub fn set_end(&mut self, end: Option<Point>) {
        self.end = end;
    }

    /// Where the last point is being drawn instead of at the pen, if anywhere.
    #[must_use]
    pub fn end(&self) -> Option<Point> {
        self.end
    }

    /// Gives the stroke its id now, so the stroke shown while drawing and the one stored are the same stroke.
    /// Without it the id is the nil id, which is fine for a preview but not for a stroke put in a document.
    #[must_use]
    pub fn with_id(mut self, id: Uuid) -> Self {
        self.id = id;
        self
    }

    /// Adds a point that arrived at `time` seconds. `pressure` is `None` for devices that do not report it.
    /// A point at exactly the same place as the last one is ignored.
    pub fn push(&mut self, point: Point, pressure: Option<f32>, time: f64) {
        if self.replaced.is_some() || self.smoother.last_raw() == Some(point) {
            return;
        }
        let speed = self.speed.push([point.x, point.y], time, self.zoom);
        self.smoother.push(point, pressure, Some(speed));
    }

    /// The number of points kept so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.smoother.raw_len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.smoother.raw_len() == 0
    }

    fn build(&self, smoothed: Smoothed) -> Stroke {
        let samples = smoothed
            .points
            .iter()
            .enumerate()
            .map(|(i, &point)| RawSample {
                point,
                attrs: [
                    quantize(smoothed.speed.as_ref().map(|s| s[i]), 0),
                    // A mouse has no pressure; the observed default is 32767.
                    quantize(smoothed.pressure.as_ref().map(|p| p[i]), 32_767),
                    65_535,
                    0,
                ],
            })
            .collect();
        Stroke {
            id: self.id,
            pen: self.pen,
            width: self.width,
            color: self.color,
            path: Vec::new(),
            samples,
            pressure: smoothed.pressure.unwrap_or_default(),
        }
    }

    /// The stroke as it is right now: what is shown while the pen is down and what is stored when it lifts. A
    /// single tap, which has one point, is given a second one at the same place so that it draws as a dot.
    fn current(&self) -> Stroke {
        if let Some(points) = &self.replaced {
            return self.build(Smoothed {
                points: points.clone(),
                pressure: None,
                speed: None,
            });
        }
        let mut smoothed = self.smoother.output();
        if smoothed.points.len() == 1 {
            smoothed.points.push(smoothed.points[0]);
            for channel in [smoothed.pressure.as_mut(), smoothed.speed.as_mut()]
                .into_iter()
                .flatten()
            {
                channel.push(channel[0]);
            }
        }
        if let Some(end) = self.end
            && let Some(last) = smoothed.points.last_mut()
        {
            *last = end;
        }
        self.build(smoothed)
    }

    /// The stroke as drawn so far, for showing it while the pen is still down. Everything but its last point
    /// is final: the finished stroke starts with exactly these points.
    #[must_use]
    pub fn snapshot(&self) -> Stroke {
        self.current()
    }

    /// Ends the stroke. It is exactly what [`snapshot`](Self::snapshot) showed the moment before.
    #[must_use]
    pub fn finish(self) -> Stroke {
        self.current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builder() -> StrokeBuilder {
        StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.1)
    }

    /// Points ten milliseconds apart.
    fn line_builder(points: &[(f32, f32)]) -> StrokeBuilder {
        let mut b = builder();
        for (i, &(x, y)) in points.iter().enumerate() {
            b.push(Point::new(x, y), None, i as f64 * 0.01);
        }
        b
    }

    #[test]
    fn an_end_drawn_elsewhere_is_the_same_while_the_pen_is_down_and_when_it_lifts() {
        let points = [(0.0, 0.0), (10.0, 0.0), (20.0, 1.0), (30.0, 1.0)];
        let plain = line_builder(&points).snapshot();
        let mut b = line_builder(&points);
        b.set_end(Some(Point::new(32.0, 0.0)));
        assert_eq!(b.end(), Some(Point::new(32.0, 0.0)));
        let shown = b.snapshot();
        assert_eq!(shown.samples.len(), plain.samples.len());
        assert_eq!(
            shown.samples.last().map(|s| s.point),
            Some(Point::new(32.0, 0.0)),
            "the last point is where it snaps to"
        );
        assert_eq!(
            shown.samples[..shown.samples.len() - 1],
            plain.samples[..plain.samples.len() - 1],
            "the points before it are not touched"
        );
        assert_eq!(b.finish(), shown, "what is shown is what is kept");
    }

    #[test]
    fn a_shape_in_place_of_the_stroke_is_what_is_shown_and_what_is_kept_and_the_pen_is_ignored_after()
     {
        let mut b = line_builder(&[(0.0, 0.0), (10.0, 3.0), (20.0, -2.0), (30.0, 1.0)]);
        assert!(!b.is_replaced());
        let shape = vec![Point::new(0.0, 0.0), Point::new(30.0, 0.0)];
        b.replace_with(shape.clone());
        assert!(b.is_replaced());
        // The pen moves on, and the end is set: neither changes the shape.
        b.push(Point::new(80.0, 40.0), None, 1.0);
        b.set_end(Some(Point::new(5.0, 5.0)));
        let shown = b.snapshot();
        let points: Vec<Point> = shown.samples.iter().map(|s| s.point).collect();
        assert_eq!(points, shape);
        assert_eq!(b.points(), shape);
        assert_eq!(b.finish(), shown, "what is shown is what is kept");
    }

    #[test]
    fn a_shape_of_fewer_than_two_points_is_not_taken() {
        let mut b = line_builder(&[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]);
        b.replace_with(vec![Point::new(1.0, 1.0)]);
        assert!(!b.is_replaced());
    }

    #[test]
    fn taking_the_end_back_puts_it_at_the_pen_and_a_tap_can_snap_too() {
        let mut b = line_builder(&[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]);
        let plain = b.snapshot();
        b.set_end(Some(Point::new(25.0, 3.0)));
        b.set_end(None);
        assert_eq!(b.snapshot(), plain);
        let mut tap = line_builder(&[(5.0, 5.0)]);
        tap.set_end(Some(Point::new(6.0, 5.0)));
        let dot = tap.finish();
        assert_eq!(dot.samples.len(), 2);
        assert_eq!(dot.samples[0].point, Point::new(5.0, 5.0));
        assert_eq!(dot.samples[1].point, Point::new(6.0, 5.0));
    }

    #[test]
    fn a_repeated_point_is_ignored_but_a_nearby_one_is_kept() {
        let b = line_builder(&[(0.0, 0.0), (0.0, 0.0), (0.1, 0.0), (10.0, 0.0)]);
        assert_eq!(b.len(), 3);
    }

    #[test]
    fn a_single_tap_becomes_a_drawable_stroke() {
        let stroke = line_builder(&[(5.0, 5.0)]).finish();
        assert_eq!(stroke.samples.len(), 2);
    }

    #[test]
    fn mouse_input_has_no_pressure_and_the_default_pressure_attribute() {
        let stroke = line_builder(&[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]).finish();
        assert_eq!(stroke.pressure, Vec::<f32>::new());
        assert_eq!(stroke.samples[0].attrs[1..], [32_767, 65_535, 0]);
    }

    #[test]
    fn the_speed_of_the_pen_is_stored_in_the_first_attribute() {
        // 128 canvas units every 10 ms is 12 800 per second: full speed
        let fast: Vec<(f32, f32)> = (0..8).map(|i| (i as f32 * 128.0, 0.0)).collect();
        let stroke = line_builder(&fast).finish();
        assert_eq!(
            stroke.samples[0].attrs[0], 0,
            "the first point has no speed yet"
        );
        assert_eq!(stroke.samples.last().map(|s| s.attrs[0]), Some(65_535));

        let slow: Vec<(f32, f32)> = (0..8).map(|i| (i as f32 * 1.28, 0.0)).collect();
        let stroke = line_builder(&slow).finish();
        let speed = f32::from(stroke.samples[stroke.samples.len() - 1].attrs[0]) / 65_535.0;
        assert!((speed - 0.01).abs() < 1e-3, "{speed}");
    }

    #[test]
    fn pen_pressure_is_recorded_per_sample() {
        let mut b = builder();
        b.push(Point::new(0.0, 0.0), Some(0.2), 0.0);
        b.push(Point::new(10.0, 0.0), Some(1.0), 0.01);
        let stroke = b.finish();
        assert_eq!(stroke.pressure, [0.2, 1.0]);
        assert_eq!(stroke.samples[1].attrs[1], 65_535);
    }

    #[test]
    fn nothing_is_trimmed_from_either_end() {
        // A small tick at the end of a line, like a pen flick. It is kept, so the stroke
        // must still end at the last point and start at the first.
        let mut pts: Vec<(f32, f32)> = (0..10).map(|i| (i as f32 * 20.0, 0.0)).collect();
        pts.push((180.0, 5.0));
        pts.push((176.0, 8.0));
        let stroke = line_builder(&pts).finish();
        let first = stroke.samples.first().expect("sample").point;
        let last = stroke.samples.last().expect("sample").point;
        assert_eq!((first.x, first.y), (0.0, 0.0));
        assert_eq!((last.x, last.y), (176.0, 8.0));
    }

    #[test]
    fn the_stroke_shown_while_drawing_only_grows_at_its_end() {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.25);
        let mut before: Vec<Point> = Vec::new();
        for i in 0..120 {
            let t = i as f32;
            b.push(
                Point::new(t * 4.0, (t / 9.0).sin() * 40.0),
                None,
                f64::from(i) * 0.01,
            );
            let now: Vec<Point> = b.snapshot().samples.iter().map(|s| s.point).collect();
            let settled = before.len().saturating_sub(1);
            assert_eq!(&now[..settled], &before[..settled], "reshaped at point {i}");
            before = now;
        }
    }

    /// A hand-drawn looking path: wobbles at several scales, with the points unevenly spaced in time.
    fn scribble(count: usize) -> Vec<(Point, f64)> {
        let mut time = 0.0;
        (0..count)
            .map(|i| {
                let t = i as f32;
                time += 0.004 + 0.006 * f64::from((t * 0.7).sin() * 0.5 + 0.5);
                let point = Point::new(
                    t * 3.1 + (t * 0.9).sin() * 4.0,
                    (t / 7.0).sin() * 55.0 + (t * 1.7).cos() * 6.0,
                );
                (point, time)
            })
            .collect()
    }

    #[test]
    fn the_finished_stroke_is_exactly_what_was_shown_while_drawing() {
        for smoothness in [0.0, 0.1, 0.25, 0.5, 0.9] {
            for zoom in [0.5, 1.0, 3.0] {
                let mut b =
                    StrokeBuilder::new(Pen::Fountain, Some(5.0), Color::BLACK, zoom, smoothness)
                        .with_id(Uuid::from_u128(77));
                for (point, time) in scribble(160) {
                    b.push(point, Some(0.6), time);
                }
                let shown = b.snapshot();
                let kept = b.finish();
                assert_eq!(shown, kept, "smoothness {smoothness}, zoom {zoom}");
            }
        }
    }

    #[test]
    fn what_was_shown_at_every_moment_is_the_start_of_the_finished_stroke() {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.25)
            .with_id(Uuid::from_u128(5));
        let mut shown: Vec<Stroke> = Vec::new();
        for (point, time) in scribble(120) {
            b.push(point, None, time);
            shown.push(b.snapshot());
        }
        let kept = b.finish();
        for (n, earlier) in shown.iter().enumerate() {
            let settled = earlier.samples.len().saturating_sub(1);
            assert_eq!(
                &earlier.samples[..settled],
                &kept.samples[..settled],
                "the stroke changed after frame {n}"
            );
        }
    }

    #[test]
    fn the_stroke_keeps_one_id_from_the_first_point() {
        let id = Uuid::from_u128(0xABCD);
        let mut b = builder().with_id(id);
        for (point, time) in scribble(10) {
            b.push(point, None, time);
        }
        assert_eq!(b.snapshot().id, id);
        assert_eq!(b.finish().id, id);
    }

    #[test]
    fn a_tap_looks_the_same_while_pressed_and_after_it_is_lifted() {
        let mut b = builder();
        b.push(Point::new(5.0, 5.0), None, 0.0);
        let pressed = b.snapshot();
        assert_eq!(pressed.samples.len(), 2, "a dot shows at once");
        assert_eq!(pressed, b.finish());
    }

    #[test]
    fn the_finished_stroke_ends_where_the_pen_ended() {
        let mut b = builder();
        for i in 0..50 {
            b.push(
                Point::new(i as f32 * 5.0, (i as f32 / 6.0).sin() * 30.0),
                None,
                f64::from(i) * 0.01,
            );
        }
        let last = Point::new(49.0 * 5.0, (49.0_f32 / 6.0).sin() * 30.0);
        let stroke = b.finish();
        assert_eq!(stroke.samples.last().map(|s| s.point), Some(last));
    }
}
