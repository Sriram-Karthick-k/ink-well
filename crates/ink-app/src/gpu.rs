//! Connects the OpenGL stroke renderer to egui: prepares stamp batches and queues a paint callback.
//!
//! The batches are cached per stroke, so a frame only rebuilds the stroke being drawn.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{PaintCallback, Painter, Rect};
use eframe::egui_glow::CallbackFn;
use eframe::glow;
use ink_brush::{StrokeBatch, batch_for};
use ink_core::{Document, Stroke};
use ink_gpu::{Frame, GpuError, PixelRect, Placed, Renderer, Screen, View};
use uuid::Uuid;

use crate::render::{StrokeCache, smoothed};
use crate::state::ViewState;
use crate::textures::Mask;

/// A stroke's stamps and the opacity of its layer.
type Planned = (Arc<StrokeBatch>, f32);

/// Stamp batches of finished strokes, keyed by stroke and invalidated when the stroke's
/// [fingerprint](Stroke::fingerprint) changes, as it does when the stroke is moved, scaled or turned.
#[derive(Debug, Default)]
struct BatchCache {
    entries: HashMap<Uuid, (u64, Arc<StrokeBatch>)>,
}

impl BatchCache {
    fn get(&mut self, stroke: &Stroke, paths: &mut StrokeCache) -> Arc<StrokeBatch> {
        let key = stroke.fingerprint();
        if let Some((k, batch)) = self.entries.get(&stroke.id)
            && *k == key
        {
            return Arc::clone(batch);
        }
        let batch = Arc::new(batch_for(stroke, paths.get(stroke)));
        self.entries.insert(stroke.id, (key, Arc::clone(&batch)));
        batch
    }

    /// The strokes of every visible layer in paint order, each with its layer's opacity, except those in `hide`.
    fn plan(
        &mut self,
        document: &Document,
        paths: &mut StrokeCache,
        hide: &HashSet<Uuid>,
    ) -> Vec<Planned> {
        let mut planned = Vec::new();
        for layer in document.layers.iter().filter(|l| l.content && l.visible) {
            for stroke in layer.strokes.iter().filter(|s| !hide.contains(&s.id)) {
                planned.push((self.get(stroke, paths), layer.opacity));
            }
        }
        planned
    }
}

fn pixel_rect(r: &eframe::epaint::ViewportInPixels) -> PixelRect {
    PixelRect {
        x: r.left_px,
        y: r.from_bottom_px,
        width: r.width_px,
        height: r.height_px,
    }
}

/// The GPU renderer together with what it needs from the app.
#[derive(Debug)]
pub struct GpuScene {
    renderer: Arc<Mutex<Renderer>>,
    /// Set by the paint callback if drawing failed, so the app can fall back to the CPU renderer.
    failed: Arc<AtomicBool>,
    batches: BatchCache,
}

impl GpuScene {
    /// Compiles the shaders on `gl`, the context the window paints with, and uploads the stamp textures.
    pub fn new(gl: &glow::Context, masks: &[Mask]) -> Result<Self, GpuError> {
        let mut renderer = Renderer::new(gl)?;
        for mask in masks {
            if let Err(e) = renderer.set_mask(gl, mask.kind, mask.size, &mask.pixels) {
                eprintln!("could not use the {:?} texture: {e}", mask.kind);
            }
        }
        Ok(Self {
            renderer: Arc::new(Mutex::new(renderer)),
            failed: Arc::new(AtomicBool::new(false)),
            batches: BatchCache::default(),
        })
    }

    /// `false` once drawing has failed; the CPU renderer should be used from then on.
    pub fn is_working(&self) -> bool {
        !self.failed.load(Ordering::Relaxed)
    }

    /// Queues the drawing for `rect`: the document without the strokes in `hide`, then the `live` strokes (with
    /// their opacities) over it.
    #[allow(
        clippy::too_many_arguments,
        reason = "each argument is a distinct piece of the frame; a parameter struct would only rename them"
    )]
    pub fn paint(
        &mut self,
        painter: &Painter,
        rect: Rect,
        view: &ViewState,
        document: &Document,
        revision: u64,
        live: &[(Stroke, f32)],
        hide: &HashSet<Uuid>,
        paths: &mut StrokeCache,
    ) {
        let committed = self.batches.plan(document, paths, hide);
        let live: Vec<Planned> = live
            .iter()
            .map(|(s, opacity)| (Arc::new(batch_for(s, &smoothed(s))), *opacity))
            .collect();
        let (zoom, angle) = (view.zoom, view.angle);
        let origin = [view.origin.x - rect.min.x, view.origin.y - rect.min.y];
        let renderer = Arc::clone(&self.renderer);
        let failed = Arc::clone(&self.failed);

        let callback = CallbackFn::new(move |info, painter| {
            let ppp = info.pixels_per_point;
            let placed: Vec<Placed<'_>> = committed
                .iter()
                .map(|(batch, opacity)| Placed {
                    batch,
                    opacity: *opacity,
                })
                .collect();
            let live: Vec<Placed<'_>> = live
                .iter()
                .map(|(batch, opacity)| Placed {
                    batch,
                    opacity: *opacity,
                })
                .collect();
            let frame = Frame {
                screen: Screen {
                    framebuffer: painter.intermediate_fbo(),
                    viewport: pixel_rect(&info.viewport_in_pixels()),
                    clip: pixel_rect(&info.clip_rect_in_pixels()),
                },
                view: View {
                    zoom: zoom * ppp,
                    origin: [origin[0] * ppp, origin[1] * ppp],
                    angle,
                },
                pixels_per_point: ppp,
                revision,
                committed: &placed,
                live: &live,
            };
            let Ok(mut renderer) = renderer.lock() else {
                failed.store(true, Ordering::Relaxed);
                return;
            };
            if let Err(e) = renderer.paint(painter.gl(), &frame) {
                eprintln!("the GPU renderer failed, switching to the CPU renderer: {e}");
                failed.store(true, Ordering::Relaxed);
            }
        });
        painter.add(PaintCallback {
            rect,
            callback: Arc::new(callback),
        });
    }
}

#[cfg(test)]
mod tests {
    use ink_core::{Color, Editor, IdGen, Pen, Point, StrokeBuilder};

    use super::*;

    fn editor_with_strokes(n: usize) -> Editor {
        let mut editor = Editor::blank(IdGen::new(3));
        for k in 0..n {
            let mut b = StrokeBuilder::new(Pen::Pen, Some(4.0), Color::BLACK, 1.0, 0.5);
            for i in 0..12 {
                b.push(
                    Point::new(i as f32 * 6.0, k as f32 * 20.0),
                    None,
                    f64::from(i) * 0.01,
                );
            }
            let stroke = b.with_id(editor.next_id()).finish();
            editor.commit_stroke(0, stroke);
        }
        editor
    }

    #[test]
    fn every_visible_stroke_is_planned_in_order_with_its_layer_opacity() {
        let mut editor = editor_with_strokes(3);
        editor.set_layer_opacity(0, 0.4);
        let (mut cache, mut paths) = (BatchCache::default(), StrokeCache::default());
        let planned = cache.plan(editor.document(), &mut paths, &HashSet::new());
        assert_eq!(planned.len(), 3);
        assert!(planned.iter().all(|(_, o)| (*o - 0.4).abs() < 1e-6));
        let ys: Vec<f32> = planned.iter().map(|(b, _)| b.stamps[1]).collect();
        assert_eq!(ys, [0.0, 20.0, 40.0]);
    }

    #[test]
    fn a_hidden_layer_is_not_planned() {
        let mut editor = editor_with_strokes(2);
        editor.set_layer_visible(0, false);
        let (mut cache, mut paths) = (BatchCache::default(), StrokeCache::default());
        assert_eq!(
            cache
                .plan(editor.document(), &mut paths, &HashSet::new())
                .len(),
            0
        );
    }

    #[test]
    fn finished_strokes_are_prepared_once() {
        let editor = editor_with_strokes(2);
        let (mut cache, mut paths) = (BatchCache::default(), StrokeCache::default());
        let first = cache.plan(editor.document(), &mut paths, &HashSet::new());
        let second = cache.plan(editor.document(), &mut paths, &HashSet::new());
        for (a, b) in first.iter().zip(&second) {
            assert!(Arc::ptr_eq(&a.0, &b.0), "the batch must be reused");
        }
    }

    #[test]
    fn a_stroke_left_out_is_not_planned_and_the_others_keep_their_batches() {
        let editor = editor_with_strokes(3);
        let (mut cache, mut paths) = (BatchCache::default(), StrokeCache::default());
        let all = cache.plan(editor.document(), &mut paths, &HashSet::new());
        let middle = editor.document().layers[0].strokes[1].id;
        let without = cache.plan(editor.document(), &mut paths, &HashSet::from([middle]));
        assert_eq!(without.len(), 2);
        assert!(Arc::ptr_eq(&all[0].0, &without[0].0));
        assert!(Arc::ptr_eq(&all[2].0, &without[1].0));
    }

    #[test]
    fn a_stroke_bent_in_the_middle_is_prepared_again() {
        // The regression: a pull between the points a coarse cache key looks at left the old picture up.
        let mut editor = editor_with_strokes(1);
        // Plenty of points, the way a nudged stroke has them.
        let dense = ink_core::nudge::densify(&editor.document().layers[0].strokes[0], 1.0);
        assert!(dense.samples.len() > 20, "{}", dense.samples.len());
        editor.overwrite_stroke(0, 0, dense.clone());
        let (mut cache, mut paths) = (BatchCache::default(), StrokeCache::default());
        let before = cache.plan(editor.document(), &mut paths, &HashSet::new());
        let mut bent = dense;
        // Point 1 is not one a key made from a handful of points spread along the stroke would look at.
        bent.samples[1].point.y += 9.0;
        editor.overwrite_stroke(0, 0, bent);
        let after = cache.plan(editor.document(), &mut paths, &HashSet::new());
        assert!(
            !Arc::ptr_eq(&before[0].0, &after[0].0),
            "the change was not noticed"
        );
    }
}
