//! Writing on the drawing: words inside a shape, words on their own, and changing words already written.
//!
//! The letters are ordinary strokes (see `text_art`), so they move, resize, color and erase like anything drawn. What
//! they say is remembered as a label (see `ink_core::marks`), so the words can be changed later. Words written inside
//! what is selected are tied to it in a group, so they go wherever it goes. The touch interface's sheet and the
//! desktop's panel both ask here.

use std::collections::HashSet;

use eframe::egui::{Context, Rect};
use ink_core::hit::point_in_polygon;
use ink_core::marks::Label;
use ink_core::{Color, Pen, Point, Stroke, StrokeBuilder};
use uuid::Uuid;

use crate::app::InkApp;
use crate::state::{Mode, ViewState};
use crate::text_art::{self, Lettering};
use crate::tools;

/// The em the words are laid out at before they are fitted to a place, so the fit is the same at any zoom.
const UNIT: f32 = 100.0;
/// How much of the room a figure has the words may take, so they do not touch its edge.
const ROOM: f32 = 0.82;
/// The least tall words inside a figure may be, as a part of the figure's height; smaller than this and they are put
/// beside it instead.
const READABLE: f32 = 0.1;
/// How tall words written on their own are, as a part of the part of the paper that can be seen.
const OWN_SIZE: f32 = 0.07;
/// The most tall words may be inside a figure, in ems, as a part of the figure's height.
const MOST_EM: f32 = 0.6;
/// The most lines a sentence is broken into to fit a figure.
const MOST_LINES: usize = 4;

/// Words being written on the paper, or changed, and what is typed so far. They are typed where they will be, on the paper
/// (see `text_editor`).
#[derive(Debug, Clone)]
pub struct TextEntry {
    /// What is typed.
    pub text: String,
    /// The words of a label are being changed, not written for the first time.
    pub editing: bool,
    /// Where the words go on the paper, in canvas units, when they are written on their own. `None` while it is not said
    /// yet, and when the words go inside what is selected.
    pub at: Option<Point>,
    /// Waiting for a touch of the paper to say where the words go.
    pub placing: bool,
    /// What the strokes the words go inside are (what was selected, or what the old words were tied to).
    pub held: Vec<Uuid>,
    /// What was selected when the words were started, put back as the selection for writing them: the selection is put
    /// away whenever the pen is the tool, and the words would otherwise lose the figure they are for.
    pub picked: Vec<Uuid>,
    /// How the view was before it was moved to keep the words clear of the keyboard, to put it back.
    pub restore: Option<ViewState>,
    /// The drawing the words are for (`0` until it is known), so that words typed in one are never left over another.
    pub doc: u64,
    /// The letters of the words being changed, which are not drawn while the new ones are typed in their place.
    pub hide: HashSet<Uuid>,
    redo: Option<Redo>,
}

impl TextEntry {
    /// The color the words being changed were written in.
    #[must_use]
    pub fn color(&self) -> Option<Color> {
        self.redo.map(|r| r.color)
    }

    /// Words that are typed already, being written inside what is selected: for screenshots and tests.
    #[must_use]
    pub fn typing(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            editing: false,
            at: None,
            placing: false,
            held: Vec::new(),
            picked: Vec::new(),
            restore: None,
            doc: 0,
            hide: HashSet::new(),
            redo: None,
        }
    }
}

/// The outline of the biggest closed figure among `strokes` (a stroke of the filling pen, or one that ends where it
/// begins), if there is one.
fn closed_outline(strokes: &[(usize, usize, &Stroke)]) -> Option<Vec<Point>> {
    strokes
        .iter()
        .filter_map(|(_, _, stroke)| {
            let points: Vec<Point> = stroke.points().collect();
            let (first, last) = (*points.first()?, *points.last()?);
            let closed = stroke.pen == Pen::Fill && points.len() >= 3
                || points.len() >= 4 && first.distance(last) < 0.5;
            closed.then(|| (polygon_area(&points), points))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, points)| points)
}

/// The area inside the polygon `points`, without a sign.
fn polygon_area(points: &[Point]) -> f32 {
    let mut twice = 0.0;
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        twice += a.x * b.y - b.x * a.y;
    }
    twice.abs() / 2.0
}

/// The middle of the area inside the polygon `points`, or of its points when it has no area.
fn centroid(points: &[Point]) -> Point {
    let (mut twice, mut cx, mut cy) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        let cross = a.x * b.y - b.x * a.y;
        twice += cross;
        cx += (a.x + b.x) * cross;
        cy += (a.y + b.y) * cross;
    }
    if twice.abs() < f32::EPSILON {
        let n = points.len().max(1) as f32;
        return Point::new(
            points.iter().map(|p| p.x).sum::<f32>() / n,
            points.iter().map(|p| p.y).sum::<f32>() / n,
        );
    }
    Point::new(cx / (3.0 * twice), cy / (3.0 * twice))
}

/// `true` when the box of `half` size around `at` is inside `polygon`: its corners and points along its edges.
fn box_inside(polygon: &[Point], at: Point, half: (f32, f32)) -> bool {
    const ALONG: i32 = 6;
    (-ALONG..=ALONG).all(|i| {
        let t = i as f32 / ALONG as f32;
        [
            Point::new(at.x + t * half.0, at.y - half.1),
            Point::new(at.x + t * half.0, at.y + half.1),
            Point::new(at.x - half.0, at.y + t * half.1),
            Point::new(at.x + half.0, at.y + t * half.1),
        ]
        .into_iter()
        .all(|p| point_in_polygon(p, polygon))
    })
}

/// Where words go, and how much the letters (made at [`UNIT`]) are scaled.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Placement {
    pub center: Point,
    pub scale: f32,
    /// The words are inside the figure, not beside it for want of room.
    pub inside: bool,
}

impl Placement {
    /// Whether this way of laying the words out is better than `other`: inside beats beside, and bigger beats smaller.
    fn is_better_than(&self, other: &Self) -> bool {
        (self.inside, self.scale) > (other.inside, other.scale)
    }
}

/// The ways `text` can be broken into lines: as it was written if it has line breaks of its own, and otherwise in one
/// line, or two, three or four of about the same length.
fn layouts(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.contains('\n') {
        return vec![text.to_owned()];
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut found = vec![words.join(" ")];
    for lines in 2..=MOST_LINES {
        if words.len() < lines {
            break;
        }
        let total: usize = words.iter().map(|w| w.chars().count() + 1).sum();
        let target = total as f32 / lines as f32;
        let (mut broken, mut line, mut length) = (Vec::new(), Vec::new(), 0.0_f32);
        for (i, word) in words.iter().enumerate() {
            line.push(*word);
            length += word.chars().count() as f32 + 1.0;
            let left = words.len() - i - 1;
            let lines_left = lines - broken.len() - 1;
            if length >= target && lines_left > 0 && left >= lines_left {
                broken.push(line.join(" "));
                (line, length) = (Vec::new(), 0.0);
            }
        }
        broken.push(line.join(" "));
        if broken.len() == lines {
            found.push(broken.join("\n"));
        }
    }
    found
}

/// Where `letters` go for a selection in `bounds`: inside its biggest closed figure, as big as fits there; above it when
/// it has no inside (a line, an arrow) or no room for words that can be read.
fn place_in(bounds: [f32; 4], outline: Option<&[Point]>, letters: &Lettering) -> Placement {
    let [x0, y0, x1, y1] = bounds;
    let (w, h) = (x1 - x0, y1 - y0);
    let middle = Point::new(f32::midpoint(x0, x1), f32::midpoint(y0, y1));
    // Short words are not blown up to fill a big shape.
    let most = (h * MOST_EM / UNIT).max(f32::EPSILON);
    let above = || {
        let longer = w.max(h);
        let scale = (longer * 0.9 / letters.width).min(longer * 0.2 / UNIT);
        let gap = longer * 0.04 + 4.0;
        Placement {
            center: Point::new(middle.x, y1 + gap + letters.height * scale / 2.0),
            scale,
            inside: false,
        }
    };
    let Some(outline) = outline else {
        return above();
    };
    let center = centroid(outline);
    if !point_in_polygon(center, outline) {
        return above();
    }
    // Bigger until the box of the words reaches the edge of the figure.
    let fits = |s: f32| {
        box_inside(
            outline,
            center,
            (letters.width * s / 2.0, letters.height * s / 2.0),
        )
    };
    let widest = (w / letters.width).min(h / letters.height);
    let (mut low, mut high) = (0.0_f32, widest.max(f32::EPSILON));
    if fits(high) {
        low = high;
    } else {
        for _ in 0..18 {
            let mid = f32::midpoint(low, high);
            if fits(mid) {
                low = mid;
            } else {
                high = mid;
            }
        }
    }
    let scale = (low * ROOM).min(most);
    if letters.height * scale >= h * READABLE {
        Placement {
            center,
            scale,
            inside: true,
        }
    } else {
        above()
    }
}

/// The first label that the selection holds a letter of.
fn label_in(app: &InkApp) -> Option<Label> {
    app.selection
        .iter()
        .find_map(|id| app.editor.marks().label_of(*id))
        .cloned()
}

fn region_stroke(
    app: &mut InkApp,
    region: &[[f32; 2]],
    center: Point,
    scale: f32,
    color: Color,
) -> Stroke {
    let points: Vec<Point> = region
        .iter()
        .map(|p| Point::new(center.x + p[0] * scale, center.y + p[1] * scale))
        .collect();
    let mut builder = StrokeBuilder::new(Pen::Fill, None, color, 1.0, 0.0);
    builder.replace_with(points);
    builder.with_id(app.editor.next_id()).finish()
}

/// What is different about words that are being changed from words that are being written.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Redo {
    /// The layer and the color the old words had.
    layer: usize,
    color: Color,
    /// Where the old words were, and the biggest scale they had, for words that are not inside a figure.
    center: Point,
    cap: f32,
}

impl InkApp {
    /// Starts writing words on the paper: the words that are selected are changed, the strokes that are selected are
    /// written inside, and with nothing selected a touch of the paper says where the words go.
    pub(crate) fn begin_text(&mut self) {
        let label = label_in(self);
        let old = label.as_ref().and_then(|l| self.old_words(l));
        self.ui.close_popovers();
        let mut entry = TextEntry::typing(label.as_ref().map_or("", |l| l.text.as_str()));
        entry.restore = Some(self.view);
        entry.doc = self.doc_id;
        entry.picked.clone_from(&self.selection);
        if let Some((ids, others, redo)) = old {
            entry.editing = true;
            entry.hide = ids.into_iter().collect();
            entry.held = others;
            entry.redo = Some(redo);
        } else {
            entry.held.clone_from(&self.selection);
            entry.placing = entry.held.is_empty();
        }
        self.ui.text = Some(entry);
    }

    /// The button for words: starts writing, or finishes what is being written.
    pub(crate) fn toggle_text(&mut self, ctx: &Context) {
        if self.ui.text.is_some() {
            self.commit_text(ctx);
        } else {
            self.begin_text();
        }
    }

    /// Puts the words that were typed on the paper, and the view back as it was. Nothing typed writes nothing (and over
    /// words that are being changed, takes them away).
    pub(crate) fn commit_text(&mut self, ctx: &Context) {
        let Some(entry) = self.ui.text.take() else {
            return;
        };
        if let Some(view) = entry.restore {
            self.view = view;
        }
        if entry.placing {
            return;
        }
        self.selection = entry.picked;
        self.write_text_at(ctx, &entry.text, entry.at);
    }

    /// Puts what is being written away, and the view back as it was.
    pub(crate) fn cancel_text(&mut self) {
        if let Some(view) = self.ui.text.take().and_then(|entry| entry.restore) {
            self.view = view;
        }
    }

    /// Writes `text`: changes the words selected if some are, otherwise puts the words inside what is selected, or in
    /// the middle of the screen when nothing is. Returns `false` when there was nothing to write.
    #[cfg(test)]
    pub(crate) fn write_text(&mut self, ctx: &Context, text: &str) -> bool {
        self.write_text_at(ctx, text, None)
    }

    /// Like [`Self::write_text`], with the words on their own going at `at` when it is given.
    pub(crate) fn write_text_at(&mut self, ctx: &Context, text: &str, at: Option<Point>) -> bool {
        let text = text.trim_end();
        if let Some(label) = label_in(self) {
            return self.change_words(ctx, &label, text);
        }
        if text.trim().is_empty() {
            return false;
        }
        let ok = self.add_words(text, None, at);
        if !ok {
            self.notify(ctx, "Choose a layer that can be drawn on first.");
        }
        ok
    }

    /// The layer the words go on: the one of the topmost stroke selected, or the active one.
    fn words_layer(&self) -> usize {
        tools::selected(&self.editor, &self.selection)
            .last()
            .map_or(self.active_layer, |(layer, _, _)| *layer)
    }

    /// How `text` is laid out and where it goes: broken into lines if that lets it be bigger, and inside the figure among
    /// `held`, as big as fits there; or at `at` (the middle of the screen if that is not said) when there is no figure.
    /// The layout that gives the biggest words wins.
    pub(crate) fn plan_words(
        &self,
        text: &str,
        redo: Option<Redo>,
        at: Option<Point>,
        held: &[Uuid],
    ) -> Option<(Lettering, Placement)> {
        let bounds = tools::selection_bounds(&self.editor, held);
        let outline = closed_outline(&tools::selected(&self.editor, held));
        let mut best: Option<(Lettering, Placement)> = None;
        for candidate in layouts(text) {
            let Some(letters) = text_art::lettering(&candidate, UNIT) else {
                continue;
            };
            let placement = match (bounds, redo) {
                (Some(bounds), _) => place_in(bounds, outline.as_deref(), &letters),
                (None, Some(old)) => Placement {
                    center: old.center,
                    scale: self.screen_width_scale(&letters).min(old.cap),
                    inside: true,
                },
                (None, None) => Placement {
                    center: at.unwrap_or_else(|| self.words_center()),
                    scale: self.own_scale(&letters),
                    inside: true,
                },
            };
            if best
                .as_ref()
                .is_none_or(|(_, b)| placement.is_better_than(b))
            {
                best = Some((letters, placement));
            }
        }
        best
    }

    /// How the words of `entry` look now, for the editor to show: what is typed, or when nothing is, a few letters to show
    /// where and how big the words will be (the second value is `true` then).
    pub(crate) fn plan_text(&self, entry: &TextEntry) -> Option<(Lettering, Placement, bool)> {
        let plan = |text: &str| self.plan_words(text, entry.redo, entry.at, &entry.held);
        match plan(&entry.text) {
            Some((letters, placement)) => Some((letters, placement, false)),
            None => plan("Text").map(|(letters, placement)| (letters, placement, true)),
        }
    }

    /// Puts `text` on the paper and selects it, with whatever was selected, tied to it. The words go inside the figure
    /// that is selected, or at `at` (the middle of the screen if that is not said) when nothing is.
    pub(crate) fn add_words(&mut self, text: &str, redo: Option<Redo>, at: Option<Point>) -> bool {
        let layer = redo.map_or_else(|| self.words_layer(), |r| r.layer);
        let holds = self
            .editor
            .document()
            .layers
            .get(layer)
            .is_some_and(|l| l.content && l.visible);
        if !holds {
            return false;
        }
        let held = self.selection.clone();
        let Some((letters, Placement { center, scale, .. })) =
            self.plan_words(text, redo, at, &held)
        else {
            return false;
        };
        let color = redo.map_or_else(
            || {
                let [r, g, b] = self.tool.color.to_srgb();
                Color::new(
                    f32::from(r) / 255.0,
                    f32::from(g) / 255.0,
                    f32::from(b) / 255.0,
                    1.0,
                )
            },
            |r| r.color,
        );
        let mark = self.editor.history_mark();
        let mut ids: Vec<Uuid> = Vec::new();
        for region in &letters.regions {
            let stroke = region_stroke(self, region, center, scale, color);
            let id = stroke.id;
            if self.editor.commit_stroke(layer, stroke) {
                ids.push(id);
            }
        }
        if ids.is_empty() {
            return false;
        }
        self.tie_words(
            Label {
                text: text.to_owned(),
                size: UNIT * scale,
                height: letters.height * scale,
                ids: ids.clone(),
            },
            &held,
        );
        self.editor.merge_history_since(mark);
        self.selection = held;
        self.selection.extend(ids);
        self.set_mode(Mode::Select);
        self.ui.close_popovers();
        true
    }

    /// The middle of what can be seen of the paper, in canvas units.
    fn words_center(&self) -> Point {
        let paper = self.canvas_rect.unwrap_or_else(|| {
            Rect::from_center_size(self.view.origin, eframe::egui::vec2(400.0, 400.0))
        });
        let dock = self.ui.anchors.dock_top;
        let open = if dock > paper.top() && dock < paper.bottom() {
            Rect::from_min_max(paper.min, eframe::egui::pos2(paper.right(), dock))
        } else {
            paper
        };
        self.view.to_canvas(open.center())
    }

    /// The scale that makes `letters` as wide as the screen allows.
    fn screen_width_scale(&self, letters: &Lettering) -> f32 {
        let paper = self
            .canvas_rect
            .map_or(600.0, |r| r.height().min(r.width()));
        paper * 0.9 / self.view.zoom / letters.width.max(1.0)
    }

    /// The scale that makes words written on their own a readable size at the zoom the paper is at, and no wider than
    /// the screen.
    fn own_scale(&self, letters: &Lettering) -> f32 {
        let paper = self
            .canvas_rect
            .map_or(600.0, |r| r.height().min(r.width()));
        let wanted = paper * OWN_SIZE / self.view.zoom;
        (wanted / UNIT).min(self.screen_width_scale(letters))
    }

    /// Remembers `label`, and ties its letters to `held` (the strokes they were written inside) as one group.
    fn tie_words(&mut self, label: Label, held: &[Uuid]) {
        let letters = label.ids.clone();
        self.editor.edit_marks(|m| {
            m.set_label(label);
            let mut together: Vec<Uuid> = held.to_vec();
            together.extend(letters.iter().copied());
            m.group(&together);
        });
    }

    /// The letters of `label` that are still in the drawing, the strokes they are tied to, and where the words were and how
    /// big, for writing them again. `None` when the letters are gone.
    pub(crate) fn old_words(&self, label: &Label) -> Option<(Vec<Uuid>, Vec<Uuid>, Redo)> {
        let old: Vec<Uuid> = label
            .ids
            .iter()
            .copied()
            .filter(|id| self.editor.find_stroke(*id).is_some())
            .collect();
        let bounds = tools::selection_bounds(&self.editor, &old)?;
        let (layer, color) = tools::selected(&self.editor, &old)
            .first()
            .map(|(layer, _, stroke)| (*layer, stroke.color))?;
        let others: Vec<Uuid> = self
            .editor
            .marks()
            .group_of(old[0])
            .map(|g| g.iter().copied().filter(|id| !old.contains(id)).collect())
            .unwrap_or_default();
        let stretch = if label.height > f32::EPSILON {
            (bounds[3] - bounds[1]) / label.height
        } else {
            1.0
        };
        let redo = Redo {
            layer,
            color,
            center: Point::new(
                f32::midpoint(bounds[0], bounds[2]),
                f32::midpoint(bounds[1], bounds[3]),
            ),
            cap: label.size * stretch / UNIT,
        };
        Some((old, others, redo))
    }

    /// Changes what `label` says: the old letters go and new ones are written in their place, inside what they were
    /// tied to, or where they were and no bigger than they were. Nothing left to say takes the words away.
    fn change_words(&mut self, ctx: &Context, label: &Label, text: &str) -> bool {
        let Some((old, others, redo)) = self.old_words(label) else {
            return false;
        };
        let mark = self.editor.history_mark();
        // Last first, so the places of the others do not move.
        let mut places: Vec<(usize, usize)> = old
            .iter()
            .filter_map(|id| self.editor.find_stroke(*id))
            .collect();
        places.sort_unstable();
        for (l, s) in places.into_iter().rev() {
            self.editor.remove_stroke(l, s);
        }
        self.editor.edit_marks(|m| {
            m.remove_label_of(old[0]);
        });
        self.selection = others;
        let written = !text.trim().is_empty() && self.add_words(text, Some(redo), None);
        self.editor.merge_history_since(mark);
        if !written {
            self.ui.close_popovers();
            if self.selection.is_empty() {
                self.notify(ctx, "The words were taken away.");
            }
        }
        true
    }
}

/// What a double touch at `at` (with `radius` of leeway, in canvas units) is on, for opening its words: the strokes of the
/// words or the figure it touched, with everything tied to them. `skip` is a stroke that is not to count (the dot the
/// first of the two touches may have drawn). `None` when it touched neither.
///
/// Words are touched anywhere in the box round their letters, a figure anywhere inside it, filled or not.
#[must_use]
pub(crate) fn words_hit(
    editor: &ink_core::Editor,
    at: Point,
    radius: f32,
    skip: Option<Uuid>,
) -> Option<Vec<Uuid>> {
    let words = editor.marks().labels().iter().find(|label| {
        let ids: Vec<Uuid> = label
            .ids
            .iter()
            .copied()
            .filter(|id| Some(*id) != skip)
            .collect();
        tools::selection_bounds(editor, &ids).is_some_and(|[x0, y0, x1, y1]| {
            at.x >= x0 - radius && at.x <= x1 + radius && at.y >= y0 - radius && at.y <= y1 + radius
        })
    });
    let first = match words {
        Some(label) => label.ids.first().copied(),
        None => touched(editor, at, radius, skip),
    }?;
    let mut ids = vec![first];
    editor.marks().expand(&mut ids);
    ids.retain(|id| editor.find_stroke(*id).is_some());
    Some(ids)
}

/// The figure at `at` that is not `skip`: of the strokes that are a region or a closed outline and that hold the point (or
/// whose edge it is on), the smallest. A line that is not closed is not a figure, so a double touch on one is for the
/// pen, as it always was.
fn touched(editor: &ink_core::Editor, at: Point, radius: f32, skip: Option<Uuid>) -> Option<Uuid> {
    editor
        .document()
        .layers
        .iter()
        .filter(|l| l.content && l.visible)
        .flat_map(|l| l.strokes.iter())
        .filter(|s| !editor.is_locked(s.id) && Some(s.id) != skip)
        .filter_map(|s| {
            let points: Vec<Point> = s.points().collect();
            let region = s.pen == ink_core::Pen::Fill && points.len() >= 3;
            let closed = points.len() >= 4
                && points
                    .first()
                    .zip(points.last())
                    .is_some_and(|(a, b)| a.distance(*b) < 0.5);
            let holds = (region || closed)
                && (point_in_polygon(at, &points) || ink_core::hit::touches(s, at, radius));
            holds.then(|| (polygon_area(&points), s.id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

/// Whether the selection holds the letters of words written before, which can be changed.
#[must_use]
pub fn holds_words(app: &InkApp) -> bool {
    label_in(app).is_some()
}

#[cfg(test)]
mod tests {
    use eframe::egui::{pos2, vec2};
    use ink_core::transform::Affine;

    use super::*;
    use crate::Options;

    fn app() -> InkApp {
        let mut app = InkApp::blank(Options::default());
        app.canvas_rect = Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0)));
        app.view.origin = pos2(400.0, 300.0);
        app
    }

    fn strokes(app: &InkApp) -> usize {
        app.editor
            .document()
            .layers
            .iter()
            .map(|l| l.strokes.len())
            .sum()
    }

    fn label_ids(app: &InkApp) -> Vec<Uuid> {
        app.editor
            .marks()
            .labels()
            .first()
            .map(|l| l.ids.clone())
            .unwrap_or_default()
    }

    fn put_shape_selected(app: &mut InkApp, ctx: &Context, id: &str) {
        app.config.shapes.fill = Some([255, 230, 120]);
        assert!(app.insert_shape(ctx, id));
    }

    #[test]
    fn words_on_their_own_land_in_the_middle_selected_and_remembered() {
        let ctx = Context::default();
        let mut app = app();
        assert!(app.write_text(&ctx, "Hello"));
        assert!(strokes(&app) >= 5, "a stroke at least for each letter");
        assert_eq!(app.selection.len(), strokes(&app));
        assert_eq!(app.tool.mode, Mode::Select);
        let label = app.editor.marks().labels()[0].clone();
        assert_eq!(label.text, "Hello");
        assert_eq!(label.ids.len(), strokes(&app));
        let [x0, y0, x1, y1] = tools::selection_bounds(&app.editor, &app.selection).expect("box");
        assert!(f32::midpoint(x0, x1).abs() < 2.0 && f32::midpoint(y0, y1).abs() < 2.0);
    }

    #[test]
    fn the_letters_are_regions_in_the_ink_in_use() {
        let ctx = Context::default();
        let mut app = app();
        app.tool.color = eframe::egui::ecolor::Hsva::from_srgb([0xE5, 0x53, 0x4B]);
        assert!(app.write_text(&ctx, "Hi"));
        let layer = &app.editor.document().layers[app.active_layer];
        assert!(layer.strokes.iter().all(|s| s.pen == Pen::Fill));
        assert!((layer.strokes[0].color.r - f32::from(0xE5_u8) / 255.0).abs() < 0.01);
    }

    #[test]
    fn nothing_typed_writes_nothing() {
        let ctx = Context::default();
        let mut app = app();
        assert!(!app.write_text(&ctx, "   "));
        assert_eq!(strokes(&app), 0);
    }

    #[test]
    fn words_written_with_a_shape_selected_go_inside_it_and_stay_tied_to_it() {
        let ctx = Context::default();
        let mut app = app();
        put_shape_selected(&mut app, &ctx, "rectangle");
        let shape = tools::selection_bounds(&app.editor, &app.selection).expect("box");
        let shape_ids = app.selection.clone();
        assert!(app.write_text(&ctx, "Hi"));
        let letters = label_ids(&app);
        let [x0, y0, x1, y1] = tools::selection_bounds(&app.editor, &letters).expect("box");
        assert!(
            x0 > shape[0] && x1 < shape[2] && y0 > shape[1] && y1 < shape[3],
            "inside: {:?} in {shape:?}",
            [x0, y0, x1, y1]
        );
        let group = app
            .editor
            .marks()
            .group_of(shape_ids[0])
            .expect("a group")
            .to_vec();
        assert!(
            letters.iter().all(|id| group.contains(id)),
            "tied to the shape"
        );
        assert!(shape_ids.iter().all(|id| group.contains(id)));
        assert!(
            shape_ids.iter().all(|id| app.selection.contains(id)),
            "the shape stays selected"
        );
    }

    #[test]
    fn words_in_a_shape_are_not_blown_up_to_fill_it() {
        let ctx = Context::default();
        let mut app = app();
        put_shape_selected(&mut app, &ctx, "rectangle");
        let shape = tools::selection_bounds(&app.editor, &app.selection).expect("box");
        assert!(app.write_text(&ctx, "I"));
        let [_, y0, _, y1] = tools::selection_bounds(&app.editor, &label_ids(&app)).expect("box");
        assert!(
            y1 - y0 <= (shape[3] - shape[1]) * 0.6,
            "one letter is not the whole height"
        );
    }

    #[test]
    fn long_words_are_made_smaller_to_fit_the_width_of_the_shape() {
        let ctx = Context::default();
        let mut app = app();
        put_shape_selected(&mut app, &ctx, "rectangle");
        let shape = tools::selection_bounds(&app.editor, &app.selection).expect("box");
        assert!(app.write_text(&ctx, "A rather long sentence indeed"));
        let [x0, _, x1, _] = tools::selection_bounds(&app.editor, &label_ids(&app)).expect("box");
        assert!(
            x1 - x0 <= (shape[2] - shape[0]) * 0.85,
            "{} in {}",
            x1 - x0,
            shape[2] - shape[0]
        );
    }

    #[test]
    fn writing_words_is_one_step_to_undo() {
        let ctx = Context::default();
        let mut app = app();
        put_shape_selected(&mut app, &ctx, "square");
        let before = strokes(&app);
        assert!(app.write_text(&ctx, "Hello"));
        assert!(strokes(&app) > before);
        app.undo();
        assert_eq!(strokes(&app), before, "all the letters go in one undo");
    }

    #[test]
    fn selected_words_are_changed_not_added_to() {
        let ctx = Context::default();
        let mut app = app();
        assert!(app.write_text(&ctx, "Hello"));
        let first = label_ids(&app);
        assert!(holds_words(&app));
        assert!(app.write_text(&ctx, "Bye"));
        assert_eq!(app.editor.marks().labels().len(), 1, "still one label");
        assert_eq!(app.editor.marks().labels()[0].text, "Bye");
        let second = label_ids(&app);
        assert!(first.iter().all(|id| !second.contains(id)), "new letters");
        assert_eq!(strokes(&app), second.len(), "the old letters are gone");
    }

    #[test]
    fn changing_words_keeps_the_place_and_the_tie_to_the_shape() {
        let ctx = Context::default();
        let mut app = app();
        put_shape_selected(&mut app, &ctx, "rectangle");
        let shape_ids = app.selection.clone();
        assert!(app.write_text(&ctx, "Hello"));
        let before = tools::selection_bounds(&app.editor, &label_ids(&app)).expect("box");
        assert!(app.write_text(&ctx, "Hello"));
        let after = tools::selection_bounds(&app.editor, &label_ids(&app)).expect("box");
        assert!(
            (f32::midpoint(before[0], before[2]) - f32::midpoint(after[0], after[2])).abs() < 1.0
        );
        assert!(
            (f32::midpoint(before[1], before[3]) - f32::midpoint(after[1], after[3])).abs() < 1.0
        );
        let group = app
            .editor
            .marks()
            .group_of(shape_ids[0])
            .expect("a group")
            .to_vec();
        assert!(label_ids(&app).iter().all(|id| group.contains(id)));
        assert!(shape_ids.iter().all(|id| app.selection.contains(id)));
    }

    #[test]
    fn words_that_were_resized_keep_that_size_when_changed() {
        let ctx = Context::default();
        let mut app = app();
        assert!(app.write_text(&ctx, "Hello"));
        let ids = label_ids(&app);
        let b = tools::selection_bounds(&app.editor, &ids).expect("box");
        let c = Point::new(f32::midpoint(b[0], b[2]), f32::midpoint(b[1], b[3]));
        assert!(tools::transform_selection(
            &mut app.editor,
            &ids,
            &Affine::scale_about(c, 2.0)
        ));
        let big = tools::selection_bounds(&app.editor, &ids).expect("box");
        assert!(app.write_text(&ctx, "Hello"));
        let again = tools::selection_bounds(&app.editor, &label_ids(&app)).expect("box");
        assert!(
            ((again[3] - again[1]) / (big[3] - big[1]) - 1.0).abs() < 0.05,
            "{again:?} against {big:?}"
        );
    }

    #[test]
    fn writing_nothing_over_words_takes_them_away() {
        let ctx = Context::default();
        let mut app = app();
        put_shape_selected(&mut app, &ctx, "square");
        let shape_strokes = strokes(&app);
        assert!(app.write_text(&ctx, "Hello"));
        assert!(app.write_text(&ctx, ""));
        assert_eq!(strokes(&app), shape_strokes, "only the shape is left");
        assert_eq!(app.editor.marks().labels().len(), 0);
        assert!(!holds_words(&app));
    }

    #[test]
    fn changing_words_is_one_step_to_undo_and_brings_the_old_words_back() {
        let ctx = Context::default();
        let mut app = app();
        assert!(app.write_text(&ctx, "Hello"));
        let first = label_ids(&app);
        assert!(app.write_text(&ctx, "Bye"));
        app.undo();
        let ids: Vec<Uuid> = app
            .editor
            .document()
            .layers
            .iter()
            .flat_map(|l| l.strokes.iter().map(|s| s.id))
            .collect();
        assert!(
            first.iter().all(|id| ids.contains(id)),
            "the first words are back"
        );
        assert_eq!(ids.len(), first.len());
    }

    #[test]
    fn with_nothing_selected_a_touch_of_the_paper_says_where_the_words_go() {
        let mut app = app();
        app.begin_text();
        let entry = app.ui.text.clone().expect("writing");
        assert!(entry.placing && entry.at.is_none() && !entry.editing);
        assert_eq!(entry.text, "");
        assert!(entry.restore.is_some(), "the view is kept to put back");
    }

    #[test]
    fn with_a_shape_selected_the_words_go_inside_it_at_once() {
        let ctx = Context::default();
        let mut app = app();
        put_shape_selected(&mut app, &ctx, "square");
        app.begin_text();
        let entry = app.ui.text.clone().expect("writing");
        assert!(!entry.placing, "no touch is needed");
        assert_eq!(entry.held, app.selection);
    }

    #[test]
    fn selected_words_are_opened_with_what_they_say_and_are_not_drawn_while_typed_again() {
        let ctx = Context::default();
        let mut app = app();
        assert!(app.write_text(&ctx, "Hello"));
        app.begin_text();
        let entry = app.ui.text.clone().expect("writing");
        assert!(entry.editing && !entry.placing);
        assert_eq!(entry.text, "Hello");
        assert_eq!(entry.hide.len(), label_ids(&app).len(), "the old letters");
    }

    #[test]
    fn words_finished_land_where_the_touch_said_and_the_view_is_put_back() {
        let ctx = Context::default();
        let mut app = app();
        app.begin_text();
        let at = Point::new(120.0, -80.0);
        if let Some(entry) = app.ui.text.as_mut() {
            entry.placing = false;
            entry.at = Some(at);
            entry.text = "Hi".to_owned();
        }
        let before = app.view;
        app.view.origin.y -= 200.0; // moved to clear the keyboard
        app.commit_text(&ctx);
        assert!(app.ui.text.is_none());
        assert_eq!(app.view, before, "the view is back");
        let [x0, y0, x1, y1] = tools::selection_bounds(&app.editor, &label_ids(&app)).expect("box");
        assert!((f32::midpoint(x0, x1) - at.x).abs() < 2.0);
        assert!((f32::midpoint(y0, y1) - at.y).abs() < 2.0);
    }

    #[test]
    fn giving_up_writes_nothing_and_puts_the_view_back() {
        let mut app = app();
        app.begin_text();
        let before = app.view;
        app.view.origin.y -= 100.0;
        app.cancel_text();
        assert!(app.ui.text.is_none());
        assert_eq!(app.view, before);
        assert_eq!(strokes(&app), 0);
    }

    #[test]
    fn finishing_before_the_place_is_said_writes_nothing() {
        let ctx = Context::default();
        let mut app = app();
        app.begin_text();
        app.commit_text(&ctx);
        assert_eq!(strokes(&app), 0);
        assert!(app.ui.text.is_none());
    }

    #[test]
    fn the_button_for_words_starts_and_finishes() {
        let ctx = Context::default();
        let mut app = app();
        app.toggle_text(&ctx);
        assert!(app.ui.text.is_some());
        app.toggle_text(&ctx);
        assert!(app.ui.text.is_none());
    }

    #[test]
    fn what_is_typed_is_laid_out_before_it_is_written_and_nothing_typed_shows_a_hint_of_where_it_goes()
     {
        let ctx = Context::default();
        let mut app = app();
        put_shape_selected(&mut app, &ctx, "circle");
        app.begin_text();
        let mut entry = app.ui.text.clone().expect("writing");
        let (_, empty_place, hint) = app.plan_text(&entry).expect("a hint");
        assert!(hint, "nothing typed yet");
        entry.text = "Hello".to_owned();
        let (letters, place, hint) = app.plan_text(&entry).expect("the words");
        assert!(!hint && !letters.regions.is_empty());
        assert!(
            (place.center.x - empty_place.center.x).abs() < 1.0,
            "the same middle"
        );
        entry.text = "Hello there my friend".to_owned();
        let (_, longer, _) = app.plan_text(&entry).expect("the words");
        assert!(
            longer.scale < place.scale,
            "longer words are smaller in the same shape"
        );
    }

    /// The points of the letters of the first label.
    fn letter_points(app: &InkApp) -> Vec<Point> {
        let ids = label_ids(app);
        tools::selected(&app.editor, &ids)
            .into_iter()
            .flat_map(|(_, _, s)| s.points().collect::<Vec<_>>())
            .collect()
    }

    #[test]
    fn words_in_a_star_a_heart_and_a_triangle_stay_inside_them() {
        let ctx = Context::default();
        for id in ["star", "heart", "triangle", "circle", "hexagon", "speech"] {
            let mut app = app();
            put_shape_selected(&mut app, &ctx, id);
            let outline = closed_outline(&tools::selected(&app.editor, &app.selection))
                .expect("a closed figure");
            assert!(app.write_text(&ctx, "Ok"), "{id}");
            let outside = letter_points(&app)
                .into_iter()
                .filter(|p| !point_in_polygon(*p, &outline))
                .count();
            assert_eq!(outside, 0, "{id}: points of the words outside it");
        }
    }

    #[test]
    fn words_on_a_line_or_an_arrow_sit_above_it() {
        let ctx = Context::default();
        for id in ["line", "arrow"] {
            let mut app = app();
            put_shape_selected(&mut app, &ctx, id);
            let shape = tools::selection_bounds(&app.editor, &app.selection).expect("box");
            assert!(app.write_text(&ctx, "Go"), "{id}");
            let letters = tools::selection_bounds(&app.editor, &label_ids(&app)).expect("box");
            assert!(letters[1] > shape[3], "{id}: the words are clear of it");
        }
    }

    #[test]
    fn a_figure_too_narrow_for_readable_words_gets_them_above_it() {
        let ctx = Context::default();
        let mut app = app();
        put_shape_selected(&mut app, &ctx, "bolt");
        let shape = tools::selection_bounds(&app.editor, &app.selection).expect("box");
        assert!(app.write_text(&ctx, "Lightning bolt of the gods"));
        let letters = tools::selection_bounds(&app.editor, &label_ids(&app)).expect("box");
        assert!(letters[1] > shape[3], "above it, not a speck inside");
    }

    #[test]
    fn the_middle_of_a_triangle_is_nearer_its_base_than_its_tip() {
        let triangle = [
            Point::new(0.0, 0.0),
            Point::new(90.0, 0.0),
            Point::new(45.0, 90.0),
        ];
        let c = centroid(&triangle);
        assert!(
            (c.x - 45.0).abs() < 1e-3 && (c.y - 30.0).abs() < 1e-3,
            "{c:?}"
        );
    }

    #[test]
    fn a_sentence_can_be_broken_into_up_to_four_lines_of_about_the_same_length_without_losing_a_word()
     {
        let all = layouts("one two three four five six");
        assert_eq!(all[0], "one two three four five six", "as one line first");
        assert!(all.len() >= 3, "{all:?}");
        for layout in &all {
            let words: Vec<&str> = layout.split_whitespace().collect();
            assert_eq!(
                words,
                ["one", "two", "three", "four", "five", "six"],
                "{layout}"
            );
            assert!(layout.lines().count() <= MOST_LINES);
        }
        assert!(all.iter().any(|l| l.lines().count() == 2));
    }

    #[test]
    fn words_that_are_few_have_no_more_lines_than_words() {
        assert_eq!(layouts("Hello"), ["Hello"]);
        assert_eq!(layouts("Hello you").len(), 2);
        assert_eq!(layouts("  padded  "), ["padded"]);
    }

    #[test]
    fn breaks_a_person_typed_are_kept_as_they_are() {
        assert_eq!(layouts("one\ntwo three"), ["one\ntwo three"]);
    }

    #[test]
    fn a_long_sentence_in_a_shape_is_broken_into_lines_to_be_readable_and_stays_inside() {
        let ctx = Context::default();
        let mut app = app();
        put_shape_selected(&mut app, &ctx, "rectangle");
        let shape = tools::selection_bounds(&app.editor, &app.selection).expect("box");
        let sentence = "A rather long sentence indeed";
        assert!(app.write_text(&ctx, sentence));
        let label = app.editor.marks().labels()[0].clone();
        assert_eq!(label.text, sentence, "what was typed is what is kept");
        assert!(label.height > label.size * 1.3, "more than one line tall");
        let [x0, y0, x1, y1] = tools::selection_bounds(&app.editor, &label_ids(&app)).expect("box");
        assert!(x0 > shape[0] && x1 < shape[2] && y0 > shape[1] && y1 < shape[3]);
    }

    #[test]
    fn leaving_for_the_library_finishes_the_words_instead_of_leaving_them_over_the_drawing() {
        let ctx = Context::default();
        let mut app = app();
        app.begin_text();
        if let Some(entry) = app.ui.text.as_mut() {
            entry.placing = false;
            entry.at = Some(Point::new(10.0, 10.0));
            entry.text = "Hi".to_owned();
        }
        app.open_library(&ctx);
        assert!(app.ui.text.is_none(), "nothing waits over the drawing");
        assert!(strokes(&app) > 0, "what was typed is on the paper");
    }

    #[test]
    fn words_that_are_for_another_drawing_are_not_shown_over_this_one() {
        let ctx = Context::default();
        let mut app = app();
        app.begin_text();
        let for_this = app.doc_id;
        if let Some(entry) = app.ui.text.as_mut() {
            entry.doc = for_this + 1;
        }
        crate::text_editor::show(&ctx, &mut app);
        assert!(
            app.ui.text.is_none(),
            "dropped, not left to cover the top bar"
        );
    }
}
