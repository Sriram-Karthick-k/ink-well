//! How the built-in pens turn a smoothed path into stamps for the GPU renderer.
//!
//! A stroke is painted as stamps placed along its path: plain soft discs, or, for the pencils and the airbrush,
//! stamps shaped by optional mask textures (see [`MaskKind`]). The numbers in [`style_for`] are tuned by eye, so
//! they are the first thing to adjust when a pen looks wrong.

use ink_core::smoothing::Smoothed;
use ink_core::{Color, Pen, Point, Stroke};

/// Floats per stamp in a [`StrokeBatch`]: x, y, radius, alpha, rotation and atlas tile.
pub const STAMP_STRIDE: usize = 6;
/// Stamps closer than this many canvas units are not worth drawing.
const MIN_STEP: f32 = 0.2;
/// Upper bound on the stamps of one stroke; longer strokes get wider spacing instead.
const MAX_STAMPS: usize = 150_000;

/// How the stamps of one stroke combine into its mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accumulate {
    /// Overlap keeps the larger value, so a stroke is no darker where it crosses itself.
    Max,
    /// Overlap builds up like layered paint (airbrush, soft pencil).
    Over,
}

/// How pressure changes the alpha of a stamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureAlpha {
    /// Pressure does not change it.
    None,
    /// Rises linearly from a faint minimum to full.
    Linear,
    /// Proportional to the square of the pressure.
    Squared,
}

/// How pen speed changes the stamp size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedWidth {
    /// Speed does not change it.
    None,
    /// Fountain pen: `clamp(1 / (28 · speed), 0.9, 1.7)` times the width, so a slow pen draws thick and a
    /// fast one thin.
    Fountain,
}

/// The texture that shapes a pen's stamps. Textures are optional and are read at run time from a folder the user
/// provides; none is shipped. Without one, a plain soft disc is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskKind {
    /// No texture: an analytic disc whose edge follows [`PenStyle::hardness`].
    None,
    /// A stippled radial falloff; the whole texture is one stamp.
    Airbrush,
    /// A 4 by 4 atlas of rough graphite stamps (tile 0 is a shading gradient and is not used).
    HardPencil,
    /// The same layout, with lighter, coarser stamps.
    SoftPencil,
    /// Fine grain laid over a stroke in screen space, 64 points to a tile (not a stamp shape).
    Marker,
    /// The soft radial stamp of the watercolor pen.
    WatercolorStamp,
    /// The ragged blob that gives a watercolor stroke its edge.
    WatercolorReveal,
}

impl MaskKind {
    /// Every kind that has a texture.
    pub const TEXTURED: [Self; 6] = [
        Self::Airbrush,
        Self::HardPencil,
        Self::SoftPencil,
        Self::Marker,
        Self::WatercolorStamp,
        Self::WatercolorReveal,
    ];

    /// The file name of the texture in the textures folder.
    #[must_use]
    pub fn file_name(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Airbrush => Some("airbrush.png"),
            Self::HardPencil => Some("hard-pencil.png"),
            Self::SoftPencil => Some("soft-pencil.png"),
            Self::Marker => Some("marker-grain.png"),
            Self::WatercolorStamp => Some("watercolor-stamp.png"),
            Self::WatercolorReveal => Some("watercolor-edge.png"),
        }
    }

    /// Whether the texture is a 4 by 4 atlas of tiles, one picked for each stamp.
    #[must_use]
    pub fn is_atlas(self) -> bool {
        matches!(self, Self::HardPencil | Self::SoftPencil)
    }
}

/// How pen speed changes the stamp alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedAlpha {
    /// Speed does not change it.
    None,
    /// Marker: `max(0, 1.35 - 3 · speed)`, stored divided by [`MARKER_GAIN`].
    Marker,
}

/// The marker's alpha range is `0..=2.7`; the stamps store it divided by this and the composite multiplies it
/// back, so values above one still shape how neighbouring stamps blend.
pub const MARKER_GAIN: f32 = 2.7;

/// The look of one pen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenStyle {
    /// Distance between stamps as a fraction of the stamp diameter.
    pub spacing: f32,
    /// `1.0` is a crisp disc, `0.0` fades from the center to the rim. Used when there is no [`mask`](Self::mask).
    pub hardness: f32,
    /// The texture that shapes each stamp.
    pub mask: MaskKind,
    /// A second texture whose stamps are united (largest value wins) and multiplied into the result, shaping the
    /// edge of the stroke. Only the watercolor pen uses it.
    pub reveal: MaskKind,
    /// A texture laid over the finished stroke in screen space, 64 points to a tile.
    pub screen_texture: MaskKind,
    /// The composite multiplies the stroke mask by this before the screen texture.
    pub mask_gain: f32,
    /// All stamps of a stroke share one rotation instead of each having its own.
    pub stroke_rotation: bool,
    /// How pen speed changes the stamp alpha.
    pub speed_alpha: SpeedAlpha,
    /// How far each stamp strays from the path at random, as a fraction of its diameter.
    pub scatter: f32,
    pub accumulate: Accumulate,
    /// Opacity of the finished stroke.
    pub opacity: f32,
    /// Alpha of a single stamp; matters for [`Accumulate::Over`].
    pub flow: f32,
    /// How strongly the paper grain breaks the stroke up, `0.0` to `1.0`.
    pub grain: f32,
    /// The drawn size as a multiple of the chosen width.
    pub width_scale: f32,
    /// Smallest size, as a fraction of the pen width, at zero pressure.
    pub size_from_pressure: Option<f32>,
    /// How pressure changes the stamp alpha.
    pub pressure_alpha: PressureAlpha,
    /// How pen speed changes the stamp size.
    pub speed_width: SpeedWidth,
    /// The chosen width is limited to this range, in canvas units.
    pub width_range: Option<(f32, f32)>,
    /// The pen ignores the chosen width and uses this one.
    pub fixed_width: Option<f32>,
}

/// The style of `pen`.
#[must_use]
pub fn style_for(pen: Pen) -> PenStyle {
    let hard = PenStyle {
        spacing: 0.1,
        hardness: 1.0,
        mask: MaskKind::None,
        reveal: MaskKind::None,
        screen_texture: MaskKind::None,
        mask_gain: 1.0,
        stroke_rotation: false,
        speed_alpha: SpeedAlpha::None,
        scatter: 0.0,
        accumulate: Accumulate::Max,
        opacity: 1.0,
        flow: 1.0,
        grain: 0.0,
        width_scale: 1.0,
        size_from_pressure: None,
        pressure_alpha: PressureAlpha::None,
        speed_width: SpeedWidth::None,
        width_range: None,
        fixed_width: None,
    };
    match pen {
        // The wire pen's width is limited to half a unit up to one unit.
        Pen::Wire => PenStyle {
            width_range: Some((0.5, 1.0)),
            ..hard
        },
        Pen::Fountain => PenStyle {
            speed_width: SpeedWidth::Fountain,
            ..hard
        },
        Pen::Fill => PenStyle {
            fixed_width: Some(1.0),
            ..hard
        },
        // The marker is solid at a slow pace and fades as the pen speeds up, with a fine grain over it.
        Pen::Marker => PenStyle {
            screen_texture: MaskKind::Marker,
            mask_gain: MARKER_GAIN,
            speed_alpha: SpeedAlpha::Marker,
            ..hard
        },
        // Soft stamps turned the same way along a stroke, cut by a ragged blob; without the textures a soft
        // translucent ribbon. The spacing, flow and opacity are tuned.
        Pen::Watercolor => PenStyle {
            spacing: 0.25,
            hardness: 0.3,
            mask: MaskKind::WatercolorStamp,
            reveal: MaskKind::WatercolorReveal,
            stroke_rotation: true,
            accumulate: Accumulate::Over,
            flow: 0.35,
            opacity: 0.8,
            ..hard
        },
        // Both pencils draw 1.5 times the chosen width with stamps 0.3 apart. The flow is tuned
        // so the wider spacing keeps the same density.
        Pen::SoftPencil => PenStyle {
            spacing: 0.3,
            width_scale: 1.5,
            hardness: 0.35,
            mask: MaskKind::SoftPencil,
            scatter: 0.12,
            accumulate: Accumulate::Over,
            flow: 0.65,
            grain: 0.75,
            pressure_alpha: PressureAlpha::Linear,
            ..hard
        },
        Pen::HardPencil => PenStyle {
            spacing: 0.3,
            width_scale: 1.5,
            hardness: 0.85,
            mask: MaskKind::HardPencil,
            scatter: 0.12,
            accumulate: Accumulate::Over,
            flow: 0.8,
            grain: 0.55,
            pressure_alpha: PressureAlpha::Linear,
            ..hard
        },
        // Stamps 0.25 apart and an alpha of pressure squared; the flow is tuned.
        Pen::Airbrush => PenStyle {
            spacing: 0.25,
            hardness: 0.0,
            mask: MaskKind::Airbrush,
            accumulate: Accumulate::Over,
            flow: 0.4,
            pressure_alpha: PressureAlpha::Squared,
            ..hard
        },
        Pen::Dotted => PenStyle {
            spacing: 2.2,
            ..hard
        },
        Pen::DynamicPen => PenStyle {
            size_from_pressure: Some(0.25),
            ..hard
        },
        other => native_style(other, hard),
    }
}

/// The styles of this app's own pens, and the plain style for any other pen. They only recombine what the
/// engine already does.
fn native_style(pen: Pen, hard: PenStyle) -> PenStyle {
    match pen {
        // A wide, see-through stroke that is no darker where it crosses itself.
        Pen::Highlighter => PenStyle {
            spacing: 0.08,
            width_scale: 2.2,
            opacity: 0.38,
            ..hard
        },
        // A round tip that thickens with pressure, and is thin when the pen barely touches.
        Pen::BrushPen => PenStyle {
            spacing: 0.08,
            hardness: 0.92,
            width_scale: 1.4,
            size_from_pressure: Some(0.12),
            ..hard
        },
        // A soft, slightly fuzzy edge that stays the same however fast the pen moves.
        Pen::FeltTip => PenStyle {
            spacing: 0.1,
            hardness: 0.55,
            width_scale: 1.25,
            opacity: 0.92,
            ..hard
        },
        // Coarse, crumbly and dark where pressed hard; strays a lot from the line.
        Pen::Charcoal => PenStyle {
            spacing: 0.22,
            width_scale: 2.0,
            hardness: 0.9,
            mask: MaskKind::HardPencil,
            scatter: 0.4,
            accumulate: Accumulate::Over,
            flow: 0.55,
            grain: 1.0,
            pressure_alpha: PressureAlpha::Linear,
            ..hard
        },
        // Dry, patchy and light, with a wide scatter.
        Pen::Chalk => PenStyle {
            spacing: 0.35,
            width_scale: 2.2,
            hardness: 0.7,
            mask: MaskKind::SoftPencil,
            scatter: 0.5,
            accumulate: Accumulate::Over,
            flow: 0.5,
            grain: 1.0,
            ..hard
        },
        // Waxy and dense, with a visible grain.
        Pen::Crayon => PenStyle {
            spacing: 0.18,
            width_scale: 1.8,
            hardness: 0.6,
            mask: MaskKind::SoftPencil,
            scatter: 0.22,
            accumulate: Accumulate::Over,
            flow: 0.85,
            grain: 0.85,
            pressure_alpha: PressureAlpha::Linear,
            ..hard
        },
        // Wide, soft and powdery.
        Pen::Pastel => PenStyle {
            spacing: 0.25,
            width_scale: 2.8,
            hardness: 0.3,
            mask: MaskKind::SoftPencil,
            scatter: 0.28,
            accumulate: Accumulate::Over,
            flow: 0.6,
            grain: 0.6,
            pressure_alpha: PressureAlpha::Linear,
            ..hard
        },
        // A wide cloud of fine specks that builds up the longer the pen stays.
        Pen::SprayPaint => PenStyle {
            spacing: 0.12,
            width_scale: 3.0,
            hardness: 0.0,
            mask: MaskKind::Airbrush,
            scatter: 0.7,
            accumulate: Accumulate::Over,
            flow: 0.28,
            pressure_alpha: PressureAlpha::Squared,
            ..hard
        },
        // Small round dots, far apart and scattered, as in a pen-and-ink drawing.
        Pen::Stipple => PenStyle {
            spacing: 0.9,
            width_scale: 0.7,
            scatter: 0.6,
            ..hard
        },
        // Thick when slow, thin when fast, and thicker with pressure: a loaded brush.
        Pen::InkBrush => PenStyle {
            spacing: 0.07,
            hardness: 0.95,
            width_scale: 1.2,
            speed_width: SpeedWidth::Fountain,
            size_from_pressure: Some(0.2),
            ..hard
        },
        _ => hard,
    }
}

/// Every stamp of one stroke, ready to upload to the GPU.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeBatch {
    pub style: PenStyle,
    /// Straight (not premultiplied) RGBA.
    pub color: [f32; 4],
    /// [`STAMP_STRIDE`] floats per stamp: x, y, radius (canvas units), alpha, rotation (radians) and, for an
    /// atlas mask, the tile.
    pub stamps: Vec<f32>,
    /// Smallest x, smallest y, largest x and largest y in canvas units, including every stamp's radius.
    pub bounds: [f32; 4],
}

impl StrokeBatch {
    /// The number of stamps.
    #[must_use]
    pub fn len(&self) -> usize {
        self.stamps.len() / STAMP_STRIDE
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stamps.is_empty()
    }
}

fn rgba(c: Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A deterministic pseudo-random 64-bit value for stamp `index` of a stroke with `seed` (splitmix64).
fn hash(seed: u64, index: usize) -> u64 {
    let mut z = seed.wrapping_add((index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A value in `0.0..1.0` from the bits of `h`.
fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// The seed for a stroke's stamp randomness: the same stroke always looks the same.
fn stamp_seed(stroke: &Stroke) -> u64 {
    let (a, b) = stroke.id.as_bytes().split_at(8);
    u64::from_le_bytes(a.try_into().unwrap_or_default())
        ^ u64::from_le_bytes(b.try_into().unwrap_or_default())
}

/// The random part of one stamp: how far it strays, how it is turned and which atlas tile it uses.
struct Shape {
    dx: f32,
    dy: f32,
    rotation: f32,
    tile: f32,
}

impl Shape {
    fn of(style: &PenStyle, seed: u64, index: usize, radius: f32) -> Self {
        if style.mask == MaskKind::None {
            return Self {
                dx: 0.0,
                dy: 0.0,
                rotation: 0.0,
                tile: 0.0,
            };
        }
        let h = hash(seed, index);
        let spread = style.scatter * radius * 2.0;
        let turn = if style.stroke_rotation {
            hash(seed, 0)
        } else {
            h
        };
        Self {
            dx: (unit(h.rotate_left(42)) - 0.5) * spread,
            dy: (unit(h.rotate_left(11)) - 0.5) * spread,
            rotation: unit(turn) * std::f32::consts::TAU,
            tile: if style.mask.is_atlas() {
                1.0 + (unit(h.rotate_left(21)) * 15.0).floor().min(14.0)
            } else {
                0.0
            },
        }
    }
}

/// What varies along a stroke besides its position.
#[derive(Clone, Copy)]
struct Attr {
    pressure: f32,
    speed: Option<f32>,
}

impl Attr {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            pressure: lerp(a.pressure, b.pressure, t),
            speed: a.speed.zip(b.speed).map(|(x, y)| lerp(x, y, t)),
        }
    }
}

/// Places the stamps of `stroke` along its smoothed `path`.
#[must_use]
pub fn batch_for(stroke: &Stroke, path: &Smoothed) -> StrokeBatch {
    let style = style_for(stroke.pen);
    let mut width = stroke.width.unwrap_or(1.0);
    if let Some((low, high)) = style.width_range {
        width = width.clamp(low, high);
    }
    if let Some(fixed) = style.fixed_width {
        width = fixed;
    }
    let points = &path.points;
    let pressure = path.pressure.as_deref().filter(|p| p.len() == points.len());
    let speeds = path.speed.as_deref().filter(|v| v.len() == points.len());
    let attr_at = |i: usize| Attr {
        pressure: pressure.map_or(1.0, |p| p[i]),
        speed: speeds.map(|v| v[i]),
    };
    let radius = |a: Attr| {
        let scale = style
            .size_from_pressure
            .map_or(1.0, |min| lerp(min, 1.0, a.pressure));
        let by_speed = match (style.speed_width, a.speed) {
            (SpeedWidth::Fountain, Some(s)) => (1.0 / (28.0 * s.max(1e-6))).clamp(0.9, 1.7),
            _ => 1.0,
        };
        width * style.width_scale * scale * by_speed / 2.0
    };
    let alpha = |a: Attr| {
        let by_pressure = match style.pressure_alpha {
            PressureAlpha::None => 1.0,
            PressureAlpha::Linear => lerp(0.35, 1.0, a.pressure),
            PressureAlpha::Squared => a.pressure * a.pressure,
        };
        let by_speed = match (style.speed_alpha, a.speed) {
            (SpeedAlpha::Marker, speed) => {
                (1.0 + 0.35 - 3.0 * speed.unwrap_or(0.0)).max(0.0) / MARKER_GAIN
            }
            _ => 1.0,
        };
        style.flow * by_pressure * by_speed
    };

    let mut batch = StrokeBatch {
        style,
        color: rgba(stroke.color),
        stamps: Vec::new(),
        bounds: [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
    };
    // The stamps of a textured pen are rotated at random, so a stamp can reach `sqrt(2)` times its radius.
    let reach = if style.mask == MaskKind::None {
        1.0
    } else {
        std::f32::consts::SQRT_2
    };
    let seed = stamp_seed(stroke);
    let mut put = |at: Point, a: Attr| {
        let r = radius(a);
        let shape = Shape::of(&style, seed, batch.len(), r);
        batch.stamps.extend([
            at.x + shape.dx,
            at.y + shape.dy,
            r,
            alpha(a),
            shape.rotation,
            shape.tile,
        ]);
        let extent = r * reach + style.scatter * r;
        batch.bounds[0] = batch.bounds[0].min(at.x - extent);
        batch.bounds[1] = batch.bounds[1].min(at.y - extent);
        batch.bounds[2] = batch.bounds[2].max(at.x + extent);
        batch.bounds[3] = batch.bounds[3].max(at.y + extent);
    };

    let length: f32 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
    let floor = (length / MAX_STAMPS as f32).max(MIN_STEP);
    let step = |a: Attr| (style.spacing * radius(a) * 2.0).max(floor);

    let Some(&first) = points.first() else {
        batch.bounds = [0.0; 4];
        return batch;
    };
    put(first, attr_at(0));
    let mut until_next = step(attr_at(0));
    for i in 1..points.len() {
        let (a, b) = (points[i - 1], points[i]);
        let segment = a.distance(b);
        if segment <= 0.0 {
            continue;
        }
        let (aa, ab) = (attr_at(i - 1), attr_at(i));
        let mut done = 0.0;
        while segment - done >= until_next {
            done += until_next;
            let t = done / segment;
            let here = Attr::lerp(aa, ab, t);
            put(Point::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t)), here);
            until_next = step(here);
        }
        until_next -= segment - done;
    }
    // Make sure the stroke reaches its last point, except for dotted pens whose rhythm that would break.
    let last = points[points.len() - 1];
    let travelled_since_stamp = until_next < step(attr_at(points.len() - 1)) * 0.999;
    if points.len() > 1 && style.spacing <= 1.0 && travelled_since_stamp {
        put(last, attr_at(points.len() - 1));
    }
    batch
}

#[cfg(test)]
mod tests {
    use ink_core::{RawSample, Stroke};
    use uuid::Uuid;

    use super::*;

    fn stroke(pen: Pen, width: f32) -> Stroke {
        Stroke {
            id: Uuid::nil(),
            pen,
            width: Some(width),
            color: Color::BLACK,
            path: Vec::new(),
            samples: Vec::<RawSample>::new(),
            pressure: Vec::new(),
        }
    }

    fn line(length: f32) -> Smoothed {
        Smoothed {
            points: (0..=100)
                .map(|i| Point::new(length * i as f32 / 100.0, 0.0))
                .collect(),
            pressure: None,
            speed: None,
        }
    }

    #[test]
    fn stamps_are_evenly_spaced_and_reach_both_ends() {
        let batch = batch_for(&stroke(Pen::Pen, 10.0), &line(100.0));
        let xs: Vec<f32> = batch.stamps.chunks(STAMP_STRIDE).map(|s| s[0]).collect();
        assert!(xs[0].abs() < 1e-4);
        assert!((xs[xs.len() - 1] - 100.0).abs() < 1e-3, "{xs:?}");
        // spacing 0.1 of a 10-unit diameter is one unit
        assert!((xs[1] - xs[0] - 1.0).abs() < 1e-3);
        assert!((xs[2] - xs[1] - 1.0).abs() < 1e-3);
    }

    const NEW_PENS: [Pen; 10] = [
        Pen::Highlighter,
        Pen::BrushPen,
        Pen::FeltTip,
        Pen::Charcoal,
        Pen::Chalk,
        Pen::Crayon,
        Pen::Pastel,
        Pen::SprayPaint,
        Pen::Stipple,
        Pen::InkBrush,
    ];

    #[test]
    fn every_new_pen_draws_something_and_has_a_look_of_its_own() {
        let plain = style_for(Pen::Pen);
        let mut seen: Vec<PenStyle> = Vec::new();
        for pen in NEW_PENS {
            let style = style_for(pen);
            assert_ne!(style, plain, "{pen:?} is not just the plain pen");
            assert!(!seen.contains(&style), "{pen:?} repeats another pen's look");
            seen.push(style);
            assert!((0.0..=1.0).contains(&style.flow), "{pen:?} flow");
            assert!((0.0..=1.0).contains(&style.opacity), "{pen:?} opacity");
            assert!((0.0..=1.0).contains(&style.grain), "{pen:?} grain");
            assert!(style.spacing > 0.0, "{pen:?} spacing");
            let batch = batch_for(&stroke(pen, 12.0), &line(200.0));
            assert!(!batch.stamps.is_empty(), "{pen:?} drew nothing");
        }
    }

    #[test]
    fn the_highlighter_is_wide_and_see_through_and_never_builds_up() {
        let style = style_for(Pen::Highlighter);
        assert!(style.width_scale > 2.0 && style.opacity < 0.5);
        assert_eq!(style.accumulate, Accumulate::Max);
    }

    #[test]
    fn the_crumbly_pens_scatter_and_use_the_grain_but_the_felt_tip_does_not() {
        for pen in [Pen::Charcoal, Pen::Chalk, Pen::Crayon, Pen::Pastel] {
            let style = style_for(pen);
            assert!(style.scatter > 0.1 && style.grain > 0.5, "{pen:?}");
            assert_eq!(style.accumulate, Accumulate::Over, "{pen:?} builds up");
        }
        let felt = style_for(Pen::FeltTip);
        assert!(felt.scatter == 0.0 && felt.grain == 0.0);
    }

    #[test]
    fn the_brush_pens_follow_pressure() {
        assert!(style_for(Pen::BrushPen).size_from_pressure.is_some());
        assert!(style_for(Pen::InkBrush).size_from_pressure.is_some());
        assert_eq!(style_for(Pen::InkBrush).speed_width, SpeedWidth::Fountain);
    }

    #[test]
    fn a_textured_stroke_gets_the_same_stamps_every_time() {
        let mut s = stroke(Pen::Charcoal, 14.0);
        s.id = Uuid::from_u128(42);
        assert_eq!(batch_for(&s, &line(150.0)), batch_for(&s, &line(150.0)));
        let mut other = s.clone();
        other.id = Uuid::from_u128(43);
        assert_ne!(
            batch_for(&s, &line(150.0)).stamps,
            batch_for(&other, &line(150.0)).stamps,
            "the id seeds the scatter"
        );
    }

    #[test]
    fn a_single_point_gets_one_stamp() {
        let path = Smoothed {
            points: vec![Point::new(3.0, 4.0)],
            pressure: None,
            speed: None,
        };
        let batch = batch_for(&stroke(Pen::Pen, 8.0), &path);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch.bounds, [-1.0, 0.0, 7.0, 8.0]);
    }

    #[test]
    fn an_empty_path_gives_an_empty_batch() {
        let path = Smoothed {
            points: Vec::new(),
            pressure: None,
            speed: None,
        };
        assert!(batch_for(&stroke(Pen::Pen, 8.0), &path).is_empty());
    }

    #[test]
    fn pressure_scales_a_dynamic_pen_and_not_a_plain_pen() {
        let mut path = line(50.0);
        path.pressure = Some((0..=100).map(|i| i as f32 / 100.0).collect());
        let dynamic = batch_for(&stroke(Pen::DynamicPen, 20.0), &path);
        let plain = batch_for(&stroke(Pen::Pen, 20.0), &path);
        let (first, last) = (
            dynamic.stamps[2],
            dynamic.stamps[dynamic.stamps.len() - STAMP_STRIDE + 2],
        );
        assert!(
            (first - 2.5).abs() < 1e-4,
            "quarter size at zero pressure: {first}"
        );
        assert!(
            (last - 10.0).abs() < 1e-4,
            "full size at full pressure: {last}"
        );
        assert!(
            plain
                .stamps
                .chunks(STAMP_STRIDE)
                .all(|s| (s[2] - 10.0).abs() < 1e-4)
        );
    }

    #[test]
    fn the_bounds_include_the_radius() {
        let batch = batch_for(&stroke(Pen::Pen, 10.0), &line(100.0));
        assert_eq!(batch.bounds, [-5.0, -5.0, 105.0, 5.0]);
    }

    #[test]
    fn a_dotted_pen_leaves_gaps() {
        let batch = batch_for(&stroke(Pen::Dotted, 4.0), &line(100.0));
        let xs: Vec<f32> = batch.stamps.chunks(STAMP_STRIDE).map(|s| s[0]).collect();
        assert!(xs[1] - xs[0] > 4.0, "dots must not touch: {xs:?}");
        assert!(xs.len() < 30);
    }

    #[test]
    fn a_very_long_stroke_stays_under_the_stamp_cap() {
        // 200 000 points five units apart: a million units, which would be five million stamps at the
        // natural spacing of a one-unit pen.
        let path = Smoothed {
            points: (0..200_000)
                .map(|i| Point::new(i as f32 * 5.0, 0.0))
                .collect(),
            pressure: None,
            speed: None,
        };
        let batch = batch_for(&stroke(Pen::Pen, 1.0), &path);
        assert!(batch.len() <= MAX_STAMPS + 2, "{}", batch.len());
    }

    #[test]
    fn wire_width_stays_between_half_a_unit_and_one_and_fill_ignores_the_width() {
        let wire = |size: f32| batch_for(&stroke(Pen::Wire, size), &line(10.0)).stamps[2];
        assert!((wire(30.0) - 0.5).abs() < 1e-6, "limited to a width of 1");
        assert!((wire(0.1) - 0.25).abs() < 1e-6, "raised to a width of 0.5");
        assert!((wire(0.8) - 0.4).abs() < 1e-6, "left alone in between");
        let fill = batch_for(&stroke(Pen::Fill, 30.0), &line(10.0));
        assert!((fill.stamps[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_fountain_pen_is_thick_when_slow_and_thin_when_fast() {
        let radius_at = |speed: f32, pen: Pen| {
            let mut path = line(50.0);
            path.speed = Some(vec![speed; 101]);
            batch_for(&stroke(pen, 10.0), &path).stamps[2]
        };
        assert!(
            (radius_at(0.0, Pen::Fountain) - 5.0 * 1.7).abs() < 1e-4,
            "stopped: 1.7 times"
        );
        assert!(
            (radius_at(0.5, Pen::Fountain) - 5.0 * 0.9).abs() < 1e-4,
            "fast: 0.9 times"
        );
        // 1 / (28 * 0.03) = 1.19
        assert!((radius_at(0.03, Pen::Fountain) - 5.0 / (28.0 * 0.03)).abs() < 1e-3);
        assert!(
            (radius_at(0.5, Pen::Pen) - 5.0).abs() < 1e-6,
            "other pens ignore speed"
        );
    }

    #[test]
    fn without_speed_the_fountain_pen_uses_its_plain_width() {
        let batch = batch_for(&stroke(Pen::Fountain, 10.0), &line(50.0));
        assert!((batch.stamps[2] - 5.0).abs() < 1e-6);
    }

    #[test]
    fn soft_pencil_pressure_changes_alpha() {
        let mut path = line(50.0);
        path.pressure = Some(vec![0.0; 101]);
        let light = batch_for(&stroke(Pen::SoftPencil, 10.0), &path);
        path.pressure = Some(vec![1.0; 101]);
        let heavy = batch_for(&stroke(Pen::SoftPencil, 10.0), &path);
        assert!(heavy.stamps[3] > light.stamps[3] * 2.0);
    }

    #[test]
    fn both_pencils_draw_one_and_a_half_times_the_width_with_stamps_a_third_apart() {
        for pen in [Pen::SoftPencil, Pen::HardPencil] {
            let batch = batch_for(&stroke(pen, 10.0), &line(100.0));
            assert!(
                (batch.stamps[2] - 7.5).abs() < 1e-4,
                "radius is 1.5 * 10 / 2 for {pen:?}"
            );
            // Each stamp strays a little at random, so the spacing is checked on average.
            let n = batch.len();
            let last = (n - 1) * STAMP_STRIDE;
            let mean_gap = (batch.stamps[last] - batch.stamps[0]) / (n - 1) as f32;
            assert!(
                (mean_gap - 0.3 * 15.0).abs() < 0.2,
                "spacing is 0.3 of the diameter, got {mean_gap}"
            );
        }
    }

    #[test]
    fn airbrush_alpha_follows_the_square_of_the_pressure() {
        let mut path = line(50.0);
        path.pressure = Some(vec![0.5; 101]);
        let half = batch_for(&stroke(Pen::Airbrush, 40.0), &path);
        path.pressure = Some(vec![1.0; 101]);
        let full = batch_for(&stroke(Pen::Airbrush, 40.0), &path);
        assert!((half.stamps[3] / full.stamps[3] - 0.25).abs() < 1e-5);
        let n = full.len();
        let mean_gap = (full.stamps[(n - 1) * STAMP_STRIDE] - full.stamps[0]) / (n - 1) as f32;
        assert!(
            (mean_gap - 0.25 * 40.0).abs() < 0.3,
            "spacing is 0.25 of the diameter, got {mean_gap}"
        );
    }

    #[test]
    fn textured_pens_rotate_and_pick_tiles_deterministically() {
        let make = |id: u128| {
            let mut s = stroke(Pen::HardPencil, 10.0);
            s.id = Uuid::from_u128(id);
            batch_for(&s, &line(100.0))
        };
        let a = make(7);
        assert_eq!(
            a.stamps,
            make(7).stamps,
            "the same stroke always looks the same"
        );
        assert_ne!(a.stamps, make(8).stamps, "another stroke differs");
        let tiles: Vec<f32> = a.stamps.chunks(STAMP_STRIDE).map(|s| s[5]).collect();
        assert!(
            tiles
                .iter()
                .all(|t| (1.0..=15.0).contains(t) && t.fract() == 0.0)
        );
        let distinct = (1..=15).filter(|k| tiles.contains(&(*k as f32))).count();
        assert!(distinct >= 8, "tiles vary: {distinct}");
        let angles: Vec<f32> = a.stamps.chunks(STAMP_STRIDE).map(|s| s[4]).collect();
        assert!(
            angles
                .iter()
                .all(|r| (0.0..std::f32::consts::TAU).contains(r))
        );
        assert!(angles.windows(2).any(|w| (w[0] - w[1]).abs() > 0.5));
    }

    #[test]
    fn plain_pens_have_no_rotation_or_tile() {
        let batch = batch_for(&stroke(Pen::Pen, 10.0), &line(50.0));
        assert!(
            batch
                .stamps
                .chunks(STAMP_STRIDE)
                .all(|s| s[4] == 0.0 && s[5] == 0.0)
        );
    }

    #[test]
    fn the_airbrush_rotates_its_single_mask_and_uses_no_tile() {
        let batch = batch_for(&stroke(Pen::Airbrush, 40.0), &line(100.0));
        assert!(batch.stamps.chunks(STAMP_STRIDE).all(|s| s[5] == 0.0));
        assert!(batch.stamps.chunks(STAMP_STRIDE).any(|s| s[4] > 0.5));
    }

    #[test]
    fn the_bounds_cover_rotated_and_scattered_stamps() {
        let batch = batch_for(&stroke(Pen::SoftPencil, 10.0), &line(100.0));
        for s in batch.stamps.chunks(STAMP_STRIDE) {
            let r = s[2] * std::f32::consts::SQRT_2;
            assert!(s[0] - r >= batch.bounds[0] && s[0] + r <= batch.bounds[2]);
            assert!(s[1] - r >= batch.bounds[1] && s[1] + r <= batch.bounds[3]);
        }
    }

    #[test]
    fn the_marker_fades_with_speed_and_is_stored_divided_by_the_gain() {
        let alpha_at = |speed: f32| {
            let mut path = line(50.0);
            path.speed = Some(vec![speed; 101]);
            batch_for(&stroke(Pen::Marker, 20.0), &path).stamps[3]
        };
        assert!(
            (alpha_at(0.0) - 1.35 / MARKER_GAIN).abs() < 1e-6,
            "slow: 1.35 before the gain"
        );
        assert!((alpha_at(0.1) - 1.05 / MARKER_GAIN).abs() < 1e-6);
        assert_eq!(alpha_at(0.5), 0.0, "fast enough to vanish");
        assert_eq!(alpha_at(1.0), 0.0, "never negative");
    }

    #[test]
    fn a_watercolor_stroke_is_turned_one_way_throughout() {
        let batch = batch_for(&stroke(Pen::Watercolor, 30.0), &line(100.0));
        let angles: Vec<f32> = batch.stamps.chunks(STAMP_STRIDE).map(|s| s[4]).collect();
        assert!(
            angles.windows(2).all(|w| w[0].to_bits() == w[1].to_bits()),
            "one rotation per stroke"
        );
        assert!(
            batch.stamps.chunks(STAMP_STRIDE).all(|s| s[5] == 0.0),
            "no atlas tile"
        );
    }

    #[test]
    fn every_textured_kind_names_its_file() {
        let names: Vec<_> = MaskKind::TEXTURED
            .iter()
            .filter_map(|k| k.file_name())
            .collect();
        assert_eq!(names.len(), MaskKind::TEXTURED.len());
        assert!(
            names
                .iter()
                .all(|n| n.to_ascii_lowercase().ends_with(".png"))
        );
    }
}
