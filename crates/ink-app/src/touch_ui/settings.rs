//! The settings on a device used by touch.
//!
//! A touch screen is looked at one thing at a time, so the settings are a list of the sections and, when one is picked,
//! a page of its own with a way back to the list. On a phone the page is the whole screen; on a tablet it is a card in
//! the middle of it. The sections beside their content, as the desktop has them, are for a mouse. The keys of a keyboard
//! are not listed: there are none to choose.

use eframe::egui::{
    Align, Align2, Color32, Context, CornerRadius, FontId, Frame, Id, Layout, Margin, Modal, Rect,
    RichText, ScrollArea, Sense, Ui, pos2, vec2,
};

use crate::app::InkApp;
use crate::icons::{self, Icon};
use crate::settings::{self, Section};
use crate::theme;
use crate::widgets;

/// A screen at least this wide shows the settings as a card in the middle of it, not across all of it.
const WIDE: f32 = 640.0;
/// The widest the card is, and how far it keeps from the edges of the screen.
const CARD_WIDTH: f32 = 720.0;
const CARD_MARGIN: f32 = 24.0;
/// The padding of the page, across and down.
const PAD: (f32, f32) = (14.0, 10.0);
const HEADER_BUTTON: f32 = 44.0;
const LIST_ROW: f32 = 56.0;
/// How tall the card is while it lists the sections: the header, a row for each and room under them.
const LIST_CARD_HEIGHT: f32 = 80.0 + SECTIONS.len() as f32 * (LIST_ROW + 8.0) + 24.0;

/// The sections a phone lists: all but the keys, which it has none of to choose.
const SECTIONS: [Section; 10] = [
    Section::Brushes,
    Section::Tools,
    Section::Workspace,
    Section::Input,
    Section::Precision,
    Section::Canvas,
    Section::Interface,
    Section::Rendering,
    Section::Files,
    Section::About,
];

/// How big the page is on a screen of `screen`, and how round its corners are: all of a phone's screen, or a card on a
/// tablet's. The card is only as tall as the list of sections while that is showing.
#[must_use]
fn page_on(screen: eframe::egui::Vec2, list: bool) -> (eframe::egui::Vec2, u8) {
    if screen.x >= WIDE {
        let tall = screen.y - 2.0 * CARD_MARGIN;
        let height = if list {
            tall.min(LIST_CARD_HEIGHT)
        } else {
            tall
        };
        (
            vec2((screen.x - 2.0 * CARD_MARGIN).min(CARD_WIDTH), height),
            28,
        )
    } else {
        (screen, 0)
    }
}

/// Shows the settings while they are open.
pub fn show(ctx: &Context, app: &mut InkApp) {
    let Some(mut section) = app.ui.settings else {
        return;
    };
    let screen = ctx.content_rect().size();
    let mut home = app.ui.settings_home;
    let (page, radius) = page_on(screen, home);
    let before = app.config.clone();

    let response = Modal::new(Id::unique("settings_page"))
        .backdrop_color(Color32::from_black_alpha(130))
        .frame(
            Frame::new()
                .fill(theme::CHROME)
                .corner_radius(CornerRadius::same(radius))
                .inner_margin(Margin::symmetric(PAD.0 as i8, PAD.1 as i8)),
        )
        .show(ctx, |ui| {
            ui.set_width(page.x - 2.0 * PAD.0);
            ui.set_height(page.y - 2.0 * PAD.1);
            let close = header(ui, &mut home);
            ui.add_space(6.0);
            if home {
                list(ui, &mut section, &mut home);
            } else {
                ScrollArea::vertical()
                    .id_salt("settings_page_body")
                    .auto_shrink([false, false])
                    .show(ui, |ui| settings::content(ui, app, section));
            }
            close
        });

    // The cross, Escape, and a tap on the dark part outside the card all put the settings away.
    app.ui.settings = if response.inner || response.should_close() {
        None
    } else {
        Some(section)
    };
    app.ui.settings_home = home;
    if app.config != before {
        app.mark_config_changed();
    }
}

/// The title with a way back to the list on a page, and the way out. Returns `true` when it was closed.
fn header(ui: &mut Ui, home: &mut bool) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if !*home && widgets::round_button(ui, HEADER_BUTTON, Icon::Left, true, false).clicked() {
            *home = true;
        }
        // A page names itself in its own content, so the header is the way back to the list it came from.
        ui.label(RichText::new("Settings").strong().size(22.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = widgets::round_button(ui, HEADER_BUTTON, Icon::Close, true, false).clicked();
        });
    });
    close
}

/// A big row for each section; picking one opens its page.
fn list(ui: &mut Ui, section: &mut Section, home: &mut bool) {
    ScrollArea::vertical()
        .id_salt("settings_list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for s in SECTIONS {
                if list_row(ui, s.label()) {
                    *section = s;
                    *home = false;
                }
            }
        });
}

/// One row of the list. Returns `true` when it was tapped.
fn list_row(ui: &mut Ui, label: &str) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), LIST_ROW), Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        theme::CHROME_HOVER
    } else {
        theme::CARD
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(14), fill);
    painter.text(
        pos2(rect.left() + 18.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(17.0),
        theme::TEXT,
    );
    icons::paint(
        painter,
        Rect::from_center_size(pos2(rect.right() - 26.0, rect.center().y), vec2(14.0, 14.0)),
        Icon::Right,
        theme::TEXT_DIM,
    );
    response.clicked()
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, RawInput};

    use super::*;
    use crate::Options;

    /// A phone, a frame at a time, showing only the settings.
    struct Phone {
        ctx: Context,
        app: InkApp,
        time: f64,
    }

    impl Phone {
        fn new() -> Self {
            Self {
                ctx: Context::default(),
                app: InkApp::blank(Options::default()),
                time: 0.0,
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(360.0, 780.0))),
                events,
                time: Some(self.time),
                ..RawInput::default()
            };
            let app = &mut self.app;
            let mut output = self.ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                show(&ctx, app);
            });
            output.textures_delta.clear(); // nothing is rendered in tests
        }

        fn settle(&mut self) {
            for _ in 0..3 {
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
    }

    /// The back arrow and the cross of the header, and the second row of the list, on a 360 by 780 screen.
    const BACK: Pos2 = pos2(36.0, 32.0);
    const CROSS: Pos2 = pos2(324.0, 32.0);
    const SECOND_ROW: Pos2 = pos2(180.0, 160.0);

    #[test]
    fn the_settings_open_on_a_list_and_a_row_opens_its_page() {
        let mut p = Phone::new();
        p.app.open_settings_home();
        p.settle();
        assert!(p.app.ui.settings_home, "the list is showing");
        p.tap(SECOND_ROW);
        assert_eq!(p.app.ui.settings, Some(Section::Tools));
        assert!(!p.app.ui.settings_home, "its page is showing");
    }

    #[test]
    fn a_page_goes_back_to_the_list_and_the_cross_closes_the_settings() {
        let mut p = Phone::new();
        p.app.open_settings(Section::Interface);
        p.settle();
        assert!(
            !p.app.ui.settings_home,
            "asked for a page, it opens on the page"
        );
        p.tap(BACK);
        assert!(p.app.ui.settings_home, "back is the list");
        assert_eq!(p.app.ui.settings, Some(Section::Interface));
        p.tap(CROSS);
        assert_eq!(p.app.ui.settings, None, "the cross closes them");
    }

    #[test]
    fn a_phone_gets_the_whole_screen_and_a_tablet_a_card_inside_it() {
        let (phone, radius) = page_on(vec2(360.0, 780.0), true);
        assert_eq!(phone, vec2(360.0, 780.0));
        assert_eq!(radius, 0, "a page that is the screen has no corners");
        let tablet = vec2(923.0, 1305.0);
        let (card, radius) = page_on(tablet, false);
        assert!(card.x <= CARD_WIDTH && card.x < tablet.x && card.y < tablet.y);
        assert!(radius > 0);
    }

    #[test]
    fn the_list_has_every_section_but_the_keys() {
        let left_out: Vec<Section> = Section::ALL
            .into_iter()
            .filter(|s| !SECTIONS.contains(s))
            .collect();
        assert_eq!(left_out, [Section::Shortcuts]);
    }
}
