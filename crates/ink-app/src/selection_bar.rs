//! The bar that floats at the top of the paper while strokes are selected: how many, and what to do with them.
//!
//! Every button runs the same command as its key (see `commands.rs`), so a tablet with no keyboard can do all
//! of it. The second row is for what ties strokes together and keeps them apart (groups, locks, layers), and
//! opens the numbers of the selection.

use eframe::egui::{
    Align, Align2, Area, Button, Context, DragValue, Id, Layout, Margin, Order, RichText, Stroke,
    Ui, vec2,
};
use ink_core::units::Measurement;

use crate::app::InkApp;
use crate::icons::Icon;
use crate::shortcuts::Command;
use crate::size_fields::{self, Field, Session};
use crate::state::Mode;
use crate::tab_strip::BAR_HEIGHT;
use crate::theme;
use crate::tools;
use crate::widgets;

/// Space between the tab bar and the selection bar.
const GAP: f32 = 14.0;

/// The width of the words at the left of the bar. Fixed, so the bar does not breathe as a drag changes the size
/// it reports.
const LABEL_WIDTH: f32 = 170.0;

/// The same, above the buttons in a narrow window: as wide as the first row of them.
const NARROW_LABEL_WIDTH: f32 = 342.0;

/// Windows narrower than this (in points) get the narrow bar.
const WIDE: f32 = 900.0;

/// The size of a button.
const BUTTON: f32 = 34.0;

type Buttons = &'static [(Icon, Command, &'static str)];

/// The buttons of the first row in the order they appear; a gap separates the groups.
const GROUPS: [Buttons; 3] = [
    &[
        (Icon::Left, Command::MoveLeft, "Move left"),
        (Icon::Up, Command::MoveUp, "Move up"),
        (Icon::Down, Command::MoveDown, "Move down"),
        (Icon::Right, Command::MoveRight, "Move right"),
    ],
    &[
        (
            Icon::FlipHorizontal,
            Command::FlipHorizontal,
            "Flip left to right",
        ),
        (
            Icon::FlipVertical,
            Command::FlipVertical,
            "Flip top to bottom",
        ),
    ],
    &[
        (Icon::Duplicate, Command::Duplicate, "Duplicate"),
        (Icon::Trash, Command::Delete, "Delete"),
    ],
];

/// The buttons of the second row.
const TIES: [Buttons; 3] = [
    &[
        (Icon::Group, Command::GroupSelection, "Group"),
        (Icon::Ungroup, Command::UngroupSelection, "Ungroup"),
    ],
    &[
        (Icon::Lock, Command::LockSelection, "Lock"),
        (Icon::Unlock, Command::UnlockAll, "Unlock everything"),
    ],
    &[
        (
            Icon::LayerDown,
            Command::SelectionLayerDown,
            "Move to the layer below",
        ),
        (
            Icon::LayerUp,
            Command::SelectionLayerUp,
            "Move to the layer above",
        ),
    ],
];

/// What the bar says about the selection: how many strokes, and (when asked for) how wide and high it is, in the
/// unit the settings choose.
#[must_use]
fn selection_label(count: usize, size: Option<(f32, f32)>, measurement: &Measurement) -> String {
    let noun = if count == 1 { "stroke" } else { "strokes" };
    match size {
        Some((width, height)) => format!(
            "{count} {noun} · {} × {}",
            measurement.length(width),
            measurement.length(height)
        ),
        None => format!("{count} {noun}"),
    }
}

/// `true` if `command` has something to do with the selection as it is. A button for one that has not is dimmed.
fn can_run(app: &InkApp, command: Command) -> bool {
    let marks = app.editor.marks();
    match command {
        Command::GroupSelection => app.selection.len() >= 2,
        Command::UngroupSelection => app.selection.iter().any(|id| marks.group_of(*id).is_some()),
        Command::UnlockAll => !marks.locked().is_empty(),
        _ => true,
    }
}

/// A row of buttons, with a gap between the groups. Sets `run` to the one that was pressed.
fn button_row(
    ui: &mut Ui,
    app: &InkApp,
    groups: &[Buttons],
    run: &mut Option<(Command, eframe::egui::Modifiers)>,
) {
    for (n, group) in groups.iter().enumerate() {
        if n > 0 {
            ui.add_space(8.0);
        }
        for (icon, command, text) in *group {
            let enabled = can_run(app, *command);
            let tooltip = app.keymap.hint(*command, text);
            let pressed = widgets::round_button(ui, BUTTON, *icon, enabled, false)
                .on_hover_text(tooltip)
                .clicked();
            if pressed && enabled {
                *run = Some((*command, ui.input(|i| i.modifiers)));
            }
        }
    }
}

/// The words at the left of a row, in a place of fixed `width`.
fn label_slot(ui: &mut Ui, text: impl Into<String>, width: f32) {
    ui.allocate_ui_with_layout(
        vec2(width, BUTTON * 0.7),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add(
                eframe::egui::Label::new(RichText::new(text).size(14.0).color(theme::TEXT_DIM))
                    .truncate(),
            );
        },
    );
}

/// A thin line across the bar as wide as what is in it so far. (`widgets::divider` takes all the width there is,
/// which would stretch the bar to the width an area may have.)
fn rule(ui: &mut Ui) {
    let width = ui.min_rect().width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 1.0), eframe::egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, eframe::egui::Color32::from_white_alpha(18));
}

/// One change to the selection made with the numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    /// A field now says this, in canvas units.
    Field(Field, f32),
    /// Turn this many more degrees.
    Turn(f32),
}

/// Puts the selected strokes through the change, as part of the edit the fields are in the middle of: all of it
/// is one step of history, however many frames a drag took. Returns `false` if nothing changed.
pub fn change(app: &mut InkApp, change: Change) -> bool {
    let Some(bounds) = tools::selection_bounds(&app.editor, &app.selection) else {
        return false;
    };
    let revision = app.editor.revision();
    let session = app
        .ui
        .size_fields
        .session
        .filter(|s| s.revision == revision);
    let pivot = session.map_or_else(|| size_fields::middle(bounds), |s| s.pivot);
    let m = match change {
        Change::Field(field, to) => {
            size_fields::fit(bounds, field, to, app.ui.size_fields.keep_shape)
        }
        Change::Turn(by) => Some(size_fields::turn(pivot, by)),
    };
    let Some(m) = m else {
        return false;
    };
    let mark = session.map_or_else(|| app.editor.history_mark(), |s| s.mark);
    if !tools::transform_selection(&mut app.editor, &app.selection, &m) {
        return false;
    }
    app.editor.merge_history_since(mark);
    app.ui.size_fields.session = Some(Session {
        mark,
        revision: app.editor.revision(),
        pivot,
    });
    true
}

/// A number box for a length, in the unit the settings choose. Returns the new length in canvas units if it was
/// changed, and whether the edit is over (the box was let go of or its number typed in).
fn length_box(
    ui: &mut Ui,
    label: &str,
    canvas: f32,
    measurement: &Measurement,
) -> (Option<f32>, bool) {
    let unit = measurement.unit;
    let per_unit = measurement.value(1.0, unit).max(1e-6);
    let before = canvas * per_unit;
    let mut shown = before;
    ui.label(RichText::new(label).size(13.0).color(theme::TEXT_DIM));
    let response = ui.add(
        DragValue::new(&mut shown)
            .speed(f64::from(per_unit))
            .max_decimals(3)
            .suffix(format!(" {}", unit.abbreviation())),
    );
    let done = response.drag_stopped() || response.lost_focus();
    // The box only shows three decimals: leaving it again must not nudge the selection by what they hide.
    let moved = response.changed() && (shown - before).abs() > 5e-4;
    (moved.then(|| shown / per_unit), done)
}

/// The numbers of the selection: where it is, how large, and how far to turn it.
fn size_panel(ui: &mut Ui, app: &mut InkApp) {
    let Some(bounds) = tools::current_bounds(&app.editor, &app.selection, app.gesture.as_ref())
    else {
        return;
    };
    let measurement = app.config.measurement;
    let mut changes: Vec<Change> = Vec::new();
    let mut done = false;
    for pair in Field::ALL.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for field in pair {
                let (to, over) = length_box(
                    ui,
                    field.label(),
                    size_fields::value(bounds, *field),
                    &measurement,
                );
                done |= over;
                changes.extend(to.map(|to| Change::Field(*field, to)));
                ui.add_space(10.0);
            }
        });
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(RichText::new("Turn by").size(13.0).color(theme::TEXT_DIM));
        let mut turned = app.ui.size_fields.turned;
        let response = ui.add(
            DragValue::new(&mut turned)
                .speed(0.5)
                .max_decimals(1)
                .suffix("\u{b0}"),
        );
        done |= response.drag_stopped() || response.lost_focus();
        if response.changed() {
            let by = turned - app.ui.size_fields.turned;
            app.ui.size_fields.turned = turned;
            if by.abs() > f32::EPSILON {
                changes.push(Change::Turn(by));
            }
        }
        ui.add_space(10.0);
        let on = app.ui.size_fields.keep_shape;
        let button = Button::new(RichText::new("Keep shape").size(13.0).color(if on {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        }))
        .fill(if on {
            theme::ACCENT.gamma_multiply(0.45)
        } else {
            theme::CHROME_HI
        })
        .stroke(if on {
            Stroke::new(1.0, theme::ACCENT)
        } else {
            Stroke::NONE
        })
        .min_size(vec2(96.0, 28.0));
        if ui
            .add(button)
            .on_hover_text("Stretching one side stretches the other as much")
            .clicked()
        {
            app.ui.size_fields.keep_shape = !on;
        }
    });
    for c in changes {
        change(app, c);
    }
    if done {
        app.ui.size_fields.finish();
    }
}

pub fn show(ctx: &Context, app: &mut InkApp) {
    if app.tool.mode != Mode::Select {
        app.ui.size_fields.open = false;
        return;
    }
    if app.selection.is_empty() {
        app.ui.size_fields.open = false;
        app.ui.size_fields.finish();
        show_locked(ctx, app);
        return;
    }
    let count = app.selection.len();
    let size = app
        .config
        .workspace
        .show_selection_size
        .then(|| tools::current_bounds(&app.editor, &app.selection, app.gesture.as_ref()))
        .flatten()
        .map(|b| (b[2] - b[0], b[3] - b[1]));
    let label = selection_label(count, size, &app.config.measurement);
    let alpha = app.chrome_alpha;
    let narrow = ctx.content_rect().width() < WIDE;
    let mut run = None;
    let area = Area::new(Id::unique("selection_bar"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, BAR_HEIGHT + GAP))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            theme::floating(ui, [22.0; 4], Margin::symmetric(14, 8), |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                // In a narrow window the words go on a row of their own, so the bar is no wider than its buttons
                // and stays clear of the rail.
                if narrow {
                    label_slot(ui, label, NARROW_LABEL_WIDTH);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        button_row(ui, app, &GROUPS, &mut run);
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        label_slot(ui, label, LABEL_WIDTH);
                        ui.add_space(8.0);
                        button_row(ui, app, &GROUPS, &mut run);
                    });
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if !narrow {
                        label_slot(ui, "", LABEL_WIDTH);
                        ui.add_space(8.0);
                    }
                    button_row(ui, app, &TIES, &mut run);
                    ui.add_space(8.0);
                    let open = app.ui.size_fields.open;
                    let tooltip = if open {
                        "Put the numbers away"
                    } else {
                        "Position, size and turn, as numbers"
                    };
                    if widgets::round_button(ui, BUTTON, Icon::Size, true, open)
                        .on_hover_text(tooltip)
                        .clicked()
                    {
                        app.ui.size_fields.open = !open;
                        app.ui.size_fields.finish();
                    }
                });
                if app.ui.size_fields.open {
                    rule(ui);
                    size_panel(ui, app);
                }
            })
        });
    app.chrome.push(area.inner.response.rect);
    if let Some((command, modifiers)) = run {
        app.run_selection_command(command, modifiers);
    }
}

/// With nothing selected but strokes locked: a small bar that says so and lets them go. A locked stroke cannot
/// be picked up, so this is the only way back to it besides the key.
fn show_locked(ctx: &Context, app: &mut InkApp) {
    let locked = app.editor.marks().locked().len();
    if locked == 0 {
        return;
    }
    let noun = if locked == 1 { "stroke" } else { "strokes" };
    let alpha = app.chrome_alpha;
    let mut pressed = false;
    let area = Area::new(Id::unique("locked_bar"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, BAR_HEIGHT + GAP))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            theme::floating(ui, [22.0; 4], Margin::symmetric(14, 8), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(
                        RichText::new(format!("{locked} {noun} locked"))
                            .size(14.0)
                            .color(theme::TEXT_DIM),
                    );
                    let tooltip = app.keymap.hint(Command::UnlockAll, "Unlock everything");
                    pressed = widgets::round_button(ui, BUTTON, Icon::Unlock, true, false)
                        .on_hover_text(tooltip)
                        .clicked();
                });
            })
        });
    app.chrome.push(area.inner.response.rect);
    if pressed {
        app.run_selection_command(Command::UnlockAll, eframe::egui::Modifiers::NONE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_says_how_many_and_how_big() {
        let px = Measurement::default();
        assert_eq!(selection_label(1, None, &px), "1 stroke");
        assert_eq!(selection_label(3, None, &px), "3 strokes");
        assert_eq!(
            selection_label(2, Some((120.4, 79.6)), &px),
            "2 strokes · 120 px × 80 px"
        );
        let cm = Measurement {
            unit: ink_core::units::Unit::Cm,
            ..px
        };
        assert_eq!(
            selection_label(2, Some((96.0, 192.0)), &cm),
            "2 strokes · 3 cm × 5 cm",
            "in the unit the settings choose"
        );
    }

    #[test]
    fn every_button_runs_a_command_that_works_on_the_selection() {
        for group in GROUPS.iter().chain(&TIES) {
            for (_, command, text) in *group {
                assert_ne!(*text, "");
                assert!(
                    matches!(
                        command,
                        Command::MoveLeft
                            | Command::MoveRight
                            | Command::MoveUp
                            | Command::MoveDown
                            | Command::FlipHorizontal
                            | Command::FlipVertical
                            | Command::Duplicate
                            | Command::Delete
                            | Command::GroupSelection
                            | Command::UngroupSelection
                            | Command::LockSelection
                            | Command::UnlockAll
                            | Command::SelectionLayerUp
                            | Command::SelectionLayerDown
                    ),
                    "{command:?} is not a selection command"
                );
            }
        }
    }
}
