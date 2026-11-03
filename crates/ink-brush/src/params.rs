//! Brush parameters, as stored in a brush pack's `brush_object` documents.

use serde::{Deserialize, Serialize};

/// The parameters of one brush. Field names follow the stored JSON (`camelCase`); keys this struct does
/// not know are ignored, and missing keys take the defaults documented on each field.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BrushParams {
    /// The pen size a brush starts at.
    pub default_width: Option<f32>,
    /// The opacity a brush starts at, `0.0..=1.0`.
    pub default_alpha: Option<f32>,
    pub default_smoothness: Option<f32>,

    /// Distance between stamps as a fraction of the pen size. Defaults to `0.1` when absent.
    pub min_shape_spacing: Option<f32>,
    pub max_shape_spacing: Option<f32>,
    /// Random offset perpendicular to the path, as a fraction of the stamp size.
    pub min_shape_scatter: f32,
    pub max_shape_scatter: f32,
    /// Stamp rotation range in radians.
    pub min_rotation: f32,
    pub max_rotation: f32,
    /// Adds the path direction to every stamp's rotation.
    pub rotate_along_path: bool,

    /// Texture ids of the stamp shapes; one is picked per stamp.
    pub shape_textures: Vec<String>,
    /// Texture id of the grain revealed through the stroke mask.
    pub grain_texture: Option<String>,
    /// World units per grain texel. Defaults to `1.0` when absent.
    pub grain_scale: Option<f32>,
    pub grain_rotation: f32,
    /// `true` anchors the grain to the canvas, `false` to the stroke start.
    pub lock_grain_position: Option<bool>,

    /// Multiplier curves over pressure `0.0..=1.0`, sampled with linear interpolation.
    pub size_variance_pressure: Vec<f32>,
    pub opacity_variance_pressure: Vec<f32>,
    pub spacing_variance_pressure: Vec<f32>,
    pub size_variance_speed: Vec<f32>,
    pub opacity_variance_speed: Vec<f32>,
    pub size_variance_tilt: Vec<f32>,
}

/// Samples a curve at `t` (clamped to `0.0..=1.0`) with linear interpolation. An empty curve is neutral
/// (`1.0`).
#[must_use]
pub fn sample_curve(curve: &[f32], t: f32) -> f32 {
    match curve {
        [] => 1.0,
        [only] => *only,
        _ => {
            let x = t.clamp(0.0, 1.0) * (curve.len() - 1) as f32;
            let i = x.floor() as usize;
            let next = (i + 1).min(curve.len() - 1);
            curve[i] + (curve[next] - curve[i]) * (x - i as f32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stored_json_and_ignores_unknown_keys() {
        let json = r#"{
            "defaultWidth": 70, "defaultAlpha": 0.67, "minShapeSpacing": 0.05,
            "rotateAlongPath": true, "shapeTextures": ["a", "b"],
            "sizeVariancePressure": [0.1, 1.0], "somethingNew": {"x": 1}
        }"#;
        let p: BrushParams = serde_json::from_str(json).expect("valid");
        assert_eq!(p.default_width, Some(70.0));
        assert_eq!(p.min_shape_spacing, Some(0.05));
        assert_eq!(p.max_shape_spacing, None);
        assert!(p.rotate_along_path);
        assert_eq!(p.shape_textures, ["a", "b"]);
        assert_eq!(p.size_variance_pressure, [0.1, 1.0]);
    }

    #[test]
    fn empty_object_gives_defaults() {
        let p: BrushParams = serde_json::from_str("{}").expect("valid");
        assert_eq!(p, BrushParams::default());
    }

    #[test]
    fn curves_interpolate_and_clamp() {
        let c = [0.0, 1.0, 3.0];
        assert!((sample_curve(&c, 0.25) - 0.5).abs() < 1e-6);
        assert!((sample_curve(&c, 0.75) - 2.0).abs() < 1e-6);
        assert!((sample_curve(&c, -1.0) - 0.0).abs() < 1e-6);
        assert!((sample_curve(&c, 9.0) - 3.0).abs() < 1e-6);
        assert!((sample_curve(&[], 0.5) - 1.0).abs() < 1e-6);
    }
}
