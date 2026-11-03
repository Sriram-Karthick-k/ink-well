//! The settings window: every option the app has, in sections, with changes saved as they are made.

use eframe::egui::{
    self, Align, Context, CornerRadius, Frame, Id, Layout, Margin, Modal, RichText, ScrollArea,
    Sense, TextEdit, Ui, vec2,
};

use crate::app::InkApp;
use crate::brushes::{self, BRUSHES, BrushInfo};
use ink_core::Pen;

use crate::config::{
    self, BrushConfig, Config, EraserKind, NudgeVariant, Pattern, Renderer, SelectShape, SliceMode,
    TapAction, ToolsConfig,
};
use ink_core::artboard::Artboard;
use ink_core::units::{Format, Measurement, Precision, Unit};

use crate::icons::{self, Icon};
use crate::precision;
use crate::render;
use crate::shortcuts_panel;
use crate::state::ToolState;
use crate::theme;
use crate::widgets;

const NAV_WIDTH: f32 = 164.0;
const MAX_SIZE: egui::Vec2 = vec2(820.0, 580.0);

/// The parts of the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Brushes,
    Tools,
    Workspace,
    Input,
    Precision,
    Canvas,
    Interface,
    Rendering,
    Files,
    Shortcuts,
    About,
}

impl Section {
    pub const ALL: [Self; 11] = [
        Self::Brushes,
        Self::Tools,
        Self::Workspace,
        Self::Input,
        Self::Precision,
        Self::Canvas,
        Self::Interface,
        Self::Rendering,
        Self::Files,
        Self::Shortcuts,
        Self::About,
    ];

    /// The section called `name` in the window, ignoring case; an empty or unknown name gives the first.
    #[must_use]
    pub fn named(name: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|s| s.label().eq_ignore_ascii_case(name.trim()))
            .unwrap_or(Self::Brushes)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Brushes => "Brushes",
            Self::Tools => "Tools",
            Self::Workspace => "Workspace",
            Self::Input => "Interaction",
            Self::Precision => "Precision",
            Self::Canvas => "Canvas",
            Self::Interface => "Interface",
            Self::Rendering => "Rendering",
            Self::Files => "Files",
            Self::Shortcuts => "Shortcuts",
            Self::About => "About",
        }
    }
}

pub fn show(ctx: &Context, app: &mut InkApp) {
    let Some(mut section) = app.ui.settings else {
        return;
    };
    let screen = ctx.content_rect().size();
    let outer = vec2(
        (screen.x - 32.0).min(MAX_SIZE.x),
        (screen.y - 32.0).min(MAX_SIZE.y),
    );
    let inner = outer - vec2(32.0, 32.0);
    let before = app.config.clone();

    let response = Modal::new(Id::unique("settings"))
        .backdrop_color(egui::Color32::from_black_alpha(130))
        .frame(
            Frame::new()
                .fill(theme::CHROME)
                .corner_radius(CornerRadius::same(22))
                .inner_margin(Margin::same(16))
                .stroke(theme::hairline())
                .shadow(theme::shadow()),
        )
        .show(ctx, |ui| {
            ui.set_width(inner.x);
            ui.set_height(inner.y);
            let mut close = false;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Settings").strong().size(20.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = icons::button(ui, Icon::Close, "Close (Esc)", true, false).clicked();
                });
            });
            ui.add_space(2.0);
            let body = ui.available_height();
            ui.horizontal_top(|ui| {
                nav(ui, &mut section);
                ui.separator();
                ui.vertical(|ui| {
                    ScrollArea::vertical()
                        .max_height(body)
                        .auto_shrink([false, false])
                        .show(ui, |ui| content(ui, app, section));
                });
            });
            close
        });

    app.ui.settings = if response.inner || response.should_close() {
        None
    } else {
        Some(section)
    };
    if app.config != before {
        app.mark_config_changed();
    }
}

fn nav(ui: &mut Ui, section: &mut Section) {
    ui.vertical(|ui| {
        ui.set_width(NAV_WIDTH);
        ui.spacing_mut().item_spacing.y = 4.0;
        for s in Section::ALL {
            let button =
                egui::Button::selectable(*section == s, RichText::new(s.label()).size(15.0))
                    .min_size(vec2(NAV_WIDTH, 40.0));
            if ui.add(button).clicked() {
                *section = s;
            }
        }
    });
}

fn content(ui: &mut Ui, app: &mut InkApp, section: Section) {
    ui.spacing_mut().item_spacing.y = 12.0;
    ui.label(RichText::new(section.label()).size(19.0).strong());
    match section {
        Section::Brushes => brushes_section(ui, app),
        Section::Tools => tools_section(ui, &mut app.config),
        Section::Workspace => workspace_section(ui, &mut app.config),
        Section::Input => input_section(ui, &mut app.config),
        Section::Precision => precision_section(ui, app),
        Section::Canvas => canvas_section(ui, &mut app.config),
        Section::Interface => interface_section(ui, &mut app.config),
        Section::Rendering => rendering_section(ui, app),
        Section::Files => files_section(ui, app),
        Section::Shortcuts => shortcuts_panel::section(ui, app),
        Section::About => about_section(ui, app),
    }
    ui.add_space(12.0);
}

// Brushes

fn brushes_section(ui: &mut Ui, app: &mut InkApp) {
    widgets::caption(
        ui,
        "Each brush remembers its own size, opacity and smoothing. Smoothing steadies the line while you draw.",
    );
    for info in &BRUSHES {
        brush_card(ui, &mut app.config, &app.tool, app.view.zoom, info);
    }
    if ui.button("Reset all brushes").clicked() {
        app.config.brushes.clear();
    }
}

fn brush_card(ui: &mut Ui, config: &mut Config, tool: &ToolState, zoom: f32, info: &BrushInfo) {
    let pen = info.pen;
    let stored = config.brush(pen);
    let mut brush = stored;
    let can_reset = config.brushes.contains_key(brushes::key(pen));
    let mut reset = false;

    widgets::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(116.0, 46.0), Sense::hover());
            ui.painter().rect_filled(
                rect,
                CornerRadius::same(11),
                widgets::backdrop_for(tool.color.to_srgb()),
            );
            render::draw_brush_preview(
                ui.painter(),
                rect,
                pen,
                tool.stroke_color(brush.opacity),
                brush.size * zoom,
            );
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(info.name).size(15.5).strong());
                widgets::caption(ui, info.blurb);
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                reset = ui
                    .add_enabled(
                        can_reset,
                        egui::Button::new("Reset").min_size(vec2(0.0, 30.0)),
                    )
                    .clicked();
            });
        });
        ui.columns(3, |columns| {
            let [size, opacity, smoothing] = columns else {
                return;
            };
            column_slider(size, "Size", |ui| {
                widgets::slider(
                    ui,
                    egui::Slider::new(
                        &mut brush.size,
                        BrushConfig::MIN_SIZE..=BrushConfig::MAX_SIZE,
                    )
                    .logarithmic(true)
                    .max_decimals(1),
                )
            });
            column_slider(opacity, "Opacity", |ui| {
                widgets::slider(ui, percent(&mut brush.opacity, BrushConfig::MIN_OPACITY))
            });
            column_slider(smoothing, "Smoothing", |ui| {
                widgets::slider(ui, percent(&mut brush.smoothness, 0.0))
            });
        });
    });

    if reset {
        config.reset_brush(pen);
    } else if brush != stored {
        *config.brush_mut(pen) = brush;
    }
}

/// A slider over `min..=1.0` written as a percentage.
fn percent(value: &mut f32, min: f32) -> egui::Slider<'_> {
    egui::Slider::new(value, min..=1.0)
        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
        .custom_parser(|text| {
            text.trim()
                .trim_end_matches('%')
                .parse::<f64>()
                .ok()
                .map(|v| v / 100.0)
        })
}

/// One labeled slider in a column of a brush card.
fn column_slider(ui: &mut Ui, title: &str, slider: impl FnOnce(&mut Ui) -> egui::Response) {
    ui.spacing_mut().item_spacing.y = 4.0;
    ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(40.0);
    widgets::caption(ui, title);
    slider(ui);
}

// Tools

fn tools_section(ui: &mut Ui, config: &mut Config) {
    let tools = &mut config.tools;
    widgets::heading(ui, "SELECT");
    widgets::setting_row(
        ui,
        "Select with a",
        Some("Draw a loop around strokes, or drag a box between two corners"),
        |ui| {
            ui.horizontal(|ui| {
                let options: Vec<(SelectShape, &str)> =
                    SelectShape::ALL.iter().map(|s| (*s, s.label())).collect();
                widgets::segmented(ui, &options, &mut tools.select_shape);
            });
        },
    );
    widgets::slider_row(
        ui,
        "A stroke is taken when",
        Some("How much must be inside the loop; none takes any touch"),
        &mut tools.select_coverage,
        0.0..=1.0,
        |v| {
            if v <= 0.0 {
                "touched".to_owned()
            } else {
                format!("{:.0}% inside", v * 100.0)
            }
        },
    );
    widgets::divider(ui);
    widgets::heading(ui, "SLICE");
    widgets::setting_row(
        ui,
        "Blade",
        Some("A straight cut from where you press, or one that follows the pen"),
        |ui| {
            ui.horizontal(|ui| {
                let options: Vec<(SliceMode, &str)> =
                    SliceMode::ALL.iter().map(|m| (*m, m.label())).collect();
                widgets::segmented(ui, &options, &mut tools.slice_mode);
            });
        },
    );
    widgets::divider(ui);
    widgets::heading(ui, "NUDGE");
    widgets::setting_row(
        ui,
        "Nudge with",
        Some("Pull drags a point; Push carries what it touches"),
        |ui| {
            ui.horizontal(|ui| {
                let options: Vec<(NudgeVariant, &str)> =
                    NudgeVariant::ALL.iter().map(|v| (*v, v.label())).collect();
                widgets::segmented(ui, &options, &mut tools.nudge_variant);
            });
        },
    );
    widgets::slider_row(
        ui,
        "Reach",
        Some("How much of the stroke moves with the pen, in points"),
        &mut tools.nudge_reach,
        ToolsConfig::NUDGE_REACH_RANGE.0..=ToolsConfig::NUDGE_REACH_RANGE.1,
        |v| format!("{v:.0}"),
    );
    widgets::divider(ui);
    widgets::heading(ui, "ERASER");
    widgets::setting_row(
        ui,
        "Takes",
        Some("The whole stroke it touches, or only the part under it (E, Shift+E and Alt+E)"),
        |ui| {
            ui.horizontal(|ui| {
                let options: Vec<(EraserKind, &str)> =
                    EraserKind::ALL.iter().map(|k| (*k, k.label())).collect();
                widgets::segmented(ui, &options, &mut tools.eraser_kind);
            });
        },
    );
    widgets::slider_row(
        ui,
        "Reach",
        Some("How far from the pen the eraser takes strokes, in points"),
        &mut tools.eraser_radius,
        ToolsConfig::ERASER_RANGE.0..=ToolsConfig::ERASER_RANGE.1,
        |v| format!("{v:.0}"),
    );
    widgets::divider(ui);
    selection_keys_settings(ui, tools);
    if ui.button("Reset the tools").clicked() {
        *tools = ToolsConfig::default();
    }
}

/// How far the arrow keys move a selection, and where a copy lands.
fn selection_keys_settings(ui: &mut Ui, tools: &mut ToolsConfig) {
    widgets::heading(ui, "MOVING A SELECTION WITH THE KEYS");
    for (title, hint, value) in [
        (
            "Arrow key",
            "How far one press moves it, in canvas units",
            &mut tools.arrow_step,
        ),
        ("With Alt held", "A fine step", &mut tools.arrow_small),
        ("With Shift held", "A big step", &mut tools.arrow_large),
    ] {
        widgets::slider_row(
            ui,
            title,
            Some(hint),
            value,
            ToolsConfig::ARROW_RANGE.0..=ToolsConfig::ARROW_RANGE.1,
            |v| format!("{v:.1}"),
        );
    }
    widgets::slider_row(
        ui,
        "Duplicate and paste offset",
        Some("How far down and to the right a copy lands, in canvas units"),
        &mut tools.duplicate_offset,
        0.0..=200.0,
        |v| format!("{v:.0}"),
    );
}

// Canvas

fn canvas_section(ui: &mut Ui, config: &mut Config) {
    let canvas = &mut config.canvas;
    ui.label(RichText::new("Paper pattern").size(15.0));
    widgets::caption(ui, "Printed on the paper behind your notes");
    let sheet = egui::Color32::from_rgb(canvas.paper[0], canvas.paper[1], canvas.paper[2]);
    let ink = canvas.grid_color.map_or_else(
        || crate::paper::ink_for(sheet),
        |[r, g, b]| egui::Color32::from_rgb(r, g, b),
    );
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for pattern in Pattern::ALL {
            if widgets::pattern_tile(ui, pattern, canvas.pattern == pattern, sheet, ink).clicked() {
                canvas.pattern = pattern;
            }
        }
    });
    widgets::slider_row(
        ui,
        "Spacing",
        Some("Distance between dots or lines, in canvas units"),
        &mut canvas.grid_size,
        4.0..=512.0,
        |v| format!("{v:.0}"),
    );
    widgets::slider_row(
        ui,
        "Pattern visibility",
        None,
        &mut canvas.grid_strength,
        0.0..=1.0,
        |v| format!("{:.0}%", v * 100.0),
    );
    widgets::divider(ui);
    widgets::setting_row(
        ui,
        "Paper color",
        Some("For new drawings. An opened drawing keeps its own."),
        |ui| {
            let mut rgb = canvas.paper;
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                canvas.paper = rgb;
            }
        },
    );
    widgets::divider(ui);
    grid_settings(ui, canvas);
}

/// The grid's main lines and the color it is printed in.
fn grid_settings(ui: &mut Ui, canvas: &mut config::CanvasConfig) {
    widgets::heading(ui, "GRID");
    let mut divisions = canvas.grid_divisions as f32;
    widgets::slider_row(
        ui,
        "Line thickness",
        Some("How thick the printed lines and dots are"),
        &mut canvas.line_weight,
        0.5..=3.0,
        |v| format!("{v:.1}"),
    );
    ui.add_enabled_ui(canvas.pattern.has_main_lines(), |ui| {
        if widgets::slider_row(
            ui,
            "Main lines",
            Some("Every this many lines is a main line; 1 makes all of them main lines"),
            &mut divisions,
            1.0..=16.0,
            |v| format!("every {v:.0}"),
        ) {
            canvas.grid_divisions = divisions.round() as u32;
        }
    });
    let mut automatic = canvas.grid_color.is_none();
    if widgets::toggle_row(
        ui,
        "Pick the pattern color automatically",
        Some("Automatic uses a color that shows on the paper"),
        &mut automatic,
    ) {
        canvas.grid_color = if automatic {
            None
        } else {
            Some([0x6B, 0x8E, 0xC8])
        };
    }
    if let Some(rgb) = &mut canvas.grid_color {
        widgets::setting_row(ui, "Pattern color", None, |ui| {
            ui.color_edit_button_srgb(rgb);
        });
    }
}

// Workspace

/// A drop-down list of `options` with `value` selected. Returns `true` when another was chosen.
fn choose<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut T,
    options: &[(T, &str)],
    width: f32,
) -> bool {
    let shown = options
        .iter()
        .find(|(v, _)| v == value)
        .map_or("", |(_, label)| *label);
    let before = *value;
    egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .width(width)
        .show_ui(ui, |ui| {
            for (option, label) in options {
                ui.selectable_value(value, *option, *label);
            }
        });
    *value != before
}

fn precision_section(ui: &mut Ui, app: &mut InkApp) {
    let config = &mut app.config;
    widgets::caption(
        ui,
        "Snapping makes a point that is drawn or dragged close to the grid or to the end of a stroke land exactly on it. The panel with the same switches is the crosshair button at the top right.",
    );
    let canvas = config.canvas.clone();
    precision::controls(ui, &mut config.precision, &canvas);
    widgets::divider(ui);
    widgets::heading(ui, "SHAPE RECOGNITION");
    widgets::caption(
        ui,
        "A line, an arc, a circle, an ellipse, a triangle or a rectangle that is held still at its end becomes the clean shape. A scribble is left as it is.",
    );
    precision::recognition_controls(ui, &mut config.precision);
    widgets::divider(ui);
    measurement_controls(ui, &mut config.measurement);
    widgets::divider(ui);
    artboard_controls(ui, app);
}

/// The size of an artboard: a preset to start from, and the sides in the unit the settings choose.
fn artboard_size(ui: &mut Ui, board: &mut Artboard, measurement: &Measurement) {
    let ppi = measurement.pixels_per_inch;
    let shown = ink_core::artboard::matching(board.width(), board.height(), ppi)
        .map_or("Custom", |p| p.name);
    let mut chosen = None;
    widgets::setting_row(ui, "Size", Some("A size to start from"), |ui| {
        egui::ComboBox::from_id_salt("artboard_preset")
            .selected_text(shown)
            .width(190.0)
            .show_ui(ui, |ui| {
                for group in ink_core::artboard::groups() {
                    ui.label(RichText::new(group).size(11.5).color(theme::TEXT_DIM));
                    for preset in ink_core::artboard::PRESETS
                        .iter()
                        .filter(|p| p.group == group)
                    {
                        if ui
                            .selectable_label(shown == preset.name, preset.name)
                            .clicked()
                        {
                            chosen = Some(preset.size(ppi));
                        }
                    }
                }
            });
    });
    if let Some((w, h)) = chosen {
        *board = board.resized(w, h);
    }
    // The sides in the unit the settings choose (leaving the drawing scale out: this is the page itself).
    let page = Measurement {
        scale: None,
        ..*measurement
    };
    let per = page.value(1.0, measurement.unit).max(1e-9);
    let (mut width, mut height) = (board.width() * per, board.height() * per);
    widgets::setting_row(
        ui,
        "Width and height",
        Some(measurement.unit.name(true)),
        |ui| {
            // A step of a hundredth of the size, so a big page and a small one are both easy to drag. The row
            // fills from the right, so the height goes in first to read after the width.
            let (width_step, height_step) = ((width * 0.01).max(0.01), (height * 0.01).max(0.01));
            ui.add(
                egui::DragValue::new(&mut height)
                    .speed(height_step)
                    .range(0.1..=1.0e6)
                    .max_decimals(3),
            );
            ui.label("\u{d7}");
            ui.add(
                egui::DragValue::new(&mut width)
                    .speed(width_step)
                    .range(0.1..=1.0e6)
                    .max_decimals(3),
            );
        },
    );
    if ((width - board.width() * per).abs() > 1e-4 || (height - board.height() * per).abs() > 1e-4)
        && width.is_finite()
        && height.is_finite()
    {
        *board = board.resized(width / per, height / per);
    }
}

/// The page of the drawing: whether there is one, its size (from a preset or by hand) and what it keeps in.
fn artboard_controls(ui: &mut Ui, app: &mut InkApp) {
    widgets::heading(ui, "ARTBOARD");
    widgets::caption(
        ui,
        "A page of a chosen size on the paper. It stays with the drawing while the drawing is open; it is not saved in the file.",
    );
    let (ppi, measurement) = (
        app.config.measurement.pixels_per_inch,
        app.config.measurement,
    );
    let mut present = app.artboard.is_some();
    if widgets::toggle_row(ui, "Use an artboard", None, &mut present) {
        app.artboard = present.then(|| {
            let (w, h) = ink_core::artboard::PRESETS
                .iter()
                .find(|p| p.name == "A4")
                .map_or((816.0, 1056.0), |p| p.size(ppi));
            let middle = app.canvas_rect.map_or_else(
                || ink_core::Point::new(0.0, 0.0),
                |r| app.view.to_canvas(r.center()),
            );
            Artboard::centered(middle, w, h)
        });
        app.fit_pending = true;
    }
    let Some(mut board) = app.artboard else {
        return;
    };
    let before = board;
    artboard_size(ui, &mut board, &measurement);
    if ui
        .add(egui::Button::new("Turn it on its side").min_size(vec2(0.0, 34.0)))
        .clicked()
    {
        board = board.swapped();
    }
    widgets::toggle_row(
        ui,
        "Keep drawing inside it",
        Some("A stroke begins inside it, and stays on its edge where the pen goes out"),
        &mut board.confine,
    );
    widgets::toggle_row(
        ui,
        "Show the pattern only inside it",
        Some("The grid, dots or lines are not drawn outside the page"),
        &mut board.grid_inside,
    );
    if ui
        .add(egui::Button::new("Show the whole page").min_size(vec2(0.0, 34.0)))
        .clicked()
    {
        app.fit_pending = true;
    }
    if board != before {
        app.artboard = Some(board);
    }
}

/// How lengths, sizes and angles are written, and the scale of the drawing.
fn measurement_controls(ui: &mut Ui, m: &mut Measurement) {
    widgets::heading(ui, "MEASUREMENT");
    widgets::caption(
        ui,
        "What the stroke length, the selection size and the guide labels are written in.",
    );
    let units: Vec<(Unit, String)> = Unit::ALL
        .iter()
        .map(|u| {
            let mut name = u.name(true).to_owned();
            name[..1].make_ascii_uppercase();
            (*u, format!("{name} ({})", u.abbreviation()))
        })
        .collect();
    let unit_options: Vec<(Unit, &str)> = units.iter().map(|(u, s)| (*u, s.as_str())).collect();
    widgets::setting_row(ui, "Unit", None, |ui| {
        choose(ui, "measurement_unit", &mut m.unit, &unit_options, 200.0);
    });
    widgets::toggle_row(
        ui,
        "Pick the unit for me",
        Some(
            "Millimeters for a small length, meters for a big one, within the system of the unit above",
        ),
        &mut m.auto,
    );
    widgets::setting_row(ui, "Written as", None, |ui| {
        ui.horizontal(|ui| {
            let options: Vec<(Format, &str)> =
                Format::ALL.iter().map(|f| (*f, f.label())).collect();
            widgets::segmented(ui, &options, &mut m.format);
        });
    });
    widgets::setting_row(
        ui,
        "Places",
        Some("Fractions are to the nearest sixteenth"),
        |ui| {
            let options: Vec<(Precision, &str)> =
                Precision::ALL.iter().map(|p| (*p, p.label())).collect();
            choose(
                ui,
                "measurement_precision",
                &mut m.precision,
                &options,
                140.0,
            );
        },
    );
    widgets::slider_row(
        ui,
        "Pixels per inch",
        Some("What turns the canvas into paper"),
        &mut m.pixels_per_inch,
        24.0..=600.0,
        |v| format!("{v:.0}"),
    );
    let mut scaled = m.scale.is_some();
    if widgets::toggle_row(
        ui,
        "Drawing scale",
        Some("The drawing is n times smaller than what it shows (1:n)"),
        &mut scaled,
    ) {
        m.scale = scaled.then_some(m.scale.unwrap_or(50.0));
    }
    if let Some(n) = &mut m.scale {
        widgets::slider_row(ui, "Scale 1 : n", None, n, 1.0..=500.0, |v| {
            format!("1:{v:.0}")
        });
    }
    widgets::caption(
        ui,
        format!("One inch on the page is {}.", m.length(m.pixels_per_inch)),
    );
}

fn workspace_section(ui: &mut Ui, config: &mut Config) {
    widgets::caption(
        ui,
        "How the paper behaves, what helps while you draw, and what is in the pen rail.",
    );
    widgets::heading(ui, "CANVAS");
    widgets::toggle_row(
        ui,
        "Zoom the canvas",
        Some("Pinch, the mouse wheel and the zoom keys. Off, the zoom stays where it is."),
        &mut config.input.zoom_enabled,
    );
    ui.add_enabled_ui(config.input.zoom_enabled, |ui| {
        widgets::toggle_row(
            ui,
            "Snap to round zoom levels",
            Some("Zooming that stops near 25%, 50%, 100%, 200%, 400% or 800% lands on it"),
            &mut config.input.zoom_snap,
        );
    });
    widgets::toggle_row(
        ui,
        "Turn the canvas",
        Some("Two fingers twisting, and Ctrl with the arrow keys. Off, the paper stays as it is."),
        &mut config.input.rotation_enabled,
    );
    ui.add_enabled_ui(config.input.rotation_enabled, |ui| {
        widgets::toggle_row(
            ui,
            "Snap the turn to every 15 degrees",
            Some("Turning that stops within a few degrees of a step lands on it"),
            &mut config.input.rotation_snap,
        );
    });
    widgets::divider(ui);
    widgets::heading(ui, "WHILE YOU DRAW");
    let workspace = &mut config.workspace;
    widgets::toggle_row(
        ui,
        "Brush preview on hover",
        Some("A ring the size of the brush follows the pen while it hovers over the paper"),
        &mut workspace.hover_brush_preview,
    );
    widgets::toggle_row(
        ui,
        "Show stroke length",
        Some("The length of the stroke is shown beside the pen as you draw it"),
        &mut workspace.show_stroke_length,
    );
    widgets::toggle_row(
        ui,
        "Show selection size",
        Some("The width and height of the selection, on the bar over the paper"),
        &mut workspace.show_selection_size,
    );
    widgets::divider(ui);
    rail_settings(ui, config);
    widgets::divider(ui);
    widgets::heading(ui, "KEYBOARD");
    widgets::toggle_row(
        ui,
        "Keyboard shortcuts",
        Some("Off, no key does anything. The keys themselves are under Shortcuts."),
        &mut config.workspace.keyboard_shortcuts,
    );
}

/// How many pens the rail holds, and which.
fn rail_settings(ui: &mut Ui, config: &mut Config) {
    widgets::heading(ui, "PEN RAIL");
    let pens = config.rail_pens();
    let mut slots = pens.len();
    let counts: Vec<(usize, String)> = (config::RAIL_SLOTS.0..=config::RAIL_SLOTS.1)
        .map(|n| (n, n.to_string()))
        .collect();
    let options: Vec<(usize, &str)> = counts.iter().map(|(n, s)| (*n, s.as_str())).collect();
    widgets::setting_row(
        ui,
        "Pens on the rail",
        Some("How many pens are in the rail. Each one has a key: 1, 2, 3 and so on."),
        |ui| {
            ui.horizontal(|ui| {
                widgets::segmented(ui, &options, &mut slots);
            });
        },
    );
    if slots != pens.len() {
        config.resize_rail(slots);
    }
    let brush_options: Vec<(Pen, &str)> = BRUSHES.iter().map(|b| (b.pen, b.name)).collect();
    for (slot, pen) in config.rail_pens().into_iter().enumerate() {
        let mut chosen = pen;
        widgets::setting_row(ui, &format!("Place {}", slot + 1), None, |ui| {
            choose(ui, ("rail_slot", slot), &mut chosen, &brush_options, 180.0);
        });
        if chosen != pen {
            config.set_rail_slot(slot, chosen);
        }
    }
}

// Interaction

fn input_section(ui: &mut Ui, config: &mut Config) {
    let input = &mut config.input;
    widgets::heading(ui, "PEN");
    widgets::toggle_row(
        ui,
        "Use pen pressure",
        Some("Off, every stroke is drawn as if by a mouse"),
        &mut input.use_pressure,
    );
    ui.add_enabled_ui(input.use_pressure, |ui| {
        pressure_response(ui, input);
    });
    widgets::divider(ui);
    widgets::heading(ui, "TOUCH");
    widgets::toggle_row(
        ui,
        "Two-finger pan and zoom",
        Some("Pinch to zoom and drag with two fingers to move the canvas"),
        &mut input.two_finger_gestures,
    );
    let actions: Vec<(TapAction, &str)> = TapAction::ALL.iter().map(|a| (*a, a.label())).collect();
    for (title, hint, action) in [
        (
            "Two-finger tap",
            "Touch the screen with two fingers at once",
            &mut input.two_finger_tap,
        ),
        (
            "Three-finger tap",
            "Three fingers at once",
            &mut input.three_finger_tap,
        ),
        (
            "Four-finger tap",
            "Four fingers at once",
            &mut input.four_finger_tap,
        ),
    ] {
        widgets::setting_row(ui, title, Some(hint), |ui| {
            choose(ui, title, action, &actions, 170.0);
        });
    }
    widgets::divider(ui);
    widgets::heading(ui, "MOUSE");
    widgets::toggle_row(
        ui,
        "Mouse wheel zooms",
        Some("Off, the wheel moves the canvas and Ctrl with the wheel zooms"),
        &mut input.wheel_zooms,
    );
}

/// The two sliders that set how hard a pen has to press, with the curve they make.
fn pressure_response(ui: &mut Ui, input: &mut config::InputConfig) {
    widgets::slider_row(
        ui,
        "Lightest touch",
        Some("Pressure up to this counts as the lightest touch"),
        &mut input.pressure_low,
        0.0..=0.9,
        |v| format!("{:.0}%", v * 100.0),
    );
    widgets::slider_row(
        ui,
        "Firmest touch",
        Some("Pressure from this up counts as the firmest touch"),
        &mut input.pressure_high,
        0.1..=1.0,
        |v| format!("{:.0}%", v * 100.0),
    );
    input.pressure_high = input.pressure_high.max(input.pressure_low + 0.05).min(1.0);
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().min(240.0), 76.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), theme::CARD);
    let inner = rect.shrink(8.0);
    painter.line_segment(
        [inner.left_bottom(), inner.right_top()],
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(28)),
    );
    let curve: Vec<egui::Pos2> = (0..=40)
        .map(|i| {
            let force = i as f32 / 40.0;
            egui::pos2(
                inner.left() + force * inner.width(),
                inner.bottom() - input.pressure(force) * inner.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        curve,
        egui::Stroke::new(2.0, theme::ACCENT),
    ));
}

// Interface

fn interface_section(ui: &mut Ui, config: &mut Config) {
    let interface = &mut config.interface;
    widgets::setting_row(
        ui,
        "Interface size",
        Some("Scales the panels and buttons"),
        |ui| {
            ui.horizontal(|ui| {
                widgets::segmented(
                    ui,
                    &[
                        (0.75, "75%"),
                        (1.0, "100%"),
                        (1.25, "125%"),
                        (1.5, "150%"),
                        (2.0, "200%"),
                    ],
                    &mut interface.scale,
                );
            });
        },
    );
    widgets::toggle_row(
        ui,
        "Left-handed layout",
        Some("Moves the panels to the other side"),
        &mut interface.left_handed,
    );
    widgets::toggle_row(
        ui,
        "Hide panels while drawing",
        Some("Panels fade out as soon as a stroke starts"),
        &mut interface.hide_while_drawing,
    );
    widgets::toggle_row(ui, "Show layers at start", None, &mut interface.show_layers);
}

// Rendering

fn rendering_section(ui: &mut Ui, app: &mut InkApp) {
    let in_use = app.renderer_name();
    let rendering = &mut app.config.rendering;
    widgets::setting_row(
        ui,
        "Renderer",
        Some(
            "GPU draws pencils, marker and watercolor properly. Takes effect when inkwell restarts.",
        ),
        |ui| {
            ui.horizontal(|ui| {
                widgets::segmented(
                    ui,
                    &[(Renderer::Gpu, "GPU"), (Renderer::Cpu, "CPU")],
                    &mut rendering.renderer,
                );
            });
        },
    );
    widgets::caption(ui, format!("In use now: {in_use}"));
    widgets::divider(ui);

    ui.label(RichText::new("Texture folder").size(15.0));
    widgets::caption(
        ui,
        "Where optional stamp textures for pencils, marker, airbrush and watercolor are read from. Empty uses a textures folder next to the program, if there is one.",
    );
    folder_field(ui, &mut rendering.textures_folder, "Automatic");
}

/// A text field for a folder with Browse and Clear. Empty means "not chosen".
fn folder_field(ui: &mut Ui, folder: &mut Option<String>, hint: &str) {
    let mut text = folder.clone().unwrap_or_default();
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                TextEdit::singleline(&mut text)
                    .hint_text(hint)
                    .desired_width(ui.available_width() - 190.0),
            )
            .changed();
        #[cfg(not(target_os = "android"))]
        if ui.button("Browse").clicked()
            && let Some(picked) = rfd::FileDialog::new().pick_folder()
        {
            text = picked.display().to_string();
            changed = true;
        }
        if ui.button("Clear").clicked() {
            text.clear();
            changed = true;
        }
    });
    if changed {
        *folder = (!text.trim().is_empty()).then_some(text);
    }
}

// Files

fn files_section(ui: &mut Ui, app: &mut InkApp) {
    let files = &mut app.config.files;
    widgets::toggle_row(
        ui,
        "Save drawings automatically",
        Some("Notes in the notes folder are saved a moment after every change"),
        &mut files.autosave,
    );
    widgets::toggle_row(
        ui,
        "Start on the Library",
        Some("Off, the app opens on a blank drawing"),
        &mut files.start_in_library,
    );
    ui.label(RichText::new("Library folder").size(15.0));
    widgets::caption(
        ui,
        format!(
            "Where your drawings and folders live. Empty uses {}.",
            config::default_library_folder().map_or_else(
                || "a folder next to the app".to_owned(),
                |p| p.display().to_string()
            )
        ),
    );
    folder_field(ui, &mut files.library_folder, "Automatic");
    widgets::divider(ui);
    widgets::toggle_row(
        ui,
        "Ask before discarding changes",
        Some("A new drawing, an open or a dropped file asks twice when there is unsaved work"),
        &mut files.confirm_discard,
    );
    widgets::toggle_row(
        ui,
        "Fit the drawing when opened",
        Some("Off, the view keeps its position and zoom"),
        &mut files.fit_on_open,
    );
    widgets::divider(ui);
    ui.label(RichText::new("Settings file").size(15.0));
    widgets::caption(
        ui,
        app.config_path.as_ref().map_or_else(
            || "Settings are not being saved in this session.".to_owned(),
            |p| p.display().to_string(),
        ),
    );
    let id = Id::unique("settings_reset_armed");
    let armed = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let label = if armed {
        "Click again to reset everything"
    } else {
        "Reset all settings"
    };
    if ui.button(label).clicked() {
        if armed {
            app.reset_config();
        }
        ui.data_mut(|d| d.insert_temp(id, !armed));
    }
}

// About

fn about_section(ui: &mut Ui, app: &InkApp) {
    ui.label(RichText::new("inkwell").size(24.0).strong());
    widgets::caption(ui, format!("Version {}", env!("CARGO_PKG_VERSION")));
    ui.label(
        "A sketching app for pen and touch. Drawings are saved as .ink files, one file for each drawing.",
    );
    widgets::caption(
        ui,
        format!(
            "Settings: {}",
            app.config_path
                .as_ref()
                .map_or_else(|| "not saved".to_owned(), |p| p.display().to_string())
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_has_a_distinct_label() {
        for (i, s) in Section::ALL.iter().enumerate() {
            assert_ne!(s.label(), "");
            assert!(Section::ALL[..i].iter().all(|o| o.label() != s.label()));
        }
    }
}
