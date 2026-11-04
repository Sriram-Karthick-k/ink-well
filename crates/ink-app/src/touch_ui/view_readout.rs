//! The zoom and the turn of the paper on a device used by touch.
//!
//! A person zooms and turns the paper with their fingers, so a number that sits in a corner all the time is only in the
//! way. It shows while the zoom or the turn is changing, in the middle under the top bar, and fades away a moment after.
//! Fitting the page to the screen and turning it to an angle are in the drawing menu.

use eframe::egui::{Align2, Area, Context, Id, Label, Margin, Order, RichText, TextWrapMode, vec2};

use crate::app::InkApp;
use crate::density;
use crate::theme;

/// How long the readout stays after the last change, and how long it takes to fade at the end of that.
const SHOWN_FOR: f64 = 1.4;
const FADE: f64 = 0.4;
/// The gap between the top bar and the readout.
const BELOW_BAR: f32 = 14.0;

/// What the readout last saw, and when it last changed.
#[derive(Debug, Clone, Copy)]
struct Seen {
    zoom: f32,
    degrees: f32,
    changed_at: f64,
}

/// How opaque the readout is `age` seconds after the last change: fully for a while, then fading out, then not at all.
#[must_use]
fn opacity(age: f64) -> f32 {
    ((SHOWN_FOR - age) / FADE).clamp(0.0, 1.0) as f32
}

/// How the zoom and the turn read: `79%`, and `79% · 30°` while the paper is turned.
#[must_use]
fn text(zoom: f32, degrees: f32) -> String {
    let percent = if zoom.is_finite() {
        zoom * 100.0
    } else {
        100.0
    };
    let percent = format!("{:.0}%", percent.clamp(0.0, 99_999.0));
    if degrees.abs() < 0.05 {
        percent
    } else {
        format!("{percent}  \u{b7}  {degrees:.0}\u{b0}")
    }
}

/// Shows the readout while the zoom or the turn is changing.
pub fn show(ctx: &Context, app: &InkApp) {
    let now = ctx.input(|i| i.time);
    let (zoom, degrees) = (app.view.zoom, app.view.degrees());
    let key = Id::NULL;
    // The first time it looks, nothing has changed: it only says what it sees.
    let mut seen: Seen = ctx.data(|d| d.get_temp(key)).unwrap_or(Seen {
        zoom,
        degrees,
        changed_at: f64::NEG_INFINITY,
    });
    if (seen.zoom - zoom).abs() > 1e-4 || (seen.degrees - degrees).abs() > 0.05 {
        seen = Seen {
            zoom,
            degrees,
            changed_at: now,
        };
    }
    ctx.data_mut(|d| d.insert_temp(key, seen));
    let age = now - seen.changed_at;
    let alpha = opacity(age);
    if alpha <= 0.0 {
        return;
    }
    // Fading is a change on every frame until it is gone.
    ctx.request_repaint();
    Area::new(Id::unique("view_readout"))
        .anchor(
            Align2::CENTER_TOP,
            vec2(0.0, density::bar_height(ctx) + BELOW_BAR),
        )
        .order(Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_opacity(alpha * app.chrome_alpha);
            theme::floating(ui, [20.0; 4], Margin::symmetric(18, 9), |ui| {
                ui.add(
                    Label::new(
                        RichText::new(text(zoom, degrees))
                            .size(17.0)
                            .strong()
                            .color(theme::TEXT),
                    )
                    .wrap_mode(TextWrapMode::Extend),
                );
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_readout_is_whole_for_a_while_then_fades_then_is_gone() {
        assert!((opacity(0.0) - 1.0).abs() < 1e-6);
        assert!((opacity(SHOWN_FOR - FADE) - 1.0).abs() < 1e-6);
        let half = opacity(SHOWN_FOR - FADE / 2.0);
        assert!(half > 0.4 && half < 0.6, "{half}");
        assert!(opacity(SHOWN_FOR).abs() < 1e-6);
        assert!(opacity(60.0).abs() < 1e-6);
    }

    #[test]
    fn it_reads_the_zoom_and_adds_the_turn_only_while_the_paper_is_turned() {
        assert_eq!(text(0.79, 0.0), "79%");
        assert_eq!(text(1.0, 0.01), "100%");
        assert_eq!(text(2.0, 30.0), "200%  \u{b7}  30\u{b0}");
        assert_eq!(text(f32::NAN, 0.0), "100%", "not a number reads as 100%");
    }
}
