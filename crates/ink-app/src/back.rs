//! Going back: what the Back button of a phone or tablet does.
//!
//! It steps out of what is open, one thing at a time, innermost first: a page of the settings to their list and then to
//! the drawing, a sheet or a panel, a folder to the one above it, a drawing to the Library. From the top of the Library
//! there is nothing left to go back out of, and the system takes the press and puts the app away.
//!
//! The app says every frame whether it would take a press (see `platform`), because the system asks before the app has
//! been told.

use eframe::egui::Context;

use crate::app::InkApp;
use crate::shell::{self, Shell};
use crate::state::Mode;

impl InkApp {
    /// `true` when Back would leave the app: the top of the Library, with nothing open over it.
    fn at_exit_point(&self) -> bool {
        self.ui.library_open
            && self.ui.settings.is_none()
            && self.library.at_root()
            && !self.library.is_searching()
            && self.library.menu.is_none()
            && self.library.move_picker.is_none()
            && self.library.renaming.is_none()
    }

    /// `true` when Back would do something in the app, and not leave it.
    pub(crate) fn can_go_back(&self) -> bool {
        !self.at_exit_point()
    }

    /// Steps out of the innermost thing that is open. Does nothing at the top of the Library.
    pub(crate) fn go_back(&mut self, ctx: &Context) {
        self.go_back_on(ctx, shell::current());
    }

    /// `go_back` for the interface `shell`, which decides what the settings are: pages with a list, or one window.
    fn go_back_on(&mut self, ctx: &Context, shell: Shell) {
        if self.ui.settings.is_some() {
            // A page of the settings goes back to their list; the list closes them.
            if shell == Shell::Touch && !self.ui.settings_home {
                self.ui.settings_home = true;
            } else {
                self.ui.settings = None;
            }
            return;
        }
        if self.ui.library_open {
            self.go_back_in_library();
            return;
        }
        if self.ui.text.is_some() {
            self.cancel_text();
            return;
        }
        if self.ui.any_popover() || self.ui.layers_open {
            self.ui.close_popovers();
            self.ui.layers_open = false;
            return;
        }
        if self.ui.renaming.is_some()
            || self.gesture.is_some()
            || !self.selection.is_empty()
            || matches!(self.tool.mode, Mode::Select | Mode::Slice | Mode::Nudge)
        {
            self.cancel();
            return;
        }
        self.open_library(ctx);
    }

    fn go_back_in_library(&mut self) {
        let library = &mut self.library;
        if library.renaming.take().is_some()
            || library.menu.take().is_some()
            || library.move_picker.take().is_some()
        {
            return;
        }
        if library.is_searching() {
            library.query.clear();
        } else if !library.at_root()
            && let Some(parent) = library.current.parent().map(std::path::Path::to_path_buf)
        {
            library.go_to(parent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;
    use crate::library::ItemMenu;
    use crate::settings::Section;

    fn app_in_library() -> (InkApp, Context) {
        let mut app = InkApp::blank(Options::default());
        app.ui.library_open = true;
        (app, Context::default())
    }

    #[test]
    fn at_the_top_of_the_library_back_is_left_to_the_system() {
        let (app, _) = app_in_library();
        assert!(!app.can_go_back(), "nothing is left to go back out of");
    }

    #[test]
    fn anything_open_over_the_library_makes_back_close_it_first() {
        let (mut app, ctx) = app_in_library();
        app.library.menu = Some(ItemMenu {
            path: "a.ink".into(),
            at: [0.0, 0.0],
        });
        assert!(app.can_go_back());
        app.go_back(&ctx);
        assert!(app.library.menu.is_none(), "the menu is put away");
        assert!(
            !app.can_go_back(),
            "and then the Library is at its top again"
        );
        app.library.query = "plan".to_owned();
        assert!(app.can_go_back());
        app.go_back(&ctx);
        assert!(app.library.query.is_empty(), "the search is cleared");
    }

    #[test]
    fn a_settings_page_goes_back_to_the_list_and_the_list_closes_the_settings() {
        let (mut app, ctx) = app_in_library();
        app.open_settings(Section::Interface);
        app.go_back_on(&ctx, Shell::Touch);
        assert!(app.ui.settings_home, "the first Back is to the list");
        app.go_back_on(&ctx, Shell::Touch);
        assert!(app.ui.settings.is_none(), "the second closes the settings");
    }

    #[test]
    fn on_the_desktop_the_settings_are_one_window_and_back_closes_it_at_once() {
        let (mut app, ctx) = app_in_library();
        app.open_settings(Section::Interface);
        app.go_back_on(&ctx, Shell::Desktop);
        assert!(app.ui.settings.is_none());
    }

    #[test]
    fn in_a_drawing_back_puts_away_a_sheet_then_leaves_for_the_library() {
        let mut app = InkApp::blank(Options::default());
        let ctx = Context::default();
        app.new_tab(&ctx);
        app.ui.layers_open = true;
        app.ui.precision_open = false;
        assert!(app.can_go_back());
        app.go_back(&ctx);
        assert!(!app.ui.layers_open, "the layers are put away first");
        assert!(!app.ui.library_open, "still in the drawing");
        app.go_back(&ctx);
        assert!(
            app.ui.library_open,
            "then the drawing is left for the Library"
        );
        assert!(!app.can_go_back(), "and from there Back leaves the app");
    }

    #[test]
    fn a_tool_that_is_not_drawing_goes_back_to_drawing_before_the_drawing_is_left() {
        let mut app = InkApp::blank(Options::default());
        let ctx = Context::default();
        app.new_tab(&ctx);
        app.set_mode(Mode::Select);
        app.go_back(&ctx);
        assert_eq!(app.tool.mode, Mode::Draw);
        assert!(!app.ui.library_open);
    }
}
