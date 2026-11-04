//! The small pictures of the Library: what is on a drawing, and a folder. Both the desktop page and the touch page paint
//! their cards with them, so a drawing looks the same on either.

use eframe::egui::{
    Color32, CornerRadius, Mesh, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2,
};

use crate::library::Preview;

/// A small folder: a body with a tab.
pub fn paint_folder_glyph(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let body = Rect::from_center_size(center + vec2(0.0, 1.5), vec2(size, size * 0.72));
    let tab = Rect::from_min_size(body.min - vec2(0.0, 3.0), vec2(size * 0.42, 4.0));
    let stroke = Stroke::new(1.6, color);
    painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Inside);
    painter.rect_filled(
        tab,
        CornerRadius {
            nw: 2,
            ne: 2,
            sw: 0,
            se: 0,
        },
        color,
    );
}

/// A drawing's strokes, scaled to fit `rect` with a margin, as they are on its card.
pub fn paint_preview(painter: &Painter, rect: Rect, preview: &Preview) {
    let painter = painter.with_clip_rect(rect);
    let [x0, y0, x1, y1] = preview.bounds;
    let inner = rect.shrink(rect.height().min(rect.width()) * 0.1);
    let scale = (inner.width() / (x1 - x0).max(1.0))
        .min(inner.height() / (y1 - y0).max(1.0))
        .min(3.0);
    let (cx, cy) = (f32::midpoint(x0, x1), f32::midpoint(y0, y1));
    let map = |p: [f32; 2]| {
        pos2(
            rect.center().x + (p[0] - cx) * scale,
            rect.center().y - (p[1] - cy) * scale,
        )
    };
    let thickest = if rect.height() > 100.0 { 9.0 } else { 4.0 };
    for stroke in &preview.strokes {
        let [r, g, b, a] = stroke.color;
        let color = Color32::from_rgba_unmultiplied(r, g, b, a);
        let width = (stroke.width * scale).clamp(1.0, thickest);
        if !stroke.fill.is_empty() {
            // A region is filled, and its edge softened with a line a pixel wide: the triangles have no antialiasing.
            let mut mesh = Mesh::default();
            for v in stroke.fill.as_chunks::<2>().0 {
                mesh.colored_vertex(map([v[0], v[1]]), color);
            }
            mesh.indices = (0..mesh.vertices.len() as u32).collect();
            painter.add(Shape::mesh(mesh));
            painter.add(Shape::closed_line(
                stroke.points.iter().map(|p| map(*p)).collect(),
                Stroke::new(1.0, color),
            ));
            continue;
        }
        match stroke.points.as_slice() {
            [] => {}
            [only] => {
                painter.circle_filled(map(*only), width / 2.0, color);
            }
            points => {
                painter.add(Shape::line(
                    points.iter().map(|p| map(*p)).collect(),
                    Stroke::new(width, color),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Context, RawInput};

    use super::*;
    use crate::library::PreviewStroke;

    fn meshes(preview: &Preview) -> usize {
        fn count(shape: &Shape) -> usize {
            match shape {
                Shape::Mesh(_) => 1,
                Shape::Vec(all) => all.iter().map(count).sum(),
                _ => 0,
            }
        }
        let ctx = Context::default();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 200.0))),
            ..RawInput::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(200.0, 150.0));
            paint_preview(ui.painter(), rect, preview);
        });
        output.textures_delta.clear();
        output.shapes.iter().map(|s| count(&s.shape)).sum()
    }

    fn square(filled: bool) -> Preview {
        Preview {
            paper: [255, 255, 255],
            bounds: [0.0, 0.0, 100.0, 100.0],
            strokes: vec![PreviewStroke {
                color: [240, 200, 0, 255],
                width: 2.0,
                points: vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
                fill: if filled {
                    ink_brush::fill::triangulate(&[
                        ink_core::Point::new(0.0, 0.0),
                        ink_core::Point::new(100.0, 0.0),
                        ink_core::Point::new(100.0, 100.0),
                        ink_core::Point::new(0.0, 100.0),
                    ])
                } else {
                    Vec::new()
                },
            }],
        }
    }

    #[test]
    fn a_filled_shape_is_painted_filled_and_a_line_is_not() {
        assert!(meshes(&square(true)) >= 1, "the inside is painted");
        assert_eq!(meshes(&square(false)), 0, "a line has no inside");
    }
}
