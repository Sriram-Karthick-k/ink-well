//! Text as shapes: the outlines of the letters of some words, as regions that can be filled like any other.
//!
//! The letters are cut from a font the interface already carries, so no font has to be shipped. Each stroke of a letter
//! that is one piece (an `o` is two outlines, one inside the other) becomes one region: the outer outline, with the
//! inner ones joined to it by a line gone along twice, which fills the letter and leaves its holes empty (see
//! `ink_brush::fill`). A letter of several pieces (an `i`) is several regions.

use std::sync::OnceLock;

use eframe::egui::{FontData, FontDefinitions};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

/// The font the letters are cut from.
const FONT: &str = "Ubuntu-Light";

/// How far apart the lines of text are, as a part of the size, where the font does not say.
const LINE_SPACING: f32 = 1.2;

/// The letters of some words: regions in a box centered on the origin, with the y axis up like the canvas.
#[derive(Debug, Clone, PartialEq)]
pub struct Lettering {
    pub regions: Vec<Vec<[f32; 2]>>,
    /// The width and height of the box round the letters.
    pub width: f32,
    pub height: f32,
    /// Where the next letter would go: after the last one of the last line, on its baseline.
    pub caret: [f32; 2],
    /// How tall a line is, from the top of its capitals to the bottom of its descenders: how tall the caret is.
    pub line: f32,
}

fn font_data() -> Option<&'static FontData> {
    static DATA: OnceLock<Option<FontData>> = OnceLock::new();
    DATA.get_or_init(|| {
        FontDefinitions::default()
            .font_data
            .get(FONT)
            .map(|data| (**data).clone())
    })
    .as_ref()
}

/// Collects the outlines a glyph is drawn with as lists of points, curves cut into short lines.
struct Contours {
    done: Vec<Vec<[f32; 2]>>,
    open: Vec<[f32; 2]>,
    /// Where the glyph is, so its outlines come out in place.
    at: [f32; 2],
    /// How many lines a curve is cut into.
    steps: usize,
}

impl Contours {
    fn point(&self, x: f32, y: f32) -> [f32; 2] {
        [self.at[0] + x, self.at[1] + y]
    }

    fn last(&self) -> [f32; 2] {
        self.open.last().copied().unwrap_or(self.at)
    }

    fn finish(&mut self) {
        let mut points = std::mem::take(&mut self.open);
        if points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        if points.len() >= 3 {
            self.done.push(points);
        }
    }
}

impl OutlinePen for Contours {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        self.open.push(self.point(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.open.push(p);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (a, c, b) = (self.last(), self.point(cx0, cy0), self.point(x, y));
        for i in 1..=self.steps {
            let t = i as f32 / self.steps as f32;
            let u = 1.0 - t;
            self.open.push([
                u * u * a[0] + 2.0 * u * t * c[0] + t * t * b[0],
                u * u * a[1] + 2.0 * u * t * c[1] + t * t * b[1],
            ]);
        }
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (a, c, d, b) = (
            self.last(),
            self.point(cx0, cy0),
            self.point(cx1, cy1),
            self.point(x, y),
        );
        for i in 1..=self.steps {
            let t = i as f32 / self.steps as f32;
            let u = 1.0 - t;
            let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.open.push([
                w0 * a[0] + w1 * c[0] + w2 * d[0] + w3 * b[0],
                w0 * a[1] + w1 * c[1] + w2 * d[1] + w3 * b[1],
            ]);
        }
    }

    fn close(&mut self) {
        self.finish();
    }
}

/// The area inside `points`, without a sign.
fn area(points: &[[f32; 2]]) -> f32 {
    let mut twice = 0.0;
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        twice += a[0] * b[1] - b[0] * a[1];
    }
    twice.abs() / 2.0
}

/// `true` if `p` is inside the polygon `points`.
fn inside(p: [f32; 2], points: &[[f32; 2]]) -> bool {
    let mut is_inside = false;
    let mut previous = points[points.len() - 1];
    for &current in points {
        if (current[1] > p[1]) != (previous[1] > p[1])
            && p[0]
                < (previous[0] - current[0]) * (p[1] - current[1]) / (previous[1] - current[1])
                    + current[0]
        {
            is_inside = !is_inside;
        }
        previous = current;
    }
    is_inside
}

fn distance_squared(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// The regions of one glyph's outlines. An outline inside an odd number of others is a hole in the one that holds it
/// most closely; every other outline is a region of its own.
fn regions_of(contours: &[Vec<[f32; 2]>]) -> Vec<Vec<[f32; 2]>> {
    let areas: Vec<f32> = contours.iter().map(|c| area(c)).collect();
    // The outlines that hold outline `i`, the biggest first.
    let holders = |i: usize| -> Vec<usize> {
        let mut found: Vec<usize> = (0..contours.len())
            .filter(|&j| j != i && areas[j] > areas[i] && inside(contours[i][0], &contours[j]))
            .collect();
        found.sort_by(|&a, &b| areas[b].total_cmp(&areas[a]));
        found
    };
    let mut regions: Vec<(usize, Vec<[f32; 2]>, Vec<usize>)> = Vec::new();
    let mut holes: Vec<(usize, usize)> = Vec::new();
    for (i, contour) in contours.iter().enumerate() {
        let held = holders(i);
        if held.len() % 2 == 0 {
            regions.push((i, contour.clone(), Vec::new()));
        } else {
            // The holder just above it is the last, being the smallest.
            holes.push((i, held[held.len() - 1]));
        }
    }
    for (hole, parent) in holes {
        if let Some(region) = regions.iter_mut().find(|r| r.0 == parent) {
            region.2.push(hole);
        }
    }
    regions
        .into_iter()
        .map(|(_, outer, holes)| {
            join(
                &outer,
                &holes.iter().map(|&h| &contours[h]).collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// `outer` with each of `holes` joined to it by a line that is gone along and back, from the point of the hole that
/// is nearest to a point of the outline.
fn join(outer: &[[f32; 2]], holes: &[&Vec<[f32; 2]>]) -> Vec<[f32; 2]> {
    // For each hole: where on the outline it joins, and the hole's points starting and ending at where it joins.
    let mut joins: Vec<(usize, Vec<[f32; 2]>)> = holes
        .iter()
        .map(|hole| {
            let (mut best, mut at) = (f32::MAX, (0, 0));
            for (i, o) in outer.iter().enumerate() {
                for (j, h) in hole.iter().enumerate() {
                    let d = distance_squared(*o, *h);
                    if d < best {
                        (best, at) = (d, (i, j));
                    }
                }
            }
            let (i, j) = at;
            let mut around: Vec<[f32; 2]> = hole[j..].iter().chain(&hole[..j]).copied().collect();
            around.push(hole[j]);
            (i, around)
        })
        .collect();
    joins.sort_by_key(|(i, _)| *i);
    let mut region =
        Vec::with_capacity(outer.len() + joins.iter().map(|j| j.1.len() + 1).sum::<usize>());
    for (k, point) in outer.iter().enumerate() {
        region.push(*point);
        for (_, around) in joins.iter().filter(|(i, _)| *i == k) {
            region.extend_from_slice(around);
            region.push(*point);
        }
    }
    region
}

/// The letters of `text` at `size` (the height of an em, in canvas units), lines centered on one another and the whole
/// box centered on the origin. `None` when there is nothing to draw: no text, only spaces, or no font.
#[must_use]
pub fn lettering(text: &str, size: f32) -> Option<Lettering> {
    let data = font_data()?;
    let font = FontRef::from_index(data.bytes(), data.index).ok()?;
    let size_in = Size::new(size);
    let location = LocationRef::default();
    let charmap = font.charmap();
    let outlines = font.outline_glyphs();
    let glyph_metrics = font.glyph_metrics(size_in, location);
    let metrics = font.metrics(size_in, location);
    let natural = metrics.ascent.abs() + metrics.descent.abs() + metrics.leading;
    let line_height = if natural > 0.0 {
        natural
    } else {
        size * LINE_SPACING
    };
    let steps = ((size / 8.0).ceil() as usize).clamp(3, 12);

    let mut regions: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut caret = [0.0, 0.0];
    // A line break at the very end starts a line that has nothing in it yet, and the caret is there.
    for (line_index, line) in text.split('\n').enumerate() {
        let glyphs: Vec<_> = line
            .chars()
            .filter_map(|c| charmap.map(c))
            .map(|id| (id, glyph_metrics.advance_width(id).unwrap_or(0.0)))
            .collect();
        let width: f32 = glyphs.iter().map(|g| g.1).sum();
        let baseline = -(line_index as f32) * line_height;
        let mut pen = -width / 2.0;
        for (id, advance) in glyphs {
            if let Some(glyph) = outlines.get(id) {
                let mut contours = Contours {
                    done: Vec::new(),
                    open: Vec::new(),
                    at: [pen, baseline],
                    steps,
                };
                if glyph
                    .draw(DrawSettings::unhinted(size_in, location), &mut contours)
                    .is_ok()
                {
                    contours.finish();
                    regions.extend(regions_of(&contours.done));
                }
            }
            pen += advance;
        }
        caret = [pen, baseline];
    }
    let bounds = regions
        .iter()
        .flatten()
        .fold(None, |b: Option<[f32; 4]>, p| {
            Some(b.map_or([p[0], p[1], p[0], p[1]], |b| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            }))
        })?;
    let (cx, cy) = (
        f32::midpoint(bounds[0], bounds[2]),
        f32::midpoint(bounds[1], bounds[3]),
    );
    for p in regions.iter_mut().flatten() {
        *p = [p[0] - cx, p[1] - cy];
    }
    Some(Lettering {
        regions,
        width: bounds[2] - bounds[0],
        height: bounds[3] - bounds[1],
        caret: [caret[0] - cx, caret[1] - cy],
        line: line_height,
    })
}

#[cfg(test)]
mod tests {
    use ink_core::Point;

    use super::*;

    fn filled(region: &[[f32; 2]]) -> f32 {
        let points: Vec<Point> = region.iter().map(|p| Point::new(p[0], p[1])).collect();
        ink_brush::fill::area(&ink_brush::fill::triangulate(&points))
    }

    #[test]
    fn the_letters_of_some_words_are_regions_in_a_box_round_the_origin() {
        let l = lettering("Hello", 100.0).expect("letters");
        assert!(l.regions.len() >= 5, "a region at least for each letter");
        assert!(l.width > 150.0 && l.width < 400.0, "{}", l.width);
        assert!(l.height > 50.0 && l.height < 130.0, "{}", l.height);
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in l.regions.iter().flatten() {
            (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
        }
        assert!((x0 + x1).abs() < 1e-2 && (y0 + y1).abs() < 1e-2, "centered");
        assert!((x1 - x0 - l.width).abs() < 1e-2);
    }

    #[test]
    fn nothing_to_draw_gives_nothing() {
        assert!(lettering("", 50.0).is_none());
        assert!(lettering("   ", 50.0).is_none());
        assert!(lettering("\n\n", 50.0).is_none());
    }

    #[test]
    fn the_hole_in_an_o_stays_empty() {
        let o = lettering("o", 100.0).expect("letters");
        assert_eq!(o.regions.len(), 1, "the ring and its hole are one region");
        let outer_only = area(&o.regions[0][..o.regions[0].len() / 2]);
        let ring = filled(&o.regions[0]);
        assert!(ring > 100.0, "something is filled: {ring}");
        assert!(
            ring < 0.9 * (o.width * o.height * std::f32::consts::FRAC_PI_4),
            "less than the whole disc: {ring}, {outer_only}"
        );
    }

    #[test]
    fn a_letter_of_two_pieces_is_two_regions() {
        let i = lettering("i", 100.0).expect("letters");
        assert_eq!(i.regions.len(), 2, "the stem and the dot");
    }

    #[test]
    fn the_y_axis_points_up_so_the_bar_of_a_t_is_at_the_top() {
        let t = lettering("T", 100.0).expect("letters");
        let points: Vec<[f32; 2]> = t.regions.iter().flatten().copied().collect();
        let across = |keep: &dyn Fn(f32) -> bool| {
            let xs: Vec<f32> = points.iter().filter(|p| keep(p[1])).map(|p| p[0]).collect();
            xs.iter().copied().fold(f32::MIN, f32::max)
                - xs.iter().copied().fold(f32::MAX, f32::min)
        };
        let top = across(&|y| y > 0.0);
        let bottom = across(&|y| y < -10.0);
        assert!(
            top > 2.0 * bottom,
            "the top is wider: {top} against {bottom}"
        );
    }

    #[test]
    fn a_second_line_makes_the_box_taller_and_lines_are_centered_on_each_other() {
        let one = lettering("MM", 80.0).expect("letters");
        let two = lettering("MM\nM", 80.0).expect("letters");
        assert!(
            two.height > 1.7 * one.height,
            "{} against {}",
            two.height,
            one.height
        );
        assert!(
            (two.width - one.width).abs() < 1.0,
            "the widest line decides the width"
        );
    }

    #[test]
    fn the_size_scales_the_letters() {
        let small = lettering("Ab", 40.0).expect("letters");
        let big = lettering("Ab", 80.0).expect("letters");
        assert!((big.width / small.width - 2.0).abs() < 0.05);
        assert!((big.height / small.height - 2.0).abs() < 0.05);
    }

    #[test]
    fn a_character_the_font_does_not_have_is_skipped_and_the_rest_drawn() {
        let l = lettering("A\u{10FFFF}B", 60.0).expect("letters");
        let plain = lettering("AB", 60.0).expect("letters");
        assert!((l.width - plain.width).abs() < 1.0);
    }

    #[test]
    fn every_region_has_an_inside() {
        for word in ["Hello world", "@&8%B", "gjy"] {
            let l = lettering(word, 60.0).expect("letters");
            for region in &l.regions {
                assert!(region.len() >= 3);
                assert!(filled(region) > 0.5, "{word}: {}", filled(region));
            }
        }
    }

    #[test]
    fn the_caret_is_after_the_last_letter_and_moves_down_a_line_at_a_line_break() {
        let one = lettering("Ab", 100.0).expect("letters");
        let right = one
            .regions
            .iter()
            .flatten()
            .map(|p| p[0])
            .fold(f32::MIN, f32::max);
        assert!(
            one.caret[0] >= right - 1.0,
            "{:?} against {right}",
            one.caret
        );
        let two = lettering("Ab\nCd", 100.0).expect("letters");
        let three = lettering("Ab\nCd\n", 100.0).expect("letters");
        assert!(
            three.caret[1] < two.caret[1] - 0.5 * two.line,
            "a break at the end starts a new line: {:?} against {:?}",
            three.caret,
            two.caret
        );
    }

    #[test]
    fn a_space_at_the_end_moves_the_caret_on() {
        let plain = lettering("Ab", 100.0).expect("letters");
        let spaced = lettering("Ab ", 100.0).expect("letters");
        assert!(spaced.caret[0] > plain.caret[0] + 10.0);
    }
}
