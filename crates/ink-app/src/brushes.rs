//! The built-in brushes: their names, groups and starting values.

use ink_core::Pen;

use crate::config::BrushConfig;

/// How the case of pens groups them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Pens,
    Pencils,
    Paint,
    Special,
}

impl Group {
    pub const ALL: [Self; 4] = [Self::Pens, Self::Pencils, Self::Paint, Self::Special];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Pens => "Pens and brush pens",
            Self::Pencils => "Pencils, charcoal and chalk",
            Self::Paint => "Markers and paint",
            Self::Special => "Special",
        }
    }
}

/// What the interface shows about one brush.
#[derive(Debug, Clone, Copy)]
pub struct BrushInfo {
    pub pen: Pen,
    pub name: &'static str,
    pub group: Group,
    /// One line that says how the brush behaves.
    pub blurb: &'static str,
}

const fn brush(pen: Pen, name: &'static str, group: Group, blurb: &'static str) -> BrushInfo {
    BrushInfo {
        pen,
        name,
        group,
        blurb,
    }
}

/// Every brush, in the order the case lists them.
pub const BRUSHES: [BrushInfo; 22] = [
    brush(
        Pen::Pen,
        "Pen",
        Group::Pens,
        "A clean line of constant width",
    ),
    brush(
        Pen::Fountain,
        "Fountain pen",
        Group::Pens,
        "Thick when you slow down, thin when you speed up",
    ),
    brush(
        Pen::DynamicPen,
        "Dynamic pen",
        Group::Pens,
        "Width follows the pressure of the pen",
    ),
    brush(
        Pen::BrushPen,
        "Brush pen",
        Group::Pens,
        "A round tip that swells with pressure",
    ),
    brush(
        Pen::InkBrush,
        "Ink brush",
        Group::Pens,
        "A loaded brush: thick when slow, thin when fast",
    ),
    brush(
        Pen::FixedWidth,
        "Fixed width pen",
        Group::Pens,
        "Always exactly the chosen width",
    ),
    brush(
        Pen::Wire,
        "Wire pen",
        Group::Pens,
        "A hairline for technical drawing",
    ),
    brush(
        Pen::SoftPencil,
        "Soft pencil",
        Group::Pencils,
        "Soft, grainy graphite that builds up",
    ),
    brush(
        Pen::HardPencil,
        "Hard pencil",
        Group::Pencils,
        "Sharper, darker graphite",
    ),
    brush(
        Pen::Charcoal,
        "Charcoal",
        Group::Pencils,
        "Coarse and crumbly, dark where you press",
    ),
    brush(
        Pen::Chalk,
        "Chalk",
        Group::Pencils,
        "Dry and patchy, with a wide scatter",
    ),
    brush(
        Pen::Crayon,
        "Crayon",
        Group::Pencils,
        "Waxy and dense, with a visible grain",
    ),
    brush(
        Pen::Pastel,
        "Pastel",
        Group::Pencils,
        "Wide, soft and powdery",
    ),
    brush(
        Pen::Marker,
        "Marker",
        Group::Paint,
        "A broad stroke that fades when drawn fast",
    ),
    brush(
        Pen::Highlighter,
        "Highlighter",
        Group::Paint,
        "A wide, see-through stroke that never builds up",
    ),
    brush(
        Pen::FeltTip,
        "Felt tip",
        Group::Paint,
        "A soft, slightly fuzzy edge at any speed",
    ),
    brush(
        Pen::Watercolor,
        "Watercolor",
        Group::Paint,
        "A translucent wash with a ragged edge",
    ),
    brush(
        Pen::Airbrush,
        "Airbrush",
        Group::Paint,
        "A soft, sprayed edge",
    ),
    brush(
        Pen::SprayPaint,
        "Spray paint",
        Group::Paint,
        "A wide cloud of specks that builds up",
    ),
    brush(
        Pen::Fill,
        "Fill",
        Group::Special,
        "A thin outline for shapes you fill",
    ),
    brush(Pen::Dotted, "Dotted", Group::Special, "Evenly spaced dots"),
    brush(
        Pen::Stipple,
        "Stipple",
        Group::Special,
        "Scattered dots, for shading in ink",
    ),
];

/// The entry for `pen`. A pen this version does not know is shown as the plain pen.
#[must_use]
pub fn info(pen: Pen) -> &'static BrushInfo {
    BRUSHES.iter().find(|b| b.pen == pen).unwrap_or(&BRUSHES[0])
}

/// A short stable name for `pen`, used as the key in the settings file.
#[must_use]
pub fn key(pen: Pen) -> &'static str {
    match info(pen).pen {
        Pen::Fountain => "fountain",
        Pen::DynamicPen => "dynamic",
        Pen::FixedWidth => "fixed",
        Pen::Wire => "wire",
        Pen::SoftPencil => "soft_pencil",
        Pen::HardPencil => "hard_pencil",
        Pen::Marker => "marker",
        Pen::Watercolor => "watercolor",
        Pen::Airbrush => "airbrush",
        Pen::Fill => "fill",
        Pen::Dotted => "dotted",
        Pen::Highlighter => "highlighter",
        Pen::BrushPen => "brush_pen",
        Pen::FeltTip => "felt_tip",
        Pen::Charcoal => "charcoal",
        Pen::Chalk => "chalk",
        Pen::Crayon => "crayon",
        Pen::Pastel => "pastel",
        Pen::SprayPaint => "spray_paint",
        Pen::Stipple => "stipple",
        Pen::InkBrush => "ink_brush",
        Pen::Pen | Pen::Unknown(_) => "pen",
    }
}

/// The pen with the given [`key`].
#[must_use]
pub fn from_key(name: &str) -> Option<Pen> {
    BRUSHES.iter().map(|b| b.pen).find(|p| key(*p) == name)
}

/// A sensible starting size for each pen, in canvas units.
#[must_use]
pub const fn default_width(pen: Pen) -> f32 {
    match pen {
        Pen::Fountain | Pen::FeltTip => 8.0,
        Pen::DynamicPen | Pen::Crayon | Pen::InkBrush => 12.0,
        Pen::FixedWidth | Pen::BrushPen => 10.0,
        Pen::Wire | Pen::Fill => 1.0,
        Pen::SoftPencil => 4.0,
        Pen::HardPencil => 3.5,
        Pen::Marker => 24.0,
        Pen::Watercolor => 40.0,
        Pen::Airbrush => 60.0,
        Pen::Dotted => 5.0,
        Pen::Highlighter => 28.0,
        Pen::Charcoal => 14.0,
        Pen::Chalk => 16.0,
        Pen::Pastel => 20.0,
        Pen::SprayPaint => 30.0,
        Pen::Stipple => 6.0,
        Pen::Pen | Pen::Unknown(_) => 3.0,
    }
}

/// The smoothness a pen starts with: Pen and Soft Pencil `0.1`, Marker `0.25`, Dynamic Pen and Dotted `0.0`;
/// the other pens use `0.1`.
#[must_use]
pub const fn default_smoothness(pen: Pen) -> f32 {
    match pen {
        Pen::Marker | Pen::InkBrush => 0.25,
        Pen::Highlighter | Pen::BrushPen => 0.2,
        Pen::DynamicPen | Pen::Dotted | Pen::SprayPaint | Pen::Stipple => 0.0,
        _ => 0.1,
    }
}

/// What a brush starts with before anything is changed.
#[must_use]
pub fn defaults(pen: Pen) -> BrushConfig {
    BrushConfig {
        size: default_width(pen),
        opacity: 1.0,
        smoothness: default_smoothness(pen),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_round_trip() {
        let keys: Vec<&str> = BRUSHES.iter().map(|b| key(b.pen)).collect();
        for (i, k) in keys.iter().enumerate() {
            assert!(!keys[..i].contains(k), "{k} is used twice");
            assert_eq!(from_key(k), Some(BRUSHES[i].pen));
        }
        assert_eq!(from_key("no such brush"), None);
    }

    #[test]
    fn every_group_has_brushes_and_every_brush_a_name_and_a_description() {
        for group in Group::ALL {
            assert!(BRUSHES.iter().any(|b| b.group == group), "{group:?}");
        }
        assert!(
            BRUSHES
                .iter()
                .all(|b| !b.name.is_empty() && !b.blurb.is_empty())
        );
    }

    #[test]
    fn no_pen_is_listed_twice_and_all_of_them_are_there() {
        for (i, b) in BRUSHES.iter().enumerate() {
            assert!(BRUSHES[..i].iter().all(|o| o.pen != b.pen), "{:?}", b.pen);
        }
        assert_eq!(BRUSHES.len(), 22);
    }

    #[test]
    fn an_unknown_pen_is_shown_as_the_plain_pen() {
        assert_eq!(info(Pen::Unknown(99)).pen, Pen::Pen);
        assert_eq!(key(Pen::Unknown(99)), "pen");
    }

    #[test]
    fn the_defaults_are_as_documented_and_every_pen_has_a_usable_size() {
        let marker = defaults(Pen::Marker);
        assert!((marker.smoothness - 0.25).abs() < 1e-6);
        assert!((defaults(Pen::DynamicPen).smoothness).abs() < 1e-6);
        assert!((defaults(Pen::Pen).opacity - 1.0).abs() < 1e-6);
        for b in &BRUSHES {
            let d = defaults(b.pen);
            assert!(
                d.size >= BrushConfig::MIN_SIZE && d.size <= BrushConfig::MAX_SIZE,
                "{}",
                b.name
            );
            assert!((0.0..=1.0).contains(&d.smoothness), "{}", b.name);
        }
    }
}
