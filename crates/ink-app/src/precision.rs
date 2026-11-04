//! Precision: snapping while drawing and while editing, and the panel that switches it on.
//!
//! The Precision panel has a snap toggle, snap to grid, snap to key points and separate options for drawing and
//! for editing. This module works out what a snap would do from the settings and
//! the view ([`plan`]), and draws the panel ([`show`]) and the controls shared with the settings window
//! ([`controls`]). The geometry itself is in `ink_core::snap`.

use eframe::egui::{
    Align2, Area, Button, Color32, Context, Id, Margin, Order, Painter, Pos2, RichText, ScrollArea,
    Stroke, Ui, vec2,
};
use ink_core::Point;
use ink_core::guide::Kind as GuideKind;
use ink_core::snap::{self, Grid, Lattice, Targets};

use crate::app::InkApp;
use crate::config::{CanvasConfig, Config, Pattern, PrecisionConfig};
use crate::density::bar_height;
use crate::paper;
use crate::rail;
use crate::shortcuts::Command;
use crate::theme;
use crate::widgets;

/// How snapping goes right now: what it snaps to and how far away that still counts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    pub grid: Option<Grid>,
    /// The nodes of the isometric or triangle paper.
    pub lattice: Option<Lattice>,
    /// Snap to the ends of strokes.
    pub keys: bool,
    /// Snap to the middle of strokes.
    pub middles: bool,
    /// Snap to the places strokes cross.
    pub crossings: bool,
    /// In canvas units.
    pub reach: f32,
}

impl Plan {
    /// The things to snap to, given the key points of the drawing.
    #[must_use]
    pub fn targets<'a>(&self, keys: &'a [Point]) -> Targets<'a> {
        Targets {
            grid: self.grid,
            lattice: self.lattice,
            keys,
            reach: self.reach,
        }
    }
}

/// The grid the paper shows, as far as it can be snapped to: the crossings of dots and squares, the lines of ruled
/// paper. The other patterns (isometric, triangle, perspective) are not on a square grid, and a grid that is too
/// close together to be drawn is not snapped to either.
#[must_use]
pub fn grid_of(canvas: &CanvasConfig, zoom: f32) -> Option<Grid> {
    let (columns, rows) = match canvas.pattern {
        Pattern::Dots | Pattern::Grid => (true, true),
        Pattern::Lines => (false, true),
        _ => return None,
    };
    let shown = canvas.grid_size * zoom;
    if shown < paper::min_spacing(canvas.pattern).max(2.0) {
        return None;
    }
    Some(Grid {
        spacing: canvas.grid_size,
        columns,
        rows,
    })
}

/// The nodes of the paper if it is an isometric or a triangle one, where its lines meet, as far as they can be
/// snapped to: not when they are too close together to be drawn.
#[must_use]
pub fn lattice_of(canvas: &CanvasConfig, zoom: f32) -> Option<Lattice> {
    let families = match canvas.pattern {
        Pattern::Isometric => Lattice::ISOMETRIC,
        Pattern::Triangle => Lattice::TRIANGLE,
        _ => return None,
    };
    if canvas.grid_size * zoom < paper::min_spacing(canvas.pattern).max(2.0) {
        return None;
    }
    Some(Lattice {
        spacing: canvas.grid_size,
        families,
    })
}

/// The ends and, as the settings ask, the middles of the strokes of the drawing except those in `leave`: the
/// points of the drawing a snap can land on wherever the pointer is. (The places strokes cross are found near the
/// pointer, by [`crossings`].)
#[must_use]
pub fn keys<S: std::hash::BuildHasher>(
    plan: &Plan,
    document: &ink_core::Document,
    leave: &std::collections::HashSet<uuid::Uuid, S>,
) -> Vec<Point> {
    let mut found = if plan.keys {
        snap::key_points(document, leave)
    } else {
        Vec::new()
    };
    if plan.middles {
        found.extend(snap::middle_points(document, leave));
    }
    found
}

/// The places strokes of the drawing except those in `leave` cross, near `at`, if the plan snaps to them.
#[must_use]
pub fn crossings<S: std::hash::BuildHasher>(
    plan: &Plan,
    document: &ink_core::Document,
    leave: &std::collections::HashSet<uuid::Uuid, S>,
    at: Point,
) -> Vec<Point> {
    if plan.crossings {
        snap::crossings_near(document, leave, at, plan.reach)
    } else {
        Vec::new()
    }
}

/// What a snap does while drawing (`editing` is `false`) or while editing, or `None` if it does nothing: snapping
/// is off, switched off for that, or there is nothing it could snap to.
#[must_use]
pub fn plan(config: &Config, zoom: f32, editing: bool) -> Option<Plan> {
    let p = &config.precision;
    let phase = if editing {
        p.snap_while_editing
    } else {
        p.snap_while_drawing
    };
    let usable = zoom.is_finite() && zoom > 0.0;
    if !(p.snap && phase && usable) {
        return None;
    }
    let grid = p
        .snap_to_grid
        .then(|| grid_of(&config.canvas, zoom))
        .flatten();
    let lattice = p
        .snap_to_grid
        .then(|| lattice_of(&config.canvas, zoom))
        .flatten();
    let (keys, middles, crossings) = (p.snap_to_key_points, p.snap_to_middles, p.snap_to_crossings);
    (grid.is_some() || lattice.is_some() || keys || middles || crossings).then_some(Plan {
        grid,
        lattice,
        keys,
        middles,
        crossings,
        reach: p.snap_reach / zoom,
    })
}

/// Marks, at `at` on the screen, a place a point has been drawn to: a ring with a dot, where the pen is going to
/// land.
pub fn paint_marker(painter: &Painter, at: Pos2) {
    painter.circle_filled(at, 9.0, theme::ACCENT.gamma_multiply(0.22));
    painter.circle_stroke(at, 9.0, Stroke::new(1.6, theme::ACCENT));
    painter.circle_filled(at, 2.6, Color32::WHITE);
}

/// The switches and the reach, as they are in the panel and in the settings.
pub fn controls(ui: &mut Ui, precision: &mut PrecisionConfig, canvas: &CanvasConfig) {
    widgets::toggle_row(
        ui,
        "Snap",
        Some("Points land on what is close to them"),
        &mut precision.snap,
    );
    ui.add_enabled_ui(precision.snap, |ui| {
        let grid_hint = if grid_of(canvas, 1.0).is_some() || canvas.pattern == Pattern::Blank {
            "To the dots, squares or ruled lines of the paper"
        } else if lattice_of(canvas, 1.0).is_some() {
            "To the points where the lines of the paper meet"
        } else {
            "This paper has no grid points, so there is nothing to snap to"
        };
        widgets::toggle_row(
            ui,
            "To the grid",
            Some(grid_hint),
            &mut precision.snap_to_grid,
        );
        widgets::toggle_row(
            ui,
            "To key points",
            Some("The ends of strokes already drawn"),
            &mut precision.snap_to_key_points,
        );
        widgets::toggle_row(
            ui,
            "To middles",
            Some("Halfway along the strokes already drawn"),
            &mut precision.snap_to_middles,
        );
        widgets::toggle_row(
            ui,
            "To crossings",
            Some("Where strokes already drawn cross, for a stroke you draw and the handles of a guide"),
            &mut precision.snap_to_crossings,
        );
        widgets::toggle_row(
            ui,
            "While drawing",
            Some("The start and the end of a stroke"),
            &mut precision.snap_while_drawing,
        );
        widgets::toggle_row(
            ui,
            "While editing",
            Some("A selection, as you drag it"),
            &mut precision.snap_while_editing,
        );
        widgets::slider_row(
            ui,
            "Reach",
            Some("How close counts as near"),
            &mut precision.snap_reach,
            PrecisionConfig::REACH.0..=PrecisionConfig::REACH.1,
            |v| format!("{v:.0} pt"),
        );
    });
}

/// Whether a stroke held still at its end becomes the clean shape it is, and how long to hold for.
pub fn recognition_controls(ui: &mut Ui, precision: &mut PrecisionConfig) {
    widgets::toggle_row(
        ui,
        "Recognize shapes",
        Some("Hold the pen still at the end of a stroke to make it a clean shape"),
        &mut precision.recognize_shapes,
    );
    ui.add_enabled_ui(precision.recognize_shapes, |ui| {
        widgets::slider_row(
            ui,
            "Hold for",
            Some("How long the pen is still"),
            &mut precision.hold_delay,
            PrecisionConfig::HOLD.0..=PrecisionConfig::HOLD.1,
            |v| format!("{v:.1} s"),
        );
    });
}

/// A button for each kind of shape guide: lit when the drawing has one, and pressing it puts one on the paper or
/// takes it away.
pub(crate) fn guide_buttons(ui: &mut Ui, app: &mut InkApp) {
    widgets::heading(ui, "SHAPE GUIDES");
    widgets::caption(
        ui,
        "A stroke drawn along a guide follows it. Drag the dots to reshape or move it.",
    );
    let mut pressed = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for kind in GuideKind::ALL {
            let on = app.guides.has(kind);
            let tip = app
                .keymap
                .hint(Command::for_guide(kind), &format!("{} guide", kind.label()));
            let button = Button::new(RichText::new(kind.label()).size(14.0).color(if on {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            }))
            .fill(if on {
                theme::ACCENT.gamma_multiply(0.45)
            } else {
                theme::CHROME_HI
            })
            .stroke(if on {
                Stroke::new(1.0, theme::ACCENT)
            } else {
                Stroke::NONE
            })
            .min_size(vec2(70.0, 34.0));
            if ui.add(button).on_hover_text(tip).clicked() {
                pressed = Some(kind);
            }
        }
    });
    if let Some(kind) = pressed {
        app.toggle_guide(kind);
    }
}

/// The panel under the top bar with the snap switches.
pub fn show(ctx: &Context, app: &mut InkApp) {
    let t = theme::pop(ctx, Id::unique("precision_pop"), app.ui.precision_open);
    if t <= 0.001 {
        return;
    }
    let alpha = t * app.chrome_alpha;
    let room =
        (ctx.content_rect().height() - bar_height(ctx) - 12.0 - 16.0 - 28.0 - rail::dock_lift(ctx))
            .max(120.0);
    let area = Area::new(Id::unique("precision_panel"))
        .anchor(
            Align2::RIGHT_TOP,
            vec2(-16.0, bar_height(ctx) + 12.0 - (1.0 - t) * 10.0),
        )
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            // See `flyouts::flyout`: the area would otherwise clip the scroll area to the size it had last time.
            ui.set_max_height(ctx.content_rect().height());
            theme::floating(ui, [24.0; 4], Margin::same(14), |ui| {
                // Wide enough for a title and a hint on the left of a slider and its value, as in the settings, but
                // never wider than the screen leaves room for (a phone).
                ui.set_width((ctx.content_rect().width() - 2.0 * 16.0 - 28.0).clamp(240.0, 430.0));
                ScrollArea::vertical()
                    .max_height(room)
                    .auto_shrink([true, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 10.0;
                        ui.label(RichText::new("Precision").size(18.0).strong());
                        controls(ui, &mut app.config.precision, &app.config.canvas.clone());
                        widgets::divider(ui);
                        guide_buttons(ui, app);
                        widgets::divider(ui);
                        widgets::heading(ui, "SHAPE RECOGNITION");
                        recognition_controls(ui, &mut app.config.precision);
                    });
            })
        });
    app.chrome.push(area.inner.response.rect);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(pattern: Pattern) -> Config {
        let mut config = Config::default();
        config.precision.snap = true;
        config.canvas.pattern = pattern;
        config.canvas.grid_size = 32.0;
        config
    }

    #[test]
    fn snapping_is_off_to_begin_with_and_a_plan_needs_it_on() {
        assert_eq!(plan(&Config::default(), 1.0, false), None);
        assert!(plan(&config(Pattern::Grid), 1.0, false).is_some());
    }

    #[test]
    fn drawing_and_editing_are_switched_separately() {
        let mut c = config(Pattern::Grid);
        c.precision.snap_while_drawing = false;
        assert_eq!(plan(&c, 1.0, false), None);
        assert!(plan(&c, 1.0, true).is_some());
        c.precision.snap_while_editing = false;
        assert_eq!(plan(&c, 1.0, true), None);
    }

    #[test]
    fn the_reach_is_in_screen_points_so_it_shrinks_in_canvas_units_as_the_zoom_grows() {
        let c = config(Pattern::Grid);
        let at_1 = plan(&c, 1.0, false).expect("a plan").reach;
        let at_4 = plan(&c, 4.0, false).expect("a plan").reach;
        assert!((at_1 - 14.0).abs() < 1e-4);
        assert!((at_4 - 3.5).abs() < 1e-4);
    }

    #[test]
    fn only_paper_on_a_square_grid_has_a_grid_to_snap_to() {
        for (pattern, columns, rows) in [
            (Pattern::Dots, true, true),
            (Pattern::Grid, true, true),
            (Pattern::Lines, false, true),
        ] {
            let grid = grid_of(&config(pattern).canvas, 1.0).expect("a grid");
            assert_eq!((grid.columns, grid.rows), (columns, rows));
            assert!((grid.spacing - 32.0).abs() < 1e-4);
        }
        for pattern in [
            Pattern::Blank,
            Pattern::Isometric,
            Pattern::Triangle,
            Pattern::Perspective1,
            Pattern::Perspective2,
            Pattern::Perspective3,
        ] {
            assert_eq!(grid_of(&config(pattern).canvas, 1.0), None, "{pattern:?}");
        }
    }

    #[test]
    fn a_grid_too_small_to_be_drawn_is_not_snapped_to() {
        let c = config(Pattern::Grid);
        assert!(grid_of(&c.canvas, 1.0).is_some());
        assert_eq!(
            grid_of(&c.canvas, 0.2),
            None,
            "32 canvas units are 6.4 points: not drawn"
        );
        let dots = config(Pattern::Dots);
        assert_eq!(grid_of(&dots.canvas, 0.4), None, "dots need 14 points");
    }

    #[test]
    fn with_no_grid_and_no_key_points_there_is_nothing_to_snap_to() {
        let mut c = config(Pattern::Blank);
        c.precision.snap_to_key_points = false;
        assert_eq!(plan(&c, 1.0, false), None);
        c.precision.snap_to_key_points = true;
        let p = plan(&c, 1.0, false).expect("key points still work on blank paper");
        assert_eq!(p.grid, None);
        assert!(p.keys);
    }

    #[test]
    fn the_isometric_and_triangle_papers_have_a_lattice_that_is_snapped_to_and_the_others_do_not() {
        for (pattern, found) in [
            (Pattern::Isometric, true),
            (Pattern::Triangle, true),
            (Pattern::Grid, false),
            (Pattern::Dots, false),
            (Pattern::Blank, false),
            (Pattern::Perspective1, false),
        ] {
            let lattice = lattice_of(&config(pattern).canvas, 1.0);
            assert_eq!(lattice.is_some(), found, "{pattern:?}");
        }
        assert_eq!(
            lattice_of(&config(Pattern::Triangle).canvas, 0.2),
            None,
            "too close together to be drawn"
        );
        let p = plan(&config(Pattern::Triangle), 1.0, false).expect("a plan");
        assert!(p.lattice.is_some() && p.grid.is_none());
        let mut off = config(Pattern::Triangle);
        off.precision.snap_to_grid = false;
        off.precision.snap_to_key_points = false;
        assert_eq!(plan(&off, 1.0, false), None, "nothing else to snap to");
    }

    #[test]
    fn every_place_the_drawn_lines_of_a_lattice_paper_meet_is_a_place_snapping_lands_on() {
        use eframe::egui::{Color32, Rect, pos2};
        for pattern in [Pattern::Isometric, Pattern::Triangle] {
            let c = config(pattern);
            let origin = pos2(300.0, 200.0);
            let ruling = paper::Ruling {
                pattern,
                spacing: c.canvas.grid_size,
                strength: 1.0,
                ink: Color32::BLACK,
                min_spacing: 0.0,
                major_every: 4,
                zoom: 1.0,
                weight: 1.0,
                turn: 0.0,
            };
            let near_origin = Rect::from_center_size(origin, vec2(260.0, 260.0));
            let segments = paper::line_segments(near_origin, origin, &ruling);
            let targets = plan(&c, 1.0, false).expect("a plan").targets(&[]);
            let mut checked = 0;
            for (i, (a, _)) in segments.iter().enumerate() {
                for (b, _) in &segments[i + 1..] {
                    let (r, s) = (a[1] - a[0], b[1] - b[0]);
                    let cross = r.x * s.y - r.y * s.x;
                    if cross.abs() < 1e-3 * r.length() * s.length() {
                        continue;
                    }
                    let q = b[0] - a[0];
                    let t = (q.x * s.y - q.y * s.x) / cross;
                    let meet = a[0] + r * t;
                    if !near_origin.shrink(10.0).contains(meet) {
                        continue;
                    }
                    // The canvas point under it: the screen counts down, the canvas up.
                    let canvas = Point::new(meet.x - origin.x, origin.y - meet.y);
                    let snap = targets.nearest(canvas).expect("a node within reach");
                    assert!(
                        snap.point.distance(canvas) < 0.05,
                        "{pattern:?}: the lines meet at {canvas:?}, snapping lands on {:?}",
                        snap.point
                    );
                    checked += 1;
                }
            }
            assert!(
                checked > 20,
                "{pattern:?}: only {checked} meetings looked at"
            );
        }
    }
}
