//! OpenGL stroke renderer.
//!
//! A stroke is drawn in two stages:
//!
//! 1. its stamps are drawn as instanced quads into an offscreen *mask*, accumulating by `max` or by `over`;
//! 2. the mask is composited onto the target through the paper grain, which lets pigment through only where
//!    the mask is strong enough, tinted with the stroke color.
//!
//! The strokes already in the document are rendered into a cached texture that is redrawn only when the
//! document or the view changes, so drawing a new stroke costs one stroke and not the whole page.
//!
//! The renderer needs OpenGL 3.3 or OpenGL ES 3.0. It draws inside a callback of the host's own painter and
//! does not create a context.

mod renderer;
mod shaders;

pub use renderer::{Frame, GpuError, PixelRect, Placed, Renderer, Screen, View};
