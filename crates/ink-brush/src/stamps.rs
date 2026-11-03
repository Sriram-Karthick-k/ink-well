//! Stamp placement along a smoothed path.
//!
//! Assumptions:
//!
//! - spacing is a fraction of the pen size, taken as the mean of the min and max values; the spacing,
//!   speed and tilt curves are not applied yet,
//! - size and opacity curves are multipliers over pressure, applied only when pressure is supplied,
//! - rotations are radians, and scatter is a fraction of the stamp size,
//! - randomness is deterministic: the same seed gives the same stamps on every platform.

use ink_core::Point;

use crate::params::{BrushParams, sample_curve};

/// Hard cap on stamps per stroke, so a huge path cannot exhaust memory.
const MAX_STAMPS: f32 = 20_000.0;
/// Smallest allowed distance between stamps, in canvas units.
const MIN_SPACING: f32 = 0.3;

/// One stamp to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stamp {
    pub position: Point,
    /// Diameter in canvas units.
    pub size: f32,
    /// Radians.
    pub rotation: f32,
    /// `0.0..=1.0`.
    pub alpha: f32,
    /// Index into the brush's shape textures.
    pub shape: usize,
}

/// What stamp placement needs to know about the stroke.
#[derive(Debug, Clone, Copy)]
pub struct StampInput<'a> {
    /// The smoothed path.
    pub path: &'a [Point],
    /// One pressure value per path point, when the input device reports pressure.
    pub pressure: Option<&'a [f32]>,
    /// Pen size in canvas units.
    pub width: f32,
    /// Seed for the stroke's random choices; see [`seed_from_bytes`].
    pub seed: u32,
}

/// A small deterministic PRNG (mulberry32), so results never depend on a platform's random source.
struct Rng(u32);

impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 0x3D));
        f64::from(t ^ (t >> 14)) as f32 / 4_294_967_296.0
    }
}

/// A stable seed from arbitrary bytes (for example a stroke id), using FNV-1a.
#[must_use]
pub fn seed_from_bytes(bytes: &[u8]) -> u32 {
    bytes.iter().fold(2_166_136_261_u32, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(16_777_619)
    })
}

/// Places stamps along `input.path`. `shape_count` is the number of loaded shape textures (at least 1 is
/// required to get any stamps).
#[must_use]
pub fn generate_stamps(
    input: StampInput<'_>,
    params: &BrushParams,
    shape_count: usize,
) -> Vec<Stamp> {
    let path = input.path;
    if path.is_empty() || shape_count == 0 {
        return Vec::new();
    }

    let mut cumulative = vec![0.0_f32];
    for pair in path.windows(2) {
        cumulative.push(cumulative[cumulative.len() - 1] + pair[0].distance(pair[1]));
    }
    let total = cumulative[cumulative.len() - 1];

    let min_spacing = params.min_shape_spacing.unwrap_or(0.1);
    let max_spacing = params.max_shape_spacing.unwrap_or(min_spacing);
    let spacing = (f32::midpoint(min_spacing, max_spacing) * input.width)
        .max(MIN_SPACING)
        .max(total / MAX_STAMPS);

    let mut rng = Rng(input.seed);
    let last = path.len() - 1;
    let mut stamps = Vec::new();
    let mut seg = 0;
    let mut d = 0.0_f32;
    while d <= total + 1e-6 {
        while seg + 1 < last && cumulative[seg + 1] < d {
            seg += 1;
        }
        let (a, b) = (path[seg], path[(seg + 1).min(last)]);
        let span = cumulative[(seg + 1).min(last)] - cumulative[seg];
        let t = if span > 0.0 {
            (d - cumulative[seg]) / span
        } else {
            0.0
        };
        let direction = (b.y - a.y).atan2(b.x - a.x);

        let pressure = input
            .pressure
            .and_then(|p| p.get(seg.min(p.len().saturating_sub(1))).copied());
        let size =
            input.width * pressure.map_or(1.0, |p| sample_curve(&params.size_variance_pressure, p));
        let alpha = pressure
            .map_or(1.0, |p| sample_curve(&params.opacity_variance_pressure, p))
            .min(1.0);

        let scatter_span = params.max_shape_scatter - params.min_shape_scatter;
        let mut scatter = (params.min_shape_scatter + rng.next_f32() * scatter_span) * size;
        if rng.next_f32() < 0.5 {
            scatter = -scatter;
        }
        let rotation = if params.rotate_along_path {
            direction
        } else {
            0.0
        } + params.min_rotation
            + rng.next_f32() * (params.max_rotation - params.min_rotation);

        stamps.push(Stamp {
            position: Point::new(
                a.x + (b.x - a.x) * t - direction.sin() * scatter,
                a.y + (b.y - a.y) * t + direction.cos() * scatter,
            ),
            size,
            rotation,
            alpha,
            shape: ((rng.next_f32() * shape_count as f32) as usize).min(shape_count - 1),
        });
        d += spacing;
    }
    stamps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(len: f32) -> Vec<Point> {
        vec![Point::new(0.0, 0.0), Point::new(len, 0.0)]
    }

    fn input(path: &[Point]) -> StampInput<'_> {
        StampInput {
            path,
            pressure: None,
            width: 10.0,
            seed: 1,
        }
    }

    #[test]
    fn stamps_follow_a_straight_line_at_the_requested_spacing() {
        let path = line(100.0);
        let stamps = generate_stamps(input(&path), &BrushParams::default(), 1);
        // default spacing is 0.1 of the size: one stamp per unit over 100 units
        assert!((100..=102).contains(&stamps.len()), "{}", stamps.len());
        for pair in stamps.windows(2) {
            assert!(pair[1].position.x > pair[0].position.x);
            assert!(pair[1].position.y.abs() < 1e-4, "no scatter was requested");
        }
    }

    #[test]
    fn rotate_along_path_follows_the_direction() {
        let path = vec![Point::new(0.0, 0.0), Point::new(0.0, 50.0)];
        let params = BrushParams {
            rotate_along_path: true,
            ..BrushParams::default()
        };
        let stamps = generate_stamps(input(&path), &params, 1);
        for s in stamps {
            assert!((s.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        }
    }

    #[test]
    fn same_seed_same_stamps_different_seed_different_stamps() {
        let path = line(60.0);
        let params = BrushParams {
            max_shape_scatter: 0.5,
            max_rotation: 3.0,
            ..BrushParams::default()
        };
        let a = generate_stamps(input(&path), &params, 4);
        let b = generate_stamps(input(&path), &params, 4);
        let c = generate_stamps(
            StampInput {
                seed: 2,
                ..input(&path)
            },
            &params,
            4,
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn pressure_curves_scale_size_and_opacity() {
        let path = line(20.0);
        let pressure = [0.5, 0.5];
        let params = BrushParams {
            size_variance_pressure: vec![0.0, 1.0],
            opacity_variance_pressure: vec![0.0, 2.0],
            ..BrushParams::default()
        };
        let stamps = generate_stamps(
            StampInput {
                pressure: Some(&pressure),
                ..input(&path)
            },
            &params,
            1,
        );
        assert!(
            (stamps[0].size - 5.0).abs() < 1e-5,
            "half pressure halves the size"
        );
        assert!(
            (stamps[0].alpha - 1.0).abs() < 1e-5,
            "opacity is capped at 1"
        );
    }

    #[test]
    fn shape_indices_stay_in_range() {
        let path = line(200.0);
        for s in generate_stamps(input(&path), &BrushParams::default(), 3) {
            assert!(s.shape < 3);
        }
    }

    #[test]
    fn degenerate_paths_are_handled() {
        let params = BrushParams::default();
        assert_eq!(generate_stamps(input(&[]), &params, 1), Vec::<Stamp>::new());
        assert_eq!(
            generate_stamps(input(&line(10.0)), &params, 0),
            Vec::<Stamp>::new()
        );
        let one = [Point::new(3.0, 4.0)];
        assert_eq!(generate_stamps(input(&one), &params, 1).len(), 1);
    }
}
