//! Drawing a shape small, for the tiles of the shapes menu and the buttons of the tool bar.
//!
//! One painter for both interfaces, so a shape looks the same wherever it is shown. It draws what putting the shape on
//! the paper would: the fill under the outline, a shape made by hand in the colors it was made in.

use eframe::egui::{
    Align2, Color32, CornerRadius, FontId, Mesh, Painter, Pos2, Rect, Response, Sense,
    Shape as Paint, Stroke, StrokeKind, Ui, pos2, vec2,
};
use ink_core::shapes::{Shape, is_closed};
use ink_core::{Pen, Point};

use crate::theme;

/// How a shape is shown: the color of its outline and, if it is filled, of its inside.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub line: Color32,
    pub fill: Option<Color32>,
    /// Draw the outline of a shape that is filled; an unfilled shape always has one.
    pub outline: bool,
}

/// The inside of the closed path `points`, laid out in `rect`, as one mesh.
fn fill_mesh(points: &[Pos2], color: Color32) -> Option<Paint> {
    let canvas: Vec<Point> = points.iter().map(|p| Point::new(p.x, p.y)).collect();
    let triangles = ink_brush::fill::triangulate(&canvas);
    if triangles.is_empty() {
        return None;
    }
    let mut mesh = Mesh::default();
    for v in triangles.as_chunks::<2>().0 {
        mesh.colored_vertex(pos2(v[0], v[1]), color);
    }
    mesh.indices = (0..mesh.vertices.len() as u32).collect();
    Some(Paint::mesh(mesh))
}

/// The color a path made by hand was drawn in.
fn ink_color(color: [f32; 4]) -> Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(
        byte(color[0]),
        byte(color[1]),
        byte(color[2]),
        byte(color[3]),
    )
}

/// Paints `shape` as large as fits in `rect`, with a line `weight` points wide.
pub fn paint(painter: &Painter, rect: Rect, shape: &Shape, look: Look, weight: f32) {
    let (w, h) = shape.extent();
    let longer = w.max(h);
    if longer <= f32::EPSILON {
        return;
    }
    let scale = rect.width().min(rect.height()) / longer;
    // The shape is centered on the origin of its own box, with y pointing up.
    let at = |p: [f32; 2]| {
        pos2(
            rect.center().x + p[0] * scale,
            rect.center().y - p[1] * scale,
        )
    };
    for path in &shape.paths {
        let points: Vec<Pos2> = path.points.iter().copied().map(at).collect();
        if points.len() < 2 {
            continue;
        }
        if let Some(ink) = path.ink {
            let color = ink_color(ink.color);
            if ink.pen == Pen::Fill {
                if let Some(mesh) = fill_mesh(&points, color) {
                    painter.add(mesh);
                }
            } else {
                let width = ink.width.map_or(weight, |w| (w * longer * scale).max(1.0));
                painter.add(Paint::line(points, Stroke::new(width, color)));
            }
            continue;
        }
        let closed = is_closed(&path.points);
        if let Some(fill) = look.fill.filter(|_| closed)
            && let Some(mesh) = fill_mesh(&points, fill)
        {
            painter.add(mesh);
        }
        if look.outline || look.fill.is_none() || !closed {
            painter.add(Paint::line(points, Stroke::new(weight, look.line)));
        }
    }
}

/// The colors offered for the inside of a shape, besides the ink in use.
pub const FILL_COLORS: [[u8; 3]; 9] = [
    [0xF5, 0xC5, 0x42],
    [0xF2, 0x8C, 0x38],
    [0xE5, 0x53, 0x4B],
    [0xE8, 0x6A, 0xA6],
    [0x8E, 0x6B, 0xD8],
    [0x4C, 0x8D, 0xF6],
    [0x4C, 0xB7, 0x82],
    [0x8A, 0x8F, 0x98],
    [0xFF, 0xFF, 0xFF],
];

/// What a tile of the shapes menu shows.
pub struct Tile<'a> {
    pub shape: &'a Shape,
    pub look: Look,
    /// The color of the paper the shape is drawn on, so the tile shows what will be seen on the drawing.
    pub paper: Color32,
    /// Ringed: the shape picked to be arranged.
    pub selected: bool,
    /// A dot in the corner: the shape is on the tool bar.
    pub pinned: bool,
}

/// How tall a tile `side` wide is: the picture, and the name under it.
#[must_use]
pub fn tile_height(side: f32) -> f32 {
    side + 22.0
}

/// A tile: a small sheet of paper with the shape on it, and its name under. Returns where it was tapped.
pub fn tile(ui: &mut Ui, side: f32, tile: &Tile<'_>) -> Response {
    tile_with(ui, side, tile, Sense::click())
}

/// Like [`tile`], answering to what `sense` says (to be dragged, for one).
pub fn tile_with(ui: &mut Ui, side: f32, tile: &Tile<'_>, sense: Sense) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(side, tile_height(side)), sense);
    let picture = Rect::from_min_size(rect.min, vec2(side, side));
    let painter = ui.painter();
    let corners = CornerRadius::same(14);
    painter.rect_filled(picture, corners, tile.paper);
    if response.is_pointer_button_down_on() {
        painter.rect_filled(picture, corners, Color32::from_black_alpha(36));
    }
    paint(
        painter,
        picture.shrink(side * 0.2),
        tile.shape,
        tile.look,
        (side * 0.035).clamp(1.4, 3.0),
    );
    if tile.selected {
        painter.rect_stroke(
            picture,
            corners,
            Stroke::new(2.5, theme::ACCENT),
            StrokeKind::Inside,
        );
    }
    if tile.pinned {
        painter.circle_filled(
            pos2(picture.right() - 9.0, picture.top() + 9.0),
            4.5,
            theme::ACCENT,
        );
    }
    painter.text(
        pos2(rect.center().x, rect.bottom() - 9.0),
        Align2::CENTER_CENTER,
        elide(painter, &tile.shape.name, side),
        FontId::proportional(11.5),
        if tile.selected {
            theme::ACCENT
        } else {
            theme::TEXT_DIM
        },
    );
    response
}

/// `text` cut short with an ellipsis to fit `width`.
fn elide(painter: &Painter, text: &str, width: f32) -> String {
    let font = FontId::proportional(11.5);
    let fits = |s: &str| {
        painter
            .layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= width - 4.0
    };
    if fits(text) {
        return text.to_owned();
    }
    let mut cut: Vec<char> = text.chars().collect();
    while cut.len() > 1 {
        cut.pop();
        let shortened: String = cut.iter().collect::<String>() + "…";
        if fits(&shortened) {
            return shortened;
        }
    }
    text.chars().take(1).collect()
}

/// A round swatch `side` across for a color: `None` is no color at all (a slash through an empty circle). Returns where
/// it was tapped.
pub fn swatch(ui: &mut Ui, side: f32, color: Option<Color32>, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let painter = ui.painter();
    let center = rect.center();
    let radius = side / 2.0 - 4.0;
    if let Some(color) = color {
        painter.circle_filled(center, radius, color);
        painter.circle_stroke(
            center,
            radius,
            Stroke::new(1.0, Color32::from_black_alpha(70)),
        );
    } else {
        painter.circle_stroke(center, radius, Stroke::new(1.5, theme::TEXT_DIM));
        let d = radius * 0.7;
        painter.line_segment(
            [center + vec2(-d, d), center + vec2(d, -d)],
            Stroke::new(1.5, theme::TEXT_DIM),
        );
    }
    if selected {
        painter.circle_stroke(center, radius + 3.5, Stroke::new(2.0, theme::ACCENT));
    }
    response
}
