//! What can be done with objects: put a shape on the drawing, save what is selected as an object of one's own, rename
//! one and delete one. The touch interface's sheet and the desktop's panel are two ways of asking for these, and both ask
//! here, so a shape lands on the paper the same way from either.

use eframe::egui::{Context, Rect};
use ink_core::shapes::{Shape, Style};
use ink_core::{Color, Pen, Point, Stroke};

use crate::app::InkApp;
use crate::selection;
use crate::state::Mode;
use crate::tools;

/// How the strokes of a shape are made: the pen, size and ink in use, and the fill and outline chosen for shapes.
fn style_of(app: &InkApp) -> Style {
    // The pen that fills a region is for the inside of a shape and not for its outline.
    let pen = if app.tool.pen == Pen::Fill {
        Pen::Pen
    } else {
        app.tool.pen
    };
    let brush = app.config.brush(pen);
    Style {
        pen,
        width: Some(brush.size),
        color: app.tool.stroke_color(brush.opacity),
        fill: app.config.shapes.fill.map(|[r, g, b]| {
            Color::new(
                f32::from(r) / 255.0,
                f32::from(g) / 255.0,
                f32::from(b) / 255.0,
                1.0,
            )
        }),
        outline: app.config.shapes.outline,
    }
}

/// The part of `paper` a person can see and draw on: what the panels over its bottom edge and the drawer at its side leave.
fn open_part(paper: Rect, dock_top: f32, drawer: Option<Rect>) -> Rect {
    let mut open = if dock_top > paper.top() && dock_top < paper.bottom() {
        Rect::from_min_max(paper.min, eframe::egui::pos2(paper.right(), dock_top))
    } else {
        paper
    };
    if let Some(drawer) = drawer {
        if drawer.center().x < open.center().x {
            open.min.x = open.min.x.max(drawer.right());
        } else {
            open.max.x = open.max.x.min(drawer.left());
        }
    }
    open
}

impl InkApp {
    /// Where a shape goes and how big it is: the middle of the open part of the paper, in canvas units, and a share of
    /// its width (or of its height, where that is less).
    fn shape_place(&self) -> (Point, f32) {
        let paper = self.canvas_rect.unwrap_or_else(|| {
            Rect::from_center_size(self.view.origin, eframe::egui::vec2(400.0, 400.0))
        });
        let open = open_part(paper, self.ui.anchors.dock_top, self.ui.anchors.drawer);
        let center = self.view.to_canvas(open.center());
        let size = open.width().min(open.height()) * self.config.shapes.size / self.view.zoom;
        (center, size.max(1.0))
    }

    /// How big a shape is on the screen when it is put down, in points: what is carried out of the drawer is drawn that big.
    pub(crate) fn shape_screen_size(&self) -> f32 {
        let (_, size) = self.shape_place();
        size * self.view.zoom
    }

    /// Opens the drawer of shapes at the side of the paper, and puts the menu away: a rail on one side leaves the other.
    pub(crate) fn dock_shapes(&mut self) {
        self.config.interface.shapes_drawer = if self.config.interface.left_handed {
            crate::config::DrawerSide::Left
        } else {
            crate::config::DrawerSide::Right
        };
        self.ui.close_popovers();
        self.mark_config_changed();
    }

    /// Puts the shape `id` on the active layer, in the middle of what is on the screen, selects it and switches to the
    /// select tool so it can be moved and resized at once. Returns `false` when there is no such shape or the layer
    /// cannot hold it.
    pub(crate) fn insert_shape(&mut self, ctx: &Context, id: &str) -> bool {
        self.insert_shape_at(ctx, id, None)
    }

    /// Like [`Self::insert_shape`], with the shape's center at `at` (where it was dropped) when that is given.
    pub(crate) fn insert_shape_at(&mut self, ctx: &Context, id: &str, at: Option<Point>) -> bool {
        let Some(shape) = self.objects.get(id).cloned() else {
            return false;
        };
        let holds_strokes = self
            .editor
            .document()
            .layers
            .get(self.active_layer)
            .is_some_and(|l| l.content && l.visible);
        if !holds_strokes {
            self.notify(ctx, "Choose a layer that can be drawn on first.");
            return false;
        }
        let (mut center, size) = self.shape_place();
        // A shape put down twice in a row would hide the first; the second goes a step to the side. One that was dropped
        // is where it was dropped.
        if let Some(at) = at {
            center = at;
        } else if let Some([x0, y0, x1, y1]) =
            tools::selection_bounds(&self.editor, &self.selection)
            && (f32::midpoint(x0, x1) - center.x).abs() < 1.0
            && (f32::midpoint(y0, y1) - center.y).abs() < 1.0
        {
            let step = self.config.tools.duplicate_offset;
            center = Point::new(center.x + step, center.y - step);
        }
        let placed = self.put_shape(&shape, center, size);
        if placed.is_empty() {
            return false;
        }
        self.selection = placed;
        self.set_mode(Mode::Select);
        self.ui.close_popovers();
        true
    }

    /// Draws `shape` on the active layer with its center at `center`, as one step, and returns the ids of its strokes.
    fn put_shape(&mut self, shape: &Shape, center: Point, size: f32) -> Vec<uuid::Uuid> {
        let style = style_of(self);
        let strokes = shape.to_strokes(center, size, &style, &mut || self.editor.next_id());
        let layer = self.active_layer;
        let mark = self.editor.history_mark();
        let mut placed = Vec::with_capacity(strokes.len());
        for stroke in strokes {
            let id = stroke.id;
            if self.editor.commit_stroke(layer, stroke) {
                placed.push(id);
            }
        }
        // The fill and the outline, or the lines of a figure, move and resize as one thing.
        if placed.len() > 1 {
            selection::group(&mut self.editor, &placed);
        }
        self.editor.merge_history_since(mark);
        placed
    }

    /// Covers the paper with some of the shapes, in different fills, to look at them (a screenshot option).
    pub(crate) fn demo_shapes(&mut self) {
        let (middle, size) = self.shape_place();
        let colors = crate::shape_art::FILL_COLORS;
        let shapes: Vec<Shape> = self.objects.built_in().to_vec();
        let wanted = [
            "rectangle",
            "circle",
            "star",
            "heart",
            "triangle",
            "hexagon",
            "bolt",
            "arrow",
            "rounded_rectangle",
            "diamond",
            "cube",
            "speech",
        ];
        let step = size * 1.25;
        for (i, id) in wanted.iter().enumerate() {
            let Some(shape) = shapes.iter().find(|s| s.id == *id) else {
                continue;
            };
            let (col, row) = ((i % 4) as f32 - 1.5, 1.0 - (i / 4) as f32);
            self.config.shapes.fill = (i % 3 != 2).then_some(colors[i % colors.len()]);
            self.config.shapes.outline = i % 4 != 1;
            let placed = self.put_shape(
                shape,
                Point::new(middle.x + col * step, middle.y + row * step),
                size * 0.9,
            );
            // Most of them say what they are, inside.
            if i % 5 != 4 {
                self.selection = placed;
                self.add_words(&shape.name, None, None);
            }
        }
        self.selection.clear();
        self.tool.mode = Mode::Draw;
        self.config.shapes.fill = None;
        self.config.shapes.outline = true;
    }

    /// Makes an object of the selected strokes and returns its id. `name` may be empty, which gives it a number.
    pub(crate) fn save_selection_as_object(&mut self, ctx: &Context, name: &str) -> Option<String> {
        let strokes: Vec<&Stroke> = tools::selected(&self.editor, &self.selection)
            .into_iter()
            .map(|(_, _, stroke)| stroke)
            .collect();
        if strokes.is_empty() {
            self.notify(ctx, "Select what to save first.");
            return None;
        }
        let id = self.objects.add(name, &strokes);
        if id.is_none() {
            self.notify(ctx, "That is too small to make an object of.");
        }
        id
    }

    /// Deletes an object of the person's own, and takes it off the tool bar.
    pub(crate) fn delete_object(&mut self, id: &str) {
        if self.objects.remove(id) {
            self.config.toolbar_forget_shape(id);
            self.mark_config_changed();
        }
    }

    /// Whether anything is selected that could be made an object of.
    pub(crate) fn can_save_object(&self) -> bool {
        !self.selection.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{pos2, vec2};

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

    #[test]
    fn a_shape_lands_in_the_middle_of_the_paper_selected_and_ready_to_move() {
        let ctx = Context::default();
        let mut app = app();
        assert!(app.insert_shape(&ctx, "square"));
        assert_eq!(strokes(&app), 1);
        assert_eq!(app.selection.len(), 1);
        assert_eq!(app.tool.mode, Mode::Select);
        let [x0, y0, x1, y1] = tools::selection_bounds(&app.editor, &app.selection).expect("box");
        assert!(
            (f32::midpoint(x0, x1)).abs() < 1.0 && (f32::midpoint(y0, y1)).abs() < 1.0,
            "centered on the middle of the screen, which is the canvas origin here"
        );
    }

    #[test]
    fn a_shape_is_a_share_of_the_open_part_of_the_screen_and_the_dock_does_not_cover_it() {
        let ctx = Context::default();
        let mut app = app();
        app.ui.anchors.dock_top = 400.0;
        assert!(app.insert_shape(&ctx, "square"));
        let [_, y0, _, y1] = tools::selection_bounds(&app.editor, &app.selection).expect("box");
        // The open part is 400 tall, so its middle is at 200 on the screen: 100 above the canvas origin.
        assert!((f32::midpoint(y0, y1) - 100.0).abs() < 2.0, "{y0} {y1}");
    }

    #[test]
    fn putting_a_shape_down_is_one_step_to_undo() {
        let ctx = Context::default();
        let mut app = app();
        app.config.shapes.fill = Some([255, 200, 0]);
        assert!(app.insert_shape(&ctx, "star"));
        assert_eq!(strokes(&app), 2, "the fill and the outline");
        app.undo();
        assert_eq!(strokes(&app), 0, "both go in one undo");
    }

    #[test]
    fn the_strokes_of_a_shape_are_one_group_so_the_fill_never_gets_left_behind() {
        let ctx = Context::default();
        let mut app = app();
        app.config.shapes.fill = Some([255, 200, 0]);
        assert!(app.insert_shape(&ctx, "star"));
        let group = app
            .editor
            .marks()
            .group_of(app.selection[0])
            .map(<[_]>::to_vec);
        assert_eq!(group.map(|g| g.len()), Some(2));
        assert!(app.insert_shape(&ctx, "cube"));
        assert_eq!(
            app.editor.marks().groups().len(),
            2,
            "each shape is a group of its own"
        );
    }

    #[test]
    fn a_filled_shape_has_the_fill_under_its_outline() {
        let ctx = Context::default();
        let mut app = app();
        app.config.shapes.fill = Some([10, 20, 30]);
        assert!(app.insert_shape(&ctx, "circle"));
        let layer = &app.editor.document().layers[app.active_layer];
        assert_eq!(layer.strokes[0].pen, Pen::Fill);
        assert_ne!(layer.strokes[1].pen, Pen::Fill);
    }

    #[test]
    fn with_no_outline_a_filled_shape_is_only_the_fill() {
        let ctx = Context::default();
        let mut app = app();
        app.config.shapes.fill = Some([10, 20, 30]);
        app.config.shapes.outline = false;
        assert!(app.insert_shape(&ctx, "circle"));
        assert_eq!(strokes(&app), 1);
    }

    #[test]
    fn the_fill_pen_in_use_does_not_draw_the_outline_of_a_shape() {
        let ctx = Context::default();
        let mut app = app();
        app.tool.pen = Pen::Fill;
        assert!(app.insert_shape(&ctx, "triangle"));
        let layer = &app.editor.document().layers[app.active_layer];
        assert!(layer.strokes.iter().all(|s| s.pen != Pen::Fill));
    }

    #[test]
    fn the_same_shape_twice_in_a_row_does_not_land_on_the_first() {
        let ctx = Context::default();
        let mut app = app();
        assert!(app.insert_shape(&ctx, "square"));
        let first = tools::selection_bounds(&app.editor, &app.selection).expect("box");
        assert!(app.insert_shape(&ctx, "square"));
        let second = tools::selection_bounds(&app.editor, &app.selection).expect("box");
        assert!((second[0] - first[0]).abs() > 1.0);
        assert_eq!(strokes(&app), 2);
    }

    #[test]
    fn a_shape_that_does_not_exist_is_not_put_down() {
        let ctx = Context::default();
        let mut app = app();
        assert!(!app.insert_shape(&ctx, "no_such_shape"));
        assert_eq!(strokes(&app), 0);
    }

    #[test]
    fn what_is_selected_becomes_an_object_that_can_be_put_down_again() {
        let ctx = Context::default();
        let mut app = app();
        assert!(app.insert_shape(&ctx, "star"));
        let id = app.save_selection_as_object(&ctx, "My star").expect("made");
        assert!(app.objects.is_custom(&id));
        app.selection.clear();
        assert!(app.insert_shape(&ctx, &id));
        assert_eq!(
            strokes(&app),
            2,
            "one star, then the object, both one stroke"
        );
    }

    #[test]
    fn nothing_selected_makes_no_object() {
        let ctx = Context::default();
        let mut app = app();
        assert!(app.save_selection_as_object(&ctx, "Nothing").is_none());
        assert_eq!(app.objects.custom().len(), 0);
    }

    #[test]
    fn deleting_an_object_takes_it_off_the_tool_bar_too() {
        let ctx = Context::default();
        let mut app = app();
        assert!(app.insert_shape(&ctx, "star"));
        let id = app.save_selection_as_object(&ctx, "Pinned").expect("made");
        app.config
            .toolbar_add(crate::toolbar::Item::Shape(id.clone()));
        app.delete_object(&id);
        assert!(app.objects.get(&id).is_none());
        assert!(
            !app.config
                .toolbar_items()
                .contains(&crate::toolbar::Item::Shape(id))
        );
    }
}
