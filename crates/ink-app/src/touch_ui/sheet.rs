//! Bottom sheets: how a touch interface asks for a choice or a short piece of text.
//!
//! A sheet comes up from the bottom edge over a dimmed screen. It is as wide as a phone's screen, or a column in the
//! middle of a tablet's, it has a handle on top, and a tap outside it puts it away. Its rows are big enough for a
//! fingertip. It takes the place of the small menus a mouse opens beside the thing it points at.

use eframe::egui::{
    Align2, Area, Color32, Context, CornerRadius, FontId, Frame, Id, Key, Margin, Order, Rect,
    RichText, ScrollArea, Sense, Shadow, Stroke, TextEdit, Ui, UiBuilder, pos2, vec2,
};

use crate::icons::{self, Icon};
use crate::platform;
use crate::theme;

/// The widest a sheet is, so on a tablet it is a column and not a slab.
const MAX_WIDTH: f32 = 560.0;
/// The room between the edge of the sheet and what is in it, across.
const SIDE: f32 = 20.0;
/// How much of the screen's height a sheet may take.
const MAX_HEIGHT_SHARE: f32 = 0.72;
/// How far a sheet slides in from.
const SLIDE: f32 = 56.0;
/// The height of a row, a fingertip and a bit.
pub const ROW: f32 = 56.0;

/// What a sheet answered.
#[derive(Debug)]
pub struct Answer<R> {
    /// What the contents returned.
    pub inner: R,
    /// A tap outside the sheet asked for it to be put away.
    pub dismissed: bool,
    /// Where the sheet is, so a press on it is not taken for a press outside the panels.
    pub rect: Rect,
}

/// The width a sheet takes on a screen `screen_width` wide.
#[must_use]
pub fn width_on(screen_width: f32) -> f32 {
    screen_width.min(MAX_WIDTH)
}

/// The most height the contents of a sheet may have on a screen `screen_height` tall.
#[must_use]
pub fn content_height_on(screen_height: f32) -> f32 {
    screen_height * MAX_HEIGHT_SHARE
}

/// Shows a sheet with `title` (if any) and `content`. Call it every frame the sheet is open, and stop when it is not.
pub fn show<R>(
    ctx: &Context,
    id: &str,
    title: Option<&str>,
    content: impl FnOnce(&mut Ui) -> R,
) -> Answer<R> {
    let screen = ctx.content_rect();
    let t = theme::pop(ctx, Id::unique(("sheet_open", id)), true);
    // The screen goes dark behind it, and a tap there puts it away.
    let scrim = Area::new(Id::unique(("sheet_scrim", id)))
        .fixed_pos(screen.min)
        .order(Order::Tooltip)
        .interactable(true)
        .show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha((150.0 * t) as u8));
            response
        });
    let width = width_on(screen.width());
    // The on-screen keyboard is drawn over the app, so a sheet with a field in it rises above it, easing as the keyboard
    // slides in and away.
    let lift = ctx.animate_value_with_time(
        Id::unique(("sheet_lift", id)),
        platform::keyboard_lift(ctx),
        0.18,
    );
    let sheet = Area::new(Id::unique(("sheet", id)))
        .anchor(Align2::CENTER_BOTTOM, vec2(0.0, (1.0 - t) * SLIDE - lift))
        .order(Order::Tooltip)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_opacity(t);
            // An area would otherwise clip its contents to the size it had last time, and the scroll area in it would
            // never grow; see `flyouts::flyout`.
            ui.set_max_height(screen.height());
            // The sheet takes every press, so none falls through to the scrim and puts it away.
            ui.scope_builder(UiBuilder::new().sense(Sense::click_and_drag()), |ui| {
                Frame::new()
                    .fill(theme::CHROME)
                    .corner_radius(CornerRadius {
                        nw: 28,
                        ne: 28,
                        sw: 0,
                        se: 0,
                    })
                    .stroke(theme::hairline())
                    .shadow(Shadow {
                        offset: [0, -8],
                        blur: 40,
                        spread: 0,
                        color: Color32::from_black_alpha(120),
                    })
                    .inner_margin(Margin {
                        left: SIDE as i8,
                        right: SIDE as i8,
                        top: 10,
                        bottom: 16,
                    })
                    .show(ui, |ui| {
                        ui.set_width(width - 2.0 * SIDE);
                        handle(ui);
                        if let Some(title) = title {
                            ui.add_space(6.0);
                            ui.label(RichText::new(title).size(19.0).strong());
                            ui.add_space(6.0);
                        }
                        ScrollArea::vertical()
                            .id_salt(("sheet_scroll", id))
                            .max_height(content_height_on((screen.height() - lift).max(0.0)))
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                content(ui)
                            })
                            .inner
                    })
                    .inner
            })
            .inner
        });
    // A touch on the scrim brings it to the front of its layer, and the order of layers is remembered: a sheet opened again
    // soon after would then sit under its own scrim, dimmed, with every touch taken by the scrim. So the sheet is put in
    // front of the scrim, every frame it is shown.
    ctx.move_to_top(scrim.response.layer_id);
    ctx.move_to_top(sheet.response.layer_id);
    ctx.request_repaint_after(std::time::Duration::from_millis(16));
    Answer {
        inner: sheet.inner,
        dismissed: scrim.inner.clicked(),
        rect: sheet.response.rect,
    }
}

/// The small bar at the top that says this can be pulled away.
fn handle(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 10.0), Sense::hover());
    ui.painter().rect_filled(
        Rect::from_center_size(pos2(rect.center().x, rect.center().y), vec2(40.0, 4.0)),
        CornerRadius::same(2),
        Color32::from_white_alpha(46),
    );
}

/// What a row of a sheet is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    /// Something that cannot be taken back as easily, in red.
    Danger,
}

/// A row of a sheet: a label across the whole width, and an icon at its right edge if there is one. Returns `true` when it was tapped.
pub fn row(ui: &mut Ui, icon: Option<Icon>, label: &str, tone: Tone) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    let painter = ui.painter();
    if response.is_pointer_button_down_on() {
        painter.rect_filled(rect, CornerRadius::same(16), Color32::from_white_alpha(18));
    }
    let color = match tone {
        Tone::Normal => theme::TEXT,
        Tone::Danger => Color32::from_rgb(0xF0, 0x7A, 0x6E),
    };
    // Every label starts at the same place; an icon, where a row has one, is at the right edge.
    painter.text(
        pos2(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(17.0),
        color,
    );
    if let Some(icon) = icon {
        icons::paint(
            painter,
            Rect::from_center_size(pos2(rect.right() - 24.0, rect.center().y), vec2(22.0, 22.0)),
            icon,
            color,
        );
    }
    response.clicked()
}

/// A rounded button a fingertip tall, for a few things done to what is above it. Returns `true` when it was tapped; a
/// button that is not `enabled` is dim and does nothing.
pub fn pill(ui: &mut Ui, label: &str, enabled: bool, tone: Tone) -> bool {
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), FontId::proportional(15.5), Color32::WHITE);
    let (rect, response) = ui.allocate_exact_size(
        vec2(galley.size().x + 36.0, 42.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let painter = ui.painter();
    let fill = if enabled && response.is_pointer_button_down_on() {
        theme::CHROME_HOVER
    } else {
        theme::CHROME_HI
    };
    painter.rect_filled(rect, CornerRadius::same(21), fill);
    let color = match (enabled, tone) {
        (false, _) => theme::TEXT_DIM.gamma_multiply(0.5),
        (true, Tone::Normal) => theme::TEXT,
        (true, Tone::Danger) => Color32::from_rgb(0xF0, 0x7A, 0x6E),
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(15.5),
        color,
    );
    enabled && response.clicked()
}

/// A wide button of a sheet, a fingertip and a half tall. `primary` fills it with the accent.
pub fn button(ui: &mut Ui, label: &str, width: f32, primary: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 52.0), Sense::click());
    let painter = ui.painter();
    let down = response.is_pointer_button_down_on();
    let fill = if primary {
        theme::ACCENT.gamma_multiply(if down { 0.82 } else { 1.0 })
    } else if down {
        theme::CHROME_HOVER
    } else {
        theme::CHROME_HI
    };
    painter.rect_filled(rect, CornerRadius::same(26), fill);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(17.0),
        if primary {
            Color32::from_rgb(0x1B, 0x1A, 0x18)
        } else {
            theme::TEXT
        },
    );
    response.clicked()
}

/// What a sheet that asks for a piece of text came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typed {
    /// Still being typed.
    Editing,
    /// The text was handed in (Enter, or the button).
    Save,
    /// It was put away: the other button, or a tap outside.
    Cancel,
}

/// A sheet with a text field in it, a button to cancel and one to `save` with. Call it every frame it is open; the text
/// is `text`, changed as it is typed. `rect` is where the sheet is, for the caller to count as part of the panels.
pub fn ask_text(
    ctx: &Context,
    id: &str,
    title: &str,
    text: &mut String,
    save_label: &str,
) -> (Typed, Rect) {
    let mut save = false;
    let mut cancel = false;
    let answer = show(ctx, id, Some(title), |ui| {
        ui.add_space(4.0);
        let field = Frame::new()
            .fill(theme::CARD)
            .corner_radius(CornerRadius::same(16))
            .stroke(edge())
            .inner_margin(Margin::symmetric(16, 4))
            .show(ui, |ui| {
                ui.add(
                    TextEdit::singleline(text)
                        .frame(Frame::NONE)
                        .font(FontId::proportional(18.0))
                        .align(Align2::LEFT_CENTER)
                        .desired_width(f32::INFINITY)
                        .min_size(vec2(0.0, 44.0)),
                )
            })
            .inner;
        // Asked first: taking the focus back would make the field look as if it had not been left. And asked only when it
        // is not there already: asking interrupts the keyboard (egui hides it and shows it again), so asking every frame
        // would have the keyboard opening and closing without end.
        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            save = true;
        } else if !field.has_focus() {
            field.request_focus();
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let half = (ui.available_width() - 10.0) / 2.0;
            if button(ui, "Cancel", half, false) {
                cancel = true;
            }
            if button(ui, save_label, half, true) {
                save = true;
            }
        });
    });
    let typed = if cancel || answer.dismissed {
        Typed::Cancel
    } else if save {
        Typed::Save
    } else {
        Typed::Editing
    };
    (typed, answer.rect)
}

/// A thin line between groups of rows.
pub fn divider(ui: &mut Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, Color32::from_white_alpha(18));
    ui.add_space(4.0);
}

/// A small label above a group of rows.
pub fn caption(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).size(12.5).color(theme::TEXT_DIM));
    ui.add_space(2.0);
}

/// A stroke of the hairline's colour, for the edges of things in a sheet.
#[must_use]
pub fn edge() -> Stroke {
    Stroke::new(1.0, Color32::from_white_alpha(22))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_gets_a_sheet_across_the_screen_and_a_tablet_a_column() {
        assert!((width_on(360.0) - 360.0).abs() < 1e-4);
        assert!((width_on(1200.0) - MAX_WIDTH).abs() < 1e-4);
    }

    #[test]
    fn a_sheet_never_takes_more_than_the_share_of_the_screen() {
        assert!(content_height_on(800.0) < 800.0);
        assert!((content_height_on(1000.0) - 720.0).abs() < 1e-3);
    }

    #[test]
    fn a_row_is_at_least_a_fingertip_tall() {
        const { assert!(ROW >= 48.0) };
    }

    /// How many frames, of `frames`, the sheet asking for a name interrupts the keyboard in.
    fn interruptions_over(frames: u32) -> u32 {
        use eframe::egui::{RawInput, pos2};
        let ctx = Context::default();
        let mut text = String::new();
        let mut interrupted = 0;
        for frame in 0..frames {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                time: Some(f64::from(frame) / 60.0),
                ..RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                ask_text(&ctx, "name", "Name", &mut text, "Save");
            });
            output.textures_delta.clear(); // nothing is rendered in tests
            if output
                .platform_output
                .ime
                .is_some_and(|ime| ime.should_interrupt_composition)
            {
                interrupted += 1;
            }
        }
        interrupted
    }

    #[test]
    fn a_sheet_asking_for_text_does_not_interrupt_the_keyboard_every_frame() {
        // Asking for the focus interrupts the keyboard (egui hides it and shows it again on Android), so a field that
        // asks every frame has the keyboard opening and closing without end.
        assert!(
            interruptions_over(40) <= 2,
            "the keyboard was interrupted {} times in 40 frames",
            interruptions_over(40)
        );
    }

    #[test]
    fn a_sheet_opened_again_after_its_scrim_was_touched_is_still_above_the_scrim() {
        use eframe::egui::{LayerId, RawInput, pos2};
        let ctx = Context::default();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let frame = |ctx: &Context, time: f64| {
            let input = RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                ..RawInput::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                show(&ctx, "layers", Some("Layers"), |ui| ui.label("rows"));
            });
            output.textures_delta.clear(); // nothing is rendered in tests
        };
        for i in 0..5 {
            frame(&ctx, f64::from(i) / 60.0);
        }
        // The scrim is touched (to put the sheet away) and comes to the front.
        let scrim = LayerId::new(Order::Tooltip, Id::unique(("sheet_scrim", "layers")));
        ctx.move_to_top(scrim);
        // The sheet is shown again.
        for i in 5..12 {
            frame(&ctx, f64::from(i) / 60.0);
        }
        let on_the_sheet = pos2(400.0, 590.0);
        let sheet = LayerId::new(Order::Tooltip, Id::unique(("sheet", "layers")));
        assert_eq!(
            ctx.layer_id_at(on_the_sheet),
            Some(sheet),
            "a touch on the sheet reaches the sheet, not the scrim"
        );
    }
}
