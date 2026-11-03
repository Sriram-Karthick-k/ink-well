//! The pens, drawn as objects: shaded barrels, metal ferrules, nibs and bristles.
//!
//! Every tool is drawn lying on its side with the tip at a given point and the body running away from it. Parts
//! that carry ink (the tip, the color band, the bristles) take the current ink color, so the rail always shows
//! what color a pen would write in.

use eframe::egui::{
    Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, epaint::Shadow, pos2,
};
use ink_core::Pen;

/// The length of every tool in drawing units, tip to end.
pub const LENGTH: f32 = 112.0;

const STEEL: Color32 = Color32::from_rgb(0xC9, 0xCD, 0xD4);
const STEEL_DARK: Color32 = Color32::from_rgb(0x8C, 0x92, 0x9B);
const CREAM: Color32 = Color32::from_rgb(0xEE, 0xE9, 0xDE);
const GOLD: Color32 = Color32::from_rgb(0xD9, 0xB1, 0x5B);
const GRAPHITE: Color32 = Color32::from_rgb(0x3B, 0x3A, 0x3C);
const WOOD: Color32 = Color32::from_rgb(0xE8, 0xCB, 0xA1);
const PINK: Color32 = Color32::from_rgb(0xEF, 0xA9, 0xB3);

/// Which tool to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    Pen,
    Fountain,
    Dynamic,
    Fixed,
    Wire,
    SoftPencil,
    HardPencil,
    Marker,
    Watercolor,
    Airbrush,
    Fill,
    Dotted,
    Highlighter,
    BrushPen,
    FeltTip,
    Charcoal,
    Chalk,
    Crayon,
    Pastel,
    Spray,
    Stipple,
    InkBrush,
}

impl Art {
    #[must_use]
    pub fn of(pen: Pen) -> Self {
        match pen {
            Pen::Fountain => Self::Fountain,
            Pen::DynamicPen => Self::Dynamic,
            Pen::FixedWidth => Self::Fixed,
            Pen::Wire => Self::Wire,
            Pen::SoftPencil => Self::SoftPencil,
            Pen::HardPencil => Self::HardPencil,
            Pen::Marker => Self::Marker,
            Pen::Watercolor => Self::Watercolor,
            Pen::Airbrush => Self::Airbrush,
            Pen::Fill => Self::Fill,
            Pen::Dotted => Self::Dotted,
            Pen::Highlighter => Self::Highlighter,
            Pen::BrushPen => Self::BrushPen,
            Pen::FeltTip => Self::FeltTip,
            Pen::Charcoal => Self::Charcoal,
            Pen::Chalk => Self::Chalk,
            Pen::Crayon => Self::Crayon,
            Pen::Pastel => Self::Pastel,
            Pen::SprayPaint => Self::Spray,
            Pen::Stipple => Self::Stipple,
            Pen::InkBrush => Self::InkBrush,
            Pen::Pen | Pen::Unknown(_) => Self::Pen,
        }
    }
}

/// Where and how large to draw.
#[derive(Clone, Copy)]
pub struct Placement {
    /// Where the tip is.
    pub tip: Pos2,
    /// Screen points per drawing unit.
    pub scale: f32,
    /// `1.0` for a tool lying to the left of its tip, `-1.0` for one lying to the right.
    pub direction: f32,
    /// How far the tool is lifted off the surface, `0.0..=1.0`; a lifted tool casts a longer shadow.
    pub lift: f32,
}

/// Drawing in the tool's own coordinates: `u` runs from the tip along the body, `v` across it.
struct Rig<'a> {
    painter: &'a Painter,
    at: Placement,
    ink: Color32,
}

impl Rig<'_> {
    fn point(&self, u: f32, v: f32) -> Pos2 {
        pos2(
            self.at.tip.x - self.at.direction * u * self.at.scale,
            self.at.tip.y + v * self.at.scale,
        )
    }

    fn rect(&self, u0: f32, u1: f32, v0: f32, v1: f32) -> Rect {
        Rect::from_two_pos(self.point(u0, v0), self.point(u1, v1))
    }

    fn polygon(&self, points: &[(f32, f32)], fill: Color32) {
        let mut pts: Vec<Pos2> = points.iter().map(|&(u, v)| self.point(u, v)).collect();
        let area: f32 = pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .map(|(a, b)| a.x * b.y - b.x * a.y)
            .sum();
        if area < 0.0 {
            pts.reverse();
        }
        self.painter
            .add(Shape::convex_polygon(pts, fill, Stroke::NONE));
    }

    fn line(&self, a: (f32, f32), b: (f32, f32), width: f32, color: Color32) {
        self.painter.line_segment(
            [self.point(a.0, a.1), self.point(b.0, b.1)],
            Stroke::new(width * self.at.scale, color),
        );
    }

    fn dot(&self, u: f32, v: f32, radius: f32, color: Color32) {
        self.painter
            .circle_filled(self.point(u, v), radius * self.at.scale, color);
    }

    /// A rounded cylinder lit from above: a base color with a bright stripe near the top and a shaded underside.
    fn tube(&self, u0: f32, u1: f32, centre: f32, half: f32, base: Color32) {
        let body = self.rect(u0, u1, centre - half, centre + half);
        let radius = (half * self.at.scale).min(body.width() / 2.0);
        let round = CornerRadius::same(radius.round() as u8);
        self.painter.rect_filled(body, round, base);
        let h = body.height();
        let inset = radius * 0.7;
        let stripe = Rect::from_min_max(
            pos2(body.left() + inset, body.top() + h * 0.14),
            pos2(body.right() - inset, body.top() + h * 0.36),
        );
        if stripe.is_positive() {
            self.painter.rect_filled(
                stripe,
                CornerRadius::same((h * 0.11).round() as u8),
                Color32::from_white_alpha(78),
            );
        }
        let shade = Rect::from_min_max(
            pos2(body.left() + inset * 0.5, body.top() + h * 0.68),
            pos2(body.right() - inset * 0.5, body.bottom() - h * 0.06),
        );
        if shade.is_positive() {
            self.painter.rect_filled(
                shade,
                CornerRadius::same((h * 0.14).round() as u8),
                Color32::from_black_alpha(58),
            );
        }
    }

    /// The color of a band or cap that carries the ink color. Black ink shows as graphite, so a band never
    /// looks like a hole in the pen.
    fn band(&self) -> Color32 {
        let luma = 0.299 * f32::from(self.ink.r())
            + 0.587 * f32::from(self.ink.g())
            + 0.114 * f32::from(self.ink.b());
        if luma < 70.0 {
            Color32::from_rgb(0x5C, 0x5E, 0x63)
        } else {
            self.ink
        }
    }

    fn shadow(&self, half: f32) {
        if self.at.lift <= 0.0 {
            return;
        }
        let body = self.rect(0.0, LENGTH, -half, half);
        let lift = self.at.lift;
        self.painter.add(
            Shadow {
                offset: [0, (6.0 * lift).round() as i8],
                blur: (12.0 * lift).round() as u8 + 1,
                spread: 0,
                color: Color32::from_black_alpha((110.0 * lift).round() as u8),
            }
            .as_shape(body, CornerRadius::same(4)),
        );
    }
}

/// Draws `art` at `at`, with ink-carrying parts in `ink`.
pub fn paint(painter: &Painter, art: Art, at: Placement, ink: Color32) {
    let rig = Rig { painter, at, ink };
    match art {
        Art::Pen => pen(&rig),
        Art::Fountain => fountain(&rig),
        Art::Dynamic => dynamic(&rig),
        Art::Fixed => fixed(&rig),
        Art::Wire => wire(&rig),
        Art::SoftPencil => pencil(&rig, PencilKind::Soft),
        Art::HardPencil => pencil(&rig, PencilKind::Hard),
        Art::Marker => marker(&rig),
        Art::Watercolor => brush(&rig),
        Art::Airbrush => airbrush(&rig),
        Art::Fill => fill(&rig),
        Art::Dotted => dotted(&rig),
        Art::Highlighter => highlighter(&rig),
        Art::BrushPen => brush_pen(&rig),
        Art::FeltTip => felt_tip(&rig),
        Art::Charcoal => charcoal(&rig),
        Art::Chalk => chalk(&rig),
        Art::Crayon => crayon(&rig),
        Art::Pastel => pastel(&rig),
        Art::Spray => spray(&rig),
        Art::Stipple => stipple(&rig),
        Art::InkBrush => ink_brush(&rig),
    }
}

fn pen(r: &Rig) {
    r.shadow(7.0);
    r.tube(38.0, 108.0, 0.0, 6.6, CREAM);
    r.tube(54.0, 68.0, 0.0, 6.9, r.band());
    r.line((70.0, -8.6), (102.0, -8.6), 1.7, STEEL_DARK);
    r.tube(32.0, 40.0, 0.0, 5.3, STEEL);
    r.polygon(&[(1.5, 0.0), (34.0, -4.7), (34.0, 4.7)], STEEL);
    r.line(
        (6.0, -1.2),
        (30.0, -3.2),
        0.9,
        Color32::from_white_alpha(110),
    );
    r.dot(1.4, 0.0, 1.8, r.ink);
}

fn fountain(r: &Rig) {
    let dark = Color32::from_rgb(0x25, 0x24, 0x2B);
    r.shadow(8.0);
    r.tube(46.0, 108.0, 0.0, 7.8, dark);
    r.tube(46.0, 50.0, 0.0, 8.0, GOLD);
    r.tube(34.0, 47.0, 0.0, 6.0, Color32::from_rgb(0x1A, 0x19, 0x1F));
    r.polygon(
        &[
            (0.5, 0.0),
            (22.0, -5.2),
            (36.0, -6.0),
            (36.0, 6.0),
            (22.0, 5.2),
        ],
        GOLD,
    );
    r.line(
        (12.0, -2.4),
        (32.0, -4.4),
        1.1,
        Color32::from_white_alpha(120),
    );
    r.line((1.0, 0.0), (22.0, 0.0), 0.8, Color32::from_black_alpha(150));
    r.dot(22.0, 0.0, 1.3, Color32::from_black_alpha(170));
    r.polygon(&[(0.0, 0.0), (6.0, -1.7), (6.0, 1.7)], r.ink);
}

fn dynamic(r: &Rig) {
    r.shadow(7.0);
    r.tube(34.0, 108.0, 0.0, 7.0, Color32::from_rgb(0x4B, 0x50, 0x59));
    r.tube(60.0, 63.5, 0.0, 7.3, STEEL);
    r.tube(68.0, 71.5, 0.0, 7.3, STEEL);
    r.tube(80.0, 96.0, 0.0, 7.2, r.band());
    r.tube(30.0, 37.0, 0.0, 5.4, STEEL);
    r.polygon(&[(1.5, 0.0), (32.0, -4.3), (32.0, 4.3)], STEEL_DARK);
    r.dot(1.4, 0.0, 1.8, r.ink);
}

fn fixed(r: &Rig) {
    r.shadow(6.0);
    r.tube(0.0, 16.0, 0.0, 1.5, STEEL);
    r.tube(14.0, 38.0, 0.0, 3.7, STEEL);
    r.tube(38.0, 102.0, 0.0, 5.7, Color32::from_rgb(0x33, 0x31, 0x2F));
    r.tube(44.0, 52.0, 0.0, 5.9, r.band());
    r.tube(56.0, 58.5, 0.0, 5.9, CREAM);
    r.dot(0.6, 0.0, 1.2, r.ink);
}

fn wire(r: &Rig) {
    r.shadow(3.0);
    r.polygon(&[(0.0, 0.0), (44.0, -2.1), (44.0, 2.1)], STEEL);
    r.tube(44.0, 108.0, 0.0, 2.6, STEEL_DARK);
    r.tube(70.0, 82.0, 0.0, 2.8, r.band());
    r.dot(0.4, 0.0, 0.9, r.ink);
}

#[derive(Clone, Copy)]
enum PencilKind {
    Soft,
    Hard,
}

fn pencil(r: &Rig, kind: PencilKind) {
    let (stripes, lead, ferrule, cap) = match kind {
        PencilKind::Soft => (
            [
                Color32::from_rgb(0xF4, 0xCC, 0x55),
                Color32::from_rgb(0xE7, 0xB1, 0x34),
                Color32::from_rgb(0xC6, 0x8D, 0x22),
            ],
            2.7,
            GOLD,
            PINK,
        ),
        PencilKind::Hard => (
            [
                Color32::from_rgb(0x44, 0x57, 0x5A),
                Color32::from_rgb(0x2F, 0x3E, 0x41),
                Color32::from_rgb(0x20, 0x2B, 0x2D),
            ],
            1.7,
            STEEL,
            CREAM,
        ),
    };
    r.shadow(7.0);
    r.polygon(&[(0.0, 0.0), (30.0, -7.0), (30.0, 7.0)], WOOD);
    r.polygon(
        &[(0.0, 0.0), (30.0, 1.0), (30.0, 7.0)],
        Color32::from_black_alpha(28),
    );
    r.polygon(&[(0.0, 0.0), (9.0, -lead), (9.0, lead)], GRAPHITE);
    for (i, color) in stripes.into_iter().enumerate() {
        let v0 = -7.0 + 4.67 * i as f32;
        r.painter
            .rect_filled(r.rect(30.0, 98.0, v0, v0 + 4.67), CornerRadius::ZERO, color);
    }
    r.painter.rect_filled(
        r.rect(30.0, 98.0, -7.0, -4.9),
        CornerRadius::ZERO,
        Color32::from_white_alpha(40),
    );
    r.tube(98.0, 106.0, 0.0, 7.0, ferrule);
    r.line(
        (101.0, -7.0),
        (101.0, 7.0),
        0.7,
        Color32::from_black_alpha(80),
    );
    r.line(
        (103.5, -7.0),
        (103.5, 7.0),
        0.7,
        Color32::from_black_alpha(80),
    );
    r.tube(106.0, 114.0, 0.0, 6.4, cap);
}

fn marker(r: &Rig) {
    r.shadow(9.5);
    r.tube(24.0, 100.0, 0.0, 9.4, Color32::from_rgb(0x3D, 0x3C, 0x39));
    r.tube(50.0, 72.0, 0.0, 9.7, r.band());
    r.tube(14.0, 26.0, 0.0, 7.4, Color32::from_rgb(0xCF, 0xD2, 0xD8));
    r.polygon(&[(0.0, -3.8), (0.0, 3.8), (15.0, 7.0), (15.0, -7.0)], r.ink);
    r.line(
        (2.0, -2.6),
        (13.0, -5.2),
        1.1,
        Color32::from_white_alpha(100),
    );
}

fn brush(r: &Rig) {
    r.shadow(6.0);
    r.polygon(
        &[
            (0.0, 0.0),
            (5.0, -3.0),
            (14.0, -6.4),
            (26.0, -6.6),
            (26.0, 6.6),
            (14.0, 6.4),
            (5.0, 3.0),
        ],
        r.ink,
    );
    r.line(
        (8.0, -3.2),
        (23.0, -5.2),
        1.2,
        Color32::from_white_alpha(90),
    );
    r.tube(26.0, 48.0, 0.0, 6.6, STEEL);
    r.line(
        (34.0, -6.6),
        (34.0, 6.6),
        0.8,
        Color32::from_black_alpha(80),
    );
    r.line(
        (40.0, -6.6),
        (40.0, 6.6),
        0.8,
        Color32::from_black_alpha(80),
    );
    let handle = Color32::from_rgb(0x7B, 0x3B, 0x2B);
    r.polygon(
        &[(48.0, -4.6), (108.0, -2.6), (108.0, 2.6), (48.0, 4.6)],
        handle,
    );
    r.line(
        (50.0, -2.8),
        (106.0, -1.5),
        1.3,
        Color32::from_white_alpha(70),
    );
    r.polygon(
        &[(48.0, 1.6), (108.0, 1.0), (108.0, 2.6), (48.0, 4.6)],
        Color32::from_black_alpha(60),
    );
}

fn airbrush(r: &Rig) {
    r.shadow(7.0);
    r.tube(20.0, 96.0, 0.0, 6.6, Color32::from_rgb(0xD3, 0xD6, 0xDC));
    r.polygon(&[(2.0, 0.0), (22.0, -3.3), (22.0, 3.3)], STEEL_DARK);
    r.tube(60.0, 80.0, -11.0, 4.6, r.band());
    r.tube(66.0, 72.0, -7.0, 2.2, STEEL);
    r.tube(40.0, 52.0, 8.4, 2.6, STEEL_DARK);
    for i in 1..=6_u8 {
        let f = f32::from(i);
        let alpha = (200.0 - f * 28.0).max(30.0) as u8;
        let color = Color32::from_rgba_unmultiplied(r.ink.r(), r.ink.g(), r.ink.b(), alpha);
        let side = if i % 2 == 0 { 1.0 } else { -1.0 };
        r.dot(-4.0 - f * 3.4, f * 1.7 * side, 0.9 + f * 0.06, color);
    }
}

fn fill(r: &Rig) {
    r.shadow(5.0);
    r.tube(26.0, 106.0, 0.0, 5.0, Color32::from_rgb(0x3E, 0x3D, 0x3A));
    r.polygon(&[(12.0, 0.0), (28.0, -3.8), (28.0, 3.8)], STEEL);
    r.polygon(&[(0.0, 0.0), (6.0, -3.4), (6.0, 3.4)], r.ink);
    r.dot(8.0, 0.0, 4.6, r.ink);
    r.dot(9.4, -1.9, 1.3, Color32::from_white_alpha(130));
}

fn dotted(r: &Rig) {
    r.shadow(6.5);
    r.tube(30.0, 108.0, 0.0, 6.3, Color32::from_rgb(0xE9, 0xDA, 0xC5));
    for i in 0..5_u8 {
        r.dot(52.0 + f32::from(i) * 7.0, 0.0, 1.7, r.ink);
    }
    r.polygon(&[(1.0, 0.0), (31.0, -4.2), (31.0, 4.2)], STEEL);
    for (i, radius) in [(1.0_f32, 1.9), (2.0, 1.5), (3.0, 1.1)] {
        let alpha = (230.0 - 60.0 * i) as u8;
        r.dot(
            -6.0 - 8.0 * (i - 1.0),
            0.0,
            radius,
            Color32::from_rgba_unmultiplied(r.ink.r(), r.ink.g(), r.ink.b(), alpha),
        );
    }
}

/// A chunky chisel-tipped marker in highlighter yellow.
fn highlighter(r: &Rig) {
    r.shadow(10.0);
    r.tube(26.0, 102.0, 0.0, 10.0, Color32::from_rgb(0xF2, 0xE2, 0x4A));
    r.tube(52.0, 74.0, 0.0, 10.3, r.band());
    r.tube(14.0, 28.0, 0.0, 8.0, Color32::from_rgb(0xF6, 0xF4, 0xEA));
    r.polygon(&[(0.0, -5.5), (0.0, 5.5), (16.0, 9.0), (16.0, -9.0)], r.ink);
    r.line(
        (2.0, -3.6),
        (14.0, -6.8),
        1.2,
        Color32::from_white_alpha(110),
    );
}

/// A slim pen with a pointed, tapering brush tip.
fn brush_pen(r: &Rig) {
    r.shadow(5.5);
    r.polygon(
        &[
            (0.0, 0.0),
            (7.0, -2.6),
            (24.0, -4.8),
            (30.0, -4.8),
            (30.0, 4.8),
            (24.0, 4.8),
            (7.0, 2.6),
        ],
        r.ink,
    );
    r.line(
        (6.0, -1.4),
        (24.0, -3.8),
        1.1,
        Color32::from_white_alpha(90),
    );
    r.tube(30.0, 44.0, 0.0, 5.2, STEEL);
    r.tube(44.0, 108.0, 0.0, 5.6, Color32::from_rgb(0x2B, 0x2B, 0x31));
    r.tube(70.0, 84.0, 0.0, 5.8, r.band());
}

/// A thin felt pen with a colored cap and a fiber tip.
fn felt_tip(r: &Rig) {
    r.shadow(6.5);
    r.tube(34.0, 106.0, 0.0, 6.4, Color32::from_rgb(0xDE, 0xE3, 0xEB));
    r.tube(52.0, 76.0, 0.0, 6.7, r.band());
    r.tube(18.0, 36.0, 0.0, 5.0, Color32::from_rgb(0x3C, 0x3B, 0x39));
    r.polygon(&[(0.0, -2.2), (0.0, 2.2), (20.0, 5.0), (20.0, -5.0)], r.ink);
    r.line(
        (2.0, -1.4),
        (18.0, -3.6),
        0.9,
        Color32::from_white_alpha(100),
    );
}

/// A stick of charcoal, black and rough at the end, wrapped in brown paper.
fn charcoal(r: &Rig) {
    let black = Color32::from_rgb(0x2B, 0x2A, 0x29);
    r.shadow(7.0);
    r.tube(30.0, 84.0, 0.0, 7.2, Color32::from_rgb(0xB9, 0x9C, 0x72));
    r.tube(0.0, 36.0, 0.0, 6.6, black);
    r.line(
        (4.0, -4.0),
        (14.0, -2.0),
        1.0,
        Color32::from_white_alpha(40),
    );
    r.line((20.0, 2.0), (30.0, 4.0), 1.0, Color32::from_white_alpha(30));
    for (u, v) in [(-1.5, -2.0), (-3.5, 1.5), (-2.0, 3.5)] {
        r.dot(u, v, 0.9, black);
    }
}

/// A stick of chalk with dust at its tip.
fn chalk(r: &Rig) {
    r.shadow(6.6);
    r.tube(0.0, 70.0, 0.0, 6.6, Color32::from_rgb(0xF4, 0xF2, 0xEA));
    r.tube(0.0, 22.0, 0.0, 6.8, r.band());
    for (u, v, a) in [
        (-3.0, -3.0, 160),
        (-6.0, 1.5, 110),
        (-9.0, -1.0, 70),
        (-5.0, 4.0, 90),
    ] {
        let dust = Color32::from_rgba_unmultiplied(r.ink.r(), r.ink.g(), r.ink.b(), a);
        r.dot(u, v, 1.1, dust);
    }
}

/// A wrapped crayon with a blunt cone tip.
fn crayon(r: &Rig) {
    r.shadow(6.8);
    r.polygon(
        &[
            (0.0, 0.0),
            (4.0, -3.6),
            (26.0, -6.6),
            (26.0, 6.6),
            (4.0, 3.6),
        ],
        r.ink,
    );
    r.line(
        (6.0, -3.0),
        (24.0, -5.4),
        1.0,
        Color32::from_white_alpha(80),
    );
    r.tube(26.0, 100.0, 0.0, 6.7, CREAM);
    r.tube(36.0, 46.0, 0.0, 6.9, r.band());
    r.tube(80.0, 90.0, 0.0, 6.9, r.band());
}

/// A square stick of soft pastel with a paper wrap.
fn pastel(r: &Rig) {
    r.shadow(7.0);
    r.tube(0.0, 72.0, 0.0, 6.9, r.band());
    r.tube(34.0, 72.0, 0.0, 7.1, CREAM);
    r.line(
        (2.0, -4.6),
        (26.0, -4.6),
        1.0,
        Color32::from_white_alpha(70),
    );
}

/// A spray can with a nozzle and a puff of paint.
fn spray(r: &Rig) {
    r.shadow(9.0);
    r.tube(32.0, 104.0, 0.0, 9.0, Color32::from_rgb(0xCC, 0xD0, 0xD7));
    r.tube(50.0, 86.0, 0.0, 9.3, r.band());
    r.tube(16.0, 34.0, 0.0, 6.4, Color32::from_rgb(0x3D, 0x3C, 0x39));
    r.tube(4.0, 18.0, 0.0, 3.0, STEEL);
    for i in 1..=7_u8 {
        let f = f32::from(i);
        let alpha = (210.0 - f * 26.0).max(30.0) as u8;
        let color = Color32::from_rgba_unmultiplied(r.ink.r(), r.ink.g(), r.ink.b(), alpha);
        let side = if i % 2 == 0 { 1.0 } else { -1.0 };
        r.dot(-3.0 - f * 3.0, f * 1.9 * side, 0.9 + f * 0.1, color);
    }
}

/// A fine pen that leaves scattered dots.
fn stipple(r: &Rig) {
    r.shadow(5.6);
    r.tube(30.0, 108.0, 0.0, 5.6, Color32::from_rgb(0x2B, 0x2A, 0x2E));
    r.tube(60.0, 72.0, 0.0, 5.8, r.band());
    r.polygon(&[(1.0, 0.0), (31.0, -3.8), (31.0, 3.8)], STEEL);
    for (u, v, size) in [
        (-5.0, -2.0, 1.5),
        (-9.0, 3.0, 1.1),
        (-13.0, -1.0, 1.4),
        (-8.0, -5.0, 0.9),
        (-16.0, 2.5, 1.0),
    ] {
        r.dot(u, v, size, r.ink);
    }
}

/// A loaded round brush with a bamboo handle.
fn ink_brush(r: &Rig) {
    r.shadow(6.5);
    r.polygon(
        &[
            (0.0, 0.0),
            (6.0, -3.4),
            (20.0, -6.4),
            (34.0, -5.4),
            (34.0, 5.4),
            (20.0, 6.4),
            (6.0, 3.4),
        ],
        r.ink,
    );
    r.line(
        (8.0, -3.0),
        (28.0, -5.0),
        1.2,
        Color32::from_white_alpha(80),
    );
    r.tube(34.0, 48.0, 0.0, 5.2, GOLD);
    r.tube(48.0, 108.0, 0.0, 4.4, Color32::from_rgb(0x2F, 0x2B, 0x26));
    r.line(
        (50.0, -2.4),
        (104.0, -2.4),
        1.0,
        Color32::from_white_alpha(50),
    );
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Context, RawInput, Rect, pos2, vec2};

    use super::*;

    #[test]
    fn every_tool_can_be_drawn_facing_either_way() {
        let ctx = Context::default();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0))),
            ..RawInput::default()
        };
        let mut drawn = 0;
        let mut output = ctx.run_ui(input, |ui| {
            for art in [
                Art::Pen,
                Art::Fountain,
                Art::Dynamic,
                Art::Fixed,
                Art::Wire,
                Art::SoftPencil,
                Art::HardPencil,
                Art::Marker,
                Art::Watercolor,
                Art::Airbrush,
                Art::Fill,
                Art::Dotted,
                Art::Highlighter,
                Art::BrushPen,
                Art::FeltTip,
                Art::Charcoal,
                Art::Chalk,
                Art::Crayon,
                Art::Pastel,
                Art::Spray,
                Art::Stipple,
                Art::InkBrush,
            ] {
                for direction in [1.0, -1.0] {
                    paint(
                        ui.painter(),
                        art,
                        Placement {
                            tip: pos2(200.0, 150.0),
                            scale: 1.0,
                            direction,
                            lift: 0.5,
                        },
                        Color32::RED,
                    );
                    drawn += 1;
                }
            }
        });
        output.textures_delta.clear();
        assert_eq!(drawn, 44);
    }

    #[test]
    fn every_pen_has_its_own_art() {
        use std::collections::HashSet;
        let arts: HashSet<_> = crate::brushes::BRUSHES
            .iter()
            .map(|b| format!("{:?}", Art::of(b.pen)))
            .collect();
        assert_eq!(arts.len(), crate::brushes::BRUSHES.len());
    }
}
