//! Colors, surfaces and motion for the app chrome.
//!
//! The chrome is warm obsidian with one saffron accent. Floating pieces are drawn as raised surfaces: two soft
//! shadows, a vertical sheen, a hairline edge and a highlight along the top edge, so they read as objects
//! sitting on the paper rather than flat boxes.

use std::f32::consts::{FRAC_PI_2, PI};

use eframe::egui::{
    self, Color32, CornerRadius, Frame, Id, InnerResponse, Margin, Pos2, Rect, Shape, Stroke, Ui,
    Vec2,
    epaint::{Mesh, Shadow},
    pos2, vec2,
};

/// The one accent: the selected pen, unsaved changes, focus.
pub const ACCENT: Color32 = Color32::from_rgb(0xF0, 0xB4, 0x45);
/// Panel background.
pub const CHROME: Color32 = Color32::from_rgb(0x1E, 0x1D, 0x1B);
/// Raised controls on a panel.
pub const CHROME_HI: Color32 = Color32::from_rgb(0x2D, 0x2B, 0x28);
/// A raised control under the pointer.
pub const CHROME_HOVER: Color32 = Color32::from_rgb(0x39, 0x37, 0x32);
/// A grouped block of controls inside a panel.
pub const CARD: Color32 = Color32::from_rgb(0x27, 0x25, 0x22);
/// The default paper color.
pub const PAPER: Color32 = Color32::from_rgb(0xF9, 0xF9, 0xF9);
pub const TEXT: Color32 = Color32::from_rgb(0xF3, 0xEE, 0xE3);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0xA7, 0xA0, 0x93);

/// Top and bottom of the sheen on a floating surface.
const SURFACE_TOP: Color32 = Color32::from_rgb(0x2E, 0x2C, 0x29);
const SURFACE_BOTTOM: Color32 = Color32::from_rgb(0x1A, 0x19, 0x17);

/// A hairline that separates things on a dark panel.
#[must_use]
pub fn hairline() -> Stroke {
    Stroke::new(1.0, Color32::from_white_alpha(22))
}

/// The drop shadow under windows.
#[must_use]
pub fn shadow() -> Shadow {
    Shadow {
        offset: [0, 10],
        blur: 36,
        spread: 0,
        color: Color32::from_black_alpha(100),
    }
}

fn corner(radii: [f32; 4]) -> CornerRadius {
    CornerRadius {
        nw: radii[0].round() as u8,
        ne: radii[1].round() as u8,
        se: radii[2].round() as u8,
        sw: radii[3].round() as u8,
    }
}

/// `radii` with none larger than half the shorter side of `rect`. Bigger corners would run into the opposite ones
/// and fold the outline over itself, which shows as a pinched, broken shape on a small surface.
#[must_use]
pub fn fit_radii(rect: Rect, radii: [f32; 4]) -> [f32; 4] {
    let limit = (rect.width().min(rect.height()) / 2.0).max(0.0);
    radii.map(|r| r.clamp(0.0, limit))
}

/// The outline of a rounded rectangle, clockwise from the start of the top-left arc. `radii` are the radii of
/// the top-left, top-right, bottom-right and bottom-left corners (kept small enough to fit the rectangle).
#[must_use]
pub fn rounded_outline(rect: Rect, radii: [f32; 4]) -> Vec<Pos2> {
    const STEPS: u32 = 8;
    let [nw, ne, se, sw] = fit_radii(rect, radii);
    let corners = [
        (pos2(rect.left() + nw, rect.top() + nw), nw, PI),
        (pos2(rect.right() - ne, rect.top() + ne), ne, 1.5 * PI),
        (pos2(rect.right() - se, rect.bottom() - se), se, 0.0),
        (pos2(rect.left() + sw, rect.bottom() - sw), sw, FRAC_PI_2),
    ];
    let mut points = Vec::new();
    for (center, radius, start) in corners {
        if radius <= 0.0 {
            points.push(center);
            continue;
        }
        for i in 0..=STEPS {
            let a = start + FRAC_PI_2 * i as f32 / STEPS as f32;
            points.push(center + vec2(a.cos(), a.sin()) * radius);
        }
    }
    points
}

/// A convex shape filled with a vertical gradient from `top` to `bottom` across `rect`.
pub fn gradient_fill(points: &[Pos2], rect: Rect, top: Color32, bottom: Color32) -> Shape {
    let color_at =
        |y: f32| top.lerp_to_gamma(bottom, ((y - rect.top()) / rect.height()).clamp(0.0, 1.0));
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.center(), color_at(rect.center().y));
    for p in points {
        mesh.colored_vertex(*p, color_at(p.y));
    }
    let n = points.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    Shape::mesh(mesh)
}

/// The shapes of a raised surface covering `rect`, back to front.
#[must_use]
pub fn surface(rect: Rect, radii: [f32; 4]) -> Vec<Shape> {
    let radii = fit_radii(rect, radii);
    let corners = corner(radii);
    let outline = rounded_outline(rect, radii);
    let mut shapes: Vec<Shape> = vec![
        Shadow {
            offset: [0, 16],
            blur: 44,
            spread: 0,
            color: Color32::from_black_alpha(96),
        }
        .as_shape(rect, corners)
        .into(),
        Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(80),
        }
        .as_shape(rect, corners)
        .into(),
    ];
    shapes.push(gradient_fill(&outline, rect, SURFACE_TOP, SURFACE_BOTTOM));
    shapes.push(Shape::closed_line(
        outline,
        Stroke::new(1.0, Color32::from_white_alpha(18)),
    ));
    let inset = radii[0].max(radii[1]);
    if rect.width() > 2.0 * inset + 4.0 {
        shapes.push(Shape::line_segment(
            [
                pos2(rect.left() + radii[0], rect.top() + 1.0),
                pos2(rect.right() - radii[1], rect.top() + 1.0),
            ],
            Stroke::new(1.0, Color32::from_white_alpha(40)),
        ));
    }
    shapes
}

/// Lays `content` out on a raised surface. The surface is painted behind it once its size is known.
pub fn floating<R>(
    ui: &mut Ui,
    radii: [f32; 4],
    margin: Margin,
    content: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let back = ui.painter().add(Shape::Noop);
    let inner = Frame::new().inner_margin(margin).show(ui, content);
    ui.painter()
        .set(back, Shape::Vec(surface(inner.response.rect, radii)));
    inner
}

/// An eased 0 to 1 value that follows `open`, for things that fade and slide in.
#[must_use]
pub fn pop(ctx: &egui::Context, id: Id, open: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(id, open, 0.2, egui::emath::easing::cubic_out)
}

/// Applies the dark, touch-friendly style to the whole app.
pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = CHROME;
    visuals.window_fill = CHROME;
    visuals.window_stroke = hairline();
    visuals.window_corner_radius = CornerRadius::same(22);
    visuals.menu_corner_radius = CornerRadius::same(14);
    visuals.window_shadow = shadow();
    visuals.popup_shadow = shadow();
    visuals.override_text_color = Some(TEXT);
    visuals.handle_shape = egui::style::HandleShape::Circle;
    visuals.slider_trailing_fill = true;
    visuals.extreme_bg_color = Color32::from_rgb(0x14, 0x13, 0x12);

    let radius = CornerRadius::same(12);
    for w in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = radius;
    }
    visuals.widgets.inactive.weak_bg_fill = CHROME_HI;
    visuals.widgets.inactive.bg_fill = CHROME_HI;
    visuals.widgets.hovered.weak_bg_fill = CHROME_HOVER;
    visuals.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.4);
    // Hovering or pressing a widget changes its color and nothing else. egui's own habit is to grow it by a point,
    // outline it and thicken its strokes, which makes every button look as if it twitches under the pointer.
    let (outline, stroke) = (
        visuals.widgets.inactive.bg_stroke,
        visuals.widgets.inactive.fg_stroke,
    );
    for w in [&mut visuals.widgets.hovered, &mut visuals.widgets.active] {
        w.expansion = 0.0;
        w.bg_stroke = outline;
        w.fg_stroke.width = stroke.width;
    }
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.4);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    ctx.set_visuals(visuals);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.interact_size = Vec2::new(40.0, 40.0);
        style.spacing.button_padding = Vec2::new(12.0, 8.0);
        style.spacing.slider_width = 140.0;
        style.spacing.slider_rail_height = 6.0;
    });
}

#[cfg(test)]
mod tests {
    use eframe::egui::{pos2, vec2};

    use super::*;

    /// The area inside a polygon, by the shoelace formula.
    fn area(points: &[Pos2]) -> f32 {
        let n = points.len();
        (0..n)
            .map(|i| {
                let (a, b) = (points[i], points[(i + 1) % n]);
                a.x * b.y - b.x * a.y
            })
            .sum::<f32>()
            .abs()
            / 2.0
    }

    #[test]
    fn corners_are_never_bigger_than_half_the_shorter_side() {
        let chip = Rect::from_min_size(pos2(0.0, 0.0), vec2(90.0, 33.0));
        for r in fit_radii(chip, [18.0; 4]) {
            assert!((r - 16.5).abs() < 1e-6, "{r}");
        }
        let roomy = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 200.0));
        assert_eq!(
            fit_radii(roomy, [18.0, 0.0, 22.0, 5.0]),
            [18.0, 0.0, 22.0, 5.0]
        );
        let nothing = Rect::from_min_size(pos2(0.0, 0.0), vec2(0.0, 0.0));
        assert_eq!(fit_radii(nothing, [18.0; 4]), [0.0; 4]);
    }

    #[test]
    fn a_small_surface_keeps_its_outline_inside_itself_and_unfolded() {
        // The zoom chip: 33 points tall with corners of 18, which used to fold the outline over itself.
        for width in [46.0, 60.0, 74.0, 120.0] {
            let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(width, 33.0));
            let outline = rounded_outline(rect, [18.0; 4]);
            assert!(
                outline.iter().all(|p| rect.expand(0.01).contains(*p)),
                "a point is outside at width {width}"
            );
            let r: f32 = 16.5;
            let expected = width * 33.0 - (4.0 - PI) * r * r;
            let got = area(&outline);
            assert!(
                (got - expected).abs() / expected < 0.02,
                "width {width}: the outline holds {got}, a clean pill holds {expected}"
            );
        }
    }

    #[test]
    fn a_surface_of_any_size_can_be_built() {
        for (w, h) in [(0.0, 0.0), (1.0, 40.0), (40.0, 1.0), (33.0, 33.0)] {
            let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h));
            assert_ne!(surface(rect, [26.0; 4]).len(), 0);
        }
    }
}
