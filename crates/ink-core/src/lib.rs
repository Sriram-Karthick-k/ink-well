//! Document model and stroke algorithms for inkwell.
//!
//! This crate is pure Rust: no I/O, no GPU, no platform code. Everything that touches a file format or a
//! screen lives in other crates and depends on this one.

pub mod artboard;
pub mod builder;
pub mod document;
pub mod editor;
pub mod erase;
pub mod geometry;
pub mod guide;
pub mod hit;
pub mod ids;
pub mod marks;
pub mod nudge;
pub mod recognize;
pub mod slice;
pub mod smoothing;
pub mod snap;
pub mod speed;
pub mod transform;
pub mod units;

pub use builder::StrokeBuilder;
pub use document::{Document, Layer, LayerLabel, Metadata, Pen, RawSample, Stroke};
pub use editor::Editor;
pub use geometry::{Color, Point};
pub use ids::IdGen;
pub use transform::Affine;
