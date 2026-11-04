//! Brush parameters and CPU-side stamp placement.
//!
//! A brush paints a stroke as a run of *stamps*: a shape texture placed along the smoothed path with a
//! size, rotation, offset and opacity. This crate decides where stamps go. Drawing them, and revealing a
//! grain texture through the accumulated mask, is the renderer's job.
//!
//! How a brush's parameters translate into stamp spacing, size and opacity is reverse-engineered and
//! partly assumed; each assumption is listed in [`stamps`].

pub mod fill;
pub mod grain;
pub mod params;
pub mod pens;
pub mod stamps;

pub use params::BrushParams;
pub use pens::{
    Accumulate, MARKER_GAIN, MaskKind, PenStyle, PressureAlpha, STAMP_STRIDE, SpeedAlpha,
    StrokeBatch, batch_for, style_for,
};
pub use stamps::{Stamp, StampInput, generate_stamps, seed_from_bytes};
