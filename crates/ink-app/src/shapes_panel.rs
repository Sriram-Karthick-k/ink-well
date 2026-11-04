//! The shapes menu on the computer: a flyout beside the rail with every shape the app has, the person's own objects, and
//! how a shape is filled.
//!
//! A click on a shape puts it on the paper. A right click on one of the person's own opens a small menu to rename or
//! delete it. The touch interface has a sheet of its own for the same things (see `touch_ui::shapes_sheet`); both
//! ask `object_ops` for what they do.

use eframe::egui::{Color32, Context, Key, RichText, ScrollArea, Slider, TextEdit, Ui, vec2};
use ink_core::shapes::Shape;

use crate::app::InkApp;
use crate::canvas;
use crate::flyouts;
use crate::shape_art::{self, FILL_COLORS, Look, Tile};
use crate::text_ops;
use crate::theme;
use crate::widgets;

/// How many tiles go across, how wide each is, and the space between them.
const COLUMNS: usize = 4;
const TILE: f32 = 78.0;
const GAP: f32 = 8.0;
/// The size of a color swatch.
const SWATCH: f32 = 28.0;

/// What a click on a tile does for a shape.
enum Act {
    Put(String),
    Rename(String),
    Delete(String),
}

/// Shows the flyout while the shapes menu is open.
pub fn show(ctx: &Context, app: &mut InkApp) {
    let open = app.ui.shapes_open;
    let anchor = app.ui.anchors.shapes;
    flyouts::flyout(ctx, app, "shapes", open, anchor, 640.0, |ui, app| {
        ui.set_width(COLUMNS as f32 * TILE + (COLUMNS as f32 - 1.0) * GAP);
        content(ui, app);
    });
}

fn content(ui: &mut Ui, app: &mut InkApp) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Shapes").size(18.0).strong());
        ui.with_layout(
            eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
            |ui| {
                let can = app.can_save_object();
                let save = ui
                    .add_enabled(can, eframe::egui::Button::new("Save selection"))
                    .on_hover_text("Make an object of what is selected, to put on any drawing")
                    .on_disabled_hover_text("Select something you drew first");
                if save.clicked() {
                    app.ui.objects.naming = Some(String::new());
                }
                if ui
                    .button("Dock")
                    .on_hover_text("Keep the shapes open at the side of the paper, to drag from")
                    .clicked()
                {
                    app.dock_shapes();
                }
                let words = if text_ops::holds_words(app) {
                    "Change words"
                } else {
                    "Add text"
                };
                if ui
                    .button(words)
                    .on_hover_text("Write inside the shape that is selected, or on its own")
                    .clicked()
                {
                    app.begin_text();
                }
            },
        );
    });
    ui.add_space(4.0);
    naming(ui, app);
    look(ui, app);
    ui.add_space(6.0);
    let room = (ui.ctx().content_rect().height() - 320.0).clamp(200.0, 520.0);
    let mut act = None;
    ScrollArea::vertical().max_height(room).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        heading(ui, "BASIC SHAPES");
        let built_in = app.objects.built_in().to_vec();
        grid(ui, app, &built_in, &mut act);
        heading(ui, "MY OBJECTS");
        if app.objects.custom().is_empty() {
            widgets::caption(
                ui,
                "Select something you drew, then Save selection. It will be here in every drawing.",
            );
        } else {
            let custom = app.objects.custom().to_vec();
            grid(ui, app, &custom, &mut act);
        }
    });
    match act {
        Some(Act::Put(id)) => {
            let ctx = ui.ctx().clone();
            app.insert_shape(&ctx, &id);
        }
        Some(Act::Rename(id)) => {
            let name = app
                .objects
                .get(&id)
                .map(|s| s.name.clone())
                .unwrap_or_default();
            app.ui.objects.renaming = Some((id, name));
        }
        Some(Act::Delete(id)) => app.delete_object(&id),
        None => {}
    }
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    widgets::heading(ui, text);
}

/// The field where the name of a new object, or the new name of one, is typed.
fn naming(ui: &mut Ui, app: &mut InkApp) {
    let renaming = app.ui.objects.renaming.is_some();
    let Some(text) = app
        .ui
        .objects
        .naming
        .as_mut()
        .or_else(|| app.ui.objects.renaming.as_mut().map(|(_, name)| name))
    else {
        return;
    };
    let mut save = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        let field = ui.add(
            TextEdit::singleline(text)
                .hint_text(if renaming {
                    "New name"
                } else {
                    "Name this object"
                })
                .desired_width(ui.available_width() - 120.0),
        );
        // Asked first: taking the focus back would make the field look as if it had not been left.
        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            save = true;
        } else if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            cancel = true;
        } else if !field.has_focus() && !field.lost_focus() {
            field.request_focus();
        }
        if ui.button("Save").clicked() {
            save = true;
        }
        if ui.button("Cancel").clicked() {
            cancel = true;
        }
    });
    ui.add_space(4.0);
    let ctx = ui.ctx().clone();
    if cancel {
        app.ui.objects.naming = None;
        app.ui.objects.renaming = None;
    } else if save {
        if let Some(name) = app.ui.objects.naming.take() {
            app.save_selection_as_object(&ctx, &name);
        } else if let Some((id, name)) = app.ui.objects.renaming.take()
            && app.objects.rename(&id, &name)
        {
            app.mark_config_changed();
        }
    }
}

/// How the inside of a shape is chosen, whether it has an outline, and how big it lands.
fn look(ui: &mut Ui, app: &mut InkApp) {
    ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
    ui.horizontal_wrapped(|ui| {
        let fill = app.config.shapes.fill;
        if shape_art::swatch(ui, SWATCH, None, fill.is_none())
            .on_hover_text("No fill")
            .clicked()
        {
            app.config.shapes.fill = None;
            app.mark_config_changed();
        }
        let ink = app.tool.color.to_srgb();
        if shape_art::swatch(
            ui,
            SWATCH,
            Some(Color32::from_rgb(ink[0], ink[1], ink[2])),
            false,
        )
        .on_hover_text("Fill with the ink in use")
        .clicked()
        {
            app.config.shapes.fill = Some(ink);
            app.mark_config_changed();
        }
        for rgb in FILL_COLORS {
            let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            if shape_art::swatch(ui, SWATCH, Some(color), fill == Some(rgb)).clicked() {
                app.config.shapes.fill = Some(rgb);
                app.mark_config_changed();
            }
        }
    });
    ui.add_space(4.0);
    let mut outline = app.config.shapes.outline;
    if widgets::toggle_row(ui, "Outline", None, &mut outline) {
        app.config.shapes.outline = outline;
        app.mark_config_changed();
    }
    let mut size = app.config.shapes.size;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Size").color(theme::TEXT_DIM));
        ui.spacing_mut().slider_width = 160.0;
        if widgets::slider(ui, Slider::new(&mut size, 0.1..=0.8).show_value(false)).changed() {
            app.config.shapes.size = size;
            app.mark_config_changed();
        }
        ui.label(
            RichText::new(format!("{:.0}%", size * 100.0))
                .color(theme::TEXT_DIM)
                .monospace(),
        );
    });
}

/// `shapes` as tiles, four to a row.
fn grid(ui: &mut Ui, app: &InkApp, shapes: &[Shape], act: &mut Option<Act>) {
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
    for row in shapes.chunks(COLUMNS) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for shape in row {
                let tile = Tile {
                    shape,
                    look,
                    paper: sheet,
                    selected: false,
                    pinned: false,
                };
                let response = shape_art::tile(ui, TILE, &tile).on_hover_text(&shape.name);
                if response.clicked() {
                    *act = Some(Act::Put(shape.id.clone()));
                }
                if app.objects.is_custom(&shape.id) {
                    response.context_menu(|ui| {
                        if ui.button("Rename").clicked() {
                            *act = Some(Act::Rename(shape.id.clone()));
                            ui.close();
                        }
                        if ui.button("Delete").clicked() {
                            *act = Some(Act::Delete(shape.id.clone()));
                            ui.close();
                        }
                    });
                }
            }
        });
    }
    ui.add_space(2.0);
}
