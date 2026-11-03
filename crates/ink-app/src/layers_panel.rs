//! The layers panel: a stack of rows with a thumbnail each, drag to reorder, and under the stack the opacity of
//! the layer being drawn on with buttons to copy or delete it.
//!
//! Click a row to draw on that layer, double-click its name to rename it, drag a row to move it up or down the
//! stack, press the eye to hide it. Right-click or press and hold a row for the same things as a menu. Adding,
//! copying, deleting and moving are all undone with the undo button.

use eframe::egui::{
    self, Align, Align2, Area, Color32, Context, CornerRadius, CursorIcon, FontId, Id, Key, Layout,
    Margin, Order, Pos2, Rect, RichText, ScrollArea, Sense, Slider, Stroke, TextEdit, Ui, pos2,
    vec2,
};
use ink_core::Editor;
use ink_core::transform::bounds_of;
use uuid::Uuid;

use crate::app::InkApp;
use crate::canvas::parse_hex;
use crate::icons::{self, Icon};
use crate::shortcuts::Command;
use crate::tab_strip::BAR_HEIGHT;
use crate::theme;
use crate::widgets;

const WIDTH: f32 = 276.0;
const ROW_HEIGHT: f32 = 58.0;
const THUMB: egui::Vec2 = vec2(54.0, 42.0);
/// How many strokes of a layer, and how many points of each, go into its thumbnail.
const THUMB_STROKES: usize = 160;
const THUMB_POINTS: usize = 24;

/// What one layer row shows. Copied out of the document so the list can be drawn while it is edited.
struct Row {
    index: usize,
    id: Uuid,
    name: String,
    visible: bool,
    opacity: f32,
    strokes: usize,
}

/// Something the user did in the panel, applied after the list is drawn.
enum Action {
    Select(usize),
    ToggleVisible(usize),
    SetOpacity(usize, f32),
    StartRename(usize, String),
    FinishRename(usize, String),
    CancelRename,
    Add,
    Duplicate(usize),
    Delete(usize),
    Move(usize, usize),
}

/// A line of a thumbnail, in canvas coordinates.
#[derive(Clone)]
struct Sketch {
    color: Color32,
    width: f32,
    points: Vec<Pos2>,
}

/// What the thumbnails draw, kept between frames and rebuilt when the drawing changes.
#[derive(Clone)]
struct Thumbs {
    /// The drawing and its revision this was made from.
    key: (u64, u64),
    /// Left, bottom, right and top of everything, so every thumbnail shows the same part of the paper.
    bounds: [f32; 4],
    layers: Vec<(Uuid, Vec<Sketch>)>,
}

fn build_thumbs(editor: &Editor, key: (u64, u64)) -> Thumbs {
    let document = editor.document();
    let bounds = bounds_of(document.layers.iter().flat_map(|l| l.strokes.iter()))
        .unwrap_or([-120.0, -90.0, 120.0, 90.0]);
    let layers = document
        .layers
        .iter()
        .map(|layer| {
            let every = layer.strokes.len().div_ceil(THUMB_STROKES).max(1);
            let sketches = layer
                .strokes
                .iter()
                .step_by(every)
                .map(|s| {
                    let geometry = s.geometry();
                    let thin = geometry.len().div_ceil(THUMB_POINTS).max(1);
                    let mut points: Vec<Pos2> = geometry
                        .iter()
                        .step_by(thin)
                        .map(|p| pos2(p.x, p.y))
                        .collect();
                    if let Some(last) = geometry.last() {
                        points.push(pos2(last.x, last.y));
                    }
                    let c = s.color;
                    Sketch {
                        color: Color32::from_rgba_unmultiplied(
                            (c.r * 255.0) as u8,
                            (c.g * 255.0) as u8,
                            (c.b * 255.0) as u8,
                            (c.a * 255.0) as u8,
                        ),
                        width: s.width.unwrap_or(2.0),
                        points,
                    }
                })
                .collect();
            (layer.id, sketches)
        })
        .collect();
    Thumbs {
        key,
        bounds,
        layers,
    }
}

/// The thumbnails for the drawing as it is now, from the cache when nothing has changed.
fn thumbs(ctx: &Context, app: &InkApp) -> Thumbs {
    let id = Id::unique("layer_thumbs");
    let key = (app.doc_id, app.editor.revision());
    if let Some(cached) = ctx.data(|d| d.get_temp::<Thumbs>(id))
        && cached.key == key
    {
        return cached;
    }
    let fresh = build_thumbs(&app.editor, key);
    ctx.data_mut(|d| d.insert_temp(id, fresh.clone()));
    fresh
}

fn paint_thumb(
    painter: &egui::Painter,
    rect: Rect,
    paper: Color32,
    thumbs: &Thumbs,
    id: Uuid,
    dim: f32,
) {
    painter.rect_filled(
        rect,
        CornerRadius::same(7),
        paper.gamma_multiply(dim.max(0.5)),
    );
    let Some((_, sketches)) = thumbs.layers.iter().find(|(layer, _)| *layer == id) else {
        return;
    };
    let [x0, y0, x1, y1] = thumbs.bounds;
    let inner = rect.shrink(4.0);
    let scale = (inner.width() / (x1 - x0).max(1.0)).min(inner.height() / (y1 - y0).max(1.0));
    let (mid_x, mid_y) = (f32::midpoint(x0, x1), f32::midpoint(y0, y1));
    let map = |p: Pos2| {
        pos2(
            inner.center().x + (p.x - mid_x) * scale,
            inner.center().y - (p.y - mid_y) * scale,
        )
    };
    let painter = painter.with_clip_rect(rect);
    for sketch in sketches {
        let width = (sketch.width * scale).clamp(0.8, 2.6);
        let color = sketch.color.gamma_multiply(dim);
        if sketch.points.len() == 1 {
            painter.circle_filled(map(sketch.points[0]), width / 2.0, color);
        } else {
            painter.add(egui::Shape::line(
                sketch.points.iter().map(|p| map(*p)).collect(),
                Stroke::new(width, color),
            ));
        }
    }
    painter.rect_stroke(
        rect,
        CornerRadius::same(7),
        Stroke::new(1.0, Color32::from_black_alpha(46)),
        egui::StrokeKind::Inside,
    );
}

pub fn show(ctx: &Context, app: &mut InkApp) {
    if !app.ui.layers_open {
        return;
    }
    let rows: Vec<Row> = app
        .editor
        .content_layers()
        .map(|(index, l)| Row {
            index,
            id: l.id,
            name: l.display_name(),
            visible: l.visible,
            opacity: l.opacity,
            strokes: l.strokes.len(),
        })
        .collect();
    let thumbs = thumbs(ctx, app);
    let paper = parse_hex(&app.editor.document().metadata.background_solid_color);
    let mut actions = Vec::new();

    let (anchor, offset) = if app.config.interface.left_handed {
        (Align2::LEFT_TOP, vec2(16.0, BAR_HEIGHT + 14.0))
    } else {
        (Align2::RIGHT_TOP, vec2(-16.0, BAR_HEIGHT + 14.0))
    };
    let room = (ctx.content_rect().height() - BAR_HEIGHT - 14.0 - 250.0).clamp(ROW_HEIGHT, 460.0);
    let alpha = app.chrome_alpha;
    let area = Area::new(Id::unique("layers"))
        .anchor(anchor, offset)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            theme::floating(ui, [22.0; 4], Margin::same(12), |ui| {
                ui.set_width(WIDTH);
                header(ui, app, rows.len(), &mut actions);
                ui.add_space(6.0);
                ScrollArea::vertical()
                    .max_height(room)
                    .auto_shrink([false, true])
                    .show(ui, |ui| stack(ui, app, &rows, &thumbs, paper, &mut actions));
                ui.add_space(8.0);
                widgets::divider(ui);
                ui.add_space(8.0);
                footer(ui, app, &rows, &mut actions);
            })
        });
    app.chrome.push(area.inner.response.rect);

    for action in actions {
        app.apply_layer_action(action);
    }
}

fn header(ui: &mut Ui, app: &InkApp, count: usize, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Layers").strong().size(16.0));
        ui.label(
            RichText::new(count.to_string())
                .size(13.0)
                .color(theme::TEXT_DIM),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let tip = app
                .keymap
                .hint(Command::NewLayer, "New layer above this one");
            if widgets::round_button(ui, 32.0, Icon::Plus, true, false)
                .on_hover_text(tip)
                .clicked()
            {
                actions.push(Action::Add);
            }
        });
    });
}

/// The rows, top layer first, with the line that shows where a dragged layer would land.
fn stack(
    ui: &mut Ui,
    app: &mut InkApp,
    rows: &[Row],
    thumbs: &Thumbs,
    paper: Color32,
    actions: &mut Vec<Action>,
) {
    let drag_id = Id::unique("layer_drag");
    let dragging: Option<usize> = ui.data(|d| d.get_temp(drag_id));
    let mut rects: Vec<(usize, Rect)> = Vec::with_capacity(rows.len());
    let mut dropped = None;
    ui.spacing_mut().item_spacing.y = 4.0;
    // The top layer is last in the document and first in the list.
    for row in rows.iter().rev() {
        let (rect, response) = row_ui(ui, row, app, thumbs, paper, dragging, actions);
        rects.push((row.index, rect));
        if response.drag_started() {
            ui.data_mut(|d| d.insert_temp(drag_id, row.index));
        }
        if response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
        if response.drag_stopped() {
            dropped = Some(row.index);
        }
    }
    if let Some(from) = dropped {
        ui.data_mut(|d| d.remove_temp::<usize>(drag_id));
        if let Some(target) = drop_target(ui, &rects, rows, from) {
            actions.push(Action::Move(from, target));
        }
    }
    if let Some(from) = dragging
        && let Some(target) = drop_target(ui, &rects, rows, from)
        && let Some((_, rect)) = rects.iter().find(|(i, _)| *i == target)
    {
        // Moving up the stack lands above the row under the pointer, moving down lands below it.
        let y = if target > from {
            rect.top() - 2.0
        } else {
            rect.bottom() + 2.0
        };
        ui.painter().rect_filled(
            Rect::from_min_max(
                pos2(rect.left() + 6.0, y - 1.5),
                pos2(rect.right() - 6.0, y + 1.5),
            ),
            CornerRadius::same(2),
            theme::ACCENT,
        );
    }
}

/// The place in the document a layer dragged from `from` would go to, if the pointer is over another row.
fn drop_target(ui: &Ui, rects: &[(usize, Rect)], rows: &[Row], from: usize) -> Option<usize> {
    let at = ui.ctx().pointer_latest_pos()?;
    rects
        .iter()
        .find(|(_, rect)| rect.x_range().contains(at.x) && rect.y_range().contains(at.y))
        .map(|(index, _)| *index)
        .or_else(|| {
            // Past either end of the list, the layer goes all the way to that end.
            let first = rects.first()?.1;
            let last = rects.last()?.1;
            if at.y < first.top() {
                rows.last().map(|r| r.index)
            } else if at.y > last.bottom() {
                rows.first().map(|r| r.index)
            } else {
                None
            }
        })
        .filter(|target| *target != from)
}

#[allow(
    clippy::too_many_arguments,
    reason = "everything one row needs to draw itself"
)]
fn row_ui(
    ui: &mut Ui,
    row: &Row,
    app: &mut InkApp,
    thumbs: &Thumbs,
    paper: Color32,
    dragging: Option<usize>,
    actions: &mut Vec<Action>,
) -> (Rect, egui::Response) {
    let selected = row.index == app.active_layer;
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), ROW_HEIGHT),
        Sense::click_and_drag(),
    );
    let lifted = dragging == Some(row.index);
    let painter = ui.painter();
    if selected {
        painter.rect_filled(
            rect,
            CornerRadius::same(14),
            theme::ACCENT.gamma_multiply(0.15),
        );
        painter.rect_stroke(
            rect,
            CornerRadius::same(14),
            Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.7)),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() && dragging.is_none() {
        painter.rect_filled(rect, CornerRadius::same(14), theme::CHROME_HI);
    }
    let fade = if lifted {
        0.45
    } else if row.visible {
        1.0
    } else {
        0.5
    };
    let thumb = Rect::from_min_size(
        pos2(rect.left() + 8.0, rect.center().y - THUMB.y / 2.0),
        THUMB,
    );
    paint_thumb(painter, thumb, paper, thumbs, row.id, fade);
    if (response.hovered() || lifted) && !selected {
        // Six dots at the left edge: the row can be picked up and moved.
        for (dx, dy) in [
            (0.0, -1.0),
            (0.0, 0.0),
            (0.0, 1.0),
            (1.0, -1.0),
            (1.0, 0.0),
            (1.0, 1.0),
        ] {
            painter.circle_filled(
                pos2(rect.left() + 2.5 + dx * 3.2, rect.center().y + dy * 5.0),
                0.9,
                theme::TEXT_DIM,
            );
        }
    }

    let text_left = thumb.right() + 10.0;
    let eye = Rect::from_center_size(pos2(rect.right() - 24.0, rect.center().y), vec2(34.0, 34.0));
    let name_rect = Rect::from_min_max(
        pos2(text_left, rect.top() + 9.0),
        pos2(eye.left() - 4.0, rect.top() + 29.0),
    );
    let note_at = pos2(text_left, rect.top() + 36.0);
    let renaming = row_text(ui, row, app, name_rect, note_at, fade, actions);
    eye_button(ui, row, eye, response.id, actions);

    if !renaming {
        if response.double_clicked() {
            actions.push(Action::StartRename(row.index, row.name.clone()));
        } else if response.clicked() && !eye_response_hit(ui, eye) {
            actions.push(Action::Select(row.index));
        }
    }
    response.context_menu(|ui| menu(ui, app, row, actions));
    (rect, response)
}

/// The name of a row (or the field it is being renamed in) and the line under it. Returns `true` while the name
/// is being edited.
fn row_text(
    ui: &mut Ui,
    row: &Row,
    app: &mut InkApp,
    name_rect: Rect,
    note_at: Pos2,
    fade: f32,
    actions: &mut Vec<Action>,
) -> bool {
    let renaming = matches!(&app.ui.renaming, Some((i, _)) if *i == row.index);
    if renaming {
        if let Some((_, text)) = &mut app.ui.renaming {
            let edit = ui.put(
                name_rect.expand2(vec2(0.0, 2.0)),
                TextEdit::singleline(text),
            );
            edit.request_focus();
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                actions.push(Action::CancelRename);
            } else if edit.lost_focus() {
                actions.push(Action::FinishRename(row.index, text.clone()));
            }
        }
    } else {
        let name = widgets::elided(
            ui.painter(),
            &row.name,
            FontId::proportional(14.5),
            theme::TEXT.gamma_multiply(fade),
            name_rect.width(),
        );
        ui.painter().galley(name_rect.left_top(), name, theme::TEXT);
    }
    let note = match (row.strokes, row.opacity < 0.995) {
        (0, false) => "Empty".to_owned(),
        (0, true) => format!("Empty · {:.0}%", row.opacity * 100.0),
        (1, false) => "1 stroke".to_owned(),
        (n, false) => format!("{n} strokes"),
        (1, true) => format!("1 stroke · {:.0}%", row.opacity * 100.0),
        (n, true) => format!("{n} strokes · {:.0}%", row.opacity * 100.0),
    };
    ui.painter().text(
        note_at,
        Align2::LEFT_CENTER,
        note,
        FontId::proportional(12.0),
        theme::TEXT_DIM.gamma_multiply(fade.max(0.7)),
    );
    renaming
}

/// The eye at the right of a row, which hides or shows the layer.
fn eye_button(ui: &mut Ui, row: &Row, eye: Rect, row_id: Id, actions: &mut Vec<Action>) {
    let response = ui.interact(eye, row_id.with("eye"), Sense::click());
    let color = if row.visible {
        theme::TEXT
    } else {
        theme::TEXT_DIM
    };
    if response.hovered() {
        ui.painter()
            .circle_filled(eye.center(), 16.0, Color32::from_white_alpha(14));
    }
    icons::paint(
        ui.painter(),
        eye.shrink(8.0),
        if row.visible { Icon::Eye } else { Icon::EyeOff },
        color,
    );
    let tip = if row.visible {
        "Hide the layer"
    } else {
        "Show the layer"
    };
    if response.on_hover_text(tip).clicked() {
        actions.push(Action::ToggleVisible(row.index));
    }
}

/// `true` when the pointer is on the eye, so a click there does not also pick the row.
fn eye_response_hit(ui: &Ui, eye: Rect) -> bool {
    ui.ctx()
        .pointer_latest_pos()
        .is_some_and(|p| eye.contains(p))
}

/// The menu of a row.
fn menu(ui: &mut Ui, app: &InkApp, row: &Row, actions: &mut Vec<Action>) {
    ui.set_min_width(190.0);
    let last = app.editor.content_layers().count() <= 1;
    let top = row.index + 1 >= app.editor.document().layers.len();
    let bottom = row.index == 0;
    let entries: [(&str, bool, Action); 5] = [
        (
            "Rename",
            true,
            Action::StartRename(row.index, row.name.clone()),
        ),
        ("Duplicate", true, Action::Duplicate(row.index)),
        ("Move up", !top, Action::Move(row.index, row.index + 1)),
        (
            "Move down",
            !bottom,
            Action::Move(row.index, row.index.saturating_sub(1)),
        ),
        ("Delete", !last, Action::Delete(row.index)),
    ];
    for (label, enabled, action) in entries {
        if ui
            .add_enabled(enabled, egui::Button::new(label).frame(false))
            .clicked()
        {
            actions.push(action);
            ui.close();
        }
    }
}

/// The opacity of the layer being drawn on, and what can be done with the layer as a whole.
fn footer(ui: &mut Ui, app: &InkApp, rows: &[Row], actions: &mut Vec<Action>) {
    let Some(active) = rows.iter().find(|r| r.index == app.active_layer) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Opacity").size(13.0).color(theme::TEXT_DIM));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{:.0}%", active.opacity * 100.0))
                    .size(13.0)
                    .monospace(),
            );
            ui.spacing_mut().slider_width = ui.available_width() - 6.0;
            let mut opacity = active.opacity;
            if widgets::slider(ui, Slider::new(&mut opacity, 0.0..=1.0).show_value(false)).changed()
            {
                actions.push(Action::SetOpacity(active.index, opacity));
            }
        });
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let half = (ui.available_width() - 8.0) / 2.0;
        let copy = app.keymap.hint(Command::DuplicateLayer, "Copy this layer");
        if action_button(ui, half, Icon::Duplicate, "Duplicate", true, &copy) {
            actions.push(Action::Duplicate(active.index));
        }
        let can_delete = rows.len() > 1;
        let tip = if can_delete {
            "Delete this layer. Undo brings it back."
        } else {
            "A drawing needs at least one layer"
        };
        if action_button(ui, half, Icon::Trash, "Delete", can_delete, tip) {
            actions.push(Action::Delete(active.index));
        }
    });
}

/// A wide button with an icon and a word. Returns `true` when it was pressed.
fn action_button(
    ui: &mut Ui,
    width: f32,
    icon: Icon,
    label: &str,
    enabled: bool,
    tooltip: &str,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
    let fill = if enabled && response.hovered() {
        theme::CHROME_HOVER
    } else {
        theme::CHROME_HI
    };
    let color = if enabled {
        theme::TEXT
    } else {
        theme::TEXT_DIM.gamma_multiply(0.5)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(18.0, 18.0)),
        icon,
        color,
    );
    ui.painter().text(
        pos2(rect.left() + 38.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.0),
        color,
    );
    let response = response.on_hover_text(tooltip);
    enabled && response.clicked()
}

/// The index of the nearest content layer in `order` (an iterator over document indices), if any.
fn next_content(app: &InkApp, order: impl Iterator<Item = usize>) -> Option<usize> {
    let mut order = order;
    order.find(|&i| {
        app.editor
            .document()
            .layers
            .get(i)
            .is_some_and(|l| l.content)
    })
}

impl InkApp {
    /// A new empty layer above the one being drawn on, which then becomes the one drawn on.
    pub(crate) fn new_layer(&mut self) {
        self.active_layer = self.editor.add_layer(Some(self.active_layer));
    }

    /// A copy of the layer being drawn on, above it.
    pub(crate) fn duplicate_active_layer(&mut self) {
        if let Some(copy) = self.editor.duplicate_layer(self.active_layer) {
            self.active_layer = copy;
        }
    }

    /// Moves the layer being drawn on one place up the stack (`up`) or down it.
    pub(crate) fn step_active_layer(&mut self, up: bool) {
        let active = self.active_layer;
        let len = self.editor.document().layers.len();
        let target = if up {
            next_content(self, active + 1..len)
        } else {
            next_content(self, (0..active).rev())
        };
        if let Some(to) = target {
            self.apply_layer_action(Action::Move(active, to));
        }
    }

    /// After an undo or redo that may have added or taken away layers: draws on a layer that exists.
    pub(crate) fn fix_active_layer(&mut self) {
        let exists = |app: &Self, i: usize| {
            app.editor
                .document()
                .layers
                .get(i)
                .is_some_and(|l| l.content)
        };
        if !exists(self, self.active_layer) {
            let len = self.editor.document().layers.len();
            self.active_layer = next_content(self, (0..len).rev()).unwrap_or(0);
        }
    }

    fn apply_layer_action(&mut self, action: Action) {
        let active = self.active_layer;
        match action {
            Action::Select(i) => self.active_layer = i,
            Action::ToggleVisible(i) => {
                let visible = self
                    .editor
                    .document()
                    .layers
                    .get(i)
                    .is_some_and(|l| l.visible);
                self.editor.set_layer_visible(i, !visible);
            }
            Action::SetOpacity(i, value) => self.editor.set_layer_opacity(i, value),
            Action::StartRename(i, name) => self.ui.renaming = Some((i, name)),
            Action::FinishRename(i, name) => {
                if !name.trim().is_empty() {
                    self.editor.rename_layer(i, name.trim().to_owned());
                }
                self.ui.renaming = None;
            }
            Action::CancelRename => self.ui.renaming = None,
            Action::Add => self.new_layer(),
            Action::Duplicate(i) => {
                if let Some(copy) = self.editor.duplicate_layer(i) {
                    self.active_layer = copy;
                }
            }
            Action::Delete(i) => {
                if self.editor.remove_layer(i) {
                    let below = next_content(self, (0..i).rev());
                    self.active_layer = below.or_else(|| next_content(self, 0..)).unwrap_or(0);
                }
            }
            Action::Move(from, to) => {
                if from != to && self.editor.move_layer(from, to) {
                    // The layers between the two places shift by one; the one drawn on stays the same layer.
                    self.active_layer = if active == from {
                        to
                    } else if from < to && (from + 1..=to).contains(&active) {
                        active - 1
                    } else if to < from && (to..from).contains(&active) {
                        active + 1
                    } else {
                        active
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ink_core::{Color, IdGen, Pen, Point, StrokeBuilder};

    use super::*;
    use crate::Options;

    fn ids(app: &InkApp) -> Vec<Uuid> {
        app.editor.document().layers.iter().map(|l| l.id).collect()
    }

    #[test]
    fn the_layer_drawn_on_stays_the_same_layer_when_others_are_moved_past_it() {
        let mut app = InkApp::blank(Options::default());
        app.new_layer();
        app.new_layer();
        app.active_layer = 1;
        let drawn_on = ids(&app)[1];
        app.apply_layer_action(Action::Move(0, 2));
        assert_eq!(
            ids(&app)[app.active_layer],
            drawn_on,
            "moved from below to above it"
        );
        app.apply_layer_action(Action::Move(2, 0));
        assert_eq!(ids(&app)[app.active_layer], drawn_on, "and back");
        let moved = ids(&app)[2];
        app.active_layer = 2;
        app.apply_layer_action(Action::Move(2, 0));
        assert_eq!(app.active_layer, 0);
        assert_eq!(
            ids(&app)[0],
            moved,
            "dragging the layer drawn on takes it along"
        );
    }

    #[test]
    fn deleting_a_layer_hands_drawing_to_the_one_below_and_undo_brings_it_back() {
        let mut app = InkApp::blank(Options::default());
        app.new_layer();
        let top = ids(&app)[1];
        app.apply_layer_action(Action::Delete(1));
        assert_eq!(app.active_layer, 0);
        assert_eq!(app.editor.document().layers.len(), 1);
        assert!(app.undo());
        assert_eq!(ids(&app)[1], top);
        app.active_layer = 1;
        assert!(app.undo(), "undo the new layer too");
        assert_eq!(app.active_layer, 0, "no longer pointing past the end");
        app.apply_layer_action(Action::Delete(0));
        assert_eq!(
            app.editor.document().layers.len(),
            1,
            "the last layer stays"
        );
    }

    #[test]
    fn a_copy_becomes_the_layer_drawn_on_and_a_blank_name_does_not_rename() {
        let mut app = InkApp::blank(Options::default());
        app.apply_layer_action(Action::Duplicate(0));
        assert_eq!(app.active_layer, 1);
        let before = app.editor.document().layers[0].display_name();
        app.apply_layer_action(Action::FinishRename(0, "   ".to_owned()));
        assert_eq!(app.editor.document().layers[0].display_name(), before);
        app.apply_layer_action(Action::FinishRename(0, " Ink ".to_owned()));
        assert_eq!(app.editor.document().layers[0].display_name(), "Ink");
    }

    #[test]
    fn stepping_a_layer_up_and_down_stops_at_the_ends() {
        let mut app = InkApp::blank(Options::default());
        app.new_layer();
        app.step_active_layer(true);
        assert_eq!(app.active_layer, 1, "already on top");
        app.step_active_layer(false);
        assert_eq!(app.active_layer, 0);
        app.step_active_layer(false);
        assert_eq!(app.active_layer, 0, "already at the bottom");
    }

    fn editor_with_a_stroke() -> Editor {
        let mut editor = Editor::blank(IdGen::new(1));
        let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.0)
            .with_id(editor.next_id());
        for i in 0..40 {
            b.push(
                Point::new(i as f32 * 5.0, (i as f32).sin() * 20.0),
                None,
                f64::from(i) * 0.01,
            );
        }
        editor.commit_stroke(0, b.finish());
        editor
    }

    #[test]
    fn a_thumbnail_is_a_thinned_copy_of_the_layer_and_is_rebuilt_only_when_the_drawing_changes() {
        let editor = editor_with_a_stroke();
        let first = build_thumbs(&editor, (1, editor.revision()));
        let (id, sketches) = &first.layers[0];
        assert_eq!(*id, editor.document().layers[0].id);
        assert_eq!(sketches.len(), 1);
        assert!(
            sketches[0].points.len() <= THUMB_POINTS + 1,
            "{}",
            sketches[0].points.len()
        );
        assert!(first.bounds[2] > first.bounds[0]);
        let empty = build_thumbs(&Editor::blank(IdGen::new(2)), (1, 0));
        assert_eq!(empty.layers[0].1.len(), 0);
        assert!(
            empty.bounds[2] > empty.bounds[0],
            "an empty drawing still has a frame"
        );
    }

    #[test]
    fn a_big_layer_is_thinned_to_a_fixed_number_of_strokes() {
        let mut editor = Editor::blank(IdGen::new(1));
        for k in 0..(THUMB_STROKES * 3) {
            let mut b = StrokeBuilder::new(Pen::Pen, Some(2.0), Color::BLACK, 1.0, 0.0)
                .with_id(editor.next_id());
            b.push(Point::new(k as f32, 0.0), None, 0.0);
            b.push(Point::new(k as f32, 10.0), None, 0.01);
            editor.commit_stroke(0, b.finish());
        }
        let thumbs = build_thumbs(&editor, (1, 0));
        assert!(thumbs.layers[0].1.len() <= THUMB_STROKES);
        assert!(thumbs.layers[0].1.len() >= THUMB_STROKES / 2);
    }
}
