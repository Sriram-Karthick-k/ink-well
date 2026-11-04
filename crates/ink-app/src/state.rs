//! Plain data the UI reads and edits: the current tool, the view transform and panel visibility.

use std::f32::consts::{PI, TAU};

use eframe::egui::{Pos2, Vec2, ecolor::Hsva, pos2};
use ink_core::{Pen, Point};

use crate::shortcuts::Command;

/// What dragging with a pen or mouse does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Draw,
    /// Touching a stroke removes it.
    Erase,
    Pan,
    /// Loop strokes to select them, then move, scale or turn them.
    Select,
    /// Draw a blade across strokes to cut them.
    Slice,
    /// Pull or push a stroke into a new shape.
    Nudge,
}

/// What the user is drawing with right now. Size, opacity and smoothness belong to the brush and live in the
/// settings (see [`crate::config::Config::brush`]).
#[derive(Debug, Clone)]
pub struct ToolState {
    pub mode: Mode,
    pub pen: Pen,
    pub color: Hsva,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            mode: Mode::Draw,
            pen: Pen::Pen,
            color: Hsva::from_srgb([0x18, 0x17, 0x12]),
        }
    }
}

impl ToolState {
    /// The color a new stroke gets, with `opacity` applied.
    #[must_use]
    pub fn stroke_color(&self, opacity: f32) -> ink_core::Color {
        let [r, g, b] = self.color.to_srgb();
        ink_core::Color::new(
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            opacity,
        )
    }
}

/// Where the canvas is on screen. Canvas coordinates have y pointing up; the screen's y points down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewState {
    /// Screen pixels (points) per canvas unit.
    pub zoom: f32,
    /// The screen position of the canvas origin.
    pub origin: Pos2,
    /// How far the paper is turned about the origin, in radians: positive turns it counterclockwise as it is seen.
    pub angle: f32,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            origin: pos2(0.0, 0.0),
            angle: 0.0,
        }
    }
}

/// `v` turned counterclockwise as it is seen on the screen (whose y axis points down) by `angle` radians.
fn seen_turned(v: Vec2, angle: f32) -> Vec2 {
    let (sin, cos) = angle.sin_cos();
    Vec2::new(v.x * cos + v.y * sin, -v.x * sin + v.y * cos)
}

impl ViewState {
    pub const MIN_ZOOM: f32 = 0.02;
    pub const MAX_ZOOM: f32 = 64.0;

    #[must_use]
    pub fn to_screen(self, p: Point) -> Pos2 {
        self.origin + seen_turned(Vec2::new(p.x * self.zoom, -p.y * self.zoom), self.angle)
    }

    #[must_use]
    pub fn to_canvas(self, s: Pos2) -> Point {
        let v = seen_turned(s - self.origin, -self.angle);
        Point::new(v.x / self.zoom, -v.y / self.zoom)
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.origin += delta;
    }

    /// Turns the paper by `delta` radians (counterclockwise as seen) about the screen point `at`, which stays
    /// where it is, with what is under it.
    pub fn rotate_at(&mut self, at: Pos2, delta: f32) {
        self.origin = at + seen_turned(self.origin - at, delta);
        self.angle = (self.angle + delta + PI).rem_euclid(TAU) - PI;
    }

    /// How far the paper is turned, in degrees, from -180 to 180 (positive counterclockwise).
    #[must_use]
    pub fn degrees(self) -> f32 {
        self.angle.to_degrees()
    }

    /// The angles that turning the paper lands on when it ends close to one, in degrees: every fifteenth.
    pub const ANGLE_STEP: f32 = 15.0;
    /// How close to one, in degrees, a turn has to be to land on it.
    const ANGLE_WINDOW: f32 = 4.0;

    /// `angle` (radians), or the nearest multiple of fifteen degrees if it is within a few degrees of one.
    #[must_use]
    pub fn snapped_angle(angle: f32) -> f32 {
        let step = Self::ANGLE_STEP.to_radians();
        let nearest = (angle / step).round() * step;
        if (angle - nearest).abs() < Self::ANGLE_WINDOW.to_radians() {
            nearest
        } else {
            angle
        }
    }

    /// Settles the turn on a multiple of fifteen degrees if it is close to one, keeping the point under `at` still.
    pub fn snap_angle_at(&mut self, at: Pos2) {
        let target = Self::snapped_angle(self.angle);
        if (target - self.angle).abs() > f32::EPSILON {
            self.rotate_at(at, target - self.angle);
        }
    }

    /// The zoom levels that zooming lands on when it ends close to one (25%, 50%, 100%, 200%, 400%, 800%).
    pub const ZOOM_STOPS: [f32; 6] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0];
    /// How close to a stop, as a share of it, a zoom has to be to land on it.
    const SNAP_WINDOW: f32 = 0.04;

    /// `zoom`, or the round level it is within a few percent of.
    #[must_use]
    pub fn snapped_zoom(zoom: f32) -> f32 {
        Self::ZOOM_STOPS
            .into_iter()
            .find(|stop| (zoom / stop - 1.0).abs() < Self::SNAP_WINDOW)
            .unwrap_or(zoom)
    }

    /// Settles the zoom on a round level if it is close to one, keeping the point under `at` still.
    pub fn snap_zoom_at(&mut self, at: Pos2) {
        let target = Self::snapped_zoom(self.zoom);
        if (target - self.zoom).abs() > f32::EPSILON {
            self.zoom_at(at, target / self.zoom);
        }
    }

    /// Zooms by `factor` keeping the canvas point under `at` fixed on screen.
    pub fn zoom_at(&mut self, at: Pos2, factor: f32) {
        let zoom = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        let k = zoom / self.zoom;
        self.origin = at + (self.origin - at) * k;
        self.zoom = zoom;
    }
}

/// Which panels are open and other short-lived interface state.
#[derive(Debug, Clone, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent on/off switches for separate panels"
)]
pub struct UiState {
    /// The notes page covers the paper.
    pub library_open: bool,
    pub layers_open: bool,
    /// The case with every pen in it.
    pub case_open: bool,
    /// The size and opacity scrubbers of the selected pen.
    pub dial_open: bool,
    pub color_open: bool,
    /// The note menu under the note chip.
    pub sheet_open: bool,
    /// The menu of shapes and the person's own objects.
    pub shapes_open: bool,
    /// The sheet where the tool bar is arranged (touch).
    pub toolbar_open: bool,
    /// What the shapes menu is in the middle of.
    pub objects: ObjectsUi,
    /// Words being written or changed, and what is typed so far.
    pub text: Option<crate::text_ops::TextEntry>,
    /// The precision panel: snapping.
    pub precision_open: bool,
    /// The numbers under the selection bar.
    pub size_fields: crate::size_fields::SizeFields,
    /// The small panel over the turn chip, where the angle of the paper is typed.
    pub turn_open: bool,
    /// The section of the settings window that is showing, or `None` while it is closed.
    pub settings: Option<crate::settings::Section>,
    /// The settings were opened on their landing page rather than on one section. A window with the sections beside
    /// their content ignores this; a screen with room for one thing at a time starts on the list of sections.
    pub settings_home: bool,
    /// Where the rail last put its pen, case button and color well, so their flyouts line up with them.
    pub anchors: Anchors,
    /// The layer being renamed and the text typed so far.
    pub renaming: Option<(usize, String)>,
    /// The command whose key is being chosen in the settings (and which of its keys, or `None` for a new one).
    pub capture: Option<(Command, Option<usize>)>,
    /// The panels over the paper are put away, for drawing with nothing in the way.
    pub panels_hidden: bool,
}

/// What the shapes menu is in the middle of.
#[derive(Debug, Clone, Default)]
pub struct ObjectsUi {
    /// A tap on a shape picks it to arrange (pin it, rename it, delete it) instead of putting it on the paper.
    pub arranging: bool,
    /// The shape picked to arrange.
    pub picked: Option<String>,
    /// The shape being carried out of the drawer, by id, and where the pointer was last seen: on a touch screen the pointer
    /// is gone as the finger lifts, and it is let go where it was.
    pub dragging: Option<String>,
    pub carried_at: Option<eframe::egui::Pos2>,
    /// The name being typed for an object about to be made of the selection.
    pub naming: Option<String>,
    /// The object being renamed and the name typed so far.
    pub renaming: Option<(String, String)>,
}

/// Vertical positions, in points, that flyouts line up with.
#[derive(Debug, Clone, Copy, Default)]
pub struct Anchors {
    pub pen: f32,
    pub case: f32,
    pub shapes: f32,
    /// Where the drawer of shapes is on the screen, while it is open.
    pub drawer: Option<eframe::egui::Rect>,
    pub color: f32,
    /// The top of the dock the rail becomes in a window taller than it is wide, or `0.0` while it is on the edge:
    /// flyouts open above it.
    pub dock_top: f32,
}

impl UiState {
    /// Closes the flyouts, the note menu and the precision panel. At most one of them is open at a time.
    pub fn close_popovers(&mut self) {
        self.case_open = false;
        self.dial_open = false;
        self.color_open = false;
        self.sheet_open = false;
        self.precision_open = false;
        self.turn_open = false;
        self.shapes_open = false;
        self.toolbar_open = false;
        self.objects = ObjectsUi::default();
    }

    /// `true` while any flyout, the note menu, the precision panel or the turn panel is open.
    #[must_use]
    pub fn any_popover(&self) -> bool {
        self.case_open
            || self.dial_open
            || self.color_open
            || self.sheet_open
            || self.precision_open
            || self.turn_open
            || self.shapes_open
            || self.toolbar_open
    }

    pub fn toggle_turn(&mut self) {
        let open = !self.turn_open;
        self.close_popovers();
        self.turn_open = open;
    }

    /// The layers panel and the two panels the top bar opens (precision and the note menu) hang from the same
    /// corner, so only one of them is shown at a time.
    pub fn toggle_layers(&mut self) {
        let open = !self.layers_open;
        self.close_popovers();
        self.layers_open = open;
    }

    pub fn toggle_precision(&mut self) {
        let open = !self.precision_open;
        self.close_popovers();
        self.precision_open = open;
        self.layers_open &= !open;
    }

    pub fn toggle_case(&mut self) {
        let open = !self.case_open;
        self.close_popovers();
        self.case_open = open;
    }

    pub fn toggle_dial(&mut self) {
        let open = !self.dial_open;
        self.close_popovers();
        self.dial_open = open;
    }

    pub fn toggle_color(&mut self) {
        let open = !self.color_open;
        self.close_popovers();
        self.color_open = open;
    }

    pub fn toggle_shapes(&mut self) {
        let open = !self.shapes_open;
        self.close_popovers();
        self.shapes_open = open;
        self.layers_open &= !open;
    }

    pub fn open_toolbar_editor(&mut self) {
        self.close_popovers();
        self.toolbar_open = true;
        self.layers_open = false;
    }

    pub fn toggle_sheet(&mut self) {
        let open = !self.sheet_open;
        self.close_popovers();
        self.sheet_open = open;
        self.layers_open &= !open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_and_canvas_are_inverse_and_y_points_up() {
        let view = ViewState {
            zoom: 2.0,
            origin: pos2(100.0, 300.0),
            ..ViewState::default()
        };
        let p = Point::new(10.0, 25.0);
        let screen = view.to_screen(p);
        assert_eq!(
            screen,
            pos2(120.0, 250.0),
            "positive canvas y is higher on screen"
        );
        let back = view.to_canvas(screen);
        assert!((back.x - p.x).abs() < 1e-4 && (back.y - p.y).abs() < 1e-4);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_cursor_still() {
        let mut view = ViewState {
            zoom: 1.0,
            origin: pos2(50.0, 80.0),
            ..ViewState::default()
        };
        let cursor = pos2(400.0, 300.0);
        let before = view.to_canvas(cursor);
        view.zoom_at(cursor, 3.0);
        let after = view.to_canvas(cursor);
        assert!((before.x - after.x).abs() < 1e-3 && (before.y - after.y).abs() < 1e-3);
        assert!((view.zoom - 3.0).abs() < 1e-6);
    }

    #[test]
    fn a_zoom_close_to_a_round_level_lands_on_it_and_others_are_left_alone() {
        assert!((ViewState::snapped_zoom(1.03) - 1.0).abs() < 1e-6);
        assert!((ViewState::snapped_zoom(0.97) - 1.0).abs() < 1e-6);
        assert!((ViewState::snapped_zoom(2.05) - 2.0).abs() < 1e-6);
        assert!((ViewState::snapped_zoom(0.26) - 0.25).abs() < 1e-6);
        assert!(
            (ViewState::snapped_zoom(1.2) - 1.2).abs() < 1e-6,
            "between levels: untouched"
        );
        assert!(
            (ViewState::snapped_zoom(0.0) - 0.0).abs() < 1e-6,
            "and no divide by zero"
        );
    }

    #[test]
    fn snapping_keeps_the_point_under_the_cursor_still() {
        let mut view = ViewState {
            zoom: 1.03,
            origin: pos2(50.0, 80.0),
            ..ViewState::default()
        };
        let cursor = pos2(400.0, 300.0);
        let before = view.to_canvas(cursor);
        view.snap_zoom_at(cursor);
        let after = view.to_canvas(cursor);
        assert!((view.zoom - 1.0).abs() < 1e-5);
        assert!((before.x - after.x).abs() < 1e-3 && (before.y - after.y).abs() < 1e-3);
    }

    #[test]
    fn a_turned_view_still_maps_screen_and_canvas_to_each_other_and_turns_counterclockwise() {
        // A quarter turn counterclockwise: canvas right is up on the screen, canvas up is left.
        let view = ViewState {
            zoom: 2.0,
            origin: pos2(100.0, 300.0),
            angle: std::f32::consts::FRAC_PI_2,
        };
        let right = view.to_screen(Point::new(10.0, 0.0));
        assert!(
            (right.x - 100.0).abs() < 1e-3 && (right.y - 280.0).abs() < 1e-3,
            "{right:?}"
        );
        let up = view.to_screen(Point::new(0.0, 10.0));
        assert!(
            (up.x - 80.0).abs() < 1e-3 && (up.y - 300.0).abs() < 1e-3,
            "{up:?}"
        );
        for angle in [-3.0_f32, -1.0, 0.0, 0.4, 2.5] {
            let view = ViewState { angle, ..view };
            for p in [
                Point::new(0.0, 0.0),
                Point::new(37.0, -12.0),
                Point::new(-200.0, 150.0),
            ] {
                let back = view.to_canvas(view.to_screen(p));
                assert!(
                    (back.x - p.x).abs() < 1e-3 && (back.y - p.y).abs() < 1e-3,
                    "{angle}: {back:?}"
                );
            }
        }
    }

    #[test]
    fn turning_keeps_the_point_under_the_pointer_still_and_adds_up() {
        let mut view = ViewState {
            zoom: 1.5,
            origin: pos2(50.0, 80.0),
            ..ViewState::default()
        };
        let at = pos2(400.0, 300.0);
        let under = view.to_canvas(at);
        view.rotate_at(at, 0.5);
        view.rotate_at(at, 0.25);
        let after = view.to_canvas(at);
        assert!((under.x - after.x).abs() < 1e-3 && (under.y - after.y).abs() < 1e-3);
        assert!((view.angle - 0.75).abs() < 1e-5);
        assert!((view.degrees() - 0.75_f32.to_degrees()).abs() < 1e-3);
    }

    #[test]
    fn the_turn_stays_between_half_a_turn_either_way() {
        let mut view = ViewState::default();
        view.rotate_at(pos2(0.0, 0.0), 3.0);
        view.rotate_at(pos2(0.0, 0.0), 0.5);
        assert!(
            view.angle > -std::f32::consts::PI && view.angle <= std::f32::consts::PI,
            "{}",
            view.angle
        );
        assert!((view.angle - (3.5 - std::f32::consts::TAU)).abs() < 1e-4);
    }

    #[test]
    fn a_turn_close_to_a_multiple_of_fifteen_degrees_lands_on_it_and_others_are_left_alone() {
        let degrees = |d: f32| d.to_radians();
        assert!((ViewState::snapped_angle(degrees(13.0)) - degrees(15.0)).abs() < 1e-5);
        assert!((ViewState::snapped_angle(degrees(-44.0)) - degrees(-45.0)).abs() < 1e-5);
        assert!((ViewState::snapped_angle(degrees(0.5)) - 0.0).abs() < 1e-6);
        assert!(
            (ViewState::snapped_angle(degrees(7.5)) - degrees(7.5)).abs() < 1e-6,
            "between: untouched"
        );
        assert!(
            (ViewState::snapped_angle(degrees(22.0)) - degrees(22.0)).abs() < 1e-6,
            "7 from 15 and 8 from 30"
        );
        let mut view = ViewState {
            angle: degrees(88.0),
            origin: pos2(30.0, 40.0),
            ..ViewState::default()
        };
        let at = pos2(200.0, 150.0);
        let under = view.to_canvas(at);
        view.snap_angle_at(at);
        assert!((view.degrees() - 90.0).abs() < 1e-3);
        let after = view.to_canvas(at);
        assert!(
            (under.x - after.x).abs() < 1e-3 && (under.y - after.y).abs() < 1e-3,
            "the pointer's place stays"
        );
    }

    #[test]
    fn zoom_is_clamped() {
        let mut view = ViewState::default();
        view.zoom_at(pos2(0.0, 0.0), 1.0e9);
        assert!((view.zoom - ViewState::MAX_ZOOM).abs() < 1e-3);
        view.zoom_at(pos2(0.0, 0.0), 1.0e-9);
        assert!((view.zoom - ViewState::MIN_ZOOM).abs() < 1e-6);
    }

    #[test]
    fn only_one_popover_is_open_at_a_time() {
        let mut ui = UiState::default();
        ui.toggle_case();
        assert!(ui.case_open && ui.any_popover());
        ui.toggle_color();
        assert!(ui.color_open && !ui.case_open);
        ui.toggle_dial();
        assert!(ui.dial_open && !ui.color_open);
        ui.toggle_sheet();
        assert!(ui.sheet_open && !ui.dial_open);
        ui.toggle_sheet();
        assert!(!ui.any_popover(), "toggling an open one closes it");
    }

    #[test]
    fn the_layers_and_the_top_bar_panels_do_not_pile_up_in_their_corner() {
        let mut ui = UiState::default();
        ui.toggle_layers();
        assert!(ui.layers_open);
        ui.toggle_precision();
        assert!(ui.precision_open && !ui.layers_open);
        ui.toggle_layers();
        assert!(ui.layers_open && !ui.precision_open);
        ui.toggle_sheet();
        assert!(ui.sheet_open && !ui.layers_open);
        ui.toggle_layers();
        assert!(ui.layers_open && !ui.sheet_open);
        ui.toggle_layers();
        assert!(!ui.layers_open, "toggling it again puts it away");
    }

    #[test]
    fn stroke_color_carries_the_opacity() {
        let tool = ToolState::default();
        assert!((tool.stroke_color(0.4).a - 0.4).abs() < 1e-6);
    }
}
