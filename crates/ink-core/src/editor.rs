//! Editing operations with undo and redo. The UI calls these; nothing here knows about screens.
//!
//! History entries refer to layers by id, so undo keeps working after layers are added, moved or
//! removed. Layer property changes (visibility, opacity, names, order) are not in the history yet.

use uuid::Uuid;

use crate::document::{Document, Layer, LayerLabel, Metadata, Stroke};
use crate::ids::IdGen;
use crate::marks::Marks;

/// The canvas color a new document starts with.
const DEFAULT_BACKGROUND: &str = "#F9F9F9";
/// The version of the saved format new documents declare.
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone)]
enum Edit {
    StrokeAdded {
        layer: Uuid,
        index: usize,
        stroke: Stroke,
    },
    StrokeRemoved {
        layer: Uuid,
        index: usize,
        stroke: Stroke,
    },
    /// `removed` strokes starting at `index` were replaced by `inserted` ones: a stroke moved, scaled, turned or
    /// sliced into pieces. Undo puts the removed strokes back, in the same place.
    Splice {
        layer: Uuid,
        index: usize,
        removed: Vec<Stroke>,
        inserted: Vec<Stroke>,
    },
    /// Several edits that undo and redo together, such as one sweep of the eraser.
    Many(Vec<Edit>),
    /// A layer was put in at `index`.
    LayerAdded { index: usize, layer: Box<Layer> },
    /// The layer at `index` was taken out, with the strokes it held.
    LayerRemoved { index: usize, layer: Box<Layer> },
    /// A layer was moved from one place in the stack to another.
    LayerMoved { from: usize, to: usize },
}

/// A document plus its edit history.
#[derive(Debug)]
pub struct Editor {
    document: Document,
    ids: IdGen,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Counts every change to the document, so callers can tell whether it differs from a saved copy.
    revision: u64,
    /// Which strokes are locked and which are grouped. Not part of the history: locking is not undone.
    marks: Marks,
}

impl Editor {
    #[must_use]
    pub fn new(document: Document, ids: IdGen) -> Self {
        Self {
            document,
            ids,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            marks: Marks::default(),
        }
    }

    /// Which strokes are locked and which are grouped, with the ones that are gone from the drawing left out.
    #[must_use]
    pub fn marks(&self) -> &Marks {
        &self.marks
    }

    /// Changes the marks. They are put right afterwards: a stroke that is not in the drawing is not marked.
    pub fn edit_marks<R>(&mut self, change: impl FnOnce(&mut Marks) -> R) -> R {
        let result = change(&mut self.marks);
        let present: std::collections::HashSet<Uuid> = self
            .document
            .layers
            .iter()
            .flat_map(|l| l.strokes.iter().map(|s| s.id))
            .collect();
        self.marks.prune(|id| present.contains(&id));
        result
    }

    /// `true` if the stroke with `id` is locked.
    #[must_use]
    pub fn is_locked(&self, id: Uuid) -> bool {
        self.marks.is_locked(id)
    }

    /// A blank document with one empty layer. Timestamps are left empty for the app to set when saving.
    #[must_use]
    pub fn blank(ids: IdGen) -> Self {
        Self::blank_on(ids, DEFAULT_BACKGROUND)
    }

    /// Like [`Editor::blank`], on paper of the given color (`#RRGGBB`).
    #[must_use]
    pub fn blank_on(mut ids: IdGen, background: &str) -> Self {
        let metadata = Metadata {
            background_solid_color: background.to_owned(),
            creation_time: String::new(),
            modification_time: String::new(),
            format_version: FORMAT_VERSION,
            identifier: ids.next_id(),
        };
        let layer = new_layer(&mut ids, 1);
        Self::new(
            Document {
                metadata,
                layers: vec![layer],
            },
            ids,
        )
    }

    /// A number that changes whenever the document changes. Compare it with the value at the last save to
    /// know whether there are unsaved edits.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn document(&self) -> &Document {
        &self.document
    }

    #[must_use]
    pub fn into_document(self) -> Document {
        self.document
    }

    /// A fresh id for a new stroke.
    pub fn next_id(&mut self) -> Uuid {
        self.ids.next_id()
    }

    /// The layers that hold strokes, with their index in the document.
    pub fn content_layers(&self) -> impl Iterator<Item = (usize, &Layer)> {
        self.document
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.content)
    }

    fn layer_mut(&mut self, id: Uuid) -> Option<&mut Layer> {
        self.document.layers.iter_mut().find(|l| l.id == id)
    }

    // --- strokes and history ------------------------------------------------------------------

    /// Appends a finished stroke to the layer at `layer`. Returns `false` if there is no such layer.
    pub fn commit_stroke(&mut self, layer: usize, stroke: Stroke) -> bool {
        let Some(l) = self.document.layers.get_mut(layer) else {
            return false;
        };
        let (layer_id, index) = (l.id, l.strokes.len());
        l.strokes.push(stroke.clone());
        self.undo.push(Edit::StrokeAdded {
            layer: layer_id,
            index,
            stroke,
        });
        self.redo.clear();
        self.revision += 1;
        true
    }

    /// Removes a stroke (for example with an eraser). Returns `false` if the position is invalid.
    pub fn remove_stroke(&mut self, layer: usize, index: usize) -> bool {
        let Some(l) = self.document.layers.get_mut(layer) else {
            return false;
        };
        if index >= l.strokes.len() {
            return false;
        }
        let (layer_id, stroke) = (l.id, l.strokes.remove(index));
        self.undo.push(Edit::StrokeRemoved {
            layer: layer_id,
            index,
            stroke,
        });
        self.redo.clear();
        self.revision += 1;
        true
    }

    /// Replaces `remove` strokes of `layer`, starting at `index`, with `insert`, as one step of history. Returns
    /// `false` if the position is invalid.
    pub fn splice(
        &mut self,
        layer: usize,
        index: usize,
        remove: usize,
        insert: Vec<Stroke>,
    ) -> bool {
        let Some(l) = self.document.layers.get_mut(layer) else {
            return false;
        };
        if index + remove > l.strokes.len() {
            return false;
        }
        let layer_id = l.id;
        let removed: Vec<Stroke> = l
            .strokes
            .splice(index..index + remove, insert.iter().cloned())
            .collect();
        self.undo.push(Edit::Splice {
            layer: layer_id,
            index,
            removed,
            inserted: insert,
        });
        self.redo.clear();
        self.revision += 1;
        true
    }

    /// Replaces one stroke without recording it, for showing a change while it is still being made (a stroke
    /// being dragged). Afterwards [`Editor::record_replacements`] puts the whole change in the history.
    pub fn overwrite_stroke(&mut self, layer: usize, index: usize, stroke: Stroke) -> bool {
        let Some(slot) = self
            .document
            .layers
            .get_mut(layer)
            .and_then(|l| l.strokes.get_mut(index))
        else {
            return false;
        };
        *slot = stroke;
        self.revision += 1;
        true
    }

    /// Records, as one step of history, that the strokes of `layer` at the given indices were changed with
    /// [`Editor::overwrite_stroke`] from the versions given here to what they are now. Strokes that did not change
    /// are left out; nothing is recorded if none did.
    pub fn record_replacements(&mut self, layer: usize, before: Vec<(usize, Stroke)>) {
        let Some(l) = self.document.layers.get(layer) else {
            return;
        };
        let layer_id = l.id;
        let mut edits: Vec<Edit> = before
            .into_iter()
            .filter_map(|(index, old)| {
                let now = l.strokes.get(index)?.clone();
                (now != old).then_some(Edit::Splice {
                    layer: layer_id,
                    index,
                    removed: vec![old],
                    inserted: vec![now],
                })
            })
            .collect();
        match edits.len() {
            0 => return,
            1 => self.undo.append(&mut edits),
            _ => self.undo.push(Edit::Many(edits)),
        }
        self.redo.clear();
        self.revision += 1;
    }

    /// The layer and position of the stroke with `id`, on any layer.
    #[must_use]
    pub fn find_stroke(&self, id: Uuid) -> Option<(usize, usize)> {
        self.document
            .layers
            .iter()
            .enumerate()
            .find_map(|(li, l)| l.strokes.iter().position(|s| s.id == id).map(|si| (li, si)))
    }

    /// A place in the history. Edits made after it can be joined into one step with
    /// [`Editor::merge_history_since`].
    #[must_use]
    pub fn history_mark(&self) -> usize {
        self.undo.len()
    }

    /// Makes everything done since `mark` a single step for undo and redo.
    pub fn merge_history_since(&mut self, mark: usize) {
        if self.undo.len() < mark + 2 {
            return;
        }
        let tail = self.undo.split_off(mark);
        self.undo.push(Edit::Many(tail));
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Applies an edit forward (`redo`) or backward (`undo`). Fails if its layer no longer exists.
    fn apply(&mut self, edit: &Edit, forward: bool) -> bool {
        let (layer, index, stroke, add) = match edit {
            Edit::Splice {
                layer,
                index,
                removed,
                inserted,
            } => {
                let (take, put) = if forward {
                    (removed, inserted)
                } else {
                    (inserted, removed)
                };
                let Some(l) = self.layer_mut(*layer) else {
                    return false;
                };
                let end = *index + take.len();
                if end > l.strokes.len() {
                    return false;
                }
                l.strokes.splice(*index..end, put.iter().cloned());
                return true;
            }
            Edit::Many(edits) => {
                let mut any = false;
                if forward {
                    for e in edits {
                        any |= self.apply(e, true);
                    }
                } else {
                    for e in edits.iter().rev() {
                        any |= self.apply(e, false);
                    }
                }
                return any;
            }
            Edit::LayerAdded { index, layer } | Edit::LayerRemoved { index, layer } => {
                let put_in = matches!(edit, Edit::LayerAdded { .. }) == forward;
                return if put_in {
                    let at = (*index).min(self.document.layers.len());
                    self.document.layers.insert(at, (**layer).clone());
                    true
                } else if self
                    .document
                    .layers
                    .get(*index)
                    .is_some_and(|l| l.id == layer.id)
                {
                    self.document.layers.remove(*index);
                    true
                } else {
                    false
                };
            }
            Edit::LayerMoved { from, to } => {
                let (from, to) = if forward { (*from, *to) } else { (*to, *from) };
                let n = self.document.layers.len();
                if from >= n || to >= n {
                    return false;
                }
                let layer = self.document.layers.remove(from);
                self.document.layers.insert(to, layer);
                return true;
            }
            Edit::StrokeAdded {
                layer,
                index,
                stroke,
            } => (*layer, *index, stroke, forward),
            Edit::StrokeRemoved {
                layer,
                index,
                stroke,
            } => (*layer, *index, stroke, !forward),
        };
        let Some(l) = self.layer_mut(layer) else {
            return false;
        };
        if add {
            l.strokes.insert(index.min(l.strokes.len()), stroke.clone());
            true
        } else if let Some(at) = l.strokes.iter().position(|s| s.id == stroke.id) {
            l.strokes.remove(at);
            true
        } else {
            false
        }
    }

    pub fn undo(&mut self) -> bool {
        while let Some(edit) = self.undo.pop() {
            if self.apply(&edit, false) {
                self.redo.push(edit);
                self.revision += 1;
                return true;
            }
        }
        false
    }

    pub fn redo(&mut self) -> bool {
        while let Some(edit) = self.redo.pop() {
            if self.apply(&edit, true) {
                self.undo.push(edit);
                self.revision += 1;
                return true;
            }
        }
        false
    }

    // --- layers ---------------------------------------------------------------------------------

    /// Inserts a new empty layer above `after` (or on top when `None`) and returns its index.
    pub fn add_layer(&mut self, after: Option<usize>) -> usize {
        let counter = self
            .document
            .layers
            .iter()
            .filter_map(|l| {
                if let LayerLabel::Counter(n) = l.label {
                    Some(n)
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(0)
            + 1;
        let layer = new_layer(&mut self.ids, counter);
        let at = after.map_or(self.document.layers.len(), |i| {
            (i + 1).min(self.document.layers.len())
        });
        self.put_layer(at, layer);
        at
    }

    /// Puts `layer` in at `at` as one step of history.
    fn put_layer(&mut self, at: usize, layer: Layer) {
        self.document.layers.insert(at, layer.clone());
        self.undo.push(Edit::LayerAdded {
            index: at,
            layer: Box::new(layer),
        });
        self.redo.clear();
        self.revision += 1;
    }

    /// A copy of the layer at `index`, with new ids on its strokes, put in just above it. Returns the copy's
    /// index. The copy is named after the original.
    pub fn duplicate_layer(&mut self, index: usize) -> Option<usize> {
        let mut copy = self.document.layers.get(index)?.clone();
        copy.id = self.ids.next_id();
        for stroke in &mut copy.strokes {
            stroke.id = self.ids.next_id();
        }
        copy.label = LayerLabel::Name(format!(
            "{} copy",
            self.document.layers[index].display_name()
        ));
        self.put_layer(index + 1, copy);
        Some(index + 1)
    }

    /// Removes a layer, strokes and all; undo puts it back. The last content layer cannot be removed.
    pub fn remove_layer(&mut self, index: usize) -> bool {
        let Some(layer) = self.document.layers.get(index) else {
            return false;
        };
        if layer.content && self.content_layers().count() <= 1 {
            return false;
        }
        let layer = self.document.layers.remove(index);
        self.undo.push(Edit::LayerRemoved {
            index,
            layer: Box::new(layer),
        });
        self.redo.clear();
        self.revision += 1;
        true
    }

    /// Moves a layer to a new position, as one step of history.
    pub fn move_layer(&mut self, from: usize, to: usize) -> bool {
        let n = self.document.layers.len();
        if from >= n || to >= n {
            return false;
        }
        if from == to {
            return true;
        }
        let layer = self.document.layers.remove(from);
        self.document.layers.insert(to, layer);
        self.undo.push(Edit::LayerMoved { from, to });
        self.redo.clear();
        self.revision += 1;
        true
    }

    pub fn set_layer_visible(&mut self, index: usize, visible: bool) {
        if let Some(l) = self.document.layers.get_mut(index) {
            l.visible = visible;
            self.revision += 1;
        }
    }

    pub fn set_layer_opacity(&mut self, index: usize, opacity: f32) {
        if let Some(l) = self.document.layers.get_mut(index) {
            l.opacity = opacity.clamp(0.0, 1.0);
            self.revision += 1;
        }
    }

    pub fn rename_layer(&mut self, index: usize, name: String) {
        if let Some(l) = self.document.layers.get_mut(index) {
            l.label = LayerLabel::Name(name);
            self.revision += 1;
        }
    }
}

fn new_layer(ids: &mut IdGen, counter: i64) -> Layer {
    Layer {
        id: ids.next_id(),
        content: true,
        label: LayerLabel::Counter(counter),
        visible: true,
        opacity: 1.0,
        strokes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::StrokeBuilder;
    use crate::document::Pen;
    use crate::geometry::{Color, Point};

    #[test]
    fn splicing_replaces_strokes_and_undo_puts_them_back_in_place() {
        let mut editor = Editor::blank(IdGen::new(6));
        for _ in 0..3 {
            let s = stroke(&mut editor);
            editor.commit_stroke(0, s);
        }
        let originals: Vec<Uuid> = editor.document().layers[0]
            .strokes
            .iter()
            .map(|s| s.id)
            .collect();
        let mut pieces = vec![stroke(&mut editor), stroke(&mut editor)];
        let new_ids: Vec<Uuid> = pieces.iter().map(|s| s.id).collect();
        assert!(editor.splice(0, 1, 1, std::mem::take(&mut pieces)));
        let ids: Vec<Uuid> = editor.document().layers[0]
            .strokes
            .iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, [originals[0], new_ids[0], new_ids[1], originals[2]]);
        assert!(editor.undo());
        let back: Vec<Uuid> = editor.document().layers[0]
            .strokes
            .iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(back, originals);
        assert!(editor.redo());
        assert_eq!(editor.document().layers[0].strokes.len(), 4);
        assert!(
            !editor.splice(0, 9, 1, Vec::new()),
            "a position that is not there fails"
        );
    }

    #[test]
    fn a_change_shown_while_dragging_is_recorded_as_one_step() {
        use crate::transform::Affine;
        let mut editor = Editor::blank(IdGen::new(8));
        for _ in 0..3 {
            let s = stroke(&mut editor);
            editor.commit_stroke(0, s);
        }
        let before: Vec<(usize, Stroke)> = (0..2)
            .map(|i| (i, editor.document().layers[0].strokes[i].clone()))
            .collect();
        let untouched = editor.document().layers[0].strokes[2].clone();
        for (i, old) in &before {
            // Moved a little at a time, the way a drag would.
            for step in 1..=5 {
                let moved = old.transformed(&Affine::translate(step as f32, 0.0));
                assert!(editor.overwrite_stroke(0, *i, moved));
            }
        }
        assert!(
            editor.can_undo(),
            "the three strokes are in the history already"
        );
        let steps_before = editor.history_mark();
        editor.record_replacements(0, before.clone());
        assert_eq!(
            editor.history_mark(),
            steps_before + 1,
            "the whole drag is one step"
        );
        assert_ne!(editor.document().layers[0].strokes[0], before[0].1);
        assert!(editor.undo());
        assert_eq!(editor.document().layers[0].strokes[0], before[0].1);
        assert_eq!(editor.document().layers[0].strokes[1], before[1].1);
        assert_eq!(editor.document().layers[0].strokes[2], untouched);
        assert!(editor.redo());
        assert_ne!(editor.document().layers[0].strokes[1], before[1].1);
    }

    #[test]
    fn a_drag_that_ends_where_it_began_records_nothing() {
        let mut editor = Editor::blank(IdGen::new(8));
        let s = stroke(&mut editor);
        editor.commit_stroke(0, s);
        let before = vec![(0, editor.document().layers[0].strokes[0].clone())];
        let mark = editor.history_mark();
        editor.record_replacements(0, before);
        assert_eq!(editor.history_mark(), mark);
    }

    #[test]
    fn strokes_are_found_by_id_on_any_layer() {
        let mut editor = Editor::blank(IdGen::new(2));
        let second = editor.add_layer(Some(0));
        let s = stroke(&mut editor);
        let id = s.id;
        editor.commit_stroke(second, s);
        assert_eq!(editor.find_stroke(id), Some((second, 0)));
        assert_eq!(editor.find_stroke(Uuid::nil()), None);
    }

    #[test]
    fn a_sweep_of_removals_undoes_in_one_step() {
        let mut editor = Editor::blank(IdGen::new(5));
        for _ in 0..3 {
            let s = stroke(&mut editor);
            editor.commit_stroke(0, s);
        }
        let mark = editor.history_mark();
        assert!(editor.remove_stroke(0, 2));
        assert!(editor.remove_stroke(0, 0));
        editor.merge_history_since(mark);
        assert_eq!(editor.document().layers[0].strokes.len(), 1);
        assert!(editor.undo());
        assert_eq!(
            editor.document().layers[0].strokes.len(),
            3,
            "one undo brings both back"
        );
        assert!(editor.redo());
        assert_eq!(editor.document().layers[0].strokes.len(), 1);
    }

    #[test]
    fn merging_a_single_edit_changes_nothing() {
        let mut editor = Editor::blank(IdGen::new(5));
        let s = stroke(&mut editor);
        editor.commit_stroke(0, s);
        let mark = editor.history_mark();
        editor.remove_stroke(0, 0);
        editor.merge_history_since(mark);
        assert!(editor.undo());
        assert_eq!(editor.document().layers[0].strokes.len(), 1);
    }

    #[test]
    fn a_blank_drawing_can_start_on_other_paper() {
        let plain = Editor::blank(IdGen::new(1));
        assert_eq!(plain.document().metadata.background_solid_color, "#F9F9F9");
        let dark = Editor::blank_on(IdGen::new(1), "#102030");
        assert_eq!(dark.document().metadata.background_solid_color, "#102030");
        assert_eq!(dark.document().layers.len(), plain.document().layers.len());
    }

    fn stroke(editor: &mut Editor) -> Stroke {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.5);
        b.push(Point::new(0.0, 0.0), None, 0.0);
        b.push(Point::new(10.0, 0.0), None, 0.01);
        b.with_id(editor.next_id()).finish()
    }

    fn editor() -> Editor {
        Editor::blank(IdGen::new(1))
    }

    #[test]
    fn a_blank_document_has_one_visible_empty_layer() {
        let e = editor();
        assert_eq!(e.document().layers.len(), 1);
        let l = &e.document().layers[0];
        assert!(l.content && l.visible && l.strokes.is_empty());
        assert!((l.opacity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn marks_follow_the_strokes_and_are_forgotten_when_a_stroke_is_gone() {
        let mut e = Editor::blank(IdGen::new(5));
        let ids: Vec<Uuid> = (0..3)
            .map(|_| {
                let s = stroke(&mut e);
                let id = s.id;
                e.commit_stroke(0, s);
                id
            })
            .collect();
        e.edit_marks(|m| {
            m.lock([ids[0]]);
            m.group(&[ids[1], ids[2]]);
        });
        assert!(e.is_locked(ids[0]) && !e.is_locked(ids[1]));
        assert_eq!(e.marks().groups().len(), 1);
        // A stroke that was never in the drawing cannot be marked.
        e.edit_marks(|m| m.lock([Uuid::from_u128(99)]));
        assert_eq!(e.marks().locked(), vec![ids[0]]);
        // Erase the second: the group is down to one stroke, so it goes.
        assert!(e.remove_stroke(0, 1));
        e.edit_marks(|_| {});
        assert_eq!(e.marks().groups().len(), 0);
        // Undo brings the stroke back, and the lock on the first was never lost.
        assert!(e.undo());
        assert!(e.is_locked(ids[0]));
    }

    #[test]
    fn undo_and_redo_walk_the_history() {
        let mut e = editor();
        let (a, b) = (stroke(&mut e), stroke(&mut e));
        assert!(e.commit_stroke(0, a.clone()) && e.commit_stroke(0, b.clone()));
        assert_eq!(e.document().layers[0].strokes.len(), 2);

        assert!(e.undo());
        assert_eq!(e.document().layers[0].strokes, std::slice::from_ref(&a));
        assert!(e.undo());
        assert_eq!(e.document().layers[0].strokes, Vec::<Stroke>::new());
        assert!(!e.undo(), "nothing left to undo");

        assert!(e.redo() && e.redo());
        assert_eq!(e.document().layers[0].strokes, [a, b]);
        assert!(!e.redo());
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack() {
        let mut e = editor();
        let (a, b) = (stroke(&mut e), stroke(&mut e));
        e.commit_stroke(0, a);
        e.undo();
        assert!(e.can_redo());
        e.commit_stroke(0, b);
        assert!(!e.can_redo());
    }

    #[test]
    fn undo_survives_layers_being_reordered() {
        let mut e = editor();
        let first = stroke(&mut e);
        e.commit_stroke(0, first);
        e.add_layer(None);
        assert!(e.move_layer(0, 1)); // the stroke's layer is now at index 1
        assert!(e.undo(), "the move");
        assert!(e.undo(), "the new layer");
        assert!(e.undo(), "the stroke, wherever its layer is");
        assert!(e.document().layers.iter().all(|l| l.strokes.is_empty()));
        assert!(e.redo() && e.redo() && e.redo());
        assert_eq!(e.document().layers[1].strokes.len(), 1, "back where it was");
    }

    /// Each layer's id and how many strokes it holds, bottom first.
    fn stack(e: &Editor) -> Vec<(Uuid, usize)> {
        e.document()
            .layers
            .iter()
            .map(|l| (l.id, l.strokes.len()))
            .collect()
    }

    #[test]
    fn adding_moving_and_removing_layers_undo_and_redo_in_order() {
        let mut e = editor();
        let first = stroke(&mut e);
        e.commit_stroke(0, first);
        let top = e.add_layer(None);
        let second = stroke(&mut e);
        e.commit_stroke(top, second);
        let start = stack(&e);
        assert!(e.move_layer(top, 0));
        let moved = stack(&e);
        assert_ne!(moved, start);
        assert!(e.remove_layer(0), "the layer that was moved to the bottom");
        assert_eq!(e.document().layers.len(), 1);
        assert!(e.undo());
        assert_eq!(stack(&e), moved, "the layer is back with its stroke");
        assert!(e.undo());
        assert_eq!(stack(&e), start);
        assert!(e.redo());
        assert_eq!(stack(&e), moved);
        assert!(e.redo());
        assert_eq!(e.document().layers.len(), 1);
    }

    #[test]
    fn moving_a_layer_nowhere_is_not_a_step_and_a_bad_move_fails() {
        let mut e = editor();
        e.add_layer(None);
        let steps = e.history_mark();
        assert!(e.move_layer(1, 1));
        assert_eq!(e.history_mark(), steps);
        assert!(!e.move_layer(1, 9));
    }

    #[test]
    fn a_duplicated_layer_has_its_own_ids_a_new_name_and_undo_removes_it() {
        let mut e = editor();
        let s = stroke(&mut e);
        e.commit_stroke(0, s.clone());
        e.rename_layer(0, "Sketch".to_owned());
        let copy = e.duplicate_layer(0).expect("copied");
        assert_eq!(copy, 1);
        let (original, copied) = (&e.document().layers[0], &e.document().layers[1]);
        assert_ne!(original.id, copied.id);
        assert_eq!(copied.label, LayerLabel::Name("Sketch copy".to_owned()));
        assert_eq!(copied.strokes.len(), 1);
        assert_ne!(copied.strokes[0].id, s.id, "no two strokes share an id");
        assert_eq!(copied.strokes[0].samples, s.samples);
        assert!(e.undo());
        assert_eq!(e.document().layers.len(), 1);
        assert_eq!(e.duplicate_layer(9), None);
    }

    #[test]
    fn removed_strokes_can_be_restored() {
        let mut e = editor();
        let s = stroke(&mut e);
        e.commit_stroke(0, s.clone());
        assert!(e.remove_stroke(0, 0));
        assert_eq!(e.document().layers[0].strokes, Vec::<Stroke>::new());
        assert!(e.undo());
        assert_eq!(e.document().layers[0].strokes, [s]);
        assert!(!e.remove_stroke(0, 5));
    }

    #[test]
    fn the_last_content_layer_cannot_be_removed() {
        let mut e = editor();
        assert!(!e.remove_layer(0));
        let top = e.add_layer(None);
        assert_eq!(top, 1);
        assert!(e.remove_layer(1));
        assert_eq!(e.document().layers.len(), 1);
    }

    #[test]
    fn layer_properties_are_clamped_and_named() {
        let mut e = editor();
        e.set_layer_opacity(0, 7.0);
        assert!((e.document().layers[0].opacity - 1.0).abs() < 1e-6);
        e.set_layer_opacity(0, -1.0);
        assert!(e.document().layers[0].opacity.abs() < 1e-6);
        e.set_layer_visible(0, false);
        assert!(!e.document().layers[0].visible);
        e.rename_layer(0, "Sketch".to_owned());
        assert_eq!(
            e.document().layers[0].label,
            LayerLabel::Name("Sketch".to_owned())
        );
    }

    #[test]
    fn the_revision_changes_with_edits_and_not_with_failed_calls() {
        let mut e = editor();
        let start = e.revision();
        let s = stroke(&mut e);
        e.commit_stroke(0, s);
        let after_commit = e.revision();
        assert!(after_commit > start);
        assert!(
            !e.remove_stroke(0, 99),
            "an invalid removal changes nothing"
        );
        assert_eq!(e.revision(), after_commit);
        e.undo();
        assert!(e.revision() > after_commit);
        e.set_layer_visible(0, false);
        assert!(e.revision() > after_commit + 1);
    }

    #[test]
    fn new_layers_get_increasing_counters() {
        let mut e = editor();
        e.add_layer(None);
        e.add_layer(None);
        let counters: Vec<_> = e
            .document()
            .layers
            .iter()
            .map(|l| l.label.clone())
            .collect();
        assert_eq!(
            counters,
            [
                LayerLabel::Counter(1),
                LayerLabel::Counter(2),
                LayerLabel::Counter(3)
            ]
        );
    }
}
