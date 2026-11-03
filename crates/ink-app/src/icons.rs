//! Small vector icons drawn with the painter, so the interface needs no image assets.

use eframe::egui::{
    Color32, CornerRadius, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    Vec2, pos2, vec2,
};

use crate::theme;

/// The icons the interface uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Undo,
    Redo,
    Layers,
    Eye,
    EyeOff,
    Plus,
    Trash,
    Up,
    Down,
    Hand,
    Menu,
    Gear,
    Close,
    Left,
    Right,
    FlipHorizontal,
    FlipVertical,
    Duplicate,
    Eraser,
    /// The select tool: a dashed loop with its rope.
    Select,
    /// The slice tool: a line cut in two by a blade.
    Slice,
    /// The nudge tool: a line pulled out of shape at one point.
    Nudge,
    /// Every brush: four squares.
    Brushes,
    /// Precision and snapping: a crosshair around a point.
    Precision,
    /// Strokes tied together: two squares inside one dashed frame.
    Group,
    /// A group let go: two squares apart.
    Ungroup,
    /// A closed padlock.
    Lock,
    /// An open padlock.
    Unlock,
    /// Up one layer: the layers with an arrow above.
    LayerUp,
    /// Down one layer: the layers with an arrow below.
    LayerDown,
    /// The numbers of the selection: a box with a size mark on each side.
    Size,
}

fn line(painter: &Painter, points: Vec<Pos2>, stroke: Stroke) {
    painter.add(Shape::line(points, stroke));
}

/// A curved arrow: an arc with an arrowhead at its start. `flip` mirrors it left to right (redo).
fn curved_arrow(painter: &Painter, c: Pos2, s: f32, flip: bool, stroke: Stroke) {
    let x = |dx: f32| if flip { c.x - dx } else { c.x + dx };
    let (r, cy) = (s * 0.56, c.y + s * 0.12);
    let arc: Vec<Pos2> = (0..=20)
        .map(|i| {
            let a = (200.0 + i as f32 * (200.0 / 20.0)).to_radians();
            pos2(x(r * a.cos()), cy + r * a.sin())
        })
        .collect();
    let (tip, next) = (arc[0], arc[1]);
    line(painter, arc, stroke);
    // The arrowhead points along the arc, away from its second point.
    let dir = (tip - next).normalized();
    let rotate = |v: Vec2, degrees: f32| {
        let a = degrees.to_radians();
        vec2(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos())
    };
    let head = s * 0.42;
    line(
        painter,
        vec![
            tip + rotate(dir, 145.0) * head,
            tip,
            tip + rotate(dir, -145.0) * head,
        ],
        stroke,
    );
}

/// Paints `icon` centered in `rect`.
#[allow(
    clippy::too_many_lines,
    reason = "one match arm per icon; splitting would only scatter the drawing code"
)]
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5;
    let stroke = Stroke::new((s * 0.13).max(1.6), color);
    match icon {
        Icon::Undo => curved_arrow(painter, c, s, false, stroke),
        Icon::Redo => curved_arrow(painter, c, s, true, stroke),
        Icon::Layers => {
            for k in 0..3 {
                let y = c.y + (k as f32 - 1.0) * s * 0.5;
                let (w, h) = (s * 0.85, s * 0.34);
                painter.add(Shape::closed_line(
                    vec![
                        pos2(c.x, y - h),
                        pos2(c.x + w, y),
                        pos2(c.x, y + h),
                        pos2(c.x - w, y),
                    ],
                    stroke,
                ));
            }
        }
        Icon::Eye | Icon::EyeOff => {
            let lens: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 8.0 - 1.0;
                    pos2(c.x + t * s * 0.9, c.y - s * 0.5 * (1.0 - t * t))
                })
                .chain((0..=16).map(|i| {
                    let t = 1.0 - i as f32 / 8.0;
                    pos2(c.x + t * s * 0.9, c.y + s * 0.5 * (1.0 - t * t))
                }))
                .collect();
            painter.add(Shape::closed_line(lens, stroke));
            painter.circle_filled(c, s * 0.2, color);
            if icon == Icon::EyeOff {
                painter.line_segment(
                    [
                        pos2(c.x - s * 0.8, c.y + s * 0.7),
                        pos2(c.x + s * 0.8, c.y - s * 0.7),
                    ],
                    stroke,
                );
            }
        }
        Icon::Plus => {
            painter.line_segment([pos2(c.x - s * 0.6, c.y), pos2(c.x + s * 0.6, c.y)], stroke);
            painter.line_segment([pos2(c.x, c.y - s * 0.6), pos2(c.x, c.y + s * 0.6)], stroke);
        }
        Icon::Trash => {
            painter.line_segment(
                [
                    pos2(c.x - s * 0.7, c.y - s * 0.4),
                    pos2(c.x + s * 0.7, c.y - s * 0.4),
                ],
                stroke,
            );
            line(
                painter,
                vec![
                    pos2(c.x - s * 0.25, c.y - s * 0.4),
                    pos2(c.x - s * 0.25, c.y - s * 0.7),
                    pos2(c.x + s * 0.25, c.y - s * 0.7),
                    pos2(c.x + s * 0.25, c.y - s * 0.4),
                ],
                stroke,
            );
            painter.add(Shape::closed_line(
                vec![
                    pos2(c.x - s * 0.5, c.y - s * 0.4),
                    pos2(c.x - s * 0.42, c.y + s * 0.75),
                    pos2(c.x + s * 0.42, c.y + s * 0.75),
                    pos2(c.x + s * 0.5, c.y - s * 0.4),
                ],
                stroke,
            ));
        }
        Icon::Up => line(
            painter,
            vec![
                pos2(c.x - s * 0.6, c.y + s * 0.3),
                pos2(c.x, c.y - s * 0.3),
                pos2(c.x + s * 0.6, c.y + s * 0.3),
            ],
            stroke,
        ),
        Icon::Down => line(
            painter,
            vec![
                pos2(c.x - s * 0.6, c.y - s * 0.3),
                pos2(c.x, c.y + s * 0.3),
                pos2(c.x + s * 0.6, c.y - s * 0.3),
            ],
            stroke,
        ),
        Icon::Left => line(
            painter,
            vec![
                pos2(c.x + s * 0.3, c.y - s * 0.6),
                pos2(c.x - s * 0.3, c.y),
                pos2(c.x + s * 0.3, c.y + s * 0.6),
            ],
            stroke,
        ),
        Icon::Right => line(
            painter,
            vec![
                pos2(c.x - s * 0.3, c.y - s * 0.6),
                pos2(c.x + s * 0.3, c.y),
                pos2(c.x - s * 0.3, c.y + s * 0.6),
            ],
            stroke,
        ),
        Icon::FlipHorizontal | Icon::FlipVertical => {
            // Two triangles facing each other across a dashed mirror line.
            let at = |along: f32, across: f32| {
                if icon == Icon::FlipHorizontal {
                    pos2(c.x + along * s, c.y + across * s)
                } else {
                    pos2(c.x + across * s, c.y + along * s)
                }
            };
            for side in [-1.0_f32, 1.0] {
                painter.add(Shape::closed_line(
                    vec![
                        at(side * 0.16, -0.78),
                        at(side * 0.16, 0.78),
                        at(side * 0.88, 0.78),
                    ],
                    stroke,
                ));
            }
            for (from, to) in [(-0.95, -0.6), (-0.3, 0.3), (0.6, 0.95)] {
                painter.line_segment([at(0.0, from), at(0.0, to)], stroke);
            }
        }
        Icon::Duplicate => {
            let back_min = pos2(c.x - s * 0.77, c.y - s * 0.77);
            let back_max = pos2(c.x + s * 0.33, c.y + s * 0.33);
            let front = Rect::from_min_max(
                pos2(c.x - s * 0.33, c.y - s * 0.33),
                pos2(c.x + s * 0.77, c.y + s * 0.77),
            );
            line(
                painter,
                vec![
                    pos2(front.left(), back_max.y),
                    pos2(back_min.x, back_max.y),
                    back_min,
                    pos2(back_max.x, back_min.y),
                    pos2(back_max.x, front.top()),
                ],
                stroke,
            );
            painter.rect_stroke(front, CornerRadius::same(2), stroke, StrokeKind::Middle);
        }
        Icon::Eraser => {
            // A slanted block, in two parts.
            let (sin, cos) = (-0.62_f32).sin_cos();
            let turn =
                |u: f32, v: f32| pos2(c.x + (u * cos - v * sin) * s, c.y + (u * sin + v * cos) * s);
            painter.add(Shape::closed_line(
                vec![
                    turn(-0.95, -0.45),
                    turn(0.95, -0.45),
                    turn(0.95, 0.45),
                    turn(-0.95, 0.45),
                ],
                stroke,
            ));
            painter.line_segment([turn(-0.2, -0.45), turn(-0.2, 0.45)], stroke);
        }
        Icon::Select => {
            let turns = 14_u8;
            for i in (0..turns).step_by(2) {
                let angle = |k: u8| f32::from(k) / f32::from(turns) * std::f32::consts::TAU;
                let on =
                    |a: f32| pos2(c.x + 0.88 * s * a.cos(), c.y - 0.15 * s + 0.6 * s * a.sin());
                painter.line_segment([on(angle(i)), on(angle(i + 1))], stroke);
            }
            line(
                painter,
                vec![
                    pos2(c.x - 0.15 * s, c.y + 0.45 * s),
                    pos2(c.x - 0.3 * s, c.y + 0.8 * s),
                    pos2(c.x + 0.2 * s, c.y + 0.95 * s),
                ],
                stroke,
            );
        }
        Icon::Slice => {
            for (from, to) in [(-0.95, -0.22), (0.22, 0.95)] {
                painter.line_segment(
                    [
                        pos2(c.x + from * s, c.y + 0.12 * s),
                        pos2(c.x + to * s, c.y + 0.12 * s),
                    ],
                    stroke,
                );
            }
            painter.line_segment(
                [
                    pos2(c.x + 0.6 * s, c.y - 0.95 * s),
                    pos2(c.x - 0.6 * s, c.y + 0.95 * s),
                ],
                stroke,
            );
        }
        Icon::Nudge => {
            let bent: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 8.0 - 1.0;
                    pos2(
                        c.x + t * 0.95 * s,
                        c.y + 0.6 * s - 1.15 * s * (-(t * t) * 3.2).exp(),
                    )
                })
                .collect();
            line(painter, bent, stroke);
            painter.circle_filled(pos2(c.x, c.y - 0.55 * s), 0.22 * s, color);
        }
        Icon::Brushes => {
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                painter.rect_filled(
                    Rect::from_center_size(
                        pos2(c.x + dx * 0.46 * s, c.y + dy * 0.46 * s),
                        vec2(0.74 * s, 0.74 * s),
                    ),
                    CornerRadius::same(2),
                    color,
                );
            }
        }
        Icon::Precision => {
            painter.circle_stroke(c, s * 0.5, stroke);
            painter.circle_filled(c, s * 0.12, color);
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                painter.line_segment(
                    [
                        pos2(c.x + dx * s * 0.34, c.y + dy * s * 0.34),
                        pos2(c.x + dx * s * 0.9, c.y + dy * s * 0.9),
                    ],
                    stroke,
                );
            }
        }
        Icon::Group | Icon::Ungroup => {
            let square = |at: Pos2, half: f32| {
                painter.rect_stroke(
                    Rect::from_center_size(at, vec2(half * 2.0, half * 2.0)),
                    CornerRadius::same(1),
                    stroke,
                    StrokeKind::Middle,
                );
            };
            if icon == Icon::Group {
                // A dashed frame, drawn as its four corners, around two squares.
                for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                    let corner = pos2(c.x + dx * s * 0.95, c.y + dy * s * 0.95);
                    line(
                        painter,
                        vec![
                            pos2(corner.x - dx * s * 0.4, corner.y),
                            corner,
                            pos2(corner.x, corner.y - dy * s * 0.4),
                        ],
                        stroke,
                    );
                }
                square(pos2(c.x - s * 0.28, c.y - s * 0.28), s * 0.26);
                square(pos2(c.x + s * 0.28, c.y + s * 0.28), s * 0.26);
            } else {
                square(pos2(c.x - s * 0.5, c.y - s * 0.5), s * 0.34);
                square(pos2(c.x + s * 0.5, c.y + s * 0.5), s * 0.34);
            }
        }
        Icon::Lock | Icon::Unlock => {
            let body = Rect::from_min_max(
                pos2(c.x - s * 0.7, c.y - s * 0.05),
                pos2(c.x + s * 0.7, c.y + s * 0.85),
            );
            painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Middle);
            // The shackle: a half circle over the body, standing on both sides when locked and lifted off the
            // right side when not.
            let lift = if icon == Icon::Lock { 0.0 } else { s * 0.34 };
            let arch: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 - i as f32 / 12.0);
                    pos2(
                        c.x + s * 0.42 * a.cos(),
                        c.y - s * 0.05 - lift - s * 0.5 * a.sin(),
                    )
                })
                .collect();
            let mut shackle = vec![pos2(c.x - s * 0.42, c.y - s * 0.05)];
            shackle.extend(arch);
            if icon == Icon::Lock {
                shackle.push(pos2(c.x + s * 0.42, c.y - s * 0.05));
            }
            line(painter, shackle, stroke);
            painter.circle_filled(pos2(c.x, c.y + s * 0.4), s * 0.12, color);
        }
        Icon::LayerUp | Icon::LayerDown => {
            // The way the arrow points on the screen (up is toward smaller y); the layers are on the other side.
            let d = if icon == Icon::LayerUp { -1.0 } else { 1.0 };
            for y in [c.y - d * s * 0.35, c.y - d * s * 0.8] {
                let (w, h) = (s * 0.8, s * 0.26);
                painter.add(Shape::closed_line(
                    vec![
                        pos2(c.x, y - h),
                        pos2(c.x + w, y),
                        pos2(c.x, y + h),
                        pos2(c.x - w, y),
                    ],
                    stroke,
                ));
            }
            let tip = pos2(c.x, c.y + d * s * 0.95);
            painter.line_segment([tip, pos2(c.x, c.y + d * s * 0.2)], stroke);
            line(
                painter,
                vec![
                    pos2(c.x - s * 0.3, tip.y - d * s * 0.3),
                    tip,
                    pos2(c.x + s * 0.3, tip.y - d * s * 0.3),
                ],
                stroke,
            );
        }
        Icon::Size => {
            // The usual mark for resizing: a diagonal with an arrowhead at each end.
            let (top, bottom) = (
                pos2(c.x + s * 0.8, c.y - s * 0.8),
                pos2(c.x - s * 0.8, c.y + s * 0.8),
            );
            painter.line_segment([top, bottom], stroke);
            for (tip, sign) in [(top, -1.0_f32), (bottom, 1.0)] {
                line(
                    painter,
                    vec![
                        pos2(tip.x + sign * s * 0.55, tip.y),
                        tip,
                        pos2(tip.x, tip.y - sign * s * 0.55),
                    ],
                    stroke,
                );
            }
        }
        Icon::Gear => {
            painter.circle_stroke(c, s * 0.5, stroke);
            painter.circle_stroke(c, s * 0.18, stroke);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let dir = vec2(a.cos(), a.sin());
                painter.line_segment([c + dir * s * 0.62, c + dir * s * 0.9], stroke);
            }
        }
        Icon::Close => {
            for (a, b) in [((-0.6, -0.6), (0.6, 0.6)), ((-0.6, 0.6), (0.6, -0.6))] {
                painter.line_segment(
                    [
                        pos2(c.x + a.0 * s, c.y + a.1 * s),
                        pos2(c.x + b.0 * s, c.y + b.1 * s),
                    ],
                    stroke,
                );
            }
        }
        Icon::Menu => {
            for dy in [-0.55, 0.0, 0.55] {
                painter.line_segment(
                    [
                        pos2(c.x - s * 0.8, c.y + dy * s),
                        pos2(c.x + s * 0.8, c.y + dy * s),
                    ],
                    stroke,
                );
            }
        }
        Icon::Hand => {
            for (dx, dy) in [(1.0, 0.0), (0.0, 1.0)] {
                painter.line_segment(
                    [
                        pos2(c.x - dx * s * 0.8, c.y - dy * s * 0.8),
                        pos2(c.x + dx * s * 0.8, c.y + dy * s * 0.8),
                    ],
                    stroke,
                );
            }
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let tip = pos2(c.x + dx * s * 0.8, c.y + dy * s * 0.8);
                let back = pos2(tip.x - dx * s * 0.28, tip.y - dy * s * 0.28);
                let side = vec2(-dy, dx) * s * 0.22;
                line(painter, vec![back + side, tip, back - side], stroke);
            }
        }
    }
}

/// A square icon button. `selected` highlights it as the active tool.
pub fn button(ui: &mut Ui, icon: Icon, tooltip: &str, enabled: bool, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
    let painter = ui.painter();
    let fill = if selected {
        theme::ACCENT.gamma_multiply(0.35)
    } else if response.hovered() && enabled {
        theme::CHROME_HI
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, CornerRadius::same(12), fill);
    let color = if !enabled {
        theme::TEXT_DIM.gamma_multiply(0.45)
    } else if selected {
        theme::ACCENT
    } else {
        theme::TEXT
    };
    paint(painter, rect.shrink(9.0), icon, color);
    let response = response.on_hover_text(tooltip);
    if enabled {
        response
    } else {
        response.on_disabled_hover_text(tooltip)
    }
}
