//! Small shared controls, so every panel looks and behaves the same.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, Response, RichText, Sense, Slider,
    Stroke, Ui, vec2,
};

use crate::config::Pattern;
use crate::icons::{self, Icon};
use crate::paper::{self, Ruling};
use crate::theme;

/// A slider with a handle smaller than the 40-point touch targets of the buttons.
pub fn slider(ui: &mut Ui, slider: Slider<'_>) -> Response {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 32.0;
        ui.add(slider)
    })
    .inner
}

/// A rounded box that groups related controls.
pub fn card() -> Frame {
    Frame::new()
        .fill(theme::CARD)
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(14))
}

/// Small dim text for hints and units.
pub fn caption(ui: &mut Ui, text: impl Into<String>) {
    ui.label(RichText::new(text).size(12.5).color(theme::TEXT_DIM));
}

/// A section title.
pub fn heading(ui: &mut Ui, text: impl Into<String>) {
    ui.label(
        RichText::new(text)
            .size(13.0)
            .strong()
            .color(theme::TEXT_DIM),
    );
}

/// An on/off switch. Returns the response; `changed()` is set when it was toggled.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(48.0, 28.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    let track = theme::CHROME_HI.lerp_to_gamma(theme::ACCENT, t);
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(14), track);
    let knob_x = egui::lerp(rect.left() + 14.0..=rect.right() - 14.0, t);
    painter.circle_filled(egui::pos2(knob_x, rect.center().y), 10.5, Color32::WHITE);
    if response.hovered() {
        painter.rect_stroke(
            rect,
            CornerRadius::same(14),
            Stroke::new(1.0, Color32::from_white_alpha(50)),
            egui::StrokeKind::Inside,
        );
    }
    response
}

/// A row with a title and an optional hint on the left and a control on the right.
pub fn setting_row(ui: &mut Ui, title: &str, hint: Option<&str>, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).size(15.0));
            if let Some(hint) = hint {
                caption(ui, hint);
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), control);
    });
}

/// A setting that is on or off. Returns whether it changed.
pub fn toggle_row(ui: &mut Ui, title: &str, hint: Option<&str>, on: &mut bool) -> bool {
    let mut changed = false;
    setting_row(ui, title, hint, |ui| changed = switch(ui, on).changed());
    changed
}

/// A setting with a slider and its value written out. Returns whether it changed.
pub fn slider_row(
    ui: &mut Ui,
    title: &str,
    hint: Option<&str>,
    value: &mut f32,
    range: RangeInclusive<f32>,
    shown: impl Fn(f32) -> String,
) -> bool {
    let mut changed = false;
    setting_row(ui, title, hint, |ui| {
        ui.label(
            RichText::new(shown(*value))
                .size(14.0)
                .color(theme::TEXT_DIM)
                .monospace(),
        );
        ui.spacing_mut().slider_width = 180.0;
        changed = slider(ui, Slider::new(value, range).show_value(false)).changed();
    });
    changed
}

/// A row of mutually exclusive choices. Returns whether the choice changed.
pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, options: &[(T, &str)], value: &mut T) -> bool {
    let mut changed = false;
    ui.spacing_mut().item_spacing.x = 4.0;
    // In a row that fills from the right the first button added is the last one seen, so add them the other way
    // round to have the choices read in order.
    let mut in_order: Vec<&(T, &str)> = options.iter().collect();
    if ui.layout().prefer_right_to_left() {
        in_order.reverse();
    }
    for (option, label) in in_order {
        let selected = *value == *option;
        let button = egui::Button::new(RichText::new(*label).size(14.0).color(if selected {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        }))
        .fill(if selected {
            theme::ACCENT.gamma_multiply(0.45)
        } else {
            theme::CHROME_HI
        })
        .stroke(if selected {
            Stroke::new(1.0, theme::ACCENT)
        } else {
            Stroke::NONE
        })
        .min_size(vec2(0.0, 34.0));
        if ui.add(button).clicked() && !selected {
            *value = *option;
            changed = true;
        }
    }
    changed
}

/// A background that ink of the given color stands out against.
#[must_use]
pub fn backdrop_for(rgb: [u8; 3]) -> Color32 {
    let luma = 0.299 * f32::from(rgb[0]) + 0.587 * f32::from(rgb[1]) + 0.114 * f32::from(rgb[2]);
    if luma > 150.0 {
        Color32::from_rgb(0x2A, 0x2C, 0x33)
    } else {
        Color32::from_rgb(0xF0, 0xF1, 0xF3)
    }
}

/// Text cut to `max_width` on one row, ending in an ellipsis if it is longer.
pub fn elided(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    color: Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
    job.wrap.max_width = max_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    painter.layout_job(job)
}

/// A thin line between groups of settings.
pub fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, Color32::from_white_alpha(18));
}

/// A round icon button. `active` shows it as switched on.
pub fn round_button(
    ui: &mut Ui,
    diameter: f32,
    icon: Icon,
    enabled: bool,
    active: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(diameter, diameter), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered() && enabled, 0.12);
    let painter = ui.painter();
    let fill = if active {
        theme::ACCENT.gamma_multiply(0.22)
    } else {
        theme::CHROME_HI.lerp_to_gamma(theme::CHROME_HOVER, hover)
    };
    painter.circle_filled(rect.center(), diameter / 2.0, fill);
    painter.circle_stroke(
        rect.center(),
        diameter / 2.0 - 0.5,
        Stroke::new(
            1.0,
            if active {
                theme::ACCENT.gamma_multiply(0.6)
            } else {
                Color32::from_white_alpha(18)
            },
        ),
    );
    let color = if !enabled {
        theme::TEXT_DIM.gamma_multiply(0.45)
    } else if active {
        theme::ACCENT
    } else {
        theme::TEXT
    };
    icons::paint(painter, rect.shrink(diameter * 0.27), icon, color);
    response
}

/// The size of a pattern tile: a small sheet of paper with the pattern on it, and its name under it.
pub const PATTERN_TILE: egui::Vec2 = vec2(62.0, 72.0);

/// A small sheet of `sheet` paper printed with `pattern`, with its name under it. `selected` rings it.
pub fn pattern_tile(
    ui: &mut Ui,
    pattern: Pattern,
    selected: bool,
    sheet: Color32,
    ink: Color32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(PATTERN_TILE, Sense::click());
    let page = egui::Rect::from_min_size(rect.min, vec2(rect.width(), 52.0));
    let painter = ui.painter();
    painter.rect_filled(page, 12.0, sheet);
    paper::paint(
        painter,
        page.shrink(1.0),
        page.center(),
        &Ruling {
            pattern,
            spacing: 11.0,
            strength: 0.9,
            ink,
            min_spacing: 0.0,
            major_every: 4,
            zoom: 0.04,
            weight: 1.0,
            turn: 0.0,
        },
    );
    painter.rect_stroke(
        page,
        12.0,
        if selected {
            Stroke::new(2.0, theme::ACCENT)
        } else if response.hovered() {
            Stroke::new(1.5, Color32::from_white_alpha(90))
        } else {
            Stroke::new(1.0, Color32::from_white_alpha(30))
        },
        egui::StrokeKind::Inside,
    );
    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - 8.0),
        egui::Align2::CENTER_CENTER,
        pattern.label(),
        egui::FontId::proportional(12.0),
        if selected {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        },
    );
    response
}

/// A rounded-square icon button for a tool. `active` shows it as the tool in use.
pub fn tool_button(ui: &mut Ui, size: f32, icon: Icon, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.12);
    let painter = ui.painter();
    let fill = if active {
        theme::ACCENT.gamma_multiply(0.24)
    } else {
        theme::CHROME_HI.lerp_to_gamma(theme::CHROME_HOVER, hover)
    };
    painter.rect_filled(rect, CornerRadius::same(11), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(11),
        Stroke::new(
            1.0,
            if active {
                theme::ACCENT.gamma_multiply(0.7)
            } else {
                Color32::from_white_alpha(16)
            },
        ),
        egui::StrokeKind::Inside,
    );
    let color = if active { theme::ACCENT } else { theme::TEXT };
    icons::paint(painter, rect.shrink(size * 0.24), icon, color);
    response
}

/// A row of a menu: a label on the left and the keyboard shortcut on the right.
pub fn menu_row(ui: &mut Ui, label: &str, keys: &str) -> Response {
    menu_row_colored(ui, label, keys, theme::TEXT)
}

/// Like [`menu_row`], with the label in `color`, for actions that deserve a warning.
pub fn menu_row_colored(ui: &mut Ui, label: &str, keys: &str, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(12), theme::CHROME_HI);
    }
    painter.text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(15.0),
        color,
    );
    if !keys.is_empty() {
        painter.text(
            egui::pos2(rect.right() - 12.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            keys,
            egui::FontId::monospace(12.0),
            theme::TEXT_DIM,
        );
    }
    response
}

/// Text choices with an underline under the chosen one. Returns whether the choice changed.
pub fn text_tabs<T: Copy + PartialEq>(ui: &mut Ui, options: &[(T, &str)], value: &mut T) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        for (option, label) in options {
            let selected = *value == *option;
            let color = if selected {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            };
            let galley = ui.painter().layout_no_wrap(
                (*label).to_owned(),
                egui::FontId::proportional(14.0),
                color,
            );
            let (rect, response) =
                ui.allocate_exact_size(vec2(galley.size().x, 30.0), Sense::click());
            let at = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0 - 1.0);
            ui.painter().galley(at, galley, color);
            if selected {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(rect.left(), rect.bottom() - 3.0),
                        vec2(rect.width(), 2.0),
                    ),
                    CornerRadius::same(1),
                    theme::ACCENT,
                );
            }
            if response.clicked() && !selected {
                *value = *option;
                changed = true;
            }
        }
    });
    changed
}

/// A scrubber shaped like a wedge that thickens to the right, for sizes. The scale is logarithmic. The handle
/// carries a dot in `ink` that grows with the value.
pub fn wedge(ui: &mut Ui, value: &mut f32, range: RangeInclusive<f32>, ink: Color32) -> Response {
    let (lo, hi) = (*range.start(), *range.end());
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click_and_drag());
    let track = rect.shrink2(vec2(14.0, 0.0));
    if (response.dragged() || response.clicked())
        && let Some(p) = response.interact_pointer_pos()
    {
        let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        let new = lo * (hi / lo).powf(t);
        if (new - *value).abs() > f32::EPSILON {
            *value = new;
            response.mark_changed();
        }
    }
    let t = ((*value / lo).ln() / (hi / lo).ln()).clamp(0.0, 1.0);
    let cy = track.center().y;
    let half = |f: f32| 1.3 + 11.0 * f;
    let part = |f0: f32, f1: f32| {
        vec![
            egui::pos2(track.left() + f0 * track.width(), cy - half(f0)),
            egui::pos2(track.left() + f1 * track.width(), cy - half(f1)),
            egui::pos2(track.left() + f1 * track.width(), cy + half(f1)),
            egui::pos2(track.left() + f0 * track.width(), cy + half(f0)),
        ]
    };
    let painter = ui.painter();
    painter.add(egui::Shape::convex_polygon(
        part(0.0, 1.0),
        Color32::from_white_alpha(26),
        Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        part(0.0, t),
        theme::TEXT,
        Stroke::NONE,
    ));
    let at = egui::pos2(track.left() + t * track.width(), cy);
    painter.circle_filled(at + vec2(0.0, 2.0), 13.5, Color32::from_black_alpha(90));
    painter.circle_filled(at, 13.0, Color32::from_rgb(0xF6, 0xF2, 0xEA));
    painter.circle_filled(at, 3.0 + 6.0 * t, ink);
    painter.circle_stroke(at, 13.0, Stroke::new(1.0, Color32::from_black_alpha(60)));
    response
}

/// A scrubber for opacity: a checkerboard fading in to `ink`, with a handle.
pub fn fade_bar(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    ink: Color32,
) -> Response {
    let (lo, hi) = (*range.start(), *range.end());
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click_and_drag());
    let track = rect.shrink2(vec2(14.0, 10.0));
    if (response.dragged() || response.clicked())
        && let Some(p) = response.interact_pointer_pos()
    {
        let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        let new = lo + (hi - lo) * t;
        if (new - *value).abs() > f32::EPSILON {
            *value = new;
            response.mark_changed();
        }
    }
    let t = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let painter = ui.painter().with_clip_rect(track);
    let cell = 8.0;
    let (columns, rows) = (
        (track.width() / cell).ceil() as u32,
        (track.height() / cell).ceil() as u32,
    );
    for row in 0..rows {
        for column in 0..columns {
            let shade = if (row + column) % 2 == 0 { 0x3C } else { 0x4C };
            painter.rect_filled(
                egui::Rect::from_min_size(
                    track.min + vec2(column as f32 * cell, row as f32 * cell),
                    vec2(cell, cell),
                ),
                CornerRadius::ZERO,
                Color32::from_gray(shade),
            );
        }
    }
    let [r, g, b, _] = ink.to_array();
    let mut mesh = egui::Mesh::default();
    let (clear, solid) = (
        Color32::from_rgba_unmultiplied(r, g, b, 8),
        Color32::from_rgba_unmultiplied(r, g, b, 255),
    );
    mesh.colored_vertex(track.left_top(), clear);
    mesh.colored_vertex(track.right_top(), solid);
    mesh.colored_vertex(track.right_bottom(), solid);
    mesh.colored_vertex(track.left_bottom(), clear);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
    ui.painter().rect_stroke(
        track,
        CornerRadius::same(3),
        Stroke::new(1.0, Color32::from_white_alpha(30)),
        egui::StrokeKind::Outside,
    );
    let at = egui::pos2(track.left() + t * track.width(), track.center().y);
    let painter = ui.painter();
    painter.circle_filled(at + vec2(0.0, 2.0), 13.5, Color32::from_black_alpha(90));
    painter.circle_filled(at, 13.0, Color32::from_rgb(0xF6, 0xF2, 0xEA));
    painter.circle_filled(
        at,
        7.5,
        Color32::from_rgba_unmultiplied(r, g, b, (t * 255.0).round() as u8),
    );
    painter.circle_stroke(at, 13.0, Stroke::new(1.0, Color32::from_black_alpha(60)));
    response
}
