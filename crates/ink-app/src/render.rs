//! Draws strokes with egui's painter on the CPU.
//!
//! This is the fallback renderer, used when the OpenGL one (`ink-gpu`) cannot start or with `--cpu`. Every pen
//! is a smoothed polyline or ribbon with a pen-specific style: the stroke shapes are right and drawn in one
//! piece, but the textured pens (grain, stamps) only look right on the GPU.

use std::collections::{HashMap, HashSet};

use eframe::egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, epaint::PathStroke, pos2};
use ink_core::smoothing::Smoothed;
use ink_core::{Color, Document, Pen, Point, RawSample, Stroke};
use lyon_tessellation::geom::point;
use lyon_tessellation::path::Path;
use lyon_tessellation::{
    BuffersBuilder, LineCap, LineJoin, StrokeOptions, StrokeTessellator, StrokeVertex,
    VertexBuffers,
};
use uuid::Uuid;

use crate::state::ViewState;

/// Screen points closer than this are merged before drawing; it keeps dense paths cheap.
const MIN_POINT_GAP_PX: f32 = 0.6;
/// Lines at least this wide are tessellated with round joins; thinner ones use egui's feathered line.
const THICK_LINE_PX: f32 = 2.5;

/// The path of a stroke. A stroke's points are already smoothed (the app smooths a stroke as it is drawn, and
/// files store the result), so they are used as they are. Speed and pressure come from the samples' attributes;
/// pressure only counts when it varies, since a mouse stores a constant.
#[must_use]
pub fn smoothed(stroke: &Stroke) -> Smoothed {
    let points = stroke.geometry();
    let sampled = stroke.samples.len() >= 2 && stroke.samples.len() == points.len();
    let speed = sampled.then(|| {
        stroke
            .samples
            .iter()
            .map(|s| f32::from(s.attrs[0]) / 65_535.0)
            .collect()
    });
    let pressure = if !stroke.pressure.is_empty() && stroke.pressure.len() == points.len() {
        Some(stroke.pressure.clone())
    } else if sampled && stroke.samples.iter().any(|s| s.attrs[1] != 32_767) {
        Some(
            stroke
                .samples
                .iter()
                .map(|s| f32::from(s.attrs[1]) / 65_535.0)
                .collect(),
        )
    } else {
        None
    };
    Smoothed {
        points,
        pressure,
        speed,
    }
}

/// Smoothed paths of finished strokes, so they are not recomputed every frame. An entry is thrown away when
/// the stroke's [fingerprint](Stroke::fingerprint) changes, as it does when the stroke is moved.
#[derive(Debug, Default)]
pub struct StrokeCache {
    entries: HashMap<Uuid, (u64, Smoothed)>,
}

impl StrokeCache {
    pub fn get(&mut self, stroke: &Stroke) -> &Smoothed {
        let key = stroke.fingerprint();
        if self.entries.get(&stroke.id).is_none_or(|(k, _)| *k != key) {
            self.entries.insert(stroke.id, (key, smoothed(stroke)));
        }
        &self.entries[&stroke.id].1
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

fn color32(c: Color, opacity: f32) -> Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(c.r), byte(c.g), byte(c.b), byte(c.a * opacity))
}

/// Screen points for `points`, merging points closer than [`MIN_POINT_GAP_PX`]. The second vector holds
/// the index of each kept point in the original path, so per-point data (pressure) can follow along.
fn to_screen(view: &ViewState, points: &[Point]) -> (Vec<Pos2>, Vec<usize>) {
    let mut out: Vec<Pos2> = Vec::with_capacity(points.len());
    let mut index = Vec::with_capacity(points.len());
    for (i, p) in points.iter().enumerate() {
        let s = view.to_screen(*p);
        let last_point = i + 1 == points.len();
        if last_point || out.last().is_none_or(|l| l.distance(s) >= MIN_POINT_GAP_PX) {
            out.push(s);
            index.push(i);
        }
    }
    (out, index)
}

fn solid(painter: &Painter, pts: &[Pos2], width: f32, color: Color32, round_caps: bool) {
    match pts {
        [] => {}
        [only] => {
            painter.add(Shape::circle_filled(*only, width / 2.0, color));
        }
        _ if width >= THICK_LINE_PX => thick_line(painter, pts, width, color),
        _ => {
            // egui's own line is feathered, which suits thin lines.
            painter.add(Shape::line(pts.to_vec(), PathStroke::new(width, color)));
            if round_caps && color.a() == 255 {
                for end in [pts[0], pts[pts.len() - 1]] {
                    painter.add(Shape::circle_filled(end, width / 2.0, color));
                }
            }
        }
    }
}

/// A thick polyline with round joins and caps, tessellated by lyon. egui's own line has mitered corners,
/// which spike where a stroke turns sharply (a flick at the end of a stroke, for example).
fn thick_line(painter: &Painter, pts: &[Pos2], width: f32, color: Color32) {
    let mut path = Path::builder();
    path.begin(point(pts[0].x, pts[0].y));
    for p in &pts[1..] {
        path.line_to(point(p.x, p.y));
    }
    path.end(false);
    let options = StrokeOptions::default()
        .with_line_width(width)
        .with_line_join(LineJoin::Round)
        .with_line_cap(LineCap::Round)
        .with_tolerance(0.15);
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let result = StrokeTessellator::new().tessellate_path(
        &path.build(),
        &options,
        &mut BuffersBuilder::new(&mut buffers, |v: StrokeVertex| v.position().to_array()),
    );
    if result.is_err() {
        return;
    }
    let mut mesh = Mesh::default();
    for p in &buffers.vertices {
        mesh.colored_vertex(pos2(p[0], p[1]), color);
    }
    mesh.indices = buffers.indices;
    painter.add(Shape::mesh(mesh));
}

/// The inside of the polygon `points`, in canvas units, in one flat color, with a feathered line round the edge so it is
/// not jagged. Fewer than three points are a line, as they were before regions.
fn region(painter: &Painter, view: &ViewState, points: &[Point], color: Color32) {
    let triangles = ink_brush::fill::triangulate(points);
    if triangles.is_empty() {
        let (pts, _) = to_screen(view, points);
        solid(painter, &pts, 1.0, color, true);
        return;
    }
    let mut mesh = Mesh::default();
    for v in triangles.as_chunks::<2>().0 {
        mesh.colored_vertex(view.to_screen(Point::new(v[0], v[1])), color);
    }
    mesh.indices = (0..mesh.vertices.len() as u32).collect();
    painter.add(Shape::mesh(mesh));
    let mut edge: Vec<Pos2> = points.iter().map(|p| view.to_screen(*p)).collect();
    edge.push(edge[0]);
    painter.add(Shape::line(edge, PathStroke::new(1.0, color)));
}

/// One filled ribbon (left edge, right edge, round caps) so a pressure-varying stroke has no joins.
fn ribbon(painter: &Painter, pts: &[Pos2], widths: &[f32], color: Color32) {
    let n = pts.len();
    if n < 2 {
        solid(
            painter,
            pts,
            widths.first().copied().unwrap_or(1.0),
            color,
            true,
        );
        return;
    }
    let mut mesh = Mesh::default();
    for i in 0..n {
        let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(n - 1)]);
        let len = a.distance(b).max(1e-6);
        let (nx, ny) = (-(b.y - a.y) / len, (b.x - a.x) / len);
        let h = widths[i] / 2.0;
        mesh.colored_vertex(pos2(pts[i].x + nx * h, pts[i].y + ny * h), color);
        mesh.colored_vertex(pos2(pts[i].x - nx * h, pts[i].y - ny * h), color);
        if i > 0 {
            let base = (i as u32 - 1) * 2;
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base + 1, base + 3, base + 2);
        }
    }
    painter.add(Shape::mesh(mesh));
    for i in [0, n - 1] {
        painter.add(Shape::circle_filled(pts[i], widths[i] / 2.0, color));
    }
}

/// Alpha across a soft stroke, from one edge (-1) to the other (+1): opaque in the middle, fading out.
const SOFT_PROFILE: [(f32, f32); 7] = [
    (-1.0, 0.0),
    (-0.75, 0.12),
    (-0.4, 0.55),
    (0.0, 1.0),
    (0.4, 0.55),
    (0.75, 0.12),
    (1.0, 0.0),
];

/// A soft-edged stroke: a ribbon whose vertex colors fade to transparent at the edges, with round soft ends,
/// so the graphics card interpolates a smooth falloff.
fn airbrush(painter: &Painter, pts: &[Pos2], width: f32, color: Color32) {
    let n = pts.len();
    let h = width / 2.0;
    let tint = |alpha: f32| color.gamma_multiply(alpha * 0.85);
    let mut mesh = Mesh::default();
    if n >= 2 {
        for i in 0..n {
            let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(n - 1)]);
            let len = a.distance(b).max(1e-6);
            let (nx, ny) = (-(b.y - a.y) / len, (b.x - a.x) / len);
            for (offset, alpha) in SOFT_PROFILE {
                let pos = pos2(pts[i].x + nx * h * offset, pts[i].y + ny * h * offset);
                mesh.colored_vertex(pos, tint(alpha));
            }
            if i > 0 {
                let row = SOFT_PROFILE.len() as u32;
                let base = (i as u32 - 1) * row;
                for j in 0..row - 1 {
                    mesh.add_triangle(base + j, base + j + 1, base + row + j);
                    mesh.add_triangle(base + j + 1, base + row + j + 1, base + row + j);
                }
            }
        }
    }
    painter.add(Shape::mesh(mesh));
    let behind_start = if n >= 2 {
        pts[1]
    } else {
        pts[0] + eframe::egui::vec2(1.0, 0.0)
    };
    let behind_end = if n >= 2 {
        pts[n - 2]
    } else {
        pts[0] - eframe::egui::vec2(1.0, 0.0)
    };
    soft_cap(painter, pts[0], behind_start, h, &tint);
    soft_cap(painter, pts[n - 1], behind_end, h, &tint);
}

/// A half-disc that continues a soft stroke past its end with the same fade as the body, so the two join
/// without overlapping. `toward` is the neighboring path point; the cap faces away from it.
fn soft_cap(painter: &Painter, center: Pos2, toward: Pos2, h: f32, tint: &dyn Fn(f32) -> Color32) {
    const STEPS: u32 = 14;
    const RINGS: [(f32, f32); 4] = [(0.0, 1.0), (0.4, 0.55), (0.75, 0.12), (1.0, 0.0)];
    let outward = center - toward;
    let facing = outward.y.atan2(outward.x);
    let mut mesh = Mesh::default();
    for (radius, alpha) in RINGS {
        for j in 0..=STEPS {
            let angle = facing - std::f32::consts::FRAC_PI_2
                + j as f32 / STEPS as f32 * std::f32::consts::PI;
            let at = center + eframe::egui::vec2(angle.cos(), angle.sin()) * (h * radius);
            mesh.colored_vertex(at, tint(alpha));
        }
    }
    let row = STEPS + 1;
    for k in 0..RINGS.len() as u32 - 1 {
        for j in 0..STEPS {
            let a = k * row + j;
            mesh.add_triangle(a, a + 1, a + row);
            mesh.add_triangle(a + 1, a + row + 1, a + row);
        }
    }
    painter.add(Shape::mesh(mesh));
}

fn dotted(painter: &Painter, pts: &[Pos2], width: f32, color: Color32) {
    let step = (width * 2.2).max(3.0);
    let mut carry = 0.0_f32;
    for pair in pts.windows(2) {
        let len = pair[0].distance(pair[1]);
        let mut d = carry;
        while d <= len {
            let t = if len > 0.0 { d / len } else { 0.0 };
            painter.add(Shape::circle_filled(
                pair[0] + (pair[1] - pair[0]) * t,
                width / 2.0,
                color,
            ));
            d += step;
        }
        carry = d - len;
    }
}

/// Draws one stroke. `opacity` is the layer's opacity.
pub fn draw_stroke(
    painter: &Painter,
    view: &ViewState,
    stroke: &Stroke,
    path: &Smoothed,
    opacity: f32,
) {
    if path.points.is_empty() {
        return;
    }
    let (pts, kept) = to_screen(view, &path.points);
    let w = stroke.width.unwrap_or(1.0);
    let px = |canvas: f32| (canvas * view.zoom).max(0.75);
    let base = color32(stroke.color, opacity);

    match stroke.pen {
        Pen::Wire => solid(painter, &pts, px(w.min(1.5)), base, true),
        Pen::Fill => region(painter, view, &path.points, base),
        Pen::Marker => solid(painter, &pts, px(w), base.gamma_multiply(0.55), false),
        Pen::Watercolor => solid(painter, &pts, px(w), base.gamma_multiply(0.5), false),
        Pen::SoftPencil => solid(painter, &pts, px(w), base.gamma_multiply(0.85), false),
        Pen::Airbrush => airbrush(painter, &pts, px(w), base),
        Pen::SprayPaint => airbrush(painter, &pts, px(w * 2.0), base),
        Pen::Dotted | Pen::Stipple => dotted(painter, &pts, px(w), base),
        Pen::Highlighter => solid(painter, &pts, px(w * 2.2), base.gamma_multiply(0.38), false),
        Pen::Charcoal | Pen::Chalk | Pen::Crayon | Pen::Pastel => {
            solid(painter, &pts, px(w * 1.8), base.gamma_multiply(0.7), false);
        }
        Pen::DynamicPen | Pen::BrushPen | Pen::InkBrush => match &path.pressure {
            Some(pr) => {
                let widths: Vec<f32> = kept
                    .iter()
                    .map(|&i| px(w * (0.25 + 0.75 * pr[i])))
                    .collect();
                ribbon(painter, &pts, &widths, base);
            }
            None => solid(painter, &pts, px(w), base, true),
        },
        Pen::Pen
        | Pen::Fountain
        | Pen::FixedWidth
        | Pen::HardPencil
        | Pen::FeltTip
        | Pen::Unknown(_) => {
            solid(painter, &pts, px(w), base, true);
        }
    }
}

/// Draws every visible layer of the document, bottom layer first, leaving out the strokes in `hide`.
pub fn draw_document(
    painter: &Painter,
    view: &ViewState,
    document: &Document,
    cache: &mut StrokeCache,
    hide: &HashSet<Uuid>,
) {
    for layer in document.layers.iter().filter(|l| l.content && l.visible) {
        for stroke in layer.strokes.iter().filter(|s| !hide.contains(&s.id)) {
            draw_stroke(painter, view, stroke, cache.get(stroke), layer.opacity);
        }
    }
}

/// Draws a sample stroke of `pen` inside `rect` as thick as `size` points (limited to what fits), so a size
/// setting can be seen as it is changed.
pub fn draw_brush_preview(painter: &Painter, rect: Rect, pen: Pen, color: Color, size: f32) {
    draw_preview(painter, rect, pen, color, size);
}

/// A wavy stroke across `rect`, `thickness` points thick but never more than 80% of the height.
fn draw_preview(painter: &Painter, rect: Rect, pen: Pen, color: Color, thickness: f32) {
    let n = 28;
    let samples: Vec<RawSample> = (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            RawSample {
                point: Point::new(t * 100.0, (t * std::f32::consts::TAU).sin() * 16.0),
                attrs: [0; 4],
            }
        })
        .collect();
    let zoom = rect.width() * 0.84 / 100.0;
    let view = ViewState {
        zoom,
        origin: pos2(rect.left() + rect.width() * 0.08, rect.center().y),
        ..ViewState::default()
    };
    let stroke = Stroke {
        id: Uuid::nil(),
        pen,
        width: Some(thickness.min(rect.height() * 0.4) / zoom),
        color,
        path: Vec::new(),
        samples,
        pressure: Vec::new(),
    };
    let path = smoothed(&stroke);
    draw_stroke(&painter.with_clip_rect(rect), &view, &stroke, &path, 1.0);
}
