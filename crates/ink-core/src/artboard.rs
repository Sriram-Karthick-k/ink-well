//! The artboard: a page of a chosen size on the canvas, that drawing can be kept inside and that the grid can be
//! kept inside.
//!
//! An artboard has a size, and presets in groups (A series, US paper, cards, screens, social posts, phones and so
//! on). It can confine drawing to it and show the grid only inside it. The sizes of the presets are the ordinary
//! ones for those names.

use serde::{Deserialize, Serialize};

use crate::geometry::Point;
use crate::units::Unit;

/// The smallest an artboard can be, in canvas units.
const MIN_SIDE: f32 = 16.0;
/// The largest, in canvas units.
const MAX_SIDE: f32 = 100_000.0;

/// A page on the canvas, as the corner at the bottom left and the one at the top right (canvas y points up).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Artboard {
    pub min: Point,
    pub max: Point,
    /// A stroke is kept inside it: it can only begin there, and where the pen goes outside the stroke stays on
    /// the edge.
    pub confine: bool,
    /// The pattern of the paper is only drawn inside it.
    pub grid_inside: bool,
}

impl Artboard {
    /// An artboard `width` by `height` centered on `center`, with the sides kept to sensible sizes.
    #[must_use]
    pub fn centered(center: Point, width: f32, height: f32) -> Self {
        let side = |v: f32| {
            if v.is_finite() {
                v.clamp(MIN_SIDE, MAX_SIDE)
            } else {
                MIN_SIDE
            }
        };
        let (w, h) = (side(width), side(height));
        Self {
            min: Point::new(center.x - w / 2.0, center.y - h / 2.0),
            max: Point::new(center.x + w / 2.0, center.y + h / 2.0),
            confine: false,
            grid_inside: false,
        }
    }

    #[must_use]
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    #[must_use]
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    #[must_use]
    pub fn center(&self) -> Point {
        Point::new(
            f32::midpoint(self.min.x, self.max.x),
            f32::midpoint(self.min.y, self.max.y),
        )
    }

    /// The same artboard made `width` by `height` about its center.
    #[must_use]
    pub fn resized(&self, width: f32, height: f32) -> Self {
        Self {
            confine: self.confine,
            grid_inside: self.grid_inside,
            ..Self::centered(self.center(), width, height)
        }
    }

    /// The same artboard turned on its side: width and height swapped about its center.
    #[must_use]
    pub fn swapped(&self) -> Self {
        self.resized(self.height(), self.width())
    }

    #[must_use]
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    /// The nearest point of the artboard to `p`: `p` itself if it is inside.
    #[must_use]
    pub fn clamp(&self, p: Point) -> Point {
        Point::new(
            p.x.clamp(self.min.x, self.max.x),
            p.y.clamp(self.min.y, self.max.y),
        )
    }

    /// The box as left, bottom, right and top.
    #[must_use]
    pub fn bounds(&self) -> [f32; 4] {
        [self.min.x, self.min.y, self.max.x, self.max.y]
    }
}

/// A size an artboard can be made, with a name and the group the name is under.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    pub group: &'static str,
    pub name: &'static str,
    pub width: f32,
    pub height: f32,
    /// What `width` and `height` are in.
    pub unit: Unit,
}

impl Preset {
    /// The size in canvas units, given how many canvas units make an inch.
    #[must_use]
    pub fn size(&self, pixels_per_inch: f32) -> (f32, f32) {
        let per = self.unit.inches(pixels_per_inch) * pixels_per_inch;
        (self.width * per, self.height * per)
    }
}

const fn preset(
    group: &'static str,
    name: &'static str,
    width: f32,
    height: f32,
    unit: Unit,
) -> Preset {
    Preset {
        group,
        name,
        width,
        height,
        unit,
    }
}

/// The presets, in the order they are listed.
pub const PRESETS: &[Preset] = &[
    preset("A series", "A0", 841.0, 1189.0, Unit::Mm),
    preset("A series", "A1", 594.0, 841.0, Unit::Mm),
    preset("A series", "A2", 420.0, 594.0, Unit::Mm),
    preset("A series", "A3", 297.0, 420.0, Unit::Mm),
    preset("A series", "A4", 210.0, 297.0, Unit::Mm),
    preset("A series", "A5", 148.0, 210.0, Unit::Mm),
    preset("A series", "A6", 105.0, 148.0, Unit::Mm),
    preset("A series", "A7", 74.0, 105.0, Unit::Mm),
    preset("US paper", "Letter", 8.5, 11.0, Unit::In),
    preset("US paper", "Legal", 8.5, 14.0, Unit::In),
    preset("US paper", "Tabloid", 11.0, 17.0, Unit::In),
    preset("Cards", "Business card", 3.5, 2.0, Unit::In),
    preset("Cards", "Postcard", 6.0, 4.0, Unit::In),
    preset("Screen", "HD", 1920.0, 1080.0, Unit::Px),
    preset("Screen", "4K", 3840.0, 2160.0, Unit::Px),
    preset("Screen", "Surface", 2736.0, 1824.0, Unit::Px),
    preset("Phones and tablets", "iPhone", 1170.0, 2532.0, Unit::Px),
    preset("Phones and tablets", "iPad", 2048.0, 2732.0, Unit::Px),
    preset("Phones and tablets", "Pixel 4", 1080.0, 2280.0, Unit::Px),
    preset("Phones and tablets", "Pixel 4 XL", 1440.0, 3040.0, Unit::Px),
    preset("Social", "Instagram Post", 1080.0, 1080.0, Unit::Px),
    preset("Social", "Instagram Story", 1080.0, 1920.0, Unit::Px),
    preset("Social", "Facebook Post", 1200.0, 630.0, Unit::Px),
    preset("Social", "Facebook Cover", 820.0, 312.0, Unit::Px),
    preset("Social", "Twitter Post", 1200.0, 675.0, Unit::Px),
    preset("Social", "Twitter Header", 1500.0, 500.0, Unit::Px),
];

/// The groups of [`PRESETS`], in order, each once.
#[must_use]
pub fn groups() -> Vec<&'static str> {
    let mut found: Vec<&'static str> = Vec::new();
    for p in PRESETS {
        if !found.contains(&p.group) {
            found.push(p.group);
        }
    }
    found
}

/// The preset that has the size `width` by `height` (either way up) in canvas units, if one does, to within half
/// a unit.
#[must_use]
pub fn matching(width: f32, height: f32, pixels_per_inch: f32) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| {
        let (w, h) = p.size(pixels_per_inch);
        let close = |a: f32, b: f32| (a - b).abs() < 0.5;
        (close(w, width) && close(h, height)) || (close(w, height) && close(h, width))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Artboard {
        Artboard::centered(Point::new(10.0, 20.0), 200.0, 100.0)
    }

    #[test]
    fn an_artboard_is_centered_where_asked_and_the_size_asked_for() {
        let b = board();
        assert_eq!((b.width(), b.height()), (200.0, 100.0));
        assert_eq!(b.center(), Point::new(10.0, 20.0));
        assert_eq!(b.bounds(), [-90.0, -30.0, 110.0, 70.0]);
        assert!(!b.confine && !b.grid_inside);
    }

    #[test]
    fn the_sides_are_kept_to_sensible_sizes_whatever_is_asked_for() {
        let tiny = Artboard::centered(Point::new(0.0, 0.0), 0.0, -5.0);
        assert!((tiny.width() - MIN_SIDE).abs() < 1e-4 && (tiny.height() - MIN_SIDE).abs() < 1e-4);
        let huge = Artboard::centered(Point::new(0.0, 0.0), 1.0e12, 1.0e12);
        assert!((huge.width() - MAX_SIDE).abs() < 1.0);
        let nonsense = Artboard::centered(Point::new(0.0, 0.0), f32::NAN, f32::INFINITY);
        assert!(nonsense.width().is_finite() && nonsense.height().is_finite());
    }

    #[test]
    fn resizing_and_turning_on_its_side_keep_the_center_and_the_options() {
        let mut b = board();
        b.confine = true;
        b.grid_inside = true;
        let swapped = b.swapped();
        assert_eq!((swapped.width(), swapped.height()), (100.0, 200.0));
        assert_eq!(swapped.center(), b.center());
        assert!(swapped.confine && swapped.grid_inside);
        let bigger = b.resized(400.0, 300.0);
        assert_eq!(bigger.center(), b.center());
        assert!(bigger.confine);
        assert_eq!(b.swapped().swapped(), b);
    }

    #[test]
    fn a_point_is_inside_or_is_brought_to_the_nearest_place_on_the_edge() {
        let b = board();
        assert!(b.contains(Point::new(0.0, 0.0)) && b.contains(Point::new(110.0, 70.0)));
        assert!(!b.contains(Point::new(111.0, 0.0)));
        assert_eq!(b.clamp(Point::new(0.0, 0.0)), Point::new(0.0, 0.0));
        assert_eq!(b.clamp(Point::new(500.0, 0.0)), Point::new(110.0, 0.0));
        assert_eq!(
            b.clamp(Point::new(-500.0, -500.0)),
            Point::new(-90.0, -30.0)
        );
    }

    #[test]
    fn a_preset_is_its_size_in_canvas_units_at_the_pixels_per_inch() {
        let a4 = PRESETS.iter().find(|p| p.name == "A4").expect("A4");
        let (w, h) = a4.size(96.0);
        // 210 by 297 millimeters at 96 pixels to the inch.
        assert!(
            (w - 210.0 / 25.4 * 96.0).abs() < 0.01 && (h - 297.0 / 25.4 * 96.0).abs() < 0.01,
            "{w} {h}"
        );
        let letter = PRESETS.iter().find(|p| p.name == "Letter").expect("Letter");
        assert_eq!(letter.size(96.0), (816.0, 1056.0));
        let hd = PRESETS.iter().find(|p| p.name == "HD").expect("HD");
        assert_eq!(
            hd.size(96.0),
            (1920.0, 1080.0),
            "pixels are pixels at any density"
        );
        assert_eq!(hd.size(300.0), (1920.0, 1080.0));
    }

    #[test]
    fn the_groups_come_once_each_in_order_and_every_name_is_in_one() {
        let groups = groups();
        assert_eq!(groups.first(), Some(&"A series"));
        assert_eq!(
            groups.len(),
            groups
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
        );
        for p in PRESETS {
            assert!(groups.contains(&p.group), "{}", p.name);
            assert!(p.width > 0.0 && p.height > 0.0);
        }
        let mut names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), PRESETS.len(), "no name twice");
    }

    #[test]
    fn a_size_is_matched_to_its_preset_either_way_up_or_not_at_all() {
        let (w, h) = PRESETS
            .iter()
            .find(|p| p.name == "A4")
            .expect("A4")
            .size(96.0);
        assert_eq!(matching(w, h, 96.0).map(|p| p.name), Some("A4"));
        assert_eq!(
            matching(h, w, 96.0).map(|p| p.name),
            Some("A4"),
            "on its side"
        );
        assert_eq!(matching(w + 5.0, h, 96.0), None);
    }
}
