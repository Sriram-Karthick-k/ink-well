//! What each item of the tool bar looks like and is called, for the bar itself and for the sheet where it is arranged.

use eframe::egui::{Color32, Painter, Rect, Stroke, vec2};

use crate::app::InkApp;
use crate::icons::{self, Icon};
use crate::shape_art::{self, Look};
use crate::theme;
use crate::toolbar::Item;

/// The icon of a tool; a shape and the ink are drawn as themselves.
#[must_use]
pub fn icon(item: &Item) -> Option<Icon> {
    match item {
        Item::Undo => Some(Icon::Undo),
        Item::Redo => Some(Icon::Redo),
        Item::Eraser => Some(Icon::Eraser),
        Item::Select => Some(Icon::Select),
        Item::Slice => Some(Icon::Slice),
        Item::Nudge => Some(Icon::Nudge),
        Item::Move => Some(Icon::Hand),
        Item::Shapes => Some(Icon::Shapes),
        Item::Text => Some(Icon::Text),
        Item::Pens => Some(Icon::Brushes),
        Item::Color | Item::Shape(_) => None,
    }
}

/// What the item is called: a tool by its name, a shape by the shape's.
#[must_use]
pub fn label(app: &InkApp, item: &Item) -> String {
    match item {
        Item::Shape(id) => app
            .objects
            .get(id)
            .map_or_else(|| "Shape".to_owned(), |shape| shape.name.clone()),
        other => other.label().unwrap_or("").to_owned(),
    }
}

/// Paints the picture of `item` in `rect` (about an icon's size), in `color` where it is drawn in one.
pub fn paint(painter: &Painter, rect: Rect, app: &InkApp, item: &Item, color: Color32) {
    if let Some(icon) = icon(item) {
        icons::paint(painter, rect, icon, color);
        return;
    }
    match item {
        Item::Color => {
            let [r, g, b] = app.tool.color.to_srgb();
            let center = rect.center();
            let radius = rect.width().min(rect.height()) / 2.0;
            painter.circle_filled(
                center + vec2(0.0, 1.0),
                radius,
                Color32::from_black_alpha(90),
            );
            painter.circle_filled(center, radius, Color32::from_rgb(0xB8, 0xB3, 0xA8));
            painter.circle_filled(center, radius - 3.0, Color32::from_rgb(r, g, b));
            painter.circle_stroke(
                center,
                radius - 3.0,
                Stroke::new(1.0, Color32::from_black_alpha(70)),
            );
        }
        Item::Shape(id) => {
            if let Some(shape) = app.objects.get(id) {
                let look = Look {
                    line: color,
                    fill: None,
                    outline: true,
                };
                shape_art::paint(painter, rect.shrink(1.0), shape, look, 1.8);
            } else {
                // A shape that is gone leaves a dot where it was.
                painter.circle_filled(rect.center(), 3.0, theme::TEXT_DIM);
            }
        }
        _ => {}
    }
}
