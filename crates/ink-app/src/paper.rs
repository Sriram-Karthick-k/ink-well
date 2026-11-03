//! The pattern printed on the paper: dots, grids, ruled lines, isometric and triangle lattices, and perspective
//! grids that run to vanishing points.
//!
//! The patterns made of lines are worked out as plain segments first ([`line_segments`]), so their geometry can be
//! tested without a window, and then drawn.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_3, FRAC_PI_6, PI};

use eframe::egui::{Color32, CornerRadius, Painter, Pos2, Rect, Stroke, Vec2, pos2, vec2};

use crate::config::Pattern;

/// What to print and how.
#[derive(Debug, Clone, Copy)]
pub struct Ruling {
    pub pattern: Pattern,
    /// Distance between dots or lines on screen, in points.
    pub spacing: f32,
    /// How visible the pattern is, `0.0..=1.0`.
    pub strength: f32,
    /// The color the pattern is printed in; its transparency is set by `strength`.
    pub ink: Color32,
    /// A pattern closer together than this is left out: it would only be noise.
    pub min_spacing: f32,
    /// Every this many lines is a main line, a little stronger than the rest. `1` makes all of them main lines.
    pub major_every: i64,
    /// Screen points per canvas unit, for the patterns that are laid out in canvas units (the vanishing points).
    pub zoom: f32,
    /// How thick the lines are, in points.
    pub weight: f32,
    /// How far the pattern is turned on the screen, in radians (a screen angle: the paper turned counterclockwise
    /// is a negative turn, as the screen's y axis points down).
    pub turn: f32,
}

/// How close together, in points, the dots or lines of `pattern` can be before they are left out. Snapping uses
/// the same number, so nothing snaps to a grid that is not showing.
#[must_use]
pub fn min_spacing(pattern: Pattern) -> f32 {
    if pattern == Pattern::Dots { 14.0 } else { 8.0 }
}

/// Where the vanishing points of perspective grids are, in canvas units from the origin.
const VANISH_SPREAD: f32 = 1500.0;
/// A family of lines with more lines than this in view is left out: it would be a solid wash.
const MAX_LINES: i64 = 2500;
/// A lattice of dots with more places than this in view is left out.
const MAX_DOTS: i64 = 80_000;

/// How strong a line of a pattern is printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Minor,
    Major,
    Horizon,
}

/// A piece of line on screen, from one end to the other.
pub type Segment = [Pos2; 2];

/// The part of the endless line through `point`, running at `angle` (in radians, with screen coordinates), that
/// lies inside `rect`; `None` if the line misses it.
#[must_use]
pub fn clip_line(rect: Rect, point: Pos2, angle: f32) -> Option<Segment> {
    let direction = vec2(angle.cos(), angle.sin());
    let (mut from, mut to) = (f32::NEG_INFINITY, f32::INFINITY);
    for (at, step, low, high) in [
        (point.x, direction.x, rect.left(), rect.right()),
        (point.y, direction.y, rect.top(), rect.bottom()),
    ] {
        if step.abs() < 1e-9 {
            if at < low || at > high {
                return None;
            }
        } else {
            let (a, b) = ((low - at) / step, (high - at) / step);
            from = from.max(a.min(b));
            to = to.min(a.max(b));
        }
    }
    (from < to).then(|| [point + direction * from, point + direction * to])
}

/// The lines of one family, all running at `angle` and `spacing` apart, one of them through `origin`. Each comes
/// with its number counted from that one (negative on one side, positive on the other).
#[must_use]
pub fn family(rect: Rect, origin: Pos2, angle: f32, spacing: f32) -> Vec<(i64, Segment)> {
    if spacing <= 0.0 {
        return Vec::new();
    }
    let normal = vec2(-angle.sin(), angle.cos());
    let offsets = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ]
    .map(|corner| (corner - origin).dot(normal));
    let low = offsets.iter().copied().fold(f32::INFINITY, f32::min);
    let high = offsets.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let (first, last) = (
        (low / spacing).floor() as i64,
        (high / spacing).ceil() as i64,
    );
    if last - first > MAX_LINES {
        return Vec::new();
    }
    (first..=last)
        .filter_map(|k| {
            clip_line(rect, origin + normal * (k as f32 * spacing), angle).map(|s| (k, s))
        })
        .collect()
}

fn weight_of(k: i64, every: i64) -> Weight {
    if k.rem_euclid(every.max(1)) == 0 {
        Weight::Major
    } else {
        Weight::Minor
    }
}

/// Lines through `vanish` at every `step` radians from `from` to `to` around `center_angle`; each is numbered by
/// how many steps it is from the center one.
fn fan(
    rect: Rect,
    vanish: Pos2,
    center_angle: f32,
    reach: f32,
    step: f32,
    every: i64,
) -> Vec<(Segment, Weight)> {
    let most = (reach / step).floor() as i64;
    (-most..=most)
        .filter_map(|k| {
            clip_line(rect, vanish, center_angle + k as f32 * step)
                .map(|s| (s, weight_of(k, every)))
        })
        .collect()
}

/// Every line of the patterns that are made of lines, with how strong each is. Dots, and the plain grid and ruled
/// lines (which are cheaper to draw as they are), give none.
#[must_use]
pub fn line_segments(rect: Rect, origin: Pos2, ruling: &Ruling) -> Vec<(Segment, Weight)> {
    let spacing = ruling.spacing;
    let every = ruling.major_every;
    let turn = ruling.turn;
    // Every direction below is as the paper lies unturned; the pattern is then turned about `origin`.
    let lattice = |angles: [f32; 3]| -> Vec<(Segment, Weight)> {
        angles
            .into_iter()
            .flat_map(|angle| family(rect, origin, angle + turn, spacing))
            .map(|(k, s)| (s, weight_of(k, every)))
            .collect()
    };
    let zoom = ruling.zoom.max(1e-4);
    let step = ((spacing / zoom) / 6.0).clamp(1.5, 20.0).to_radians();
    let spread = VANISH_SPREAD * zoom;
    let horizon = clip_line(rect, origin, turn).map(|s| (s, Weight::Horizon));
    let front = |angle: f32| {
        family(rect, origin, angle + turn, spacing)
            .into_iter()
            .map(|(_, s)| (s, Weight::Minor))
    };
    // A vanishing point `across` to the right of the origin and `down` below it, turned with the paper.
    let vanishing = |across: f32, down: f32| {
        let (sin, cos) = turn.sin_cos();
        pos2(
            origin.x + across * cos - down * sin,
            origin.y + across * sin + down * cos,
        )
    };
    match ruling.pattern {
        Pattern::Isometric => lattice([FRAC_PI_2, FRAC_PI_6, PI - FRAC_PI_6]),
        Pattern::Triangle => lattice([0.0, FRAC_PI_3, 2.0 * FRAC_PI_3]),
        Pattern::Perspective1 => {
            let mut all: Vec<_> = horizon.into_iter().collect();
            all.extend(fan(rect, origin, turn, FRAC_PI_2, step, every));
            all.extend(front(0.0));
            all.extend(front(FRAC_PI_2));
            all
        }
        Pattern::Perspective2 | Pattern::Perspective3 => {
            let mut all: Vec<_> = horizon.into_iter().collect();
            let reach = if ruling.pattern == Pattern::Perspective3 {
                60_f32.to_radians()
            } else {
                75_f32.to_radians()
            };
            for side in [-spread, spread] {
                all.extend(fan(rect, vanishing(side, 0.0), turn, reach, step, every));
            }
            if ruling.pattern == Pattern::Perspective3 {
                // The third vanishing point is below the paper, and its lines come up to meet the page.
                all.extend(fan(
                    rect,
                    vanishing(0.0, spread),
                    turn - FRAC_PI_2,
                    reach,
                    step,
                    every,
                ));
            } else {
                all.extend(front(FRAC_PI_2));
            }
            all
        }
        Pattern::Blank | Pattern::Dots | Pattern::Grid | Pattern::Lines => Vec::new(),
    }
}

/// The dots of a lattice `spacing` apart, one of them at `origin`, its rows turned by `turn` radians, that are
/// inside `rect`.
#[must_use]
pub fn dots(rect: Rect, origin: Pos2, spacing: f32, turn: f32) -> Vec<Pos2> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return Vec::new();
    }
    let (across, down) = (vec2(turn.cos(), turn.sin()), vec2(-turn.sin(), turn.cos()));
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    let range = |axis: Vec2| {
        let along = corners.map(|c| (c - origin).dot(axis) / spacing);
        (
            along.iter().copied().fold(f32::INFINITY, f32::min).floor() as i64,
            along
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil() as i64,
        )
    };
    let ((i0, i1), (j0, j1)) = (range(across), range(down));
    if (i1 - i0 + 1) * (j1 - j0 + 1) > MAX_DOTS {
        return Vec::new();
    }
    (j0..=j1)
        .flat_map(|j| (i0..=i1).map(move |i| (i, j)))
        .map(|(i, j)| origin + across * (i as f32 * spacing) + down * (j as f32 * spacing))
        .filter(|p| rect.contains(*p))
        .collect()
}

/// The first position at or after `start` that lines up with `origin`.
fn first(start: f32, origin: f32, spacing: f32) -> f32 {
    start + (origin - start).rem_euclid(spacing)
}

/// How many steps of `spacing` position `at` is from `origin`.
fn index(at: f32, origin: f32, spacing: f32) -> i64 {
    ((at - origin) / spacing).round() as i64
}

/// Paints dots, a grid or ruled lines that are turned, along the turn. `ink` is the color for a strength of
/// `0.0..=255.0`.
fn paint_turned(
    painter: &Painter,
    rect: Rect,
    origin: Pos2,
    ruling: &Ruling,
    ink: &dyn Fn(f32) -> Color32,
) {
    let (spacing, width) = (ruling.spacing, ruling.weight.max(0.25));
    match ruling.pattern {
        Pattern::Dots => {
            let size = (spacing * 0.07).clamp(1.5, 2.6) * width.max(1.0);
            for at in dots(rect, origin, spacing, ruling.turn) {
                painter.rect_filled(
                    Rect::from_center_size(at, vec2(size, size)),
                    CornerRadius::same((size / 2.0).ceil() as u8),
                    ink(190.0),
                );
            }
        }
        Pattern::Grid => {
            let every = ruling.major_every.max(1);
            for angle in [ruling.turn, ruling.turn + FRAC_PI_2] {
                for (k, segment) in family(rect, origin, angle, spacing) {
                    let strength = if k.rem_euclid(every) == 0 {
                        120.0
                    } else {
                        58.0
                    };
                    painter.line_segment(segment, Stroke::new(width, ink(strength)));
                }
            }
        }
        _ => {
            let line = Stroke::new(width, ink(96.0));
            for (_, segment) in family(rect, origin, ruling.turn, spacing) {
                painter.line_segment(segment, line);
            }
        }
    }
}

/// Paints `ruling` over `rect`, with a dot or line crossing at `origin`.
pub fn paint(painter: &Painter, rect: Rect, origin: Pos2, ruling: &Ruling) {
    let spacing = ruling.spacing;
    let perspective = ruling.pattern.is_perspective();
    if ruling.pattern == Pattern::Blank
        || (!perspective && spacing < ruling.min_spacing)
        || spacing < 2.0
    {
        return;
    }
    let alpha = |scale: f32| (ruling.strength.clamp(0.0, 1.0) * scale).round() as u8;
    let tint =
        |a: u8| Color32::from_rgba_unmultiplied(ruling.ink.r(), ruling.ink.g(), ruling.ink.b(), a);
    let width = ruling.weight.max(0.25);
    let painter = painter.with_clip_rect(rect);
    let (x0, y0) = (
        first(rect.left(), origin.x, spacing),
        first(rect.top(), origin.y, spacing),
    );
    // Turned paper has no rows and columns that are level, so its dots and lines are laid out along the turn.
    let turned = ruling.turn.abs() > 1e-4;
    if turned
        && matches!(
            ruling.pattern,
            Pattern::Dots | Pattern::Grid | Pattern::Lines
        )
    {
        paint_turned(&painter, rect, origin, ruling, &|scale| tint(alpha(scale)));
        return;
    }
    match ruling.pattern {
        Pattern::Blank => {}
        Pattern::Dots => {
            let size = (spacing * 0.07).clamp(1.5, 2.6) * width.max(1.0);
            let color = tint(alpha(190.0));
            let mut y = y0;
            while y <= rect.bottom() {
                let mut x = x0;
                while x <= rect.right() {
                    painter.rect_filled(
                        Rect::from_center_size(pos2(x, y), vec2(size, size)),
                        CornerRadius::same((size / 2.0).ceil() as u8),
                        color,
                    );
                    x += spacing;
                }
                y += spacing;
            }
        }
        Pattern::Grid => {
            let line =
                |major: bool| Stroke::new(width, tint(alpha(if major { 120.0 } else { 58.0 })));
            let every = ruling.major_every.max(1);
            let mut x = x0;
            while x <= rect.right() {
                let major = index(x, origin.x, spacing).rem_euclid(every) == 0;
                painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], line(major));
                x += spacing;
            }
            let mut y = y0;
            while y <= rect.bottom() {
                let major = index(y, origin.y, spacing).rem_euclid(every) == 0;
                painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], line(major));
                y += spacing;
            }
        }
        Pattern::Lines => {
            let line = Stroke::new(width, tint(alpha(96.0)));
            let mut y = y0;
            while y <= rect.bottom() {
                painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], line);
                y += spacing;
            }
        }
        Pattern::Isometric
        | Pattern::Triangle
        | Pattern::Perspective1
        | Pattern::Perspective2
        | Pattern::Perspective3 => {
            for (segment, weight) in line_segments(rect, origin, ruling) {
                let (strength, thickness) = match weight {
                    Weight::Minor => (58.0, width),
                    Weight::Major => (120.0, width),
                    Weight::Horizon => (190.0, width * 1.4),
                };
                painter.line_segment(segment, Stroke::new(thickness, tint(alpha(strength))));
            }
        }
    }
}

/// The pattern color that shows on paper of the given color: dark ink on light paper, light on dark.
#[must_use]
pub fn ink_for(paper: Color32) -> Color32 {
    let luma =
        0.299 * f32::from(paper.r()) + 0.587 * f32::from(paper.g()) + 0.114 * f32::from(paper.b());
    if luma > 128.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0))
    }

    fn ruling(pattern: Pattern) -> Ruling {
        Ruling {
            pattern,
            spacing: 20.0,
            strength: 1.0,
            ink: Color32::BLACK,
            min_spacing: 0.0,
            major_every: 4,
            zoom: 1.0,
            weight: 1.0,
            turn: 0.0,
        }
    }

    /// How far `point` is from the endless line through the two ends of `segment`.
    fn off_the_line(segment: Segment, point: Pos2) -> f32 {
        let along = (segment[1] - segment[0]).normalized();
        let to = point - segment[0];
        (to.x * along.y - to.y * along.x).abs()
    }

    #[test]
    fn turned_dots_are_a_lattice_that_runs_along_the_turn_and_stays_in_the_window() {
        let turn = 0.5_f32;
        let origin = pos2(150.0, 120.0);
        let found = dots(view(), origin, 20.0, turn);
        assert!(found.iter().all(|p| view().contains(*p)));
        assert!(found.contains(&origin), "one dot is at the origin");
        // The window is 400 by 300 with a dot to each 20 by 20: about 300, wherever it is turned.
        assert!((250..=350).contains(&found.len()), "{}", found.len());
        // A dot a step along the turn from the origin is in the lattice, and one halfway is not.
        let step = origin + vec2(turn.cos(), turn.sin()) * 20.0;
        assert!(found.iter().any(|p| p.distance(step) < 1e-3));
        let half = origin + vec2(turn.cos(), turn.sin()) * 10.0;
        assert!(!found.iter().any(|p| p.distance(half) < 1.0));
        // Not turned, it is the ordinary square lattice.
        let level = dots(view(), origin, 20.0, 0.0);
        assert!(
            level
                .iter()
                .all(|p| ((p.x - origin.x) / 20.0).fract().abs() < 1e-3)
        );
    }

    #[test]
    fn a_lattice_nobody_could_see_or_that_is_nonsense_has_no_dots() {
        assert_eq!(dots(view(), pos2(0.0, 0.0), 0.0, 0.3).len(), 0);
        assert_eq!(dots(view(), pos2(0.0, 0.0), f32::NAN, 0.3).len(), 0);
        assert_eq!(dots(view(), pos2(0.0, 0.0), -4.0, 0.3).len(), 0);
        assert_eq!(
            dots(view(), pos2(0.0, 0.0), 0.01, 0.3).len(),
            0,
            "too many to be a pattern"
        );
    }

    #[test]
    fn a_turned_pattern_has_its_lines_along_the_turn() {
        let turn = 0.4_f32;
        let turned = Ruling {
            turn,
            ..ruling(Pattern::Isometric)
        };
        let upright = line_segments(view(), pos2(200.0, 150.0), &ruling(Pattern::Isometric));
        let segments = line_segments(view(), pos2(200.0, 150.0), &turned);
        assert_eq!(
            segments.len(),
            upright.len(),
            "the same number of lines, turned"
        );
        // Every line of the upright pattern is at an angle 30, 90 or 150 degrees; turned, each is 0.4 more.
        let angle = |s: Segment| {
            let d = s[1] - s[0];
            d.y.atan2(d.x).rem_euclid(PI)
        };
        for (segment, _) in &segments {
            let degrees = (angle(*segment) - turn).rem_euclid(PI).to_degrees();
            let near = |target: f32| {
                (degrees - target).abs() < 0.05 || (degrees - target - 180.0).abs() < 0.05
            };
            assert!(near(30.0) || near(90.0) || near(150.0), "{degrees}");
        }
    }

    #[test]
    fn the_horizon_and_the_vanishing_points_turn_with_the_paper() {
        let turn = -0.3_f32;
        let origin = pos2(200.0, 150.0);
        let turned = Ruling {
            turn,
            ..ruling(Pattern::Perspective2)
        };
        let segments = line_segments(
            Rect::from_min_size(pos2(-3000.0, -3000.0), vec2(6000.0, 6000.0)),
            origin,
            &turned,
        );
        let horizon: Vec<_> = segments
            .iter()
            .filter(|(_, w)| *w == Weight::Horizon)
            .collect();
        assert_eq!(horizon.len(), 1);
        let d = horizon[0].0[1] - horizon[0].0[0];
        assert!(
            (d.y.atan2(d.x).rem_euclid(PI) - turn.rem_euclid(PI)).abs() < 1e-3,
            "the horizon runs at the turn"
        );
        // The two vanishing points are on the horizon, one to each side: lines of the fans cross there.
        let on_horizon = |p: Pos2| off_the_line(horizon[0].0, p) < 1e-2;
        let spread = VANISH_SPREAD * turned.zoom;
        let right = origin + vec2(turn.cos(), turn.sin()) * spread;
        assert!(on_horizon(right));
        let through_right = segments
            .iter()
            .filter(|(s, w)| *w != Weight::Horizon && off_the_line(*s, right) < 0.05)
            .count();
        assert!(
            through_right > 3,
            "lines of a fan meet at the point: {through_right}"
        );
    }

    #[test]
    fn lines_up_with_the_origin_whatever_side_the_view_starts_on() {
        assert!((first(0.0, 10.0, 32.0) - 10.0).abs() < 1e-4);
        assert!((first(0.0, -10.0, 32.0) - 22.0).abs() < 1e-4);
        assert!((first(100.0, 20.0, 32.0) - 116.0).abs() < 1e-4);
    }

    #[test]
    fn grid_steps_count_from_the_origin() {
        assert_eq!(index(64.0 + 5.0, 5.0, 32.0), 2);
        assert_eq!(index(-27.0, 5.0, 32.0), -1);
    }

    #[test]
    fn dark_paper_gets_light_ink() {
        assert_eq!(ink_for(Color32::from_rgb(0xF9, 0xF9, 0xF9)), Color32::BLACK);
        assert_eq!(ink_for(Color32::from_rgb(0x10, 0x20, 0x30)), Color32::WHITE);
    }

    #[test]
    fn an_endless_line_is_cut_where_it_enters_and_leaves_the_view() {
        let level = clip_line(view(), pos2(50.0, 100.0), 0.0).expect("crosses");
        assert!((level[0].x - 0.0).abs() < 1e-3 && (level[1].x - 400.0).abs() < 1e-3);
        assert!((level[0].y - 100.0).abs() < 1e-3);
        let upright = clip_line(view(), pos2(120.0, 50.0), FRAC_PI_2).expect("crosses");
        assert!((upright[0].y - 0.0).abs() < 1e-3 && (upright[1].y - 300.0).abs() < 1e-3);
        let slanted =
            clip_line(view(), pos2(200.0, 150.0), std::f32::consts::FRAC_PI_4).expect("crosses");
        for end in slanted {
            assert!(view().expand(0.01).contains(end), "{end:?}");
        }
        assert!(
            clip_line(view(), pos2(0.0, 500.0), 0.0).is_none(),
            "below the view"
        );
        assert!(
            clip_line(view(), pos2(-50.0, 0.0), FRAC_PI_2).is_none(),
            "left of the view"
        );
    }

    #[test]
    fn a_family_covers_the_view_with_evenly_spaced_lines_counted_from_the_origin() {
        let lines = family(view(), pos2(0.0, 0.0), 0.0, 20.0);
        assert_eq!(lines.len(), 16, "0 to 300 in steps of 20, both ends");
        assert_eq!(lines[0].0, 0);
        assert!((lines[3].1[0].y - 60.0).abs() < 1e-3);
        let shifted = family(view(), pos2(0.0, 5.0), 0.0, 20.0);
        assert!(
            (shifted[0].1[0].y - 5.0).abs() < 1e-3,
            "lined up with the origin"
        );
        assert_eq!(family(view(), pos2(0.0, 0.0), 0.0, 0.0).len(), 0);
        assert!(
            family(view(), pos2(0.0, 0.0), 0.0, 0.01).is_empty(),
            "too many lines to be a pattern"
        );
    }

    #[test]
    fn the_three_families_of_a_lattice_meet_in_points() {
        // Wherever two lines of an isometric or a triangle lattice cross, so does the third: that is what makes
        // the cells triangles and not slivers.
        for angles in [
            [FRAC_PI_2, FRAC_PI_6, PI - FRAC_PI_6],
            [0.0, FRAC_PI_3, 2.0 * FRAC_PI_3],
        ] {
            let spacing = 20.0;
            let normal = |a: f32| vec2(-a.sin(), a.cos());
            let (n0, n1, n2) = (normal(angles[0]), normal(angles[1]), normal(angles[2]));
            for (a, b) in [(1.0_f32, 2.0_f32), (-3.0, 1.0), (4.0, 4.0)] {
                // The point on line `a` of the first family and line `b` of the second.
                let det = n0.x * n1.y - n0.y * n1.x;
                let (da, db) = (a * spacing, b * spacing);
                let p = vec2((da * n1.y - db * n0.y) / det, (n0.x * db - n1.x * da) / det);
                let third = p.dot(n2) / spacing;
                assert!(
                    (third - third.round()).abs() < 1e-3,
                    "{angles:?}: the third family passes {third} lines along, not a whole number"
                );
            }
        }
    }

    #[test]
    fn every_line_pattern_gives_lines_inside_the_view_and_the_others_give_none() {
        for pattern in Pattern::ALL {
            let segments = line_segments(view(), pos2(200.0, 150.0), &ruling(pattern));
            let made_of_lines = matches!(
                pattern,
                Pattern::Isometric
                    | Pattern::Triangle
                    | Pattern::Perspective1
                    | Pattern::Perspective2
                    | Pattern::Perspective3
            );
            assert_eq!(!segments.is_empty(), made_of_lines, "{pattern:?}");
            for (segment, _) in segments {
                for end in segment {
                    assert!(view().expand(0.01).contains(end), "{pattern:?} {end:?}");
                }
            }
        }
    }

    #[test]
    fn main_lines_come_every_so_many_lines_and_one_makes_them_all_main() {
        let count = |every: i64, weight: Weight| {
            let mut r = ruling(Pattern::Triangle);
            r.major_every = every;
            line_segments(view(), pos2(200.0, 150.0), &r)
                .iter()
                .filter(|(_, w)| *w == weight)
                .count()
        };
        assert!(count(4, Weight::Major) > 0 && count(4, Weight::Minor) > count(4, Weight::Major));
        assert_eq!(count(1, Weight::Minor), 0, "every line is a main line");
    }

    #[test]
    fn perspective_lines_all_run_to_a_vanishing_point() {
        let origin = pos2(200.0, 150.0);
        let spread = VANISH_SPREAD;
        let left = pos2(origin.x - spread, origin.y);
        let right = pos2(origin.x + spread, origin.y);
        let below = pos2(origin.x, origin.y + spread);
        for (pattern, points) in [
            (Pattern::Perspective2, vec![left, right]),
            (Pattern::Perspective3, vec![left, right, below]),
        ] {
            let segments = line_segments(view(), origin, &ruling(pattern));
            // Vanishing points far off the paper leave only a few rays crossing a small view.
            assert!(segments.len() > 8, "{pattern:?} has {}", segments.len());
            let fans = segments
                .iter()
                .filter(|(_, w)| *w != Weight::Horizon)
                .filter(|(s, _)| {
                    // The upright lines of a two point grid run to a point infinitely far up and down.
                    (s[1].x - s[0].x).abs() > 1e-3 || pattern == Pattern::Perspective3
                });
            for (segment, _) in fans {
                let near = points
                    .iter()
                    .map(|p| off_the_line(*segment, *p))
                    .fold(f32::INFINITY, f32::min);
                assert!(
                    near < 0.5,
                    "{pattern:?}: a line misses every vanishing point by {near}"
                );
            }
        }
        let one = line_segments(view(), origin, &ruling(Pattern::Perspective1));
        let through_center = one
            .iter()
            .filter(|(s, w)| *w != Weight::Horizon && off_the_line(*s, origin) < 0.5)
            .count();
        assert!(
            through_center > 10,
            "a one point grid has a fan from the middle of the paper"
        );
    }

    #[test]
    fn the_horizon_is_the_strongest_line_and_there_is_exactly_one() {
        for pattern in [
            Pattern::Perspective1,
            Pattern::Perspective2,
            Pattern::Perspective3,
        ] {
            let segments = line_segments(view(), pos2(200.0, 150.0), &ruling(pattern));
            let horizons: Vec<_> = segments
                .iter()
                .filter(|(_, w)| *w == Weight::Horizon)
                .collect();
            assert_eq!(horizons.len(), 1, "{pattern:?}");
            assert!((horizons[0].0[0].y - 150.0).abs() < 1e-3);
        }
    }

    #[test]
    fn a_horizon_off_the_view_is_left_out_and_nothing_breaks() {
        let segments = line_segments(view(), pos2(200.0, 900.0), &ruling(Pattern::Perspective2));
        assert!(segments.iter().all(|(_, w)| *w != Weight::Horizon));
    }
}
