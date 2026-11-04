//! The buttons at the right end of the top bar: precision, layers, the drawing menu and the settings. Both bars (the
//! desktop's with its tabs, and the touch one) show the same four, each at the size it asks for.
//!
//! Each button is put in `app.chrome`: they open and close their own panels, so a press on one must not count as a
//! press outside the panels, which puts the open one away. Were it counted, a tap quick enough to press and release in
//! one frame would open a panel and shut it again straight away.

use eframe::egui::Ui;

use crate::app::InkApp;
use crate::icons::Icon;
use crate::shortcuts::Command;
use crate::widgets;

/// How many buttons there can be.
pub const COUNT: f32 = 4.0;

/// The width `COUNT` buttons of `size` take with `gap` between them.
#[must_use]
pub fn width(size: f32, gap: f32) -> f32 {
    COUNT * size + (COUNT - 1.0) * gap
}

/// Shows the buttons from the right edge inward, so call it in a right-to-left layout.
pub fn show(ui: &mut Ui, app: &mut InkApp, size: f32) {
    let settings =
        widgets::round_button(ui, size, Icon::Gear, true, false).on_hover_text("Settings (Ctrl+,)");
    app.chrome.push(settings.rect);
    if settings.clicked() {
        app.open_settings_home();
    }
    if app.ui.library_open {
        return;
    }
    let menu = app.ui.sheet_open;
    let menu_button = widgets::round_button(ui, size, Icon::Menu, true, menu)
        .on_hover_text("Drawing menu: save, paper, shortcuts");
    app.chrome.push(menu_button.rect);
    if menu_button.clicked() {
        app.ui.toggle_sheet();
    }
    let layers = app.ui.layers_open;
    let layers_button =
        widgets::round_button(ui, size, Icon::Layers, true, layers).on_hover_text("Layers (L)");
    app.chrome.push(layers_button.rect);
    if layers_button.clicked() {
        app.ui.toggle_layers();
    }
    // Lit while the panel is open or snapping is on, so it can be seen without opening it.
    let precision = app.ui.precision_open || app.config.precision.snap;
    let hint = app
        .keymap
        .hint(Command::OpenPrecision, "Precision: snapping");
    let precision_button =
        widgets::round_button(ui, size, Icon::Precision, true, precision).on_hover_text(hint);
    app.chrome.push(precision_button.rect);
    if precision_button.clicked() {
        app.ui.toggle_precision();
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{
        Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2,
    };

    use super::*;
    use crate::Options;

    #[test]
    fn four_buttons_and_the_gaps_between_them_take_their_sum() {
        assert!((width(40.0, 10.0) - 190.0).abs() < 1e-4);
        assert!((width(34.0, 6.0) - 154.0).abs() < 1e-4);
    }

    /// The app, a frame at a time.
    struct Frames {
        ctx: Context,
        app: InkApp,
        time: f64,
    }

    impl Frames {
        fn new() -> Self {
            let mut f = Self {
                ctx: Context::default(),
                app: InkApp::blank(Options::default()),
                time: 0.0,
            };
            f.run(Vec::new());
            f.run(Vec::new());
            f
        }

        fn run(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
                events,
                time: Some(self.time),
                ..RawInput::default()
            };
            let app = &mut self.app;
            let mut output = self.ctx.run_ui(input, |ui| app.ui_root(ui));
            output.textures_delta.clear(); // nothing is rendered in tests
        }

        /// A tap whose press and release come in the same frame, as a quick one can.
        fn quick_tap(&mut self, at: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.run(vec![Event::PointerMoved(at), button(true), button(false)]);
            self.run(Vec::new());
            self.run(Vec::new());
        }

        /// Where the buttons are on the desktop bar: settings, menu, layers and precision, from the right edge in.
        /// Each is 34 across with 6 between, and the bar keeps 10 at its edge and is 46 tall.
        fn buttons() -> [Pos2; 4] {
            [0, 1, 2, 3].map(|i| pos2(1280.0 - 10.0 - 17.0 - i as f32 * 40.0, 23.0))
        }
    }

    #[test]
    fn a_quick_tap_opens_a_panel_and_does_not_shut_it_again() {
        let mut f = Frames::new();
        let [gear, menu, layers, precision] = Frames::buttons();

        f.quick_tap(precision);
        assert!(f.app.ui.precision_open, "precision opens");
        f.quick_tap(precision);
        assert!(!f.app.ui.precision_open, "and a second tap puts it away");

        f.quick_tap(menu);
        assert!(f.app.ui.sheet_open, "the menu opens");
        f.quick_tap(menu);
        assert!(!f.app.ui.sheet_open, "and goes");

        f.quick_tap(layers);
        assert!(f.app.ui.layers_open, "layers open");
        f.quick_tap(precision);
        assert!(
            f.app.ui.precision_open && !f.app.ui.layers_open,
            "precision takes the corner layers had"
        );

        f.quick_tap(gear);
        assert!(f.app.ui.settings.is_some(), "the settings open");
        assert!(f.app.ui.settings_home, "on their landing page");
        assert!(!f.app.ui.precision_open, "with nothing else left open");
    }
}
