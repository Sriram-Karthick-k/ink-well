//! The tool rail: a slim strip on the edge of the screen with the pens, a small grid of tools and the ink.
//!
//! The pens are drawn as objects lying in the rail with their tips toward the paper. The one in use is pulled out
//! of the rail; tapping it again opens its size and opacity. The tools (eraser, select, slice, nudge, the hand and
//! the case with every pen) are icons in a grid under the pens, so the whole rail stays short.
//!
//! In a window taller than it is wide (a tablet held upright, a phone) the rail would cover the left edge of the page,
//! where writing starts, so it becomes a dock along the bottom: a row of pens, and a row with undo and redo, the tools
//! and the ink.

use eframe::egui::{
    Align, Align2, Area, Color32, Context, CornerRadius, Id, Layout, Margin, Order, Rect, Sense,
    Stroke, Ui, Vec2, pos2, vec2,
};
use ink_core::Pen;

use crate::app::InkApp;
use crate::brushes;
use crate::icons::Icon;
use crate::pen_art::{self, Art, Placement};
use crate::shortcuts::Command;
use crate::state::Mode;
use crate::theme;
use crate::widgets;

pub const WIDTH: f32 = 78.0;
/// How far the pen in use is pulled out of the rail.
pub const PULL: f32 = 14.0;
/// Where a resting pen's tip sits, measured from the inner edge of the rail.
const TIP_INSET: f32 = 12.0;
/// Height of everything on the rail that is not a pen row.
const FIXED_HEIGHT: f32 = 350.0;
/// Space the rail keeps free above and below.
const VERTICAL_MARGIN: f32 = 20.0;
/// The tallest and the shortest a pen row gets, as the window allows.
const PEN_ROW: (f32, f32) = (38.0, 22.0);
/// How many recent colors sit under the color well.
const RECENT_SHOWN: usize = 3;
/// The size of a tool button and the gap between them.
const TOOL_BUTTON: f32 = 30.0;
const TOOL_GAP: f32 = 6.0;

/// How far the dock is from the edges of the window.
pub const DOCK_GAP: f32 = 10.0;
/// The most the dock is wide.
const DOCK_MAX_WIDTH: f32 = 560.0;
/// The height of a row of the dock, and the space between the two rows.
const DOCK_ROW: f32 = 40.0;
const DOCK_ROW_GAP: f32 = 8.0;
/// The space between the edge of the dock and what is in it.
const DOCK_MARGIN: f32 = 10.0;
/// The height of the whole dock.
pub const DOCK_HEIGHT: f32 = 2.0 * DOCK_MARGIN + 2.0 * DOCK_ROW + DOCK_ROW_GAP;

/// The buttons of the grid under the pens.
#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Eraser,
    Select,
    Slice,
    Nudge,
    Pan,
    Shapes,
    Text,
    Brushes,
}

const TOOLS: [Tool; 8] = [
    Tool::Eraser,
    Tool::Select,
    Tool::Slice,
    Tool::Nudge,
    Tool::Pan,
    Tool::Shapes,
    Tool::Text,
    Tool::Brushes,
];

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Self::Eraser => "Eraser",
            Self::Select => "Select",
            Self::Slice => "Slice",
            Self::Nudge => "Nudge",
            Self::Pan => "Move the canvas",
            Self::Shapes => "Shapes",
            Self::Text => "Text",
            Self::Brushes => "All pens",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Eraser => Icon::Eraser,
            Self::Select => Icon::Select,
            Self::Slice => Icon::Slice,
            Self::Nudge => Icon::Nudge,
            Self::Pan => Icon::Hand,
            Self::Shapes => Icon::Shapes,
            Self::Text => Icon::Text,
            Self::Brushes => Icon::Brushes,
        }
    }

    /// What using this tool puts the canvas in, for the tools that are one.
    fn mode(self) -> Option<Mode> {
        match self {
            Self::Eraser => Some(Mode::Erase),
            Self::Select => Some(Mode::Select),
            Self::Slice => Some(Mode::Slice),
            Self::Nudge => Some(Mode::Nudge),
            Self::Pan => Some(Mode::Pan),
            Self::Shapes | Self::Text | Self::Brushes => None,
        }
    }

    /// The command that picks this tool, for the key shown in its tooltip.
    fn command(self) -> Command {
        match self {
            Self::Eraser => Command::ToolEraser,
            Self::Select => Command::ToolSelect,
            Self::Slice => Command::ToolSlice,
            Self::Nudge => Command::ToolNudge,
            Self::Pan => Command::ToolPan,
            Self::Shapes => Command::OpenShapes,
            Self::Text => Command::WriteText,
            Self::Brushes => Command::OpenBrushes,
        }
    }

    fn is_active(self, app: &InkApp) -> bool {
        match self {
            Self::Brushes => app.ui.case_open,
            Self::Shapes => app.ui.shapes_open,
            Self::Text => app.ui.text.is_some(),
            Self::Pan => app.tool.mode == Mode::Pan || app.pan_held,
            tool => tool.mode() == Some(app.tool.mode),
        }
    }
}

/// The x position, in points from the edge the rail is on, where flyouts start.
#[must_use]
pub fn flyout_offset() -> f32 {
    WIDTH + PULL + 14.0
}

/// `true` when the tools are along the bottom: the window is taller than it is wide, or the interface keeps them there.
#[must_use]
pub fn is_dock(ctx: &Context) -> bool {
    let screen = ctx.content_rect();
    crate::density::get(ctx).always_docked || screen.height() > screen.width()
}

/// How far the things in the bottom corners are lifted to clear the dock, or nothing when there is no dock. The dock
/// is the height the interface in use gives it.
#[must_use]
pub fn dock_lift(ctx: &Context) -> f32 {
    if is_dock(ctx) {
        crate::density::get(ctx).dock_height + DOCK_GAP
    } else {
        0.0
    }
}

pub fn show(ctx: &Context, app: &mut InkApp) {
    if is_dock(ctx) {
        show_dock(ctx, app);
        return;
    }
    app.ui.anchors.dock_top = 0.0;
    let lefty = app.config.interface.left_handed;
    let screen = ctx.content_rect();
    let pens = app.config.rail_pens();
    let row_height = ((screen.height() - 2.0 * VERTICAL_MARGIN - FIXED_HEIGHT) / pens.len() as f32)
        .clamp(PEN_ROW.1, PEN_ROW.0);
    let (anchor, radii) = if lefty {
        (Align2::RIGHT_CENTER, [22.0, 0.0, 0.0, 22.0])
    } else {
        (Align2::LEFT_CENTER, [0.0, 22.0, 22.0, 0.0])
    };
    let alpha = app.chrome_alpha;
    let area = Area::new(Id::unique("rail"))
        .anchor(anchor, vec2(0.0, 0.0))
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            theme::floating(ui, radii, Margin::ZERO, |ui| {
                ui.set_width(WIDTH);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                contents(ui, app, &pens, row_height, lefty);
            })
        });
    // The pen in use sticks out of the rail, so the rail's touchable area is wider than its body.
    let body = area.inner.response.rect;
    let reach = if lefty {
        Rect::from_min_max(pos2(body.left() - PULL, body.top()), body.max)
    } else {
        Rect::from_min_max(body.min, pos2(body.right() + PULL, body.bottom()))
    };
    app.chrome.push(reach);
}

fn contents(ui: &mut Ui, app: &mut InkApp, pens: &[Pen], row_height: f32, lefty: bool) {
    ui.add_space(12.0);
    history_buttons(ui, app);
    rule(ui);
    let mut chosen = None;
    for pen in pens {
        if pen_row(ui, app, *pen, row_height, lefty) {
            chosen = Some(*pen);
        }
    }
    if let Some(pen) = chosen {
        app.choose_pen(pen);
    }
    rule(ui);
    tool_grid(ui, app);
    rule(ui);
    color_well(ui, app);
    recent_colors(ui, app);
    ui.add_space(10.0);
}

fn history_buttons(ui: &mut Ui, app: &mut InkApp) {
    ui.horizontal(|ui| {
        ui.add_space((WIDTH - 66.0) / 2.0);
        ui.spacing_mut().item_spacing.x = 6.0;
        let (can_undo, can_redo) = (app.editor.can_undo(), app.editor.can_redo());
        let (undo_tip, redo_tip) = (
            app.keymap.hint(Command::Undo, "Undo"),
            app.keymap.hint(Command::Redo, "Redo"),
        );
        if widgets::round_button(ui, 30.0, Icon::Undo, can_undo, false)
            .on_hover_text(undo_tip)
            .clicked()
        {
            app.undo();
        }
        if widgets::round_button(ui, 30.0, Icon::Redo, can_redo, false)
            .on_hover_text(redo_tip)
            .clicked()
        {
            app.redo();
        }
    });
}

/// A short rule between groups.
fn rule(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, 10.0), Sense::hover());
    ui.painter().line_segment(
        [
            pos2(rect.left() + 18.0, rect.center().y),
            pos2(rect.right() - 18.0, rect.center().y),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(22)),
    );
}

/// One pen lying in the rail. Returns `true` when it was tapped.
fn pen_row(ui: &mut Ui, app: &mut InkApp, pen: Pen, row_height: f32, lefty: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(WIDTH, row_height), Sense::click());
    let selected = app.tool.mode == Mode::Draw && app.tool.pen == pen;
    // Only choosing a pen moves it: a pen that slid under the pointer on hover would look like it shakes, and
    // would slide back out from under it at the edge of the row.
    let target = if selected { PULL } else { 0.0 };
    let pull = ui
        .ctx()
        .animate_value_with_time(response.id.with("pull"), target, 0.16);
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    let direction = if lefty { -1.0 } else { 1.0 };
    let base = if lefty {
        rect.left() + TIP_INSET
    } else {
        rect.right() - TIP_INSET
    };
    let tip = pos2(base + direction * pull, rect.center().y);
    let [r, g, b] = app.tool.color.to_srgb();
    let painter = ui.painter().with_clip_rect(ui.ctx().content_rect());
    let scale = (row_height / 46.0 * 1.15).clamp(0.6, 1.1);
    // Each pen rests in a shallow slot in the rail.
    let slot_height = (row_height * 0.58).min(24.0);
    let (slot_from, slot_to) = if lefty {
        (rect.left() + 8.0, rect.right() + 8.0)
    } else {
        (rect.left() - 8.0, rect.right() - 8.0)
    };
    let slot = Rect::from_min_max(
        pos2(slot_from, rect.center().y - slot_height / 2.0),
        pos2(slot_to, rect.center().y + slot_height / 2.0),
    );
    // Hovering a pen lights its slot instead of moving it.
    painter.rect_filled(
        slot,
        CornerRadius::same((slot_height / 2.0) as u8),
        Color32::from_black_alpha(58).lerp_to_gamma(Color32::from_white_alpha(26), hover),
    );
    painter.line_segment(
        [
            pos2(slot.left() + slot_height / 2.0, slot.bottom() + 0.5),
            pos2(slot.right() - slot_height / 2.0, slot.bottom() + 0.5),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(16)),
    );
    pen_art::paint(
        &painter,
        Art::of(pen),
        Placement {
            tip,
            scale,
            direction,
            lift: pull / PULL,
        },
        Color32::from_rgb(r, g, b),
    );
    if selected {
        app.ui.anchors.pen = rect.center().y;
        let x = if lefty {
            rect.left() + 2.0
        } else {
            rect.right() - 5.0
        };
        painter.rect_filled(
            Rect::from_center_size(pos2(x + 1.5, rect.center().y), vec2(3.0, 16.0)),
            CornerRadius::same(2),
            theme::ACCENT,
        );
    }
    response.on_hover_text(brushes::info(pen).name).clicked()
}

/// The tools, two to a row: what each does is in its tooltip, with its key.
fn tool_grid(ui: &mut Ui, app: &mut InkApp) {
    ui.add_space(2.0);
    for pair in TOOLS.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = TOOL_GAP;
            let across = pair.len() as f32 * TOOL_BUTTON + (pair.len() - 1) as f32 * TOOL_GAP;
            ui.add_space((WIDTH - across) / 2.0);
            for tool in pair {
                tool_button(ui, app, *tool, TOOL_BUTTON);
            }
        });
        ui.add_space(TOOL_GAP);
    }
}

fn tool_button(ui: &mut Ui, app: &mut InkApp, tool: Tool, size: f32) {
    let active = tool.is_active(app);
    let response = widgets::tool_button(ui, size, tool.icon(), active);
    if active {
        match tool {
            Tool::Brushes => app.ui.anchors.case = response.rect.center().y,
            Tool::Shapes => app.ui.anchors.shapes = response.rect.center().y,
            _ => app.ui.anchors.pen = response.rect.center().y,
        }
    }
    let tooltip = app.keymap.hint(tool.command(), tool.name());
    if !response.on_hover_text(tooltip).clicked() {
        return;
    }
    match tool.mode() {
        // Pressing the tool in use again opens its options.
        Some(mode) => app.choose_tool(mode),
        None if tool == Tool::Shapes => app.ui.toggle_shapes(),
        None if tool == Tool::Text => app.toggle_text(&ui.ctx().clone()),
        None => app.ui.toggle_case(),
    }
}

/// The ink: a glossy well showing the current color. Opens the color picker.
fn color_well(ui: &mut Ui, app: &mut InkApp) {
    color_well_sized(ui, app, vec2(WIDTH, 50.0), 16.0);
}

/// The well in a box of `size`, with a color disc of `radius`.
fn color_well_sized(ui: &mut Ui, app: &mut InkApp, size: Vec2, radius: f32) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    app.ui.anchors.color = rect.center().y;
    let open = app.ui.color_open;
    let grow = ui
        .ctx()
        .animate_bool_with_time(response.id.with("grow"), response.hovered(), 0.12);
    let center = rect.center();
    // The well keeps its size on hover and its ring brightens, so it does not twitch under the pointer.
    let [r, g, b] = app.tool.color.to_srgb();
    let painter = ui.painter();
    painter.circle_filled(
        center + vec2(0.0, 2.0),
        radius + 3.0,
        Color32::from_black_alpha(100),
    );
    painter.circle_filled(
        center,
        radius + 3.0,
        if open {
            theme::ACCENT
        } else {
            Color32::from_rgb(0xB8, 0xB3, 0xA8)
                .lerp_to_gamma(Color32::from_rgb(0xF2, 0xEE, 0xE4), grow)
        },
    );
    painter.circle_filled(center, radius, Color32::from_rgb(r, g, b));
    painter.circle_stroke(
        center,
        radius,
        Stroke::new(1.0, Color32::from_black_alpha(70)),
    );
    painter.circle_filled(
        center + vec2(-radius * 0.34, -radius * 0.4),
        radius * 0.28,
        Color32::from_white_alpha(64),
    );
    if response.on_hover_text("Color").clicked() {
        app.ui.toggle_color();
    }
}

fn recent_colors(ui: &mut Ui, app: &mut InkApp) {
    let current = app.tool.color.to_srgb();
    let recent: Vec<[u8; 3]> = app
        .config
        .colors
        .recent
        .iter()
        .copied()
        .filter(|c| *c != current)
        .take(RECENT_SHOWN)
        .collect();
    let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, 22.0), Sense::hover());
    let n = recent.len() as f32;
    for (i, rgb) in recent.into_iter().enumerate() {
        let center = pos2(
            rect.center().x + (i as f32 - (n - 1.0) / 2.0) * 20.0,
            rect.center().y,
        );
        let hit = Rect::from_center_size(center, vec2(20.0, 20.0));
        let response = ui.interact(hit, Id::unique(("recent", i)), Sense::click());
        let radius = 7.0;
        ui.painter()
            .circle_filled(center, radius, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        ui.painter().circle_stroke(
            center,
            radius,
            Stroke::new(
                1.0,
                Color32::from_white_alpha(if response.hovered() { 140 } else { 46 }),
            ),
        );
        if response.clicked() {
            app.set_color_rgb(rgb);
        }
    }
}

// --- the dock -------------------------------------------------------------------------------------------------

/// The rail as a dock along the bottom of the window.
fn show_dock(ctx: &Context, app: &mut InkApp) {
    let screen = ctx.content_rect();
    let pens = app.config.rail_pens();
    let width = (screen.width() - 2.0 * DOCK_GAP).min(DOCK_MAX_WIDTH);
    // Placed from its size, which is known, so it is never anywhere else for a frame.
    let top_left = pos2(
        screen.center().x - width / 2.0,
        screen.bottom() - DOCK_GAP - DOCK_HEIGHT,
    );
    app.ui.anchors.dock_top = top_left.y;
    let alpha = app.chrome_alpha;
    let inner = width - 2.0 * DOCK_MARGIN;
    let area = Area::new(Id::unique("dock"))
        .fixed_pos(top_left)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            theme::floating(ui, [22.0; 4], Margin::same(DOCK_MARGIN as i8), |ui| {
                ui.set_width(inner);
                ui.set_height(DOCK_HEIGHT - 2.0 * DOCK_MARGIN);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                dock_contents(ui, app, &pens, inner);
            })
        });
    app.chrome.push(area.inner.response.rect);
}

fn dock_contents(ui: &mut Ui, app: &mut InkApp, pens: &[Pen], inner: f32) {
    let gap = 6.0;
    // The pens, each in a slot with its tip showing, as many as there are across the width.
    let n = pens.len().max(1) as f32;
    let slot = ((inner - gap * (n - 1.0)) / n).clamp(40.0, 72.0);
    let row = n * slot + gap * (n - 1.0);
    let mut chosen = None;
    ui.allocate_ui_with_layout(
        vec2(inner, DOCK_ROW),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.add_space(((inner - row) / 2.0).max(0.0));
            for pen in pens {
                if pen_slot(ui, app, *pen, vec2(slot, DOCK_ROW)) {
                    chosen = Some(*pen);
                }
            }
        },
    );
    if let Some(pen) = chosen {
        app.choose_pen(pen);
    }
    ui.add_space(DOCK_ROW_GAP);
    // Undo and redo, the tools and the ink, sized to fit the width.
    let items = (TOOLS.len() + 3) as f32;
    let button = ((inner - 64.0) / items).clamp(26.0, 34.0);
    let row = items * button + 8.0 + 4.0 * (items - 1.0) + 2.0 * 12.0;
    ui.allocate_ui_with_layout(
        vec2(inner, DOCK_ROW),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_space(((inner - row) / 2.0).max(0.0));
            dock_history(ui, app, button);
            ui.add_space(12.0);
            for tool in TOOLS {
                tool_button(ui, app, tool, button);
            }
            ui.add_space(12.0);
            color_well_sized(ui, app, vec2(button + 8.0, DOCK_ROW), button * 0.42);
        },
    );
}

fn dock_history(ui: &mut Ui, app: &mut InkApp, size: f32) {
    let (can_undo, can_redo) = (app.editor.can_undo(), app.editor.can_redo());
    let (undo_tip, redo_tip) = (
        app.keymap.hint(Command::Undo, "Undo"),
        app.keymap.hint(Command::Redo, "Redo"),
    );
    if widgets::round_button(ui, size, Icon::Undo, can_undo, false)
        .on_hover_text(undo_tip)
        .clicked()
    {
        app.undo();
    }
    if widgets::round_button(ui, size, Icon::Redo, can_redo, false)
        .on_hover_text(redo_tip)
        .clicked()
    {
        app.redo();
    }
}

/// One pen of the dock: the front of the pen lying in a slot with its tip to the right. The one in use is lifted and
/// ringed. Returns `true` when it was tapped.
fn pen_slot(ui: &mut Ui, app: &mut InkApp, pen: Pen, size: Vec2) -> bool {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let selected = app.tool.mode == Mode::Draw && app.tool.pen == pen;
    // Only choosing a pen moves it: a pen that moved on hover would look like it shakes.
    let lift = ui.ctx().animate_value_with_time(
        response.id.with("lift"),
        if selected { 1.0 } else { 0.0 },
        0.16,
    );
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    let painter = ui.painter().with_clip_rect(rect);
    let slot = rect.shrink2(vec2(0.0, 5.0));
    painter.rect_filled(
        slot,
        CornerRadius::same(((slot.height() / 2.0) as u8).min(20)),
        Color32::from_black_alpha(58).lerp_to_gamma(Color32::from_white_alpha(26), hover),
    );
    if lift > 0.01 {
        painter.rect_stroke(
            slot,
            CornerRadius::same(((slot.height() / 2.0) as u8).min(20)),
            Stroke::new(1.5, theme::ACCENT.gamma_multiply(lift)),
            eframe::egui::StrokeKind::Inside,
        );
    }
    let [r, g, b] = app.tool.color.to_srgb();
    pen_art::paint(
        &painter,
        Art::of(pen),
        Placement {
            tip: pos2(rect.right() - 8.0, rect.center().y - 2.0 * lift),
            scale: (size.y / 46.0 * 1.15).clamp(0.6, 1.1),
            direction: 1.0,
            lift,
        },
        Color32::from_rgb(r, g, b),
    );
    response.on_hover_text(brushes::info(pen).name).clicked()
}
