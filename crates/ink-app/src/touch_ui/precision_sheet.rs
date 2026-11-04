//! The precision panel on a device used by touch: the snap switches, the shape guides and shape recognition in a bottom
//! sheet, where the desktop has a window hanging under the button.
//!
//! What is in it, and what it does, is the desktop's (see `precision`); only where it is and how it is put away differ.

use eframe::egui::Context;

use super::sheet;
use crate::app::InkApp;
use crate::precision;
use crate::widgets;

/// Shows the sheet while the precision panel is open.
pub fn show(ctx: &Context, app: &mut InkApp) {
    if !app.ui.precision_open {
        return;
    }
    let before = app.config.clone();
    let answer = sheet::show(ctx, "precision", Some("Precision"), |ui| {
        ui.spacing_mut().item_spacing.y = 10.0;
        let canvas = app.config.canvas.clone();
        precision::controls(ui, &mut app.config.precision, &canvas);
        widgets::divider(ui);
        precision::guide_buttons(ui, app);
        widgets::divider(ui);
        widgets::heading(ui, "SHAPE RECOGNITION");
        precision::recognition_controls(ui, &mut app.config.precision);
    });
    // A press on the sheet is not a press outside the panels, which would put it away under the finger.
    app.chrome.push(answer.rect);
    if answer.dismissed {
        app.ui.precision_open = false;
    }
    if app.config != before {
        app.mark_config_changed();
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

    use super::*;
    use crate::Options;
    use crate::density::{self, Density};

    /// The app showing only the sheet, a frame at a time, on a phone.
    struct Phone {
        ctx: Context,
        app: InkApp,
        time: f64,
    }

    impl Phone {
        fn new() -> Self {
            let mut app = InkApp::blank(Options::default());
            app.ui.precision_open = true;
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
    fn the_sheet_is_up_while_the_panel_is_open_and_covers_the_bottom_of_the_screen() {
        let p = Phone::new();
        let sheet = *p.app.chrome.last().expect("the sheet");
        assert!(sheet.bottom() > 700.0, "{sheet:?}");
        assert!(sheet.width() <= 360.0 + 2.0);
    }

    #[test]
    fn a_tap_on_the_dark_part_above_the_sheet_puts_it_away() {
        let mut p = Phone::new();
        p.tap(pos2(180.0, 30.0));
        assert!(!p.app.ui.precision_open);
    }

    #[test]
    fn a_tap_on_a_switch_inside_changes_it_and_leaves_the_sheet_up() {
        let mut p = Phone::new();
        let was = p.app.config.precision.snap;
        let sheet = *p.app.chrome.last().expect("the sheet");
        // The first switch, Snap, is in the first row: after the margin, the handle and the title, half a row down, and
        // in from the right edge by the margin and half the switch.
        let at = pos2(
            sheet.right() - 20.0 - 28.0,
            sheet.top() + 10.0 + 10.0 + 6.0 + 24.0 + 6.0 + 24.0,
        );
        p.tap(at);
        assert_ne!(p.app.config.precision.snap, was, "snapping was switched");
        assert!(p.app.ui.precision_open, "the sheet stays up");
    }
}
