//! The bar along the top on a device used by touch.
//!
//! There are no tabs: a browser's row of tabs is a mouse's idea. A drawing has a way back to the Library and its name,
//! and the other drawings are in the Library. On the right are the buttons every bar has (see `bar_buttons`), made big
//! enough for a fingertip.

use eframe::egui::{
    self, Align, Color32, FontId, Frame, Layout, Margin, Sense, Stroke, Ui, pos2, vec2,
};

use crate::app::InkApp;
use crate::bar_buttons;
use crate::density;
use crate::icons::Icon;
use crate::theme;
use crate::widgets;

const BAR: Color32 = Color32::from_rgb(0x15, 0x14, 0x12);
/// How big a button is, and how big on a screen too narrow for the usual size.
const BUTTON: f32 = 44.0;
const NARROW_BUTTON: f32 = 40.0;
const NARROW_BELOW: f32 = 480.0;
const BUTTON_GAP: f32 = 8.0;
const TITLE_SIZE: f32 = 18.0;
/// The room the dot that says a drawing has unsaved changes takes after its name.
const DOT_ROOM: f32 = 18.0;

/// Shows the bar. Call it before the central panel so the canvas gets what is left.
pub fn show(ui: &mut Ui, app: &mut InkApp) {
    let height = density::bar_height(ui.ctx());
    let label = if app.ui.library_open {
        None
    } else {
        app.active.map(|i| app.tab_label(i))
    };
    let mut back = false;
    egui::Panel::top("tab_bar")
        .exact_size(height)
        .frame(
            Frame::new()
                .fill(BAR)
                .inner_margin(Margin::symmetric(10, 0)),
        )
        .show(ui, |ui| {
            let full = ui.max_rect();
            ui.painter().line_segment(
                [full.left_bottom(), full.right_bottom()],
                Stroke::new(1.0, Color32::from_white_alpha(20)),
            );
            let button = if full.width() < NARROW_BELOW {
                NARROW_BUTTON
            } else {
                BUTTON
            };
            ui.spacing_mut().item_spacing = vec2(BUTTON_GAP, 0.0);
            ui.horizontal(|ui| {
                let left =
                    (ui.available_width() - bar_buttons::width(button, BUTTON_GAP) - BUTTON_GAP)
                        .max(60.0);
                ui.allocate_ui_with_layout(
                    vec2(left, height),
                    Layout::left_to_right(Align::Center),
                    |ui| match &label {
                        Some(label) => {
                            back = widgets::round_button(ui, button, Icon::Left, true, false)
                                .clicked();
                            title(ui, &label.name, label.dirty, height);
                        }
                        None => title(ui, "inkwell", false, height),
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    bar_buttons::show(ui, app, button);
                });
            });
        });
    if back {
        app.open_library(ui.ctx());
    }
}

/// The name of what is showing, cut with an ellipsis if it does not fit, and a dot after it while it has changes
/// that are not saved.
fn title(ui: &mut Ui, text: &str, dirty: bool, height: f32) {
    let room = (ui.available_width() - if dirty { DOT_ROOM } else { 0.0 }).max(0.0);
    let galley = widgets::elided(
        ui.painter(),
        text,
        FontId::proportional(TITLE_SIZE),
        theme::TEXT,
        room,
    );
    let width = galley.size().x + if dirty { DOT_ROOM } else { 0.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let at = pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    let end = at.x + galley.size().x;
    ui.painter().galley(at, galley, theme::TEXT);
    if dirty {
        ui.painter()
            .circle_filled(pos2(end + 10.0, rect.center().y), 3.5, theme::ACCENT);
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Context, Event, Modifiers, PointerButton, RawInput, Rect};

    use super::*;
    use crate::Options;
    use crate::density::Density;

    fn frame(ctx: &Context, app: &mut InkApp, events: Vec<Event>, time: f64) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(360.0, 780.0))),
            events,
            time: Some(time),
            ..RawInput::default()
        };
        density::set(ctx, Density::TOUCH);
        let mut output = ctx.run_ui(input, |ui| show(ui, app));
        output.textures_delta.clear(); // nothing is rendered in tests
    }

    #[test]
    fn the_back_arrow_of_a_drawing_goes_to_the_library() {
        let ctx = Context::default();
        let mut app = InkApp::blank(Options::default());
        app.new_tab(&ctx);
        assert!(!app.ui.library_open, "a drawing is open");
        let at = pos2(32.0, 28.0);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        frame(&ctx, &mut app, Vec::new(), 0.0);
        frame(&ctx, &mut app, vec![Event::PointerMoved(at)], 0.02);
        frame(&ctx, &mut app, vec![button(true)], 0.04);
        frame(&ctx, &mut app, vec![button(false)], 0.06);
        assert!(app.ui.library_open, "back is the Library");
    }

    #[test]
    fn the_bar_is_as_tall_as_a_finger_needs() {
        let ctx = Context::default();
        let mut app = InkApp::blank(Options::default());
        app.new_tab(&ctx);
        frame(&ctx, &mut app, Vec::new(), 0.0);
        frame(&ctx, &mut app, Vec::new(), 0.02);
        assert!(
            app.chrome
                .iter()
                .any(|r| r.min.y < 10.0 && r.width() >= 40.0),
            "the buttons of the bar are panels, and big ones"
        );
        assert!(density::bar_height(&ctx) >= 56.0);
    }
}
