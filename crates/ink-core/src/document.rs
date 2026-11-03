//! The in-memory document: layers of strokes.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::geometry::{Color, Point};

/// Document metadata. Field names follow the saved JSON (`camelCase`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    /// Canvas background as a hex color such as `#F9F9F9`.
    pub background_solid_color: String,
    /// ISO 8601 UTC timestamp.
    pub creation_time: String,
    /// ISO 8601 UTC timestamp.
    pub modification_time: String,
    pub format_version: u32,
    pub identifier: Uuid,
}

/// The built-in pens. A saved drawing names them (`"Pen"`, `"Fountain"`, ...); a name this version does not know
/// is kept as [`Pen::Unknown`] with its number, if it has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pen {
    FixedWidth,
    Pen,
    Fountain,
    Airbrush,
    Dotted,
    Marker,
    Wire,
    Fill,
    Watercolor,
    SoftPencil,
    HardPencil,
    DynamicPen,
    Highlighter,
    BrushPen,
    FeltTip,
    Charcoal,
    Chalk,
    Crayon,
    Pastel,
    SprayPaint,
    Stipple,
    InkBrush,
    /// A pen this version does not know, as a number.
    Unknown(u32),
}

impl Pen {
    /// A number for the pen that stays the same from one run to the next: it seeds the random scatter of textured
    /// strokes and keys the caches.
    #[must_use]
    pub const fn id(self) -> u32 {
        match self {
            Self::FixedWidth => 1,
            Self::Pen => 2,
            Self::Fountain => 3,
            Self::Airbrush => 4,
            Self::Dotted => 5,
            Self::Marker => 6,
            Self::Wire => 7,
            Self::Fill => 8,
            Self::Watercolor => 9,
            Self::SoftPencil => 10,
            Self::HardPencil => 11,
            Self::DynamicPen => 12,
            Self::Highlighter => 13,
            Self::BrushPen => 14,
            Self::FeltTip => 15,
            Self::Charcoal => 16,
            Self::Chalk => 17,
            Self::Crayon => 18,
            Self::Pastel => 19,
            Self::SprayPaint => 20,
            Self::Stipple => 21,
            Self::InkBrush => 22,
            Self::Unknown(id) => id,
        }
    }

    /// The pen's display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FixedWidth => "Fixed Width",
            Self::Pen => "Pen",
            Self::Fountain => "Fountain",
            Self::Airbrush => "Airbrush",
            Self::Dotted => "Dotted",
            Self::Marker => "Marker",
            Self::Wire => "Wire",
            Self::Fill => "Fill",
            Self::Watercolor => "Watercolor",
            Self::SoftPencil => "Soft Pencil",
            Self::HardPencil => "Hard Pencil",
            Self::DynamicPen => "Dynamic Pen",
            Self::Highlighter => "Highlighter",
            Self::BrushPen => "Brush Pen",
            Self::FeltTip => "Felt Tip",
            Self::Charcoal => "Charcoal",
            Self::Chalk => "Chalk",
            Self::Crayon => "Crayon",
            Self::Pastel => "Pastel",
            Self::SprayPaint => "Spray Paint",
            Self::Stipple => "Stipple",
            Self::InkBrush => "Ink Brush",
            Self::Unknown(_) => "Unknown",
        }
    }
}

/// One raw input sample as stored: a position and four quantized attributes (time, pressure, altitude and
/// azimuth, each scaled to `0..=65535`); see `docs/FORMAT.md`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RawSample {
    pub point: Point,
    pub attrs: [u16; 4],
}

/// A single stroke.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub id: Uuid,
    pub pen: Pen,
    /// Size at draw time; `None` for pens without a width (Fill).
    pub width: Option<f32>,
    pub color: Color,
    /// The stored path points (often a resampled line, or a single anchor for freehand strokes).
    pub path: Vec<Point>,
    /// The raw input samples.
    pub samples: Vec<RawSample>,
    /// Pressure `0.0..=1.0`, one value per sample, for strokes drawn with a pressure-sensitive pen.
    /// Empty when the input device had no pressure (a mouse).
    #[serde(default)]
    pub pressure: Vec<f32>,
}

impl Stroke {
    /// The points to draw: the raw samples when there are at least two, otherwise the stored path.
    #[must_use]
    pub fn geometry(&self) -> Vec<Point> {
        self.points().collect()
    }

    /// The same points as [`Stroke::geometry`], without copying them: for the hit tests, bounds and cache keys
    /// that run over every stroke on every frame.
    pub fn points(&self) -> impl Iterator<Item = Point> + '_ {
        let (samples, path): (&[RawSample], &[Point]) = if self.samples.len() >= 2 {
            (&self.samples, &[])
        } else {
            (&[], &self.path)
        };
        samples.iter().map(|s| s.point).chain(path.iter().copied())
    }
}

/// A layer's display name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerLabel {
    /// A name chosen by the user.
    Name(String),
    /// A number the layer is called by: `Layer 3`.
    Counter(i64),
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: Uuid,
    /// `true` for layers that hold strokes. Every layer the app makes does.
    pub content: bool,
    pub label: LayerLabel,
    pub visible: bool,
    /// `0.0..=1.0`.
    pub opacity: f32,
    pub strokes: Vec<Stroke>,
}

impl Layer {
    /// The name to show in a layer list.
    #[must_use]
    pub fn display_name(&self) -> String {
        match &self.label {
            LayerLabel::Name(name) => name.clone(),
            LayerLabel::Counter(n) => format!("Layer {n}"),
            LayerLabel::None => "Layer".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub metadata: Metadata,
    pub layers: Vec<Layer>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(samples: usize, path: usize) -> Stroke {
        let pt = |i: usize| Point::new(i as f32, 0.0);
        Stroke {
            id: Uuid::nil(),
            pen: Pen::Pen,
            width: Some(3.0),
            color: Color::BLACK,
            path: (0..path).map(pt).collect(),
            samples: (0..samples)
                .map(|i| RawSample {
                    point: pt(i + 100),
                    attrs: [0; 4],
                })
                .collect(),
            pressure: Vec::new(),
        }
    }

    #[test]
    fn geometry_prefers_samples_with_two_or_more() {
        assert_eq!(stroke(3, 5).geometry()[0], Point::new(100.0, 0.0));
        assert_eq!(stroke(1, 5).geometry().len(), 5);
        assert_eq!(stroke(0, 0).geometry().len(), 0);
    }
}
