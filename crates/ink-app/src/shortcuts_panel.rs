//! The Shortcuts page of the settings: every command with its keys, and a way to change them.
//!
//! Click a key to change it and press the new one; the plus adds a second key, the cross takes one away. A key
//! that another command had moves to the command being changed, and a message says so.

use eframe::egui::{self, Context, Event, Id, Key, RichText, Stroke, TextEdit, Ui, vec2};

use crate::app::InkApp;
use crate::icons::Icon;
use crate::shortcuts::{self, Binding, Command, Group};
use crate::theme;
use crate::widgets;

/// What the user did to a row this frame; applied after the rows are drawn.
enum Action {
    Capture(Command, Option<usize>),
    Remove(Command, usize),
    Reset(Command),
}

pub fn section(ui: &mut Ui, app: &mut InkApp) {
    let ctx = ui.ctx().clone();
    capture_press(&ctx, app);

    widgets::caption(
        ui,
        "Click a key, then press the new one. Esc cancels. A key another command had moves to this one.",
    );
    widgets::caption(
        ui,
        "Arrow keys move a selection further with Shift held and finer with Alt held.",
    );

    let search_id = Id::unique("shortcut_search");
    let mut query: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
    let mut reset_all = false;
    ui.horizontal(|ui| {
        ui.add(
            TextEdit::singleline(&mut query)
                .hint_text("Find a command")
                .desired_width(ui.available_width() - 150.0),
        );
        let armed_id = Id::unique("shortcuts_reset_armed");
        let armed = ui.data(|d| d.get_temp::<bool>(armed_id)).unwrap_or(false);
        let label = if armed {
            "Click again"
        } else {
            "Reset all keys"
        };
        if ui.button(label).clicked() {
            reset_all = armed;
            ui.data_mut(|d| d.insert_temp(armed_id, !armed));
        }
    });
    ui.data_mut(|d| d.insert_temp(search_id, query.clone()));
    let query = query.trim().to_lowercase();

    let mut action = None;
    for group in Group::ALL {
        let commands: Vec<Command> = Command::ALL
            .iter()
            .copied()
            .filter(|c| c.group() == group)
            .filter(|c| query.is_empty() || c.label().to_lowercase().contains(&query))
            .collect();
        if commands.is_empty() {
            continue;
        }
        ui.add_space(6.0);
        widgets::heading(ui, group.label().to_uppercase());
        for command in commands {
            if let Some(done) = row(ui, app, command) {
                action = Some(done);
            }
        }
    }

    if reset_all {
        app.config.shortcuts.clear();
        app.rebuild_keymap();
        app.ui.capture = None;
    }
    match action {
        Some(Action::Capture(command, slot)) => app.ui.capture = Some((command, slot)),
        Some(Action::Remove(command, slot)) => {
            shortcuts::unbind(&mut app.config.shortcuts, command, slot);
            app.rebuild_keymap();
        }
        Some(Action::Reset(command)) => {
            app.config.shortcuts.remove(command.id());
            app.rebuild_keymap();
        }
        None => {}
    }
}

/// One command: its name, its keys as chips, a plus for another key, and a reset when it was changed.
fn row(ui: &mut Ui, app: &InkApp, command: Command) -> Option<Action> {
    let mut action = None;
    let keys: Vec<Binding> = app.keymap.bindings(command).collect();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, _) = ui.allocate_exact_size(vec2(230.0, 32.0), egui::Sense::hover());
        ui.painter().text(
            egui::pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            command.label(),
            egui::FontId::proportional(15.0),
            theme::TEXT,
        );
        if keys.is_empty() {
            ui.label(RichText::new("No key").color(theme::TEXT_DIM));
        }
        for (slot, key) in keys.iter().enumerate() {
            let capturing = app.ui.capture == Some((command, Some(slot)));
            let text = if capturing {
                "Press a key".to_owned()
            } else {
                key.label()
            };
            if chip(ui, &text, capturing).clicked() {
                action = Some(Action::Capture(command, Some(slot)));
            }
            if !capturing
                && widgets::round_button(ui, 22.0, Icon::Close, true, false)
                    .on_hover_text("Take this key away")
                    .clicked()
            {
                action = Some(Action::Remove(command, slot));
            }
        }
        let adding = app.ui.capture == Some((command, None));
        if adding {
            chip(ui, "Press a key", true);
        } else if widgets::round_button(ui, 26.0, Icon::Plus, true, false)
            .on_hover_text("Add another key")
            .clicked()
        {
            action = Some(Action::Capture(command, None));
        }
        if app.config.shortcuts.contains_key(command.id())
            && ui
                .add(egui::Button::new("Reset").min_size(vec2(0.0, 26.0)))
                .on_hover_text("Back to the starting keys")
                .clicked()
        {
            action = Some(Action::Reset(command));
        }
    });
    action
}

fn chip(ui: &mut Ui, text: &str, capturing: bool) -> egui::Response {
    let color = if capturing {
        theme::ACCENT
    } else {
        theme::TEXT
    };
    let button = egui::Button::new(RichText::new(text).monospace().size(13.0).color(color))
        .fill(if capturing {
            theme::ACCENT.gamma_multiply(0.2)
        } else {
            theme::CHROME_HI
        })
        .stroke(if capturing {
            Stroke::new(1.0, theme::ACCENT)
        } else {
            Stroke::NONE
        })
        .min_size(vec2(0.0, 30.0));
    ui.add(button)
}

/// While a key is being chosen, the next key press is the answer. Every key press is taken out of the input, so
/// Esc cancels the choice and does not also close the settings.
fn capture_press(ctx: &Context, app: &mut InkApp) {
    let Some((command, slot)) = app.ui.capture else {
        return;
    };
    let pressed = ctx.input_mut(|input| {
        let mut found = None;
        input.events.retain(|event| match event {
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                found.get_or_insert((*key, *modifiers));
                false
            }
            Event::Copy => {
                found.get_or_insert((Key::C, egui::Modifiers::COMMAND));
                false
            }
            Event::Cut => {
                found.get_or_insert((Key::X, egui::Modifiers::COMMAND));
                false
            }
            Event::Paste(_) => {
                found.get_or_insert((Key::V, egui::Modifiers::COMMAND));
                false
            }
            Event::Key { .. } => false,
            _ => true,
        });
        found
    });
    let Some((key, modifiers)) = pressed else {
        return;
    };
    app.ui.capture = None;
    if key == Key::Escape && !modifiers.any() {
        return;
    }
    let binding = Binding::of(key, modifiers);
    let lost = shortcuts::assign(&mut app.config.shortcuts, command, slot, binding);
    app.rebuild_keymap();
    if let Some(other) = lost {
        app.notify(
            ctx,
            format!(
                "{} moved from \"{}\" to \"{}\"",
                binding.label(),
                other.label(),
                command.label()
            ),
        );
    }
}
