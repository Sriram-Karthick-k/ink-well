//! The sign that a pen held still is about to move the page.
//!
//! A pen that rests where it touched the paper stops drawing after a while and drags the page (see `touch_input`), and
//! a person has to be able to see that coming, or a pause to think would be a surprise. After a moment a ring begins
//! to fill round the pen; when it is full the ring shows a hand, and the page follows the pen.

use std::f32::consts::{FRAC_PI_2, TAU};

use eframe::egui::{Color32, Context, Id, LayerId, Order, Pos2, Rect, Shape, Stroke, vec2};

use crate::app::InkApp;
use crate::icons::{self, Icon};
use crate::theme;

/// How far into the wait the ring first shows: a pen set down and lifted again never flashes one.
const SHOWN_FROM: f32 = 0.15;
/// The radius of the ring, in points: wide enough to be seen round a fingertip.
const RADIUS: f32 = 30.0;
/// How many segments a whole turn of the ring is drawn with.
const STEPS: usize = 64;

/// The points of an arc of the ring, from the top clockwise, covering `progress` (`0.0` to `1.0`) of a turn.
#[must_use]
fn arc(center: Pos2, radius: f32, progress: f32) -> Vec<Pos2> {
    let progress = progress.clamp(0.0, 1.0);
    let count = (progress * STEPS as f32).ceil() as usize;
    (0..=count)
        .map(|i| {
            let angle = -FRAC_PI_2 + progress * TAU * (i as f32 / count.max(1) as f32);
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

/// Shows the ring while a pen is being held still, or is moving the page because it was.
pub fn show(ctx: &Context, app: &InkApp) {
    let now = ctx.input(|i| i.time);
    let Some(hold) = app.touch.pen_hold(now) else {
        return;
    };
    // The ring grows with the time and nothing else changes, so the frames have to keep coming.
    ctx.request_repaint();
    if !hold.panning && hold.progress < SHOWN_FROM {
        return;
    }
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("hold_ring")));
    painter.circle_filled(hold.at, RADIUS, Color32::from_black_alpha(36));
    painter.circle_stroke(
        hold.at,
        RADIUS,
        Stroke::new(3.0, Color32::from_black_alpha(70)),
    );
    if hold.panning {
        painter.circle_filled(hold.at, RADIUS - 2.0, theme::ACCENT.gamma_multiply(0.85));
        icons::paint(
            &painter,
            Rect::from_center_size(hold.at, vec2(26.0, 26.0)),
            Icon::Hand,
            Color32::WHITE,
        );
    } else {
        painter.add(Shape::line(
            arc(hold.at, RADIUS, hold.progress),
            Stroke::new(4.5, theme::ACCENT),
        ));
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::pos2;

    use super::*;

    #[test]
    fn the_arc_starts_at_the_top_and_runs_clockwise() {
        let c = pos2(100.0, 100.0);
        let points = arc(c, 30.0, 0.5);
        let first = points[0];
        assert!(
            (first.x - 100.0).abs() < 1e-3 && (first.y - 70.0).abs() < 1e-3,
            "{first:?}"
        );
        let last = *points.last().expect("points");
        assert!(
            (last.x - 100.0).abs() < 1e-3 && (last.y - 130.0).abs() < 1e-3,
            "half a turn ends at the bottom: {last:?}"
        );
        let quarter = *arc(c, 30.0, 0.25).last().expect("points");
        assert!(
            (quarter.x - 130.0).abs() < 1e-3,
            "a quarter turn is on the right: {quarter:?}"
        );
    }

    #[test]
    fn a_whole_turn_closes_and_nothing_is_still_a_point() {
        let c = pos2(0.0, 0.0);
        let whole = arc(c, 10.0, 1.0);
        let (first, last) = (whole[0], *whole.last().expect("points"));
        assert!((first - last).length() < 1e-3, "{first:?} {last:?}");
        assert_eq!(arc(c, 10.0, 0.0).len(), 1);
        assert_eq!(
            arc(c, 10.0, 7.0).len(),
            arc(c, 10.0, 1.0).len(),
            "more than a turn is a turn"
        );
    }
}
