//! The layers on a device used by touch: a bottom sheet with a big row for each layer, where the desktop has a panel
//! with hover handles, a right-click menu and tiny buttons.
//!
//! A tap on a row picks the layer to draw on; the eye at its left hides it. The layer that is picked opens its own
//! controls under its row: how see-through it is, and buttons to rename, copy, move and delete it. There is no
//! dragging to reorder, which would fight with scrolling the list; a layer moves up or down a place at a time.
//!
//! What is done to a layer is the desktop's (see `layer_ops`); only where it is asked for differs.

use eframe::egui::{
    Align, Align2, Color32, Context, CornerRadius, FontId, Key, Rect, RichText, Sense, Slider,
    Stroke, StrokeKind, TextEdit, Ui, pos2, vec2,
};

#[cfg(test)]
use eframe::egui::Id;

use super::sheet::{self, Tone};
use crate::app::InkApp;
use crate::icons::{self, Icon};
use crate::layer_ops::{Action, LayerRow};
use crate::theme;
use crate::widgets;

/// The height of a layer's row, a fingertip and a bit.
const ROW: f32 = 64.0;
/// How wide the eye's target is at the left of a row.
const EYE: f32 = 56.0;

/// What is under a place on the sheet, which the tests tap.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Hit {
    Row(usize),
    Eye(usize),
    Add,
}

#[cfg(test)]
fn record(ctx: &Context, rect: Rect, hit: Hit) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Vec<(Rect, Hit)>>(Id::NULL)
            .push((rect, hit));
    });
}

#[cfg(test)]
pub(crate) fn hits(ctx: &Context) -> Vec<(Rect, Hit)> {
    ctx.data(|d| d.get_temp(Id::NULL)).unwrap_or_default()
}

/// Shows the sheet while the layers are open.
pub fn show(ctx: &Context, app: &mut InkApp) {
    if !app.ui.layers_open {
        return;
    }
    #[cfg(test)]
    ctx.data_mut(|d| d.insert_temp(Id::NULL, Vec::<(Rect, Hit)>::new()));
    let rows = app.layer_rows();
    let document_layers = app.editor.document().layers.len();
    let mut actions = Vec::new();
    let title = format!("Layers \u{b7} {}", rows.len());
    let answer = sheet::show(ctx, "layers", Some(&title), |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        // The top layer is last in the document and first in the list.
        for row in rows.iter().rev() {
            layer_row(ui, app, row, &mut actions);
            if row.index == app.active_layer {
                controls(ui, row, rows.len(), document_layers, &mut actions);
            }
        }
        ui.add_space(6.0);
        if add_button(ui) {
            actions.push(Action::Add);
        }
    });
    // A press on the sheet is not a press outside the panels.
    app.chrome.push(answer.rect);
    if answer.dismissed {
        app.ui.layers_open = false;
    }
    for action in actions {
        app.apply_layer_action(action);
    }
}

/// One layer: the eye, the name, how much is on it, and a mark on the one being drawn on.
fn layer_row(ui: &mut Ui, app: &mut InkApp, row: &LayerRow, actions: &mut Vec<Action>) {
    let selected = row.index == app.active_layer;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    let eye_rect = Rect::from_min_size(rect.min, vec2(EYE, ROW));
    let eye = ui.interact(eye_rect, response.id.with("eye"), Sense::click());
    #[cfg(test)]
    {
        record(ui.ctx(), rect, Hit::Row(row.index));
        record(ui.ctx(), eye_rect, Hit::Eye(row.index));
    }

    let painter = ui.painter();
    if selected {
        painter.rect_filled(
            rect,
            CornerRadius::same(18),
            theme::ACCENT.gamma_multiply(0.15),
        );
        painter.rect_stroke(
            rect,
            CornerRadius::same(18),
            Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.7)),
            StrokeKind::Inside,
        );
    } else if response.is_pointer_button_down_on() && !eye.is_pointer_button_down_on() {
        painter.rect_filled(rect, CornerRadius::same(18), Color32::from_white_alpha(16));
    }
    let fade = if row.visible { 1.0 } else { 0.5 };
    icons::paint(
        painter,
        Rect::from_center_size(pos2(rect.left() + 28.0, rect.center().y), vec2(24.0, 24.0)),
        if row.visible { Icon::Eye } else { Icon::EyeOff },
        if row.visible {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        },
    );
    if eye.is_pointer_button_down_on() {
        painter.circle_filled(
            pos2(rect.left() + 28.0, rect.center().y),
            22.0,
            Color32::from_white_alpha(18),
        );
    }

    let text_x = rect.left() + EYE + 6.0;
    let renaming = matches!(&app.ui.renaming, Some((i, _)) if *i == row.index);
    if renaming {
        rename_field(ui, app, row.index, rect, text_x, actions);
    } else {
        let name = widgets::elided(
            ui.painter(),
            &row.name,
            FontId::proportional(16.5),
            theme::TEXT.gamma_multiply(fade),
            rect.right() - text_x - 16.0,
        );
        ui.painter()
            .galley(pos2(text_x, rect.top() + 12.0), name, theme::TEXT);
        ui.painter().text(
            pos2(text_x, rect.top() + 43.0),
            Align2::LEFT_CENTER,
            note(row),
            FontId::proportional(12.5),
            theme::TEXT_DIM.gamma_multiply(fade.max(0.7)),
        );
    }

    if eye.clicked() {
        actions.push(Action::ToggleVisible(row.index));
    } else if response.clicked() && !renaming {
        actions.push(Action::Select(row.index));
    }
}

/// "3 strokes", or "Empty", and how see-through the layer is when it is.
fn note(row: &LayerRow) -> String {
    let opacity = (row.opacity < 0.995).then(|| format!(" \u{b7} {:.0}%", row.opacity * 100.0));
    let opacity = opacity.unwrap_or_default();
    match row.strokes {
        0 => format!("Empty{opacity}"),
        1 => format!("1 stroke{opacity}"),
        n => format!("{n} strokes{opacity}"),
    }
}

/// The field a layer is renamed in, in place of its name.
fn rename_field(
    ui: &mut Ui,
    app: &mut InkApp,
    index: usize,
    row: Rect,
    text_x: f32,
    actions: &mut Vec<Action>,
) {
    let Some((_, text)) = &mut app.ui.renaming else {
        return;
    };
    let field = Rect::from_min_max(
        pos2(text_x, row.top() + 10.0),
        pos2(row.right() - 12.0, row.bottom() - 10.0),
    );
    let edit = ui.put(
        field,
        TextEdit::singleline(text)
            .font(FontId::proportional(16.5))
            .align(Align2::LEFT_CENTER)
            .desired_width(field.width()),
    );
    // Asked first: taking the focus back would make the field look as if it had not been left. And asked only when the
    // field has not got it: asking interrupts the keyboard (it is hidden and shown again), so asking every frame would have
    // it opening and closing without end.
    if edit.lost_focus() || ui.input(|i| i.key_pressed(Key::Enter)) {
        actions.push(Action::FinishRename(index, text.clone()));
    } else if !edit.has_focus() {
        edit.request_focus();
    }
}

/// How see-through the layer is, and what can be done to it, under its row.
fn controls(
    ui: &mut Ui,
    row: &LayerRow,
    content_layers: usize,
    document_layers: usize,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        ui.add_space(8.0);
        ui.label(RichText::new("Opacity").size(14.0).color(theme::TEXT_DIM));
        let percent = format!("{:.0}%", row.opacity * 100.0);
        ui.with_layout(eframe::egui::Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(percent).size(14.0).monospace());
            ui.spacing_mut().slider_width = (ui.available_width() - 12.0).max(80.0);
            let mut opacity = row.opacity;
            if widgets::slider(ui, Slider::new(&mut opacity, 0.0..=1.0).show_value(false)).changed()
            {
                actions.push(Action::SetOpacity(row.index, opacity));
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let top = row.index + 1 >= document_layers;
        let bottom = row.index == 0;
        if sheet::pill(ui, "Rename", true, Tone::Normal) {
            actions.push(Action::StartRename(row.index, row.name.clone()));
        }
        if sheet::pill(ui, "Duplicate", true, Tone::Normal) {
            actions.push(Action::Duplicate(row.index));
        }
        if sheet::pill(ui, "Move up", !top, Tone::Normal) {
            actions.push(Action::Move(row.index, row.index + 1));
        }
        if sheet::pill(ui, "Move down", !bottom, Tone::Normal) {
            actions.push(Action::Move(row.index, row.index.saturating_sub(1)));
        }
        // A drawing needs at least one layer.
        if sheet::pill(ui, "Delete", content_layers > 1, Tone::Danger) {
            actions.push(Action::Delete(row.index));
        }
    });
    ui.add_space(4.0);
}

/// The button that adds a layer above the one drawn on.
fn add_button(ui: &mut Ui) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
    #[cfg(test)]
    record(ui.ctx(), rect, Hit::Add);
    let painter = ui.painter();
    let down = response.is_pointer_button_down_on();
    painter.rect_filled(
        rect,
        CornerRadius::same(26),
        if down {
            theme::ACCENT.gamma_multiply(0.82)
        } else {
            theme::ACCENT
        },
    );
    let ink = Color32::from_rgb(0x1B, 0x1A, 0x18);
    icons::paint(
        painter,
        Rect::from_center_size(
            pos2(rect.center().x - 56.0, rect.center().y),
            vec2(20.0, 20.0),
        ),
        Icon::Plus,
        ink,
    );
    painter.text(
        pos2(rect.center().x - 34.0, rect.center().y),
        Align2::LEFT_CENTER,
        "New layer",
        FontId::proportional(17.0),
        ink,
    );
    response.clicked()
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, RawInput};

    use super::*;
    use crate::Options;
    use crate::density::{self, Density};

    /// An app with three layers showing only the sheet, a frame at a time, on a phone.
    struct Phone {
        ctx: Context,
        app: InkApp,
        time: f64,
    }

    impl Phone {
        fn new() -> Self {
            let mut app = InkApp::blank(Options::default());
            app.new_layer();
            app.new_layer();
            app.active_layer = 1;
            app.ui.layers_open = true;
            let mut p = Self {
                ctx: Context::default(),
                app,
                time: 0.0,
            };
            // Long enough for the sheet to finish sliding in.
            for _ in 0..40 {
                p.frame(Vec::new());
            }
            p
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(360.0, 780.0))),
                events,
                time: Some(self.time),
                ..RawInput::default()
            };
            density::set(&self.ctx, Density::TOUCH);
            let app = &mut self.app;
            let mut output = self.ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                app.chrome.clear();
                show(&ctx, app);
            });
            output.textures_delta.clear(); // nothing is rendered in tests
        }

        fn hit(&self, wanted: Hit) -> Pos2 {
            hits(&self.ctx)
                .into_iter()
                .find(|(_, hit)| *hit == wanted)
                .map_or_else(
                    || panic!("{wanted:?} is not on the sheet"),
                    |(rect, _)| rect.center(),
                )
        }

        fn tap(&mut self, at: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
            for _ in 0..5 {
                self.frame(Vec::new());
            }
        }
    }

    #[test]
    fn every_layer_is_a_row_with_the_top_one_first() {
        let p = Phone::new();
        let tops: Vec<(usize, f32)> = (0..3).map(|i| (i, p.hit(Hit::Row(i)).y)).collect();
        assert!(tops[2].1 < tops[1].1 && tops[1].1 < tops[0].1, "{tops:?}");
    }

    #[test]
    fn tapping_a_row_picks_the_layer_to_draw_on() {
        let mut p = Phone::new();
        assert_eq!(p.app.active_layer, 1);
        let row = p.hit(Hit::Row(2)) + vec2(60.0, 0.0);
        p.tap(row);
        assert_eq!(p.app.active_layer, 2);
        assert!(p.app.ui.layers_open, "the sheet stays up");
    }

    #[test]
    fn the_eye_hides_and_shows_a_layer_without_picking_it() {
        let mut p = Phone::new();
        let eye = p.hit(Hit::Eye(2));
        assert!(p.app.editor.document().layers[2].visible);
        p.tap(eye);
        assert!(!p.app.editor.document().layers[2].visible, "hidden");
        assert_eq!(p.app.active_layer, 1, "and not picked");
        let eye = p.hit(Hit::Eye(2));
        p.tap(eye);
        assert!(p.app.editor.document().layers[2].visible, "shown again");
    }

    #[test]
    fn the_button_adds_a_layer_above_the_one_drawn_on() {
        let mut p = Phone::new();
        let before = p.app.editor.document().layers.len();
        let add = p.hit(Hit::Add);
        p.tap(add);
        assert_eq!(p.app.editor.document().layers.len(), before + 1);
        assert_eq!(p.app.active_layer, 2, "the new layer is the one drawn on");
    }

    #[test]
    fn a_tap_on_the_dark_part_puts_the_sheet_away() {
        let mut p = Phone::new();
        p.tap(pos2(180.0, 20.0));
        assert!(!p.app.ui.layers_open);
    }

    #[test]
    fn a_layer_with_nothing_on_it_says_so_and_a_see_through_one_says_how_much() {
        let row = |strokes, opacity| LayerRow {
            index: 0,
            name: String::new(),
            visible: true,
            opacity,
            strokes,
        };
        assert_eq!(note(&row(0, 1.0)), "Empty");
        assert_eq!(note(&row(1, 1.0)), "1 stroke");
        assert_eq!(note(&row(12, 0.5)), "12 strokes \u{b7} 50%");
    }
}
