//! A procedural paper grain for the pencil and watercolor pens.
//!
//! The texture is made here instead of shipped as an image: the project bundles no third-party art. It
//! wraps at its edges, so it tiles without seams.

/// Width and height of the texture in pixels.
pub const SIZE: usize = 256;

/// A deterministic pseudo-random value in `0.0..=1.0` for a pixel.
fn hash(x: usize, y: usize) -> f32 {
    let mut h =
        (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1) ^ 0x9e37_79b9;
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65_535.0
}

/// Box blur with the given `radius`, wrapping at the edges.
fn blur(values: &[f32], radius: usize) -> Vec<f32> {
    let window = (2 * radius + 1) as f32;
    let wrap = |i: usize, d: isize| (i as isize + d).rem_euclid(SIZE as isize) as usize;
    let horizontal: Vec<f32> = (0..SIZE * SIZE)
        .map(|i| {
            let (x, y) = (i % SIZE, i / SIZE);
            (-(radius as isize)..=radius as isize)
                .map(|d| values[y * SIZE + wrap(x, d)])
                .sum::<f32>()
                / window
        })
        .collect();
    (0..SIZE * SIZE)
        .map(|i| {
            let (x, y) = (i % SIZE, i / SIZE);
            (-(radius as isize)..=radius as isize)
                .map(|d| horizontal[wrap(y, d) * SIZE + x])
                .sum::<f32>()
                / window
        })
        .collect()
}

/// The grain as `SIZE * SIZE` bytes, row by row, stretched to use the full `0..=255` range.
#[must_use]
pub fn paper_grain() -> Vec<u8> {
    let fine: Vec<f32> = (0..SIZE * SIZE).map(|i| hash(i % SIZE, i / SIZE)).collect();
    let medium = blur(&fine, 1);
    let coarse = blur(&fine, 3);
    let mixed: Vec<f32> = (0..fine.len())
        .map(|i| 0.45 * fine[i] + 0.35 * medium[i] + 0.2 * coarse[i])
        .collect();
    let (low, high) = mixed
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    mixed
        .iter()
        .map(|&v| ((v - low) / (high - low) * 255.0).round() as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_texture_has_the_promised_size_and_is_deterministic() {
        let grain = paper_grain();
        assert_eq!(grain.len(), SIZE * SIZE);
        assert_eq!(grain, paper_grain());
    }

    #[test]
    fn the_grain_uses_the_whole_range_and_is_centered() {
        let grain = paper_grain();
        assert_eq!(grain.iter().min(), Some(&0));
        assert_eq!(grain.iter().max(), Some(&255));
        let mean = grain.iter().map(|&v| f32::from(v)).sum::<f32>() / grain.len() as f32;
        assert!((100.0..155.0).contains(&mean), "mean {mean}");
    }

    #[test]
    fn neighbouring_pixels_are_correlated_but_not_equal() {
        let grain = paper_grain();
        let diff: f32 = (0..SIZE * SIZE - 1)
            .map(|i| (f32::from(grain[i]) - f32::from(grain[i + 1])).abs())
            .sum::<f32>()
            / (SIZE * SIZE) as f32;
        assert!(diff > 2.0, "not flat: {diff}");
        assert!(diff < 60.0, "smoother than white noise: {diff}");
    }
}
