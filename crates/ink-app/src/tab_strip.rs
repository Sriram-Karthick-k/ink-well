//! The toolbar docked along the top of the window: the Library first, then one tab for every open drawing, a
//! button for a new one, and on the right the buttons of `bar_buttons`.
//!
//! It is a panel, not something floating over the paper, so the canvas always starts below it. Tabs close with
//! their cross or a middle click and can be dragged into another order.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, Rect, Sense, Stroke,
    Ui, pos2, vec2,
};

use crate::app::InkApp;
use crate::bar_buttons;
use crate::density::Density;
use crate::tabs::TabLabel;
use crate::theme;
use crate::widgets;

/// The height of the whole bar.
const BAR_HEIGHT: f32 = Density::DESKTOP.bar_height;
const TAB_HEIGHT: f32 = 37.0;
const LIBRARY_WIDTH: f32 = 118.0;
const NEW_TAB: f32 = 36.0;
const GAP: f32 = 2.0;
const MIN_TAB: f32 = 72.0;
const MAX_TAB: f32 = 220.0;
/// The buttons on the right: how big each is and the gap between them.
const BUTTON: f32 = 34.0;
const BUTTON_GAP: f32 = 6.0;
/// Room kept free on the right for the buttons, with a little over.
const RIGHT_WIDTH: f32 =
    16.0 + bar_buttons::COUNT * BUTTON + (bar_buttons::COUNT - 1.0) * BUTTON_GAP;
const BAR: Color32 = Color32::from_rgb(0x15, 0x14, 0x12);
const ACTIVE_TAB: Color32 = Color32::from_rgb(0x2B, 0x29, 0x26);

/// Something done on the bar, applied once it has been drawn.
enum Action {
    Library,
    Activate(usize),
    Close(usize),
    New,
    Move(usize, usize),
}

/// How wide each of `count` tabs is when `room` is all the strip may take.
fn tab_width(room: f32, count: usize) -> f32 {
    if count == 0 {
        return MAX_TAB;
    }
    let fixed = LIBRARY_WIDTH + NEW_TAB + 16.0 + GAP * (count as f32 + 2.0);
    ((room - fixed) / count as f32).clamp(MIN_TAB, MAX_TAB)
}

/// Shows the bar. Call it before the central panel so the canvas gets what is left.
pub fn show(ui: &mut Ui, app: &mut InkApp) {
    let labels: Vec<TabLabel> = (0..app.tabs.len()).map(|i| app.tab_label(i)).collect();
    let showing = if app.ui.library_open {
        None
    } else {
        app.active
    };
    let library_open = app.ui.library_open;
    let mut actions = Vec::new();

    egui::Panel::top("tab_bar")
        .exact_size(BAR_HEIGHT)
        .frame(
            Frame::new()
                .fill(BAR)
                .inner_margin(Margin::symmetric(10, 0)),
        )
        .show(ui, |ui| {
            let full = ui.max_rect();
            ui.painter().line_segment(
                [full.left_bottom(), full.right_bottom()],
                Stroke::new(1.0, Color32::from_white_alpha(20)),
            );
            ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
            ui.horizontal(|ui| {
                let left = (ui.available_width() - RIGHT_WIDTH).max(240.0);
                ui.allocate_ui_with_layout(
                    vec2(left, BAR_HEIGHT),
                    Layout::left_to_right(Align::BOTTOM),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        let width = tab_width(left, labels.len());
                        library_tab(ui, library_open, &mut actions);
                        ui.add_space(6.0);
                        let first = ui.cursor().left();
                        for (i, label) in labels.iter().enumerate() {
                            let slot = Slot {
                                index: i,
                                count: labels.len(),
                                width,
                                first,
                            };
                            tab(ui, label, showing == Some(i), &slot, &mut actions);
                        }
                        new_tab_button(ui, &mut actions);
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = BUTTON_GAP;
                    bar_buttons::show(ui, app, BUTTON);
                });
            });
        });

    for action in actions {
        match action {
            Action::Library => app.open_library(ui.ctx()),
            Action::Activate(i) => app.switch_tab(ui.ctx(), i),
            Action::Close(i) => app.close_tab(ui.ctx(), i),
            Action::New => app.new_tab(ui.ctx()),
            Action::Move(from, to) => app.move_tab(from, to),
        }
    }
}

/// Where a tab sits in the strip.
struct Slot {
    index: usize,
    count: usize,
    width: f32,
    /// The x position where the first tab starts.
    first: f32,
}

/// The shape of a tab: round at the top, square where it meets the bottom edge of the bar.
const TAB_ROUNDING: CornerRadius = CornerRadius {
    nw: 12,
    ne: 12,
    sw: 0,
    se: 0,
};

/// Paints the look of the tab that is showing: raised, with a thin accent line along its top.
fn paint_selected(ui: &Ui, rect: Rect) {
    let painter = ui.painter();
    painter.rect_filled(rect, TAB_ROUNDING, ACTIVE_TAB);
    painter.rect_filled(
        Rect::from_min_size(rect.min + vec2(10.0, 0.0), vec2(rect.width() - 20.0, 2.0)),
        CornerRadius::same(1),
        theme::ACCENT,
    );
}

fn paint_hover(ui: &Ui, rect: Rect, amount: f32) {
    if amount > 0.0 {
        ui.painter().rect_filled(
            rect,
            TAB_ROUNDING,
            Color32::from_white_alpha((14.0 * amount) as u8),
        );
    }
}

fn library_tab(ui: &mut Ui, selected: bool, actions: &mut Vec<Action>) {
    let (rect, response) = ui.allocate_exact_size(vec2(LIBRARY_WIDTH, TAB_HEIGHT), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.12);
    if selected {
        paint_selected(ui, rect);
    } else {
        paint_hover(ui, rect, hover);
    }
    let color = if selected {
        theme::TEXT
    } else {
        theme::TEXT_DIM
    };
    let painter = ui.painter();
    // Four small squares: the grid of drawings.
    let origin = pos2(rect.left() + 15.0, rect.center().y - 5.5);
    for (dx, dy) in [(0.0, 0.0), (7.0, 0.0), (0.0, 7.0), (7.0, 7.0)] {
        painter.rect_filled(
            Rect::from_min_size(origin + vec2(dx, dy), vec2(5.0, 5.0)),
            CornerRadius::same(1),
            if selected { theme::ACCENT } else { color },
        );
    }
    painter.text(
        pos2(rect.left() + 37.0, rect.center().y),
        Align2::LEFT_CENTER,
        "Library",
        FontId::proportional(14.5),
        color,
    );
    if response
        .on_hover_text("All your drawings and folders (Ctrl+L)")
        .clicked()
    {
        actions.push(Action::Library);
    }
}

fn tab(ui: &mut Ui, label: &TabLabel, selected: bool, slot: &Slot, actions: &mut Vec<Action>) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(slot.width, TAB_HEIGHT), Sense::click_and_drag());
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    if selected {
        paint_selected(ui, rect);
    } else {
        paint_hover(ui, rect, hover);
    }
    let painter = ui.painter().clone();
    let color = if selected {
        theme::TEXT
    } else {
        theme::TEXT_DIM
    };

    // A small sheet of paper.
    let page = Rect::from_min_size(
        pos2(rect.left() + 14.0, rect.center().y - 7.5),
        vec2(11.0, 15.0),
    );
    painter.rect_filled(page, CornerRadius::same(2), color.gamma_multiply(0.85));
    for y in [5.0, 8.5] {
        painter.line_segment(
            [
                page.left_top() + vec2(2.5, y),
                page.right_top() + vec2(-2.5, y),
            ],
            Stroke::new(1.0, ACTIVE_TAB),
        );
    }

    let galley = widgets::elided(
        &painter,
        &label.name,
        FontId::proportional(14.5),
        color,
        (rect.width() - 32.0 - 34.0).max(10.0),
    );
    painter.galley(
        pos2(rect.left() + 33.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );

    // The cross, or a dot while there are unsaved changes and the pointer is elsewhere.
    let cross =
        Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(22.0, 22.0));
    let close = ui.interact(cross, response.id.with("close"), Sense::click());
    if selected || response.hovered() {
        if close.hovered() {
            painter.circle_filled(cross.center(), 11.0, Color32::from_white_alpha(28));
        }
        let ink = Stroke::new(1.6, theme::TEXT);
        let c = cross.center();
        painter.line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], ink);
        painter.line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], ink);
    } else if label.dirty {
        painter.circle_filled(cross.center(), 3.5, theme::ACCENT);
    }

    if close.clicked() || response.middle_clicked() {
        actions.push(Action::Close(slot.index));
    } else if response.clicked() {
        actions.push(Action::Activate(slot.index));
    }
    // Dragging a tab sideways moves it to where the pointer is.
    if response.dragged()
        && let Some(at) = response.interact_pointer_pos()
    {
        let step = slot.width + GAP;
        let target = (((at.x - slot.first) / step).floor().max(0.0) as usize)
            .min(slot.count.saturating_sub(1));
        if target != slot.index {
            actions.push(Action::Move(slot.index, target));
        }
    }
}

fn new_tab_button(ui: &mut Ui, actions: &mut Vec<Action>) {
    let (rect, response) = ui.allocate_exact_size(vec2(NEW_TAB, TAB_HEIGHT), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered(), 0.12);
    let painter = ui.painter();
    painter.circle_filled(
        rect.center(),
        13.0,
        Color32::from_white_alpha((10.0 + 22.0 * hover) as u8),
    );
    let plus = Stroke::new(1.8, theme::TEXT);
    let c = rect.center();
    painter.line_segment([c - vec2(5.0, 0.0), c + vec2(5.0, 0.0)], plus);
    painter.line_segment([c - vec2(0.0, 5.0), c + vec2(0.0, 5.0)], plus);
    if response.on_hover_text("New drawing (Ctrl+T)").clicked() {
        actions.push(Action::New);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_share_the_room_between_a_minimum_and_a_maximum() {
        assert!(
            (tab_width(2000.0, 2) - MAX_TAB).abs() < 1e-3,
            "few tabs are not stretched"
        );
        assert!(
            (tab_width(500.0, 30) - MIN_TAB).abs() < 1e-3,
            "many tabs stop shrinking"
        );
        let w = tab_width(900.0, 4);
        assert!(w > MIN_TAB && w < MAX_TAB);
        assert!((tab_width(900.0, 0) - MAX_TAB).abs() < 1e-3);
    }
}
