//! Which user interface the app shows.
//!
//! A computer and a device used by touch each have an interface of their own, in their own modules: `tab_strip` and
//! `settings` for the computer, `touch_ui` for the touch device. Neither knows about the other. This is the one place
//! that picks between them, so nothing else in the app asks what kind of device it is on.

use eframe::egui::{Context, Ui};

use crate::app::InkApp;
use crate::density::{self, Density};
use crate::hud;
use crate::layers_panel;
use crate::library_page;
use crate::platform;
use crate::rail;
use crate::settings;
use crate::shapes_panel;
use crate::tab_strip;
use crate::touch_ui;

/// The interface in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Desktop,
    Touch,
}

/// The interface this device uses.
#[must_use]
pub fn current() -> Shell {
    if platform::touch_first() {
        Shell::Touch
    } else {
        Shell::Desktop
    }
}

/// The seconds a pointer rests on something before its tooltip shows, when there are tooltips: egui's own.
const TOOLTIP_DELAY: f32 = 0.5;

/// Sets the sizes the shared widgets use, and whether hovering explains things. Call it first in every frame.
pub fn begin_frame(ctx: &Context) {
    let density = match current() {
        Shell::Desktop => Density::DESKTOP,
        Shell::Touch => Density::TOUCH,
    };
    density::set(ctx, density);
    // A tooltip that never comes is a tooltip that is off.
    let delay = if density.tooltips {
        TOOLTIP_DELAY
    } else {
        f32::INFINITY
    };
    if ctx.global_style().interaction.tooltip_delay.to_bits() != delay.to_bits() {
        ctx.all_styles_mut(|style| style.interaction.tooltip_delay = delay);
    }
}

/// The bar along the top. Call it before the central panel so the canvas gets what is left.
pub fn top_bar(ui: &mut Ui, app: &mut InkApp) {
    match current() {
        Shell::Desktop => tab_strip::show(ui, app),
        Shell::Touch => touch_ui::top_bar::show(ui, app),
    }
}

/// The settings, while they are open.
pub fn settings(ctx: &Context, app: &mut InkApp) {
    match current() {
        Shell::Desktop => settings::show(ctx, app),
        Shell::Touch => touch_ui::settings::show(ctx, app),
    }
}

/// The tools: on the desktop the rail on the edge of a window wider than it is tall, and the dock where it is taller; on
/// a touch device the interface's own panel along the bottom, held either way up.
pub fn tools(ctx: &Context, app: &mut InkApp) {
    match current() {
        Shell::Desktop => rail::show(ctx, app),
        Shell::Touch => touch_ui::tool_bar::show(ctx, app),
    }
}

/// How the zoom and the turn of the paper are shown: chips in the corner on the desktop, a readout that shows while
/// they change on a touch device.
pub fn view_chips(ctx: &Context, app: &mut InkApp) {
    match current() {
        Shell::Desktop => hud::chips(ctx, app),
        Shell::Touch => touch_ui::view_readout::show(ctx, app),
    }
}

/// What is drawn over everything else while the person is touching the screen: on a touch device, the sign that a held
/// pen is about to move the page. The desktop has none.
pub fn overlays(ctx: &Context, app: &InkApp) {
    match current() {
        Shell::Desktop => {}
        Shell::Touch => touch_ui::hold_ring::show(ctx, app),
    }
}

/// The Library, while it is open: cards and menus laid out for a mouse, or for a finger.
pub fn library(ctx: &Context, app: &mut InkApp) {
    match current() {
        Shell::Desktop => library_page::show(ctx, app),
        Shell::Touch => touch_ui::library::show(ctx, app),
    }
}

/// The drawing menu, while it is open: a small menu under its button, or a sheet from the bottom edge.
pub fn menu(ctx: &Context, app: &mut InkApp) {
    match current() {
        Shell::Desktop => hud::note_menu(ctx, app),
        Shell::Touch => touch_ui::menu_sheet::show(ctx, app),
    }
}

/// The precision panel, while it is open: a window under its button, or a sheet from the bottom edge.
pub fn precision(ctx: &Context, app: &mut InkApp) {
    match current() {
        Shell::Desktop => crate::precision::show(ctx, app),
        Shell::Touch => touch_ui::precision_sheet::show(ctx, app),
    }
}

/// The layers, while they are open: a panel at the corner, or a sheet from the bottom edge.
pub fn layers(ctx: &Context, app: &mut InkApp) {
    match current() {
        Shell::Desktop => layers_panel::show(ctx, app),
        Shell::Touch => touch_ui::layers_sheet::show(ctx, app),
    }
}

/// The menu of shapes and the person's own objects, while it is open: a panel beside the rail, or a sheet from the bottom
/// edge.
pub fn shapes(ctx: &Context, app: &mut InkApp) {
    match current() {
        Shell::Desktop => shapes_panel::show(ctx, app),
        Shell::Touch => touch_ui::shapes_sheet::show(ctx, app),
    }
}

/// Where the tool bar is arranged, while that is open. Only a touch device has a bar to arrange; the rail of the desktop
/// is arranged where the pens are chosen.
pub fn toolbar_editor(ctx: &Context, app: &mut InkApp) {
    match current() {
        Shell::Desktop => {}
        Shell::Touch => touch_ui::toolbar_sheet::show(ctx, app),
    }
}
