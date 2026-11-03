//! Pen speed, measured from the times and places the input points arrive.
//!
//! Speed is stored per point as a fraction of `0.0..=1.0` (in files, as a `u16`). Several pens read it: the
//! fountain pen is thinner the faster it moves, for example.
//!
//! For each new point the speed over the last interval is `|Δ screen position| / Δt`, normalized so that
//! 12 800 pixels per second is `1.0`, converted to canvas units by dividing by the zoom, and averaged over
//! the last three intervals.

/// Pixels per second that count as full speed.
const MAX_SPEED: f32 = 12_800.0;
/// Intervals averaged together.
const WINDOW: usize = 3;
/// Intervals shorter than this many seconds are ignored; they carry no usable speed.
const MIN_INTERVAL: f64 = 1e-4;

/// Measures the speed of a pen from timed positions.
#[derive(Debug, Clone, Default)]
pub struct SpeedFilter {
    last: Option<([f32; 2], f64)>,
    recent: [f32; WINDOW],
    count: usize,
}

impl SpeedFilter {
    /// Adds a position (canvas units) at `time` seconds, with the view `zoom` (screen pixels per canvas unit).
    /// Returns the smoothed speed fraction, which is `0.0` until two points with distinct times have arrived.
    pub fn push(&mut self, position: [f32; 2], time: f64, zoom: f32) -> f32 {
        let zoom = zoom.max(1e-6);
        if let Some((from, then)) = self.last {
            let dt = time - then;
            let moved = position.map(f32::to_bits) != from.map(f32::to_bits);
            if dt > MIN_INTERVAL && moved {
                let distance = (position[0] - from[0]).hypot(position[1] - from[1]);
                let view_speed = distance * zoom / dt as f32;
                let fraction = (view_speed.min(MAX_SPEED) / MAX_SPEED / zoom).clamp(0.0, 1.0);
                self.recent[self.count % WINDOW] = fraction;
                self.count += 1;
            }
        }
        self.last = Some((position, time));
        self.average()
    }

    fn average(&self) -> f32 {
        let n = self.count.min(WINDOW);
        if n == 0 {
            return 0.0;
        }
        (self.recent[..n].iter().sum::<f32>() / n as f32).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_point_has_no_speed() {
        assert_eq!(SpeedFilter::default().push([0.0, 0.0], 0.0, 1.0), 0.0);
    }

    #[test]
    fn a_steady_pace_gives_the_matching_fraction() {
        // 1280 pixels per second at zoom 1 is a tenth of full speed
        let mut f = SpeedFilter::default();
        let mut last = 0.0;
        for i in 0..10 {
            last = f.push([i as f32 * 12.8, 0.0], f64::from(i) * 0.01, 1.0);
        }
        assert!((last - 0.1).abs() < 1e-3, "{last}");
    }

    #[test]
    fn the_same_pace_at_a_higher_zoom_is_slower_in_canvas_units() {
        let run = |zoom: f32| {
            let mut f = SpeedFilter::default();
            let mut last = 0.0;
            for i in 0..10 {
                // the same screen motion, so half the canvas distance at twice the zoom
                last = f.push([i as f32 * 12.8 / zoom, 0.0], f64::from(i) * 0.01, zoom);
            }
            last
        };
        assert!((run(2.0) - run(1.0) / 2.0).abs() < 1e-3);
    }

    #[test]
    fn speed_is_capped_at_one() {
        let mut f = SpeedFilter::default();
        f.push([0.0, 0.0], 0.0, 1.0);
        assert_eq!(f.push([1.0e6, 0.0], 0.01, 1.0), 1.0);
    }

    #[test]
    fn identical_times_or_positions_do_not_disturb_the_average() {
        let mut f = SpeedFilter::default();
        f.push([0.0, 0.0], 0.0, 1.0);
        let moving = f.push([12.8, 0.0], 0.01, 1.0);
        assert_eq!(f.push([12.8, 0.0], 0.02, 1.0), moving, "same position");
        assert_eq!(
            f.push([25.6, 0.0], 0.02, 1.0),
            moving,
            "same time as the last point"
        );
    }

    #[test]
    fn only_the_last_three_intervals_count() {
        let mut f = SpeedFilter::default();
        let mut x = 0.0;
        let mut t = 0.0;
        // two fast intervals, then slow ones that should push them out of the window
        for step in [128.0, 128.0, 1.28, 1.28, 1.28, 1.28] {
            x += step;
            t += 0.01;
            f.push([x, 0.0], t, 1.0);
        }
        let slow = f.push([x + 1.28, 0.0], t + 0.01, 1.0);
        assert!((slow - 0.01).abs() < 1e-3, "{slow}");
    }
}
