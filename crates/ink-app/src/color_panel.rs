//! The color flyout: a hue ring around a saturation square, a hex field, named palettes and the colors used lately.

use std::f32::consts::{FRAC_PI_2, TAU};

use eframe::egui::{
    self, Color32, Context, CornerRadius, Id, Mesh, Pos2, Rect, RichText, Sense, Shape, Stroke,
    TextEdit, Ui, ecolor::Hsva, pos2, vec2,
};

use crate::app::InkApp;
use crate::flyouts;
use crate::theme;
use crate::widgets;

/// A named set of colors.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub name: &'static str,
    pub colors: [[u8; 3]; 12],
}

/// The palettes offered in the panel.
pub const PALETTES: [Palette; 4] = [
    Palette {
        name: "Basic",
        colors: [
            [0x18, 0x17, 0x12],
            [0x6B, 0x70, 0x7A],
            [0xE0, 0xE2, 0xE6],
            [0xE5, 0x4B, 0x4B],
            [0xF2, 0x8C, 0x28],
            [0xF4, 0xC8, 0x2D],
            [0x58, 0xB8, 0x5C],
            [0x2E, 0xB5, 0xC9],
            [0x2F, 0x7B, 0xE0],
            [0x6A, 0x4C, 0xC9],
            [0xC9, 0x4C, 0xA8],
            [0x8A, 0x5A, 0x3C],
        ],
    },
    Palette {
        name: "Pastel",
        colors: [
            [0xF6, 0xB8, 0xB8],
            [0xF9, 0xD0, 0xA9],
            [0xFB, 0xEB, 0xA8],
            [0xC8, 0xE8, 0xB0],
            [0xA9, 0xE3, 0xD6],
            [0xA8, 0xD4, 0xF2],
            [0xB9, 0xC1, 0xF0],
            [0xD6, 0xBC, 0xEE],
            [0xF1, 0xBA, 0xDD],
            [0xE8, 0xDD, 0xCF],
            [0xCF, 0xD6, 0xDB],
            [0xFF, 0xFF, 0xFF],
        ],
    },
    Palette {
        name: "Earth",
        colors: [
            [0x3B, 0x2A, 0x20],
            [0x6B, 0x47, 0x2E],
            [0x9B, 0x6B, 0x43],
            [0xC8, 0x9B, 0x6A],
            [0xE3, 0xC9, 0x9F],
            [0x7A, 0x6A, 0x3A],
            [0x4F, 0x6B, 0x3A],
            [0x2F, 0x4A, 0x3C],
            [0x6B, 0x7F, 0x86],
            [0x3F, 0x56, 0x6B],
            [0xA5, 0x4A, 0x3A],
            [0xD4, 0x8A, 0x3A],
        ],
    },
    Palette {
        name: "Grays",
        colors: [
            [0x00, 0x00, 0x00],
            [0x1C, 0x1E, 0x24],
            [0x2E, 0x31, 0x38],
            [0x44, 0x48, 0x50],
            [0x5E, 0x63, 0x6C],
            [0x7A, 0x7F, 0x88],
            [0x98, 0x9C, 0xA4],
            [0xB4, 0xB8, 0xBE],
            [0xCC, 0xCF, 0xD3],
            [0xE0, 0xE2, 0xE6],
            [0xF0, 0xF1, 0xF3],
            [0xFF, 0xFF, 0xFF],
        ],
    },
];

/// The palette called `name`, or the first one.
#[must_use]
pub fn palette_named(name: &str) -> &'static Palette {
    PALETTES
        .iter()
        .find(|p| p.name == name)
        .unwrap_or(&PALETTES[0])
}

/// `#RRGGBB` for a color.
#[must_use]
pub fn to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// A color from `#RRGGBB` or `RRGGBB`, or `None` if the text is not one.
#[must_use]
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// A round color chip. Returns its response.
pub fn swatch(ui: &mut Ui, rgb: [u8; 3], selected: bool, diameter: f32) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(diameter + 8.0, diameter + 8.0), Sense::click());
    let painter = ui.painter();
    painter.circle_filled(
        rect.center(),
        diameter / 2.0,
        Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
    );
    painter.circle_stroke(
        rect.center(),
        diameter / 2.0,
        Stroke::new(1.0, Color32::from_white_alpha(45)),
    );
    if selected {
        painter.circle_stroke(
            rect.center(),
            diameter / 2.0 + 3.0,
            Stroke::new(2.5, theme::ACCENT),
        );
    } else if response.hovered() {
        painter.circle_stroke(
            rect.center(),
            diameter / 2.0 + 3.0,
            Stroke::new(1.5, Color32::from_white_alpha(70)),
        );
    }
    response
}

/// What the user did in the panel during one frame.
#[derive(Default)]
struct Chosen {
    /// A swatch was tapped: use the color and remember it.
    swatch: Option<[u8; 3]>,
    /// The hex field holds a valid color.
    typed: Option<[u8; 3]>,
    /// The hex field was left, so the typed color counts as used.
    typed_done: bool,
}

pub fn show(ctx: &Context, app: &mut InkApp) {
    let before = app.tool.color;
    let open = app.ui.color_open;
    let anchor = app.ui.anchors.color;
    let chosen = flyouts::flyout(ctx, app, "color", open, anchor, 540.0, |ui, app| {
        ui.set_width(240.0);
        let mut chosen = Chosen::default();
        contents(ui, app, &mut chosen);
        chosen
    });
    let Some(chosen) = chosen else {
        return;
    };

    let dragged = app.tool.color != before;
    if let Some(rgb) = chosen.swatch {
        app.set_color_rgb(rgb);
        app.remember_color();
    } else if let Some(rgb) = chosen.typed {
        app.set_color_rgb(rgb);
        if chosen.typed_done {
            app.remember_color();
        }
    } else if dragged {
        app.sync_color();
    }
    // A color picked by dragging counts as used once the pointer is let go, not on every step.
    let pending = Id::unique("color_pending");
    if dragged {
        ctx.data_mut(|d| d.insert_temp(pending, true));
    } else if ctx.input(|i| i.pointer.any_released())
        && ctx.data_mut(|d| d.remove_temp::<bool>(pending)) == Some(true)
    {
        app.remember_color();
    }
}

fn contents(ui: &mut Ui, app: &mut InkApp, chosen: &mut Chosen) {
    ui.label(RichText::new("Ink").size(18.0).strong());
    ui.add_space(6.0);
    ring_picker(ui, &mut app.tool.color);
    ui.add_space(8.0);
    hex_field(ui, app, chosen);
    ui.add_space(4.0);

    let mut index = PALETTES
        .iter()
        .position(|p| p.name == app.config.colors.palette)
        .unwrap_or(0);
    let options: Vec<(usize, &str)> = PALETTES.iter().map(|p| p.name).enumerate().collect();
    if widgets::text_tabs(ui, &options, &mut index) {
        PALETTES[index]
            .name
            .clone_into(&mut app.config.colors.palette);
        app.mark_config_changed();
    }
    let current = app.tool.color.to_srgb();
    let palette = palette_named(&app.config.colors.palette);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        for rgb in palette.colors {
            if swatch(ui, rgb, rgb == current, 26.0).clicked() {
                chosen.swatch = Some(rgb);
            }
        }
    });

    if !app.config.colors.recent.is_empty() {
        ui.add_space(6.0);
        widgets::heading(ui, "Recent");
        let recent = app.config.colors.recent.clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for rgb in recent {
                if swatch(ui, rgb, rgb == current, 26.0).clicked() {
                    chosen.swatch = Some(rgb);
                }
            }
        });
    }
}

/// A hue ring around a saturation and brightness square.
fn ring_picker(ui: &mut Ui, color: &mut Hsva) {
    let size = ui.available_width().min(218.0);
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), size), Sense::click_and_drag());
    let center = rect.center();
    let outer = size / 2.0 - 3.0;
    let inner = outer - 26.0;
    let half = inner * 0.68;
    if (response.dragged() || response.clicked())
        && let Some(p) = response.interact_pointer_pos()
    {
        // Where the press began decides what is being dragged, so the pen can wander off the ring.
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
        if (origin - center).length() >= inner - 6.0 {
            let v = p - center;
            color.h = (v.y.atan2(v.x) / TAU + 0.25).rem_euclid(1.0);
        } else {
            color.s = ((p.x - (center.x - half)) / (2.0 * half)).clamp(0.0, 1.0);
            color.v = (1.0 - (p.y - (center.y - half)) / (2.0 * half)).clamp(0.0, 1.0);
        }
    }
    paint_ring(ui, center, inner, outer);
    let square = Rect::from_center_size(center, vec2(2.0 * half, 2.0 * half));
    paint_square(ui, square, color.h);

    let angle = color.h * TAU - FRAC_PI_2;
    let middle = f32::midpoint(inner, outer);
    marker(
        ui,
        center + vec2(angle.cos(), angle.sin()) * middle,
        Color32::from(Hsva::new(color.h, 1.0, 1.0, 1.0)),
    );
    marker(
        ui,
        pos2(
            square.left() + color.s * square.width(),
            square.bottom() - color.v * square.height(),
        ),
        Color32::from(Hsva::new(color.h, color.s, color.v, 1.0)),
    );
}

/// Every hue around a ring, red at the top and clockwise.
fn paint_ring(ui: &Ui, center: Pos2, inner: f32, outer: f32) {
    const SEGMENTS: u32 = 96;
    let mut mesh = Mesh::default();
    for i in 0..=SEGMENTS {
        let t = i as f32 / SEGMENTS as f32;
        let angle = t * TAU - FRAC_PI_2;
        let dir = vec2(angle.cos(), angle.sin());
        let color = Color32::from(Hsva::new(t, 1.0, 1.0, 1.0));
        mesh.colored_vertex(center + dir * inner, color);
        mesh.colored_vertex(center + dir * outer, color);
        if i > 0 {
            let base = (i - 1) * 2;
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base + 1, base + 3, base + 2);
        }
    }
    ui.painter().add(Shape::mesh(mesh));
}

/// White to the hue across, then transparent to black downward: together the saturation/value square.
fn paint_square(ui: &Ui, rect: Rect, hue: f32) {
    let pure = Color32::from(Hsva::new(hue, 1.0, 1.0, 1.0));
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    let layers = [
        [Color32::WHITE, pure, pure, Color32::WHITE],
        [
            Color32::TRANSPARENT,
            Color32::TRANSPARENT,
            Color32::BLACK,
            Color32::BLACK,
        ],
    ];
    for colors in layers {
        let mut mesh = Mesh::default();
        for (corner, color) in corners.into_iter().zip(colors) {
            mesh.colored_vertex(corner, color);
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ui.painter().add(Shape::mesh(mesh));
    }
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, Color32::from_white_alpha(30)),
        egui::StrokeKind::Inside,
    );
}

/// A ring that shows the chosen point on a gradient, filled with the color there.
fn marker(ui: &Ui, at: Pos2, fill: Color32) {
    ui.painter()
        .circle_stroke(at, 8.5, Stroke::new(3.0, Color32::from_black_alpha(120)));
    ui.painter()
        .circle_stroke(at, 8.5, Stroke::new(1.8, Color32::WHITE));
    ui.painter().circle_filled(at, 7.0, fill);
}

/// The hex text of the current color, editable. The text being typed is kept until the field loses focus.
fn hex_field(ui: &mut Ui, app: &InkApp, chosen: &mut Chosen) {
    let id = Id::unique("color_hex");
    let current = to_hex(app.tool.color.to_srgb());
    let focused = ui.memory(|m| m.has_focus(id));
    let mut text: String = if focused {
        ui.data(|d| d.get_temp::<String>(id))
            .unwrap_or_else(|| current.clone())
    } else {
        current
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("HEX").size(11.5).color(theme::TEXT_DIM));
        let response = ui.add(
            TextEdit::singleline(&mut text)
                .id(id)
                .desired_width(96.0)
                .font(egui::TextStyle::Monospace),
        );
        if response.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, text.clone()));
        }
        if response.changed() {
            chosen.typed = parse_hex(&text);
        }
        if response.lost_focus() {
            chosen.typed_done = true;
            chosen.typed = chosen.typed.or_else(|| parse_hex(&text));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_rejects_nonsense() {
        assert_eq!(to_hex([0x2E, 0xB5, 0xC9]), "#2EB5C9");
        assert_eq!(parse_hex("#2eb5c9"), Some([0x2E, 0xB5, 0xC9]));
        assert_eq!(parse_hex("  2EB5C9 "), Some([0x2E, 0xB5, 0xC9]));
        for bad in ["", "#12345", "#1234567", "#GG0000", "é12345"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn palettes_have_distinct_names_and_a_fallback() {
        for (i, p) in PALETTES.iter().enumerate() {
            assert!(PALETTES[..i].iter().all(|q| q.name != p.name));
        }
        assert_eq!(palette_named("nope").name, PALETTES[0].name);
        assert_eq!(palette_named("Earth").name, "Earth");
    }
}
