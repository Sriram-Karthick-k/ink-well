//! WebAssembly bindings. Everything here only converts types for JavaScript; the logic lives in
//! `ink-core`, `ink-format` and `ink-brush`.

use ink_core::Point;
use ink_core::smoothing::{SmoothParams, smooth};
use wasm_bindgen::prelude::*;

/// Reads a `.ink` file and returns the document (its layers and strokes) as a JSON string.
#[wasm_bindgen]
pub fn read_document_json(bytes: &[u8]) -> Result<String, JsError> {
    let drawing = ink_format::read(bytes)?;
    Ok(serde_json::to_string(&drawing.document)?)
}

/// Smooths a finished stroke. `xy` is interleaved `[x0, y0, x1, y1, ...]`; the
/// result uses the same layout. `smoothness` is the pen's setting (`0.0` to `1.0`) and `zoom` the screen
/// pixels per canvas unit the stroke was drawn at. A trailing unpaired value is ignored.
#[wasm_bindgen]
#[must_use]
pub fn smooth_points(xy: &[f32], smoothness: f32, zoom: f32) -> Vec<f32> {
    let points: Vec<Point> = xy
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| Point::new(c[0], c[1]))
        .collect();
    let params = SmoothParams { smoothness, zoom };
    smooth(&points, None, None, params)
        .points
        .iter()
        .flat_map(|p| [p.x, p.y])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_points_round_trips_the_layout() {
        let xy: Vec<f32> = (0..20)
            .flat_map(|i| [i as f32 * 5.0, i as f32 * 2.5])
            .collect();
        let out = smooth_points(&xy, 0.3, 1.0);
        assert_eq!(out.len() % 2, 0, "output stays interleaved");
        assert_eq!(&out[..2], &xy[..2], "first point is fixed");
        assert_eq!(
            &out[out.len() - 2..],
            &xy[xy.len() - 2..],
            "last point is fixed"
        );
    }

    #[test]
    fn an_unpaired_trailing_value_is_ignored() {
        assert!(smooth_points(&[1.0, 2.0, 3.0], 0.3, 1.0).len() <= 4);
    }
}
