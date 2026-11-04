//! The shapes menu on a device used by touch: a bottom sheet with every shape the app has, the person's own objects, and
//! how a shape is filled.
//!
//! A tap on a shape puts it on the paper. To keep one within reach, rename it or delete it, the sheet is switched to
//! arranging: a tap then picks the shape and shows what can be done with it.

use eframe::egui::{Color32, Context, Ui, vec2};
use ink_core::shapes::Shape;

use super::sheet::{self, Tone, Typed};
use crate::app::InkApp;
use crate::canvas;
use crate::shape_art::{self, FILL_COLORS, Look, Tile};
use crate::text_ops;
use crate::theme;
use crate::toolbar::Item;
use crate::widgets;

/// The smallest a tile is across; a row holds as many as fit.
const TILE: f32 = 78.0;
const GAP: f32 = 10.0;
/// The size of a color swatch.
const SWATCH: f32 = 40.0;

/// Something done in the sheet, applied after it is drawn.
enum Act {
    /// Put the shape on the paper.
    Put(String),
    /// Pick (or let go of) the shape to arrange.
    Pick(String),
    Pin(String),
    Unpin(String),
    AskRename(String),
    Delete(String),
    /// Ask for the name of an object to make of the selection.
    AskSave,
    Fill(Option<[u8; 3]>),
    /// Fill with the ink in use now.
    FillWithInk,
    Outline(bool),
    Size(f32),
    Arrange(bool),
    Customize,
    /// Write words, or change the ones selected.
    Text,
    /// Keep the shapes open at the side of the paper.
    Dock,
}

/// Shows the shapes menu while it is open.
pub fn show(ctx: &Context, app: &mut InkApp) {
    if !app.ui.shapes_open {
        return;
    }
    if name_sheet(ctx, app) {
        return;
    }
    let mut acts = Vec::new();
    let answer = sheet::show(ctx, "shapes", Some("Shapes"), |ui| {
        content(ui, app, &mut acts);
    });
    // A press on the sheet is not a press outside the panels, which would put the menu away before the shape was tapped.
    app.chrome.push(answer.rect);
    if answer.dismissed {
        app.ui.close_popovers();
        return;
    }
    for act in acts {
        apply(ctx, app, act);
    }
}

/// The sheet that asks for a name, while an object is being made or renamed. Returns `true` while one is up.
fn name_sheet(ctx: &Context, app: &mut InkApp) -> bool {
    if let Some(mut name) = app.ui.objects.naming.take() {
        let (typed, rect) =
            sheet::ask_text(ctx, "object_name", "Name this object", &mut name, "Save");
        app.chrome.push(rect);
        match typed {
            Typed::Editing => app.ui.objects.naming = Some(name),
            Typed::Cancel => {}
            Typed::Save => {
                app.save_selection_as_object(ctx, &name);
            }
        }
        return true;
    }
    if let Some((id, mut name)) = app.ui.objects.renaming.take() {
        let (typed, rect) = sheet::ask_text(ctx, "object_rename", "Rename", &mut name, "Save");
        app.chrome.push(rect);
        match typed {
            Typed::Editing => app.ui.objects.renaming = Some((id, name)),
            Typed::Cancel => {}
            Typed::Save => {
                if app.objects.rename(&id, &name) {
                    app.mark_config_changed();
                }
            }
        }
        return true;
    }
    false
}

fn content(ui: &mut Ui, app: &InkApp, acts: &mut Vec<Act>) {
    let arranging = app.ui.objects.arranging;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        if sheet::pill(ui, "Save selection", app.can_save_object(), Tone::Normal) {
            acts.push(Act::AskSave);
        }
        if sheet::pill(
            ui,
            if arranging { "Done" } else { "Arrange" },
            true,
            Tone::Normal,
        ) {
            acts.push(Act::Arrange(!arranging));
        }
        let words = if text_ops::holds_words(app) {
            "Change words"
        } else {
            "Add text"
        };
        if sheet::pill(ui, words, true, Tone::Normal) {
            acts.push(Act::Text);
        }
        if sheet::pill(ui, "Keep at the side", true, Tone::Normal) {
            acts.push(Act::Dock);
        }
        if sheet::pill(ui, "Tool bar", true, Tone::Normal) {
            acts.push(Act::Customize);
        }
    });
    if let Some(shape) = picked(app) {
        actions(ui, app, shape, acts);
    }
    sheet::caption(ui, "FILL");
    look_card(ui, app, acts);
    sheet::caption(ui, "BASIC SHAPES");
    grid(ui, app, app.objects.built_in(), acts);
    sheet::caption(ui, "MY OBJECTS");
    if app.objects.custom().is_empty() {
        ui.label(
            eframe::egui::RichText::new(
                "Select something you drew, then Save selection. It will be here in every drawing.",
            )
            .size(14.0)
            .color(theme::TEXT_DIM),
        );
        ui.add_space(6.0);
    } else {
        grid(ui, app, app.objects.custom(), acts);
    }
}

/// The shape picked to arrange, if there is one and arranging is on.
fn picked(app: &InkApp) -> Option<&Shape> {
    if !app.ui.objects.arranging {
        return None;
    }
    app.ui
        .objects
        .picked
        .as_deref()
        .and_then(|id| app.objects.get(id))
}

/// What can be done with the picked shape.
fn actions(ui: &mut Ui, app: &InkApp, shape: &Shape, acts: &mut Vec<Act>) {
    let pinned = app
        .config
        .toolbar_items()
        .contains(&Item::Shape(shape.id.clone()));
    widgets::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(eframe::egui::RichText::new(&shape.name).size(16.0).strong());
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            if pinned {
                if sheet::pill(ui, "Take off the bar", true, Tone::Normal) {
                    acts.push(Act::Unpin(shape.id.clone()));
                }
            } else if sheet::pill(ui, "Put on the bar", true, Tone::Normal) {
                acts.push(Act::Pin(shape.id.clone()));
            }
            if app.objects.is_custom(&shape.id) {
                if sheet::pill(ui, "Rename", true, Tone::Normal) {
                    acts.push(Act::AskRename(shape.id.clone()));
                }
                if sheet::pill(ui, "Delete", true, Tone::Danger) {
                    acts.push(Act::Delete(shape.id.clone()));
                }
            }
        });
    });
}

/// The colors of a shape: its inside, whether it has an outline, and how big it lands.
fn look_card(ui: &mut Ui, app: &InkApp, acts: &mut Vec<Act>) {
    let mut outline = app.config.shapes.outline;
    let mut size = app.config.shapes.size;
    widgets::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            let fill = app.config.shapes.fill;
            if shape_art::swatch(ui, SWATCH, None, fill.is_none()).clicked() {
                acts.push(Act::Fill(None));
            }
            let [r, g, b] = app.tool.color.to_srgb();
            if shape_art::swatch(ui, SWATCH, Some(Color32::from_rgb(r, g, b)), false).clicked() {
                acts.push(Act::FillWithInk);
            }
            for rgb in FILL_COLORS {
                let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                if shape_art::swatch(ui, SWATCH, Some(color), fill == Some(rgb)).clicked() {
                    acts.push(Act::Fill(Some(rgb)));
                }
            }
        });
        ui.add_space(6.0);
        if widgets::toggle_row(ui, "Outline", None, &mut outline) {
            acts.push(Act::Outline(outline));
        }
        if widgets::slider_row(
            ui,
            "Size",
            Some("How big a shape lands"),
            &mut size,
            0.1..=0.8,
            |v| format!("{:.0}%", v * 100.0),
        ) {
            acts.push(Act::Size(size));
        }
    });
}

/// `shapes` as tiles, as many to a row as the width holds.
fn grid(ui: &mut Ui, app: &InkApp, shapes: &[Shape], acts: &mut Vec<Act>) {
    let available = ui.available_width();
    let columns = (((available + GAP) / (TILE + GAP)).floor() as usize).clamp(3, 8);
    let side = (available - GAP * (columns as f32 - 1.0)) / columns as f32;
    let sheet = canvas::parse_hex(&app.editor.document().metadata.background_solid_color);
    let [r, g, b] = app.tool.color.to_srgb();
    let look = Look {
        line: Color32::from_rgb(r, g, b),
        fill: app
            .config
            .shapes
            .fill
            .map(|[r, g, b]| Color32::from_rgb(r, g, b)),
        outline: app.config.shapes.outline,
    };
    let pins: Vec<Item> = app.config.toolbar_items();
    let arranging = app.ui.objects.arranging;
    // Laid out a row at a time, so they are as many across as were counted and not as many as the wrapping finds room for.
    for row in shapes.chunks(columns) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for shape in row {
                let tile = Tile {
                    shape,
                    look,
                    paper: sheet,
                    selected: arranging
                        && app.ui.objects.picked.as_deref() == Some(shape.id.as_str()),
                    pinned: pins.contains(&Item::Shape(shape.id.clone())),
                };
                if shape_art::tile(ui, side, &tile).clicked() {
                    acts.push(if arranging {
                        Act::Pick(shape.id.clone())
                    } else {
                        Act::Put(shape.id.clone())
                    });
                }
            }
        });
        ui.add_space(GAP - 2.0);
    }
    ui.add_space(4.0);
}

fn apply(ctx: &Context, app: &mut InkApp, act: Act) {
    match act {
        Act::Put(id) => {
            if app.insert_shape(ctx, &id) {
                app.ui.close_popovers();
            }
        }
        Act::Pick(id) => {
            let again = app.ui.objects.picked.as_deref() == Some(id.as_str());
            app.ui.objects.picked = (!again).then_some(id);
        }
        Act::Pin(id) => {
            app.config.toolbar_add(Item::Shape(id));
            app.mark_config_changed();
        }
        Act::Unpin(id) => {
            app.config.toolbar_remove(&Item::Shape(id));
            app.mark_config_changed();
        }
        Act::AskRename(id) => {
            let name = app
                .objects
                .get(&id)
                .map(|s| s.name.clone())
                .unwrap_or_default();
            app.ui.objects.renaming = Some((id, name));
        }
        Act::Delete(id) => {
            app.delete_object(&id);
            app.ui.objects.picked = None;
        }
        Act::AskSave => app.ui.objects.naming = Some(String::new()),
        Act::Fill(fill) => {
            app.config.shapes.fill = fill;
            app.mark_config_changed();
        }
        Act::FillWithInk => {
            app.config.shapes.fill = Some(app.tool.color.to_srgb());
            app.mark_config_changed();
        }
        Act::Outline(on) => {
            app.config.shapes.outline = on;
            app.mark_config_changed();
        }
        Act::Size(size) => {
            app.config.shapes.size = size;
            app.mark_config_changed();
        }
        Act::Arrange(on) => {
            app.ui.objects.arranging = on;
            app.ui.objects.picked = None;
        }
        Act::Customize => app.ui.open_toolbar_editor(),
        Act::Text => app.begin_text(),
        Act::Dock => app.dock_shapes(),
    }
}
