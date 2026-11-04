//! The drawing menu on a device used by touch: a bottom sheet with big rows, where the desktop has a small menu hanging
//! from the button that opened it.
//!
//! Nothing in it names a key. A drawing saves itself, so there is no Save, only a copy to take out of the app, to
//! Downloads or a cloud drive.

use eframe::egui::{Context, vec2};

use super::sheet::{self, Tone};
use crate::app::InkApp;
use crate::canvas;
use crate::config::Pattern;
use crate::icons::Icon;
use crate::paper;
use crate::settings::Section;
use crate::widgets;

/// Something chosen in the menu.
#[derive(Clone, Copy)]
enum Choice {
    Library,
    New,
    Open,
    Copy,
    Fit,
    Turn,
    Settings,
    About,
    Paper(Pattern),
}

/// Shows the menu while it is open.
pub fn show(ctx: &Context, app: &mut InkApp) {
    if !app.ui.sheet_open {
        return;
    }
    let title = app.document_name();
    let answer = sheet::show(ctx, "drawing_menu", Some(&title), |ui| rows(ui, app));
    // A press on the sheet is not a press outside the panels, which would put the menu away before the row was tapped.
    app.chrome.push(answer.rect);
    if answer.dismissed {
        app.ui.sheet_open = false;
    }
    let Some(choice) = answer.inner else {
        return;
    };
    // Choosing a paper leaves the menu open, to see it change behind and choose again.
    if let Choice::Paper(pattern) = choice {
        app.config.canvas.pattern = pattern;
        app.mark_config_changed();
        return;
    }
    app.ui.sheet_open = false;
    match choice {
        Choice::Library => app.open_library(ctx),
        Choice::New => {
            // One drawing is shown at a time: the one made now replaces this one.
            app.close_all_tabs(ctx);
            app.new_tab(ctx);
        }
        Choice::Open => app.request_open(ctx),
        Choice::Copy => app.save_as(ctx),
        Choice::Fit => app.fit_pending = true,
        Choice::Turn => app.ui.toggle_turn(),
        Choice::Settings => app.open_settings_home(),
        Choice::About => app.open_settings(Section::About),
        Choice::Paper(_) => {}
    }
}

fn rows(ui: &mut eframe::egui::Ui, app: &InkApp) -> Option<Choice> {
    let mut chosen = None;
    let mut row = |ui: &mut eframe::egui::Ui, icon: Option<Icon>, label: &str, choice: Choice| {
        if sheet::row(ui, icon, label, Tone::Normal) {
            chosen = Some(choice);
        }
    };
    row(ui, None, "Library", Choice::Library);
    row(ui, Some(Icon::Plus), "New drawing", Choice::New);
    row(ui, None, "Open a file", Choice::Open);
    row(ui, None, "Save a copy", Choice::Copy);
    sheet::divider(ui);
    row(ui, None, "Fit to screen", Choice::Fit);
    row(ui, None, "Turn the paper", Choice::Turn);
    sheet::divider(ui);
    row(ui, Some(Icon::Gear), "Settings", Choice::Settings);
    row(ui, None, "About", Choice::About);
    sheet::divider(ui);
    sheet::caption(ui, "PAPER");
    if let Some(pattern) = paper_tiles(ui, app) {
        chosen = Some(Choice::Paper(pattern));
    }
    chosen
}

/// A small sheet of paper for each pattern, wrapping onto as many rows as the width needs. Returns the one tapped.
fn paper_tiles(ui: &mut eframe::egui::Ui, app: &InkApp) -> Option<Pattern> {
    let sheet = canvas::parse_hex(&app.editor.document().metadata.background_solid_color);
    let ink = paper::ink_for(sheet);
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 8.0);
        for pattern in Pattern::ALL {
            let selected = app.config.canvas.pattern == pattern;
            if widgets::pattern_tile(ui, pattern, selected, sheet, ink).clicked() {
                picked = Some(pattern);
            }
        }
    });
    picked
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2};

    use super::*;
    use crate::Options;
    use crate::density::{self, Density};

    /// The app showing only the menu, a frame at a time, on a phone.
    struct Phone {
        ctx: Context,
        app: InkApp,
        time: f64,
    }

    impl Phone {
        fn new() -> Self {
            let mut app = InkApp::blank(Options::default());
            app.ui.sheet_open = true;
            let mut p = Self {
                ctx: Context::default(),
                app,
                time: 0.0,
            };
            // Long enough for the sheet to finish sliding in, so where it is measured is where it stays.
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
                // The panels are laid out first, as in the app, so a press is judged against them.
                app.chrome.clear();
                show(&ctx, app);
            });
            output.textures_delta.clear(); // nothing is rendered in tests
        }

        fn settle(&mut self) {
            for _ in 0..8 {
                self.frame(Vec::new());
            }
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
            self.settle();
        }

        /// The place of a row that has `rows` rows and `lines` dividers above it, in the middle of the sheet.
        fn row(&self, rows: usize, lines: usize) -> Pos2 {
            let sheet = *self.app.chrome.last().expect("the sheet is a panel");
            // The first row follows the handle and the title; a divider is a line with a little room on each side.
            let first = sheet.top() + 10.0 + 10.0 + 6.0 + 24.0 + 6.0 + sheet::ROW / 2.0;
            pos2(
                sheet.center().x,
                first + rows as f32 * (sheet::ROW + 2.0) + lines as f32 * 11.0,
            )
        }
    }

    #[test]
    fn the_menu_is_a_sheet_that_names_the_drawing_and_stays_open_while_it_is_there() {
        let p = Phone::new();
        assert!(p.app.ui.sheet_open);
        let sheet = *p.app.chrome.last().expect("the sheet");
        assert!(
            sheet.bottom() > 700.0,
            "it rests on the bottom edge: {sheet:?}"
        );
    }

    #[test]
    fn tapping_a_row_does_it_and_puts_the_sheet_away() {
        let mut p = Phone::new();
        let new_tab = p.row(1, 0);
        p.tap(new_tab);
        assert!(!p.app.ui.sheet_open, "the sheet is put away");
        assert_eq!(
            p.app.tabs.len(),
            1,
            "the new drawing is the only one open: a touch screen shows one at a time"
        );
    }

    #[test]
    fn the_settings_row_opens_the_settings_on_their_list() {
        let mut p = Phone::new();
        let settings = p.row(6, 2);
        p.tap(settings);
        assert!(p.app.ui.settings.is_some());
        assert!(p.app.ui.settings_home, "on the list of sections");
    }

    #[test]
    fn a_tap_on_the_dark_part_puts_the_sheet_away() {
        let mut p = Phone::new();
        p.tap(pos2(180.0, 40.0));
        assert!(!p.app.ui.sheet_open);
    }
}
