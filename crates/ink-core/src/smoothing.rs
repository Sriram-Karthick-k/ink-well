//! Stroke path smoothing with a Gaussian smoother.
//!
//! Smoothing is incremental. While the pen is down, every new raw point appends output and never changes
//! what is already there, so the part of the stroke that has been drawn cannot reshape. That output is also
//! what gets stored when the pen lifts: the stroke is not recomputed at that moment, so it does not change.
//!
//! The pipeline has three steps:
//!
//! 1. **Thinning.** A raw point is kept only when the triangle formed with the last kept point and the next
//!    raw point is larger than a threshold, so a straight run collapses to its ends.
//! 2. **Resampling.** The kept points form a polyline, which is sampled every `step` of arc length.
//! 3. **Gaussian average.** Each kept point becomes the weighted mean of the resampled points within three
//!    sigma, weighted by `exp(-d² / 2σ²)` of the arc distance. The first output is the first raw point and
//!    the last output is the last raw point.
//!
//! With smoothness `s` and view zoom `z` (screen pixels per canvas unit when the stroke was drawn),
//! `σ = 30 / z · (1 / (1 − s) − 1)` and `step = min(σ, 5 / z)`.

use std::sync::OnceLock;

use crate::geometry::Point;

/// Smoothness below this means no smoothing at all.
const MIN_SMOOTHNESS: f32 = 0.001;
/// Smoothness is limited so that sigma stays finite.
const MAX_SMOOTHNESS: f32 = 0.99;
/// Entries of the Gaussian lookup table, 100 per sigma.
const TABLE_LEN: usize = 500;

/// The result of smoothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Smoothed {
    pub points: Vec<Point>,
    /// Pressure carried alongside the points, if it was supplied.
    pub pressure: Option<Vec<f32>>,
    /// Pen speed (`0.0..=1.0`, see [`crate::speed`]) carried alongside the points, if it was supplied.
    pub speed: Option<Vec<f32>>,
}

/// The raw input of a stroke.
#[derive(Debug, Clone, PartialEq)]
pub struct RawPath {
    pub points: Vec<Point>,
    pub pressure: Option<Vec<f32>>,
    pub speed: Option<Vec<f32>>,
}

/// How strongly to smooth a stroke.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmoothParams {
    /// The smoothness setting, `0.0` for none.
    pub smoothness: f32,
    /// Screen pixels per canvas unit when the stroke was drawn.
    pub zoom: f32,
}

impl SmoothParams {
    /// The Gaussian's standard deviation and the resampling step, in canvas units.
    #[must_use]
    pub fn sigma_and_step(self) -> (f32, f32) {
        let s = self.smoothness.clamp(0.0, MAX_SMOOTHNESS);
        let z = self.zoom.max(1e-6);
        let sigma = 30.0 / z * (1.0 / (1.0 - s) - 1.0);
        (sigma, sigma.min(5.0 / z))
    }

    fn is_active(self) -> bool {
        self.smoothness >= MIN_SMOOTHNESS && self.sigma_and_step().0 >= 0.001
    }
}

/// `exp(-x² / 2)` for `x` in `0.0..5.0` in steps of `0.01`.
fn gaussian_table() -> &'static [f32; TABLE_LEN] {
    static TABLE: OnceLock<[f32; TABLE_LEN]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0.0; TABLE_LEN];
        for (i, v) in table.iter_mut().enumerate() {
            let x = i as f32 / 100.0;
            *v = (-0.5 * x * x).exp();
        }
        table
    })
}

/// The weight of a sample `distance` away from the center, for standard deviation `sigma`.
fn gaussian(distance: f32, sigma: f32) -> f32 {
    let index = ((distance / sigma).abs() * 100.0) as usize;
    gaussian_table()[index.min(TABLE_LEN - 1)]
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    point: Point,
    pressure: f32,
    speed: f32,
}

impl Sample {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            point: Point::new(
                a.point.x + (b.point.x - a.point.x) * t,
                a.point.y + (b.point.y - a.point.y) * t,
            ),
            pressure: a.pressure + (b.pressure - a.pressure) * t,
            speed: a.speed + (b.speed - a.speed) * t,
        }
    }
}

/// A running weighted sum of samples.
#[derive(Default)]
struct Mean {
    x: f32,
    y: f32,
    pressure: f32,
    speed: f32,
    weight: f32,
}

impl Mean {
    fn add(&mut self, s: Sample, weight: f32) {
        self.x += s.point.x * weight;
        self.y += s.point.y * weight;
        self.pressure += s.pressure * weight;
        self.speed += s.speed * weight;
        self.weight += weight;
    }

    fn finish(&self) -> Sample {
        Sample {
            point: Point::new(self.x / self.weight, self.y / self.weight),
            pressure: self.pressure / self.weight,
            speed: self.speed / self.weight,
        }
    }
}

/// Smooths a stroke as its points arrive.
#[derive(Debug, Clone)]
pub struct LiveSmoother {
    params: SmoothParams,
    sigma: f32,
    step: f32,
    raw: Vec<Sample>,
    has_pressure: bool,
    has_speed: bool,
    /// How many raw points the last [`update`](Self::update) saw.
    seen: usize,
    /// Indices of the raw points that survived thinning.
    kept: Vec<usize>,
    /// Arc length from the first kept point to each kept point.
    arc: Vec<f32>,
    /// The kept polyline sampled every `step`; `resampled[k]` lies `k * step` along it.
    resampled: Vec<Sample>,
    /// One output per kept point. The last one is provisional: it is the raw last point.
    out: Vec<Sample>,
}

impl LiveSmoother {
    #[must_use]
    pub fn new(params: SmoothParams) -> Self {
        let (sigma, step) = params.sigma_and_step();
        Self {
            params,
            sigma,
            step,
            raw: Vec::new(),
            has_pressure: false,
            has_speed: false,
            seen: 0,
            kept: Vec::new(),
            arc: Vec::new(),
            resampled: Vec::new(),
            out: Vec::new(),
        }
    }

    /// Adds a raw point and updates the output. `pressure` and `speed` are `None` when the input has none.
    pub fn push(&mut self, point: Point, pressure: Option<f32>, speed: Option<f32>) {
        self.add(point, pressure, speed);
        self.update();
    }

    /// Adds a raw point without updating the output; call [`update`](Self::update) afterwards.
    pub fn add(&mut self, point: Point, pressure: Option<f32>, speed: Option<f32>) {
        self.has_pressure |= pressure.is_some();
        self.has_speed |= speed.is_some();
        self.raw.push(Sample {
            point,
            pressure: pressure.unwrap_or(1.0).clamp(0.0, 1.0),
            speed: speed.unwrap_or(0.0).clamp(0.0, 1.0),
        });
    }

    /// The number of raw points added.
    #[must_use]
    pub fn raw_len(&self) -> usize {
        self.raw.len()
    }

    /// The last raw point added.
    #[must_use]
    pub fn last_raw(&self) -> Option<Point> {
        self.raw.last().map(|s| s.point)
    }

    /// The raw points and, if any had them, their pressures and speeds.
    #[must_use]
    pub fn raw_points(&self) -> RawPath {
        RawPath {
            points: self.raw.iter().map(|s| s.point).collect(),
            pressure: self
                .has_pressure
                .then(|| self.raw.iter().map(|s| s.pressure).collect()),
            speed: self
                .has_speed
                .then(|| self.raw.iter().map(|s| s.speed).collect()),
        }
    }

    /// The smoothed path so far. Every point except the last is final until [`commit`](Self::commit).
    #[must_use]
    pub fn output(&self) -> Smoothed {
        Smoothed {
            points: self.out.iter().map(|s| s.point).collect(),
            pressure: self
                .has_pressure
                .then(|| self.out.iter().map(|s| s.pressure).collect()),
            speed: self
                .has_speed
                .then(|| self.out.iter().map(|s| s.speed).collect()),
        }
    }

    /// Processes the raw points added since the last update.
    pub fn update(&mut self) {
        let n = self.raw.len();
        if n == self.seen {
            return;
        }
        if !self.params.is_active() {
            self.out.extend_from_slice(&self.raw[self.seen..n]);
            self.seen = n;
            return;
        }
        let stable = self.thin(n);
        self.resample(stable);
        self.filter(stable);
        self.seen = n;
    }

    /// Thinning. Returns how many kept points, and so outputs, carry over from the previous update.
    fn thin(&mut self, n: usize) -> usize {
        let s = self.params.smoothness.clamp(0.0, MAX_SMOOTHNESS);
        let zoom = self.params.zoom.max(1e-6);
        let threshold = 0.8 * (1.0 / (1.0 - s.min(0.5)) - 1.0) / (zoom * zoom);

        // The previous last point was provisional, and the point before it is the first whose neighbours
        // are not all known yet, so resume there.
        let old = self.seen;
        let start = old.saturating_sub(2);
        if old <= 2 {
            self.kept.clear();
        }
        while self.kept.last().is_some_and(|&k| k > start) {
            self.kept.pop();
        }
        let stable = self.kept.len();
        if self.kept.is_empty() {
            self.kept.push(0);
        }
        for k in start..n.saturating_sub(2) {
            let a = self.raw[self.kept[self.kept.len() - 1]].point;
            let (b, c) = (self.raw[k + 1].point, self.raw[k + 2].point);
            let twice_area = (c.y - a.y) * (b.x - a.x) - (b.y - a.y) * (c.x - a.x);
            if twice_area.abs() >= threshold {
                self.kept.push(k + 1);
            }
        }
        if self.kept[self.kept.len() - 1] != n - 1 {
            self.kept.push(n - 1);
        }
        stable
    }

    /// Resampling: extends the arc lengths and the uniform samples to cover the kept points.
    fn resample(&mut self, stable: usize) {
        let step = self.step;
        self.arc.truncate(stable);
        // Samples beyond the last carried-over point lay on the old provisional tail.
        let carried = self
            .arc
            .last()
            .map_or(0, |a| (a / step).floor() as usize + 1);
        self.resampled.truncate(carried);
        if self.arc.is_empty() {
            self.arc.push(0.0);
        }
        if self.resampled.is_empty() {
            self.resampled.push(self.raw[self.kept[0]]);
        }
        for i in self.arc.len()..self.kept.len() {
            let (a, b) = (self.raw[self.kept[i - 1]], self.raw[self.kept[i]]);
            let total = self.arc[i - 1] + a.point.distance(b.point);
            let (from, to) = (self.arc[i - 1] / step, total / step);
            self.arc.push(total);
            while self.resampled.len() as f32 <= to.floor() {
                let k = self.resampled.len() as f32;
                let t = if to > from {
                    (k - from) / (to - from)
                } else {
                    0.0
                };
                self.resampled.push(Sample::lerp(a, b, t));
            }
        }
    }

    /// Gaussian averaging: computes the outputs that are new since the last update.
    fn filter(&mut self, stable: usize) {
        self.out.truncate(stable);
        let m = self.kept.len();
        if self.out.is_empty() {
            self.out.push(self.raw[self.kept[0]]);
        }
        // The end of the stroke counts as one more sample, so the average reaches the pen.
        let tail = self.raw[self.kept[m - 1]];
        let last_arc = self.arc[m - 1];
        let add_tail = last_arc / self.step > (self.resampled.len() - 1) as f32;
        if add_tail {
            self.resampled.push(tail);
        }
        for i in self.out.len()..m.saturating_sub(1) {
            let average = self.average(i);
            self.out.push(average);
        }
        if add_tail {
            self.resampled.pop();
        }
        if m > 1 {
            self.out.push(tail);
        }
    }

    /// The Gaussian-weighted mean around kept point `i`.
    fn average(&self, i: usize) -> Sample {
        let (sigma, step) = (self.sigma, self.step);
        let reach = ((sigma * 3.0 / step) as usize).min(self.resampled.len());
        let position = self.arc[i] / step;
        let floor = position.floor();
        let base = floor as isize;
        let last = self.resampled.len() as isize - 1;

        let mut mean = Mean::default();
        mean.add(self.raw[self.kept[i]], 1.0);
        // Weight that falls before the first or after the last sample goes to that sample.
        let (mut before, mut after) = (0.0_f32, 0.0_f32);
        let mut tap = |index: isize, distance: f32, mean: &mut Mean| {
            let weight = gaussian(distance, sigma);
            if index < 0 {
                before += weight;
            } else if index > last {
                after += weight;
            } else {
                mean.add(self.resampled[index as usize], weight);
            }
        };
        for k in 0..reach as isize {
            let offset = k as f32 * step;
            tap(base - k, step * (position - floor) + offset, &mut mean);
            tap(
                base + 1 + k,
                step * (floor + 1.0 - position) + offset,
                &mut mean,
            );
        }
        if before > 0.0 {
            mean.add(self.resampled[0], before);
        }
        if after > 0.0 {
            mean.add(self.resampled[last as usize], after);
        }
        mean.finish()
    }
}

/// Smooths a complete path in one go. The app does not use this for strokes it draws (those keep their live
/// output); it is the way to smooth a path that arrives all at once. `pressure` and `speed`, when given, must
/// have one value per point.
#[must_use]
pub fn smooth(
    raw: &[Point],
    pressure: Option<&[f32]>,
    speed: Option<&[f32]>,
    params: SmoothParams,
) -> Smoothed {
    debug_assert!(pressure.is_none_or(|p| p.len() == raw.len()));
    debug_assert!(speed.is_none_or(|v| v.len() == raw.len()));
    let mut smoother = LiveSmoother::new(params);
    for (i, &point) in raw.iter().enumerate() {
        smoother.add(point, pressure.map(|p| p[i]), speed.map(|v| v[i]));
    }
    smoother.update();
    smoother.output()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(smoothness: f32) -> SmoothParams {
        SmoothParams {
            smoothness,
            zoom: 1.0,
        }
    }

    /// Deterministic noise in `-1.0..1.0` so the tests never flake.
    fn noise(state: &mut u32) -> f32 {
        *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*state >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }

    fn wave(n: usize) -> Vec<Point> {
        let mut state = 7;
        (0..n)
            .map(|i| {
                let x = i as f32 * 4.5;
                Point::new(
                    x + noise(&mut state),
                    60.0 * (x / 70.0).sin() + noise(&mut state) * 1.3,
                )
            })
            .collect()
    }

    #[test]
    fn sigma_and_step_follow_the_smoothness_and_the_zoom() {
        let (sigma, step) = params(0.1).sigma_and_step();
        assert!((sigma - 30.0 * (1.0 / 0.9 - 1.0)).abs() < 1e-4);
        assert!(
            (step - sigma).abs() < 1e-6,
            "a light setting resamples at sigma"
        );

        let (sigma, step) = params(0.5).sigma_and_step();
        assert!((sigma - 30.0).abs() < 1e-4);
        assert!(
            (step - 5.0).abs() < 1e-6,
            "a heavy setting is capped at five pixels"
        );

        let zoomed = SmoothParams {
            smoothness: 0.5,
            zoom: 2.0,
        };
        assert!(
            (zoomed.sigma_and_step().0 - 15.0).abs() < 1e-4,
            "twice the zoom, half the canvas width"
        );
    }

    #[test]
    fn the_gaussian_table_matches_the_formula_and_stops_at_five_sigma() {
        assert!((gaussian(0.0, 2.0) - 1.0).abs() < 1e-6);
        assert!((gaussian(2.0, 2.0) - (-0.5_f32).exp()).abs() < 1e-3);
        assert!(
            (gaussian(-2.0, 2.0) - gaussian(2.0, 2.0)).abs() < 1e-6,
            "symmetric"
        );
        assert!(
            gaussian(100.0, 2.0) < 1e-4,
            "far away is nearly zero, and does not panic"
        );
    }

    #[test]
    fn no_smoothness_keeps_every_point() {
        let raw = wave(40);
        assert_eq!(smooth(&raw, None, None, params(0.0)).points, raw);
    }

    #[test]
    fn the_ends_do_not_move() {
        let raw = wave(60);
        let out = smooth(&raw, None, None, params(0.3)).points;
        assert_eq!(out.first(), raw.first());
        assert_eq!(out.last(), raw.last());
    }

    #[test]
    fn a_straight_line_stays_straight_and_is_thinned() {
        let raw: Vec<Point> = (0..50)
            .map(|i| Point::new(i as f32 * 5.0, i as f32 * 2.5))
            .collect();
        let out = smooth(&raw, None, None, params(0.3)).points;
        assert!(
            out.len() < raw.len() / 4,
            "collinear points are dropped: {}",
            out.len()
        );
        for p in &out {
            assert!((p.y - p.x / 2.0).abs() < 1e-2, "{p:?} left the line");
        }
    }

    #[test]
    fn jitter_is_reduced() {
        let truth = |x: f32| 60.0 * (x / 70.0).sin();
        let raw = wave(90);
        let rms = |pts: &[Point]| {
            (pts.iter().map(|p| (p.y - truth(p.x)).powi(2)).sum::<f32>() / pts.len() as f32).sqrt()
        };
        // 0.1 is a pen's usual setting; much heavier smoothing would flatten the wave itself.
        let out = smooth(&raw, None, None, params(0.1)).points;
        assert!(
            rms(&out) < rms(&raw) * 0.9,
            "{} vs {}",
            rms(&out),
            rms(&raw)
        );
    }

    #[test]
    fn output_already_drawn_never_changes_while_points_keep_arriving() {
        let raw = wave(300);
        for smoothness in [0.1, 0.25, 0.5] {
            let mut live = LiveSmoother::new(params(smoothness));
            let mut before: Vec<Point> = Vec::new();
            for (i, &p) in raw.iter().enumerate() {
                live.push(p, None, None);
                let now = live.output().points;
                // everything except the provisional last point must be exactly what it was
                let settled = before.len().saturating_sub(1);
                assert_eq!(
                    &now[..settled],
                    &before[..settled],
                    "smoothness {smoothness}: the path reshaped after point {i}"
                );
                before = now;
            }
        }
    }

    #[test]
    fn the_live_path_ends_at_the_latest_point() {
        let raw = wave(80);
        let mut live = LiveSmoother::new(params(0.3));
        for &p in &raw {
            live.push(p, None, None);
            assert_eq!(live.output().points.last(), Some(&p));
        }
    }

    #[test]
    fn pressure_is_carried_and_interpolated() {
        let raw: Vec<Point> = (0..40)
            .map(|i| Point::new(i as f32 * 6.0, (i as f32).sin() * 9.0))
            .collect();
        let pressure: Vec<f32> = (0..40).map(|i| i as f32 / 39.0).collect();
        let out = smooth(&raw, Some(&pressure), None, params(0.3));
        let pr = out.pressure.expect("pressure");
        assert_eq!(pr.len(), out.points.len());
        assert!((pr[0] - 0.0).abs() < 1e-6 && (pr[pr.len() - 1] - 1.0).abs() < 1e-6);
        assert!(
            pr.windows(2).all(|w| w[1] >= w[0] - 1e-3),
            "pressure should keep rising"
        );
    }

    #[test]
    fn tiny_inputs_pass_through() {
        for n in 0..3 {
            let raw: Vec<Point> = (0..n).map(|i| Point::new(i as f32, 0.0)).collect();
            assert_eq!(smooth(&raw, None, None, params(0.5)).points, raw);
        }
    }
}
