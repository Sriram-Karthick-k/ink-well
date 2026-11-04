//! Arranging the tool bar on a device used by touch: a bottom sheet that lists what is on the bar, in order, with a way
//! to move each along or take it off, and what can be put back.

use eframe::egui::{
    Align2, Color32, Context, CornerRadius, FontId, Id, Rect, Sense, Ui, pos2, vec2,
};

use super::bar_items;
use super::sheet::{self, Tone};
use crate::app::InkApp;
use crate::icons::{self, Icon};
use crate::theme;
use crate::toolbar::Item;

/// The height of a row of the list.
const ROW: f32 = 58.0;
/// The width of each of the buttons at the end of a row: wide enough for a finger.
const BUTTON: f32 = 46.0;

/// Something done in the sheet, applied after it is drawn.
enum Act {
    Earlier(Item),
    Later(Item),
    Remove(Item),
    Add(Item),
    Reset,
    /// Open the shapes menu, arranging, to pin shapes.
    PinShapes,
}

/// Shows the sheet while it is open.
pub fn show(ctx: &Context, app: &mut InkApp) {
    if !app.ui.toolbar_open {
        return;
    }
    let mut acts = Vec::new();
    let answer = sheet::show(ctx, "toolbar", Some("Tool bar"), |ui| {
        content(ui, app, &mut acts);
    });
    app.chrome.push(answer.rect);
    if answer.dismissed {
        app.ui.close_popovers();
        return;
    }
    for act in acts {
        apply(app, act);
    }
}

fn content(ui: &mut Ui, app: &InkApp, acts: &mut Vec<Act>) {
    let items = app.config.toolbar_items();
    sheet::caption(ui, "ON THE BAR, LEFT TO RIGHT");
    let last = items.len().saturating_sub(1);
    for (i, item) in items.iter().enumerate() {
        row(ui, app, item, i > 0, i < last, items.len() > 1, acts);
    }
    sheet::divider(ui);
    sheet::caption(ui, "PUT BACK");
    let absent: Vec<Item> = Item::tools()
        .into_iter()
        .filter(|item| !items.contains(item))
        .collect();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for item in absent {
            if sheet::pill(ui, &bar_items::label(app, &item), true, Tone::Normal) {
                acts.push(Act::Add(item));
            }
        }
        if sheet::pill(ui, "A shape…", true, Tone::Normal) {
            acts.push(Act::PinShapes);
        }
    });
    ui.add_space(8.0);
    if sheet::pill(ui, "Start over", true, Tone::Danger) {
        acts.push(Act::Reset);
    }
}

/// One item of the list: its picture and name, and buttons to move it earlier, later, or off the bar.
fn row(
    ui: &mut Ui,
    app: &InkApp,
    item: &Item,
    can_earlier: bool,
    can_later: bool,
    can_remove: bool,
    acts: &mut Vec<Act>,
) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
    let painter = ui.painter();
    bar_items::paint(
        painter,
        Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), vec2(24.0, 24.0)),
        app,
        item,
        theme::TEXT,
    );
    painter.text(
        pos2(rect.left() + 60.0, rect.center().y),
        Align2::LEFT_CENTER,
        bar_items::label(app, item),
        FontId::proportional(17.0),
        theme::TEXT,
    );
    // The buttons are laid from the right edge, in the order they are read: earlier, later, off.
    let buttons = [
        (Icon::Left, can_earlier, 0),
        (Icon::Right, can_later, 1),
        (Icon::Close, can_remove, 2),
    ];
    for (icon, enabled, slot) in buttons {
        let x = rect.right() - BUTTON * (3.0 - slot as f32);
        let area = Rect::from_min_size(pos2(x, rect.top()), vec2(BUTTON, ROW));
        let response = ui.interact(
            area,
            Id::unique(("toolbar_edit", item.key(), slot)),
            Sense::click(),
        );
        let color = if enabled {
            theme::TEXT
        } else {
            theme::TEXT_DIM.gamma_multiply(0.4)
        };
        if enabled && response.is_pointer_button_down_on() {
            ui.painter().rect_filled(
                area.shrink2(vec2(2.0, 6.0)),
                CornerRadius::same(14),
                Color32::from_white_alpha(22),
            );
        }
        icons::paint(
            ui.painter(),
            Rect::from_center_size(area.center(), vec2(18.0, 18.0)),
            icon,
            color,
        );
        if enabled && response.clicked() {
            acts.push(match slot {
                0 => Act::Earlier(item.clone()),
                1 => Act::Later(item.clone()),
                _ => Act::Remove(item.clone()),
            });
        }
    }
    ui.painter().line_segment(
        [
            pos2(rect.left() + 8.0, rect.bottom()),
            pos2(rect.right() - 8.0, rect.bottom()),
        ],
        sheet::edge(),
    );
}

fn apply(app: &mut InkApp, act: Act) {
    match act {
        Act::Earlier(item) => app.config.toolbar_shift(&item, -1),
        Act::Later(item) => app.config.toolbar_shift(&item, 1),
        Act::Remove(item) => app.config.toolbar_remove(&item),
        Act::Add(item) => app.config.toolbar_add(item),
        Act::Reset => app.config.toolbar_reset(),
        Act::PinShapes => {
            app.ui.toggle_shapes();
            app.ui.objects.arranging = true;
            return;
        }
    }
    app.mark_config_changed();
}
