//! The numbers behind the selection: where it is, how big, and how far to turn it.
//!
//! The pure part, which says what each field does to the strokes; `selection_bar.rs` draws the fields. Positions
//! are the way the screen counts them (`y` grows downward), whatever way up the canvas is.

use ink_core::{Affine, Point};

/// A box smaller than this on one side has nothing to stretch along it.
const FLAT: f32 = 1e-3;

/// The smallest the box may be made on either side, in canvas units.
pub const SMALLEST: f32 = 1.0;

/// One of the four numbers that say where the selection is and how large.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    X,
    Y,
    Width,
    Height,
}

impl Field {
    pub const ALL: [Self; 4] = [Self::X, Self::Y, Self::Width, Self::Height];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Width => "W",
            Self::Height => "H",
        }
    }
}

/// The four numbers of a box (left, bottom, right, top on the canvas): its left, its top counted downward, its
/// width and its height.
#[must_use]
pub fn read(bounds: [f32; 4]) -> [f32; 4] {
    [
        bounds[0],
        -bounds[3],
        bounds[2] - bounds[0],
        bounds[3] - bounds[1],
    ]
}

/// What `field` is now, for a box (left, bottom, right, top on the canvas).
#[must_use]
pub fn value(bounds: [f32; 4], field: Field) -> f32 {
    let numbers = read(bounds);
    match field {
        Field::X => numbers[0],
        Field::Y => numbers[1],
        Field::Width => numbers[2],
        Field::Height => numbers[3],
    }
}

/// The change that makes `field` of the box `bounds` equal `to`: a move for `X` and `Y`, and for the size a
/// stretch that keeps the top left corner where it is. With `keep_shape` the other side grows as much, so the
/// strokes keep their proportions. `None` when there is nothing to do: the number is not one, the side is flat
/// and cannot be stretched, or it is what it was.
#[must_use]
pub fn fit(bounds: [f32; 4], field: Field, to: f32, keep_shape: bool) -> Option<Affine> {
    if !to.is_finite() {
        return None;
    }
    let [left, bottom, right, top] = bounds;
    let (width, height) = (right - left, top - bottom);
    let m = match field {
        Field::X => Affine::translate(to - left, 0.0),
        Field::Y => Affine::translate(0.0, -to - top),
        Field::Width | Field::Height => {
            let (along, across, wide) = if field == Field::Width {
                (width, height, true)
            } else {
                (height, width, false)
            };
            if along < FLAT {
                return None;
            }
            let factor = to.max(SMALLEST) / along;
            let other = if keep_shape && across >= FLAT {
                factor
            } else {
                1.0
            };
            let (sx, sy) = if wide {
                (factor, other)
            } else {
                (other, factor)
            };
            Affine {
                a: sx,
                b: 0.0,
                c: 0.0,
                d: sy,
                e: left - left * sx,
                f: top - top * sy,
            }
        }
    };
    (m != Affine::IDENTITY).then_some(m)
}

/// Turns the strokes by `degrees` counterclockwise as seen, around `pivot`.
#[must_use]
pub fn turn(pivot: Point, degrees: f32) -> Affine {
    Affine::rotate_about(pivot, degrees.to_radians())
}

/// The middle of a box (left, bottom, right, top on the canvas).
#[must_use]
pub fn middle(bounds: [f32; 4]) -> Point {
    Point::new(
        f32::midpoint(bounds[0], bounds[2]),
        f32::midpoint(bounds[1], bounds[3]),
    )
}

/// The edit the fields are in the middle of: all the changes of one drag, or one typed number, are a single step
/// of history.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Session {
    /// The place in the history where the edit began.
    pub mark: usize,
    /// The revision the drawing was at after the last change, so that something else (an undo, a stroke) ends it.
    pub revision: u64,
    /// What turns go around, fixed so the selection does not wander as its box changes.
    pub pivot: Point,
}

/// The state of the fields under the selection bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeFields {
    /// The fields are showing.
    pub open: bool,
    /// Stretching one side stretches the other as much.
    pub keep_shape: bool,
    /// How far the turn field has been turned in this edit; back to nothing when the edit is over.
    pub turned: f32,
    pub session: Option<Session>,
}

impl Default for SizeFields {
    fn default() -> Self {
        Self {
            open: false,
            keep_shape: true,
            turned: 0.0,
            session: None,
        }
    }
}

impl SizeFields {
    /// The edit is over (the number was let go of or typed in): the next change is a new step.
    pub fn finish(&mut self) {
        self.session = None;
        self.turned = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box 100 wide and 40 high with its left at 10 and its top at 60 (so, counted downward, at y = -60).
    const BOX: [f32; 4] = [10.0, 20.0, 110.0, 60.0];

    fn apply(m: Affine, b: [f32; 4]) -> [f32; 4] {
        let low = m.apply(Point::new(b[0], b[1]));
        let high = m.apply(Point::new(b[2], b[3]));
        [
            low.x.min(high.x),
            low.y.min(high.y),
            low.x.max(high.x),
            low.y.max(high.y),
        ]
    }

    #[test]
    fn the_fields_read_the_way_the_screen_counts() {
        assert_eq!(read(BOX), [10.0, -60.0, 100.0, 40.0]);
        assert!(
            (value(BOX, Field::Y) + 60.0).abs() < 1e-4,
            "y grows downward"
        );
    }

    #[test]
    fn x_and_y_move_the_box_to_the_number() {
        let moved = apply(fit(BOX, Field::X, 50.0, true).expect("a move"), BOX);
        assert_eq!(moved, [50.0, 20.0, 150.0, 60.0]);
        let moved = apply(fit(BOX, Field::Y, -10.0, true).expect("a move"), BOX);
        assert_eq!(
            read(moved)[1],
            -10.0,
            "its top is now 10 above the origin: {moved:?}"
        );
        assert_eq!(
            read(moved)[2..],
            [100.0, 40.0],
            "and it did not change size"
        );
    }

    #[test]
    fn a_size_keeps_the_top_left_corner_and_keeps_the_shape_when_asked() {
        let wider = apply(fit(BOX, Field::Width, 200.0, true).expect("a stretch"), BOX);
        assert_eq!(
            read(wider),
            [10.0, -60.0, 200.0, 80.0],
            "both sides doubled"
        );
        let free = apply(
            fit(BOX, Field::Width, 200.0, false).expect("a stretch"),
            BOX,
        );
        assert_eq!(read(free), [10.0, -60.0, 200.0, 40.0], "only the width");
        let taller = apply(
            fit(BOX, Field::Height, 20.0, false).expect("a stretch"),
            BOX,
        );
        assert_eq!(read(taller), [10.0, -60.0, 100.0, 20.0]);
    }

    #[test]
    fn a_flat_box_cannot_be_stretched_along_its_flat_side_and_the_other_side_stays_put() {
        let line = [0.0, 5.0, 100.0, 5.0];
        assert_eq!(fit(line, Field::Height, 30.0, true), None);
        let longer = apply(
            fit(line, Field::Width, 200.0, true).expect("a stretch"),
            line,
        );
        assert_eq!(longer, [0.0, 5.0, 200.0, 5.0], "no height to keep in step");
    }

    #[test]
    fn nothing_to_do_is_none_and_nonsense_is_refused() {
        assert_eq!(fit(BOX, Field::X, 10.0, true), None, "already there");
        assert_eq!(fit(BOX, Field::Width, 100.0, true), None);
        assert_eq!(fit(BOX, Field::X, f32::NAN, true), None);
        assert_eq!(fit(BOX, Field::Width, f32::INFINITY, true), None);
    }

    #[test]
    fn a_box_cannot_be_made_smaller_than_one_unit() {
        let tiny = apply(fit(BOX, Field::Width, -5.0, false).expect("a stretch"), BOX);
        assert!((read(tiny)[2] - SMALLEST).abs() < 1e-4);
    }

    #[test]
    fn a_turn_is_counterclockwise_as_seen_around_the_pivot() {
        let m = turn(Point::new(0.0, 0.0), 90.0);
        let p = m.apply(Point::new(10.0, 0.0));
        assert!(
            p.x.abs() < 1e-4 && (p.y - 10.0).abs() < 1e-4,
            "{p:?}: a quarter turn takes right to up"
        );
        let pivot = middle(BOX);
        let q = turn(pivot, 37.0).apply(pivot);
        assert!((q.x - pivot.x).abs() < 1e-3 && (q.y - pivot.y).abs() < 1e-3);
    }
}
