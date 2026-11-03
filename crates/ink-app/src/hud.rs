//! What floats over the paper besides the rail: the note chip and its menu, the layers and settings buttons, the
//! zoom level and the messages that confirm what just happened.

use eframe::egui::{
    self, Align2, Area, Button, Context, DragValue, Id, Label, Layout, Margin, Order, RichText,
    ScrollArea, Sense, TextWrapMode, Ui, vec2,
};

use crate::app::InkApp;
use crate::canvas;
use crate::config::Pattern;
use crate::files::MESSAGE_SECONDS;
use crate::paper;
use crate::settings::Section;
use crate::theme;
use crate::widgets;

/// Something chosen in the note menu.
enum Action {
    Library,
    New,
    Open,
    Save,
    SaveAs,
    Settings(Section),
    Pattern(Pattern),
}

pub fn show(ctx: &Context, app: &mut InkApp) {
    if !app.ui.library_open {
        zoom_chip(ctx, app);
        turn_chip(ctx, app);
        turn_panel(ctx, app);
    }
    note_menu(ctx, app);
    toast(ctx, app);
}

/// How the zoom level reads in the chip: a whole percentage, `100%` at the size the drawing was made. The most
/// it can be is `6400%`, and a value that is not a number reads as `100%` rather than as garbage.
#[must_use]
fn zoom_label(zoom: f32) -> String {
    let percent = if zoom.is_finite() {
        zoom * 100.0
    } else {
        100.0
    };
    format!("{:.0}%", percent.clamp(0.0, 99_999.0))
}

/// The room the chip keeps for its number, in points: enough for `6400%`, so the chip stays the same size
/// while the zoom changes and does not jump about under the pinch.
const ZOOM_CHIP_TEXT_WIDTH: f32 = 46.0;
/// The chip is as tall as its corners are round, so it is a full pill.
const ZOOM_CHIP_HEIGHT: f32 = 34.0;
/// How wide a chip is: its text and a margin on each side.
const CHIP_WIDTH: f32 = ZOOM_CHIP_TEXT_WIDTH + 28.0;
/// How far a chip is from the corner of the window, and from the chip beside it.
const CHIP_GAP: f32 = 16.0;
const CHIP_SPACING: f32 = 12.0;

/// Where the chip that is `beside` chips in from the corner has its top left. The chips have a size that is known,
/// so they are placed from it and not from what an area measured last frame: that is wrong for the first frame
/// after a chip appears, and a chip that is anywhere else for a frame is a flicker.
fn chip_place(ctx: &Context, beside: u8) -> egui::Pos2 {
    let corner = ctx.content_rect().right_bottom();
    egui::pos2(
        corner.x - CHIP_GAP - CHIP_WIDTH - f32::from(beside) * (CHIP_WIDTH + CHIP_SPACING),
        corner.y - CHIP_GAP - ZOOM_CHIP_HEIGHT,
    )
}

/// The place a chip takes up, which is what keeps the pen from drawing under it. It is worked out, not measured:
/// what an area reports for the first frame it is on the screen is not where it will be.
fn chip_rect(ctx: &Context, beside: u8) -> egui::Rect {
    egui::Rect::from_min_size(chip_place(ctx, beside), vec2(CHIP_WIDTH, ZOOM_CHIP_HEIGHT))
}

/// The zoom level, bottom right. Tapping it fits the drawing to the window.
fn zoom_chip(ctx: &Context, app: &mut InkApp) {
    let alpha = app.chrome_alpha * 0.9;
    let label = zoom_label(app.view.zoom);
    let area = Area::new(Id::unique("zoom_chip"))
        .fixed_pos(chip_place(ctx, 0))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            let chip = theme::floating(
                ui,
                [ZOOM_CHIP_HEIGHT / 2.0; 4],
                Margin::symmetric(14, 0),
                |ui| {
                    // A box of its own, so the centering fills this and not the whole window.
                    ui.allocate_ui_with_layout(
                        vec2(ZOOM_CHIP_TEXT_WIDTH, ZOOM_CHIP_HEIGHT),
                        Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            ui.add(
                                Label::new(
                                    RichText::new(label)
                                        .size(13.0)
                                        .monospace()
                                        .color(theme::TEXT_DIM),
                                )
                                .wrap_mode(TextWrapMode::Extend),
                            );
                        },
                    );
                },
            );
            ui.interact(
                chip.response.rect,
                Id::unique("zoom_chip_hit"),
                Sense::click(),
            )
            .on_hover_text("Fit the drawing to the window")
        });
    app.chrome.push(chip_rect(ctx, 0));
    if area.inner.clicked() {
        app.fit_pending = true;
    }
}

/// How far the paper is turned, beside the zoom chip, while it is turned at all (or its panel is open). Tapping
/// it opens the panel where the angle is typed.
fn turn_chip(ctx: &Context, app: &mut InkApp) {
    let degrees = app.view.degrees();
    if degrees.abs() < 0.05 && !app.ui.turn_open {
        return;
    }
    let alpha = app.chrome_alpha * 0.9;
    let area = Area::new(Id::unique("turn_chip"))
        .fixed_pos(chip_place(ctx, 1))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            let chip = theme::floating(
                ui,
                [ZOOM_CHIP_HEIGHT / 2.0; 4],
                Margin::symmetric(14, 0),
                |ui| {
                    ui.allocate_ui_with_layout(
                        vec2(ZOOM_CHIP_TEXT_WIDTH, ZOOM_CHIP_HEIGHT),
                        Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            ui.add(
                                Label::new(
                                    RichText::new(format!("{degrees:.0}\u{b0}"))
                                        .size(13.0)
                                        .monospace()
                                        .color(theme::ACCENT),
                                )
                                .wrap_mode(TextWrapMode::Extend),
                            );
                        },
                    );
                },
            );
            ui.interact(
                chip.response.rect,
                Id::unique("turn_chip_hit"),
                Sense::click(),
            )
            .on_hover_text("Turn the paper to an angle")
        });
    app.chrome.push(chip_rect(ctx, 1));
    if area.inner.clicked() {
        app.ui.toggle_turn();
    }
}

/// The inside width of the turn panel.
const TURN_PANEL_WIDTH: f32 = 196.0;

/// The panel over the turn chip: the angle of the paper as a number to drag or type, and a button that turns it
/// upright. The paper turns about the middle of the window as the number changes.
fn turn_panel(ctx: &Context, app: &mut InkApp) {
    let t = theme::pop(ctx, Id::unique("turn_panel_pop"), app.ui.turn_open);
    if t <= 0.001 {
        return;
    }
    let alpha = t * app.chrome_alpha;
    let chip = chip_rect(ctx, 1);
    let before = app.view.degrees();
    let mut degrees = before;
    let mut upright = false;
    let area = Area::new(Id::unique("turn_panel"))
        .pivot(Align2::RIGHT_BOTTOM)
        .fixed_pos(egui::pos2(chip.right(), chip.top() - 8.0 + (1.0 - t) * 6.0))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            theme::floating(ui, [20.0; 4], Margin::same(12), |ui| {
                ui.set_width(TURN_PANEL_WIDTH);
                ui.spacing_mut().item_spacing.y = 8.0;
                widgets::heading(ui, "TURN THE PAPER");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.add_sized(
                        vec2(88.0, 30.0),
                        DragValue::new(&mut degrees)
                            .range(-180.0..=180.0)
                            .speed(0.5)
                            .max_decimals(1)
                            .suffix("\u{b0}"),
                    )
                    .on_hover_text(
                        "Drag, or click and type an angle: counterclockwise is positive",
                    );
                    upright = ui
                        .add(
                            Button::new(RichText::new("Upright").size(14.0))
                                .min_size(vec2(88.0, 30.0)),
                        )
                        .on_hover_text("Turn the paper upright")
                        .clicked();
                });
            })
        });
    app.chrome.push(area.inner.response.rect);
    let by = if upright {
        -app.view.angle
    } else if (degrees - before).abs() > 0.04 {
        (degrees - before).to_radians()
    } else {
        0.0
    };
    if by != 0.0 {
        let center = app
            .canvas_rect
            .map_or_else(|| ctx.content_rect().center(), |r| r.center());
        app.view.rotate_at(center, by);
        canvas::note_turning(ctx);
    }
}

/// The menu under the note chip: files, the paper, settings.
fn note_menu(ctx: &Context, app: &mut InkApp) {
    let t = theme::pop(ctx, Id::unique("note_menu_pop"), app.ui.sheet_open);
    if t <= 0.001 {
        return;
    }
    let alpha = t * app.chrome_alpha;
    // The menu hangs 58 points from the top and keeps 16 from the bottom; its own margins take 20 more.
    let room = (ctx.content_rect().height() - 58.0 - 16.0 - 20.0).max(120.0);
    let area = Area::new(Id::unique("note_menu"))
        .anchor(Align2::RIGHT_TOP, vec2(-16.0, 58.0 - (1.0 - t) * 10.0))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            // See `flyouts::flyout`: the area would otherwise clip the scroll area to the size it had last time.
            ui.set_max_height(ctx.content_rect().height());
            theme::floating(ui, [24.0; 4], Margin::same(10), |ui| {
                ui.set_width(292.0);
                // Never taller than the window: on a small one the rest of the menu scrolls.
                ScrollArea::vertical()
                    .max_height(room)
                    .auto_shrink([false, true])
                    .show(ui, |ui| menu_items(ui, app))
                    .inner
            })
        });
    app.chrome.push(area.inner.response.rect);
    if let Some(action) = area.inner.inner {
        apply(ctx, app, &action);
    }
}

fn menu_items(ui: &mut Ui, app: &InkApp) -> Option<Action> {
    let mut action = None;
    let mut item = |ui: &mut Ui, label: &str, keys: &str, to: Action| {
        if widgets::menu_row(ui, label, keys).clicked() {
            action = Some(to);
        }
    };
    ui.spacing_mut().item_spacing.y = 2.0;
    item(ui, "Library", "Ctrl+L", Action::Library);
    item(ui, "New drawing", "Ctrl+T", Action::New);
    item(ui, "Open file", "Ctrl+O", Action::Open);
    item(ui, "Save", "Ctrl+S", Action::Save);
    item(ui, "Save as", "Ctrl+Shift+S", Action::SaveAs);
    ui.add_space(6.0);
    widgets::divider(ui);
    ui.add_space(6.0);
    item(ui, "Settings", "Ctrl+,", Action::Settings(Section::Brushes));
    item(
        ui,
        "Keyboard shortcuts",
        "",
        Action::Settings(Section::Shortcuts),
    );
    item(ui, "About", "", Action::Settings(Section::About));
    // The paper comes last: on a small window it is the part that scrolls, and nothing needed is below it.
    ui.add_space(6.0);
    widgets::divider(ui);
    ui.add_space(8.0);
    ui.label(RichText::new("PAPER").size(11.5).color(theme::TEXT_DIM));
    ui.add_space(2.0);
    let picked = paper_tiles(ui, app);
    picked.map(Action::Pattern).or(action)
}

/// A small sheet of paper for each pattern, wrapping onto more rows as needed so none runs off the menu. Returns
/// the one tapped.
fn paper_tiles(ui: &mut Ui, app: &InkApp) -> Option<Pattern> {
    let sheet = canvas::parse_hex(&app.editor.document().metadata.background_solid_color);
    let ink = paper::ink_for(sheet);
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        for pattern in Pattern::ALL {
            let selected = app.config.canvas.pattern == pattern;
            if widgets::pattern_tile(ui, pattern, selected, sheet, ink).clicked() {
                picked = Some(pattern);
            }
        }
    });
    picked
}

fn apply(ctx: &Context, app: &mut InkApp, action: &Action) {
    match *action {
        Action::Pattern(pattern) => {
            app.config.canvas.pattern = pattern;
            app.mark_config_changed();
            return;
        }
        Action::Library => app.open_library(ctx),
        Action::New => app.new_tab(ctx),
        Action::Open => app.request_open(ctx),
        Action::Save => app.save(ctx),
        Action::SaveAs => app.save_as(ctx),
        Action::Settings(section) => app.open_settings(section),
    }
    app.ui.sheet_open = false;
}

/// A short message under the top edge that confirms what just happened, and fades away by itself.
fn toast(ctx: &Context, app: &InkApp) {
    let now = ctx.input(|i| i.time);
    let Some(text) = app.current_message(now) else {
        return;
    };
    let age = app.message.as_ref().map_or(0.0, |(_, shown)| now - shown);
    let fade = (age / 0.2)
        .min((MESSAGE_SECONDS - age) / 0.5)
        .clamp(0.0, 1.0) as f32;
    Area::new(Id::unique("toast"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, 62.0 + (1.0 - fade) * -8.0))
        .order(Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_opacity(fade);
            theme::floating(ui, [20.0; 4], Margin::symmetric(18, 10), |ui| {
                ui.label(RichText::new(text).size(14.0));
            });
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(60));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zoom_reads_as_a_whole_percentage_and_never_as_garbage() {
        assert_eq!(zoom_label(1.0), "100%");
        assert_eq!(zoom_label(1.74), "174%");
        assert_eq!(zoom_label(0.02), "2%");
        assert_eq!(zoom_label(64.0), "6400%");
        assert_eq!(zoom_label(f32::NAN), "100%");
        assert_eq!(zoom_label(f32::INFINITY), "100%");
        assert_eq!(zoom_label(-3.0), "0%");
        assert_eq!(zoom_label(1.0e9), "99999%");
    }
}
