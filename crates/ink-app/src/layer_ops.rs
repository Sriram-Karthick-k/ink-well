//! What can be done to the layers of a drawing: add, copy, delete, move, hide, rename and set the opacity of one. The
//! desktop's panel and the touch interface's sheet are two ways of asking for these, and both ask here, so a layer
//! is moved and deleted the same way from either.

use crate::app::InkApp;

/// What one layer shows of itself: copied out of the document so a list can be drawn while it is edited.
#[derive(Debug, Clone)]
pub(crate) struct LayerRow {
    pub index: usize,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub strokes: usize,
}

impl InkApp {
    /// The layers that have drawings on them, in document order (the top layer last).
    pub(crate) fn layer_rows(&self) -> Vec<LayerRow> {
        self.editor
            .content_layers()
            .map(|(index, l)| LayerRow {
                index,
                name: l.display_name(),
                visible: l.visible,
                opacity: l.opacity,
                strokes: l.strokes.len(),
            })
            .collect()
    }
}

/// Something done to a layer, by either layers panel, applied after the list is drawn.
pub(crate) enum Action {
    Select(usize),
    ToggleVisible(usize),
    SetOpacity(usize, f32),
    StartRename(usize, String),
    FinishRename(usize, String),
    CancelRename,
    Add,
    Duplicate(usize),
    Delete(usize),
    Move(usize, usize),
}

/// The index of the nearest content layer in `order` (an iterator over document indices), if any.
pub(crate) fn next_content(app: &InkApp, order: impl Iterator<Item = usize>) -> Option<usize> {
    let mut order = order;
    order.find(|&i| {
        app.editor
            .document()
            .layers
            .get(i)
            .is_some_and(|l| l.content)
    })
}

impl InkApp {
    /// A new empty layer above the one being drawn on, which then becomes the one drawn on.
    pub(crate) fn new_layer(&mut self) {
        self.active_layer = self.editor.add_layer(Some(self.active_layer));
    }

    /// A copy of the layer being drawn on, above it.
    pub(crate) fn duplicate_active_layer(&mut self) {
        if let Some(copy) = self.editor.duplicate_layer(self.active_layer) {
            self.active_layer = copy;
        }
    }

    /// Moves the layer being drawn on one place up the stack (`up`) or down it.
    pub(crate) fn step_active_layer(&mut self, up: bool) {
        let active = self.active_layer;
        let len = self.editor.document().layers.len();
        let target = if up {
            next_content(self, active + 1..len)
        } else {
            next_content(self, (0..active).rev())
        };
        if let Some(to) = target {
            self.apply_layer_action(Action::Move(active, to));
        }
    }

    /// After an undo or redo that may have added or taken away layers: draws on a layer that exists.
    pub(crate) fn fix_active_layer(&mut self) {
        let exists = |app: &Self, i: usize| {
            app.editor
                .document()
                .layers
                .get(i)
                .is_some_and(|l| l.content)
        };
        if !exists(self, self.active_layer) {
            let len = self.editor.document().layers.len();
            self.active_layer = next_content(self, (0..len).rev()).unwrap_or(0);
        }
    }

    /// Does what a panel asked for to a layer.
    pub(crate) fn apply_layer_action(&mut self, action: Action) {
        let active = self.active_layer;
        match action {
            Action::Select(i) => self.active_layer = i,
            Action::ToggleVisible(i) => {
                let visible = self
                    .editor
                    .document()
                    .layers
                    .get(i)
                    .is_some_and(|l| l.visible);
                self.editor.set_layer_visible(i, !visible);
            }
            Action::SetOpacity(i, value) => self.editor.set_layer_opacity(i, value),
            Action::StartRename(i, name) => self.ui.renaming = Some((i, name)),
            Action::FinishRename(i, name) => {
                if !name.trim().is_empty() {
                    self.editor.rename_layer(i, name.trim().to_owned());
                }
                self.ui.renaming = None;
            }
            Action::CancelRename => self.ui.renaming = None,
            Action::Add => self.new_layer(),
            Action::Duplicate(i) => {
                if let Some(copy) = self.editor.duplicate_layer(i) {
                    self.active_layer = copy;
                }
            }
            Action::Delete(i) => {
                if self.editor.remove_layer(i) {
                    let below = next_content(self, (0..i).rev());
                    self.active_layer = below.or_else(|| next_content(self, 0..)).unwrap_or(0);
                }
            }
            Action::Move(from, to) => {
                if from != to && self.editor.move_layer(from, to) {
                    // The layers between the two places shift by one; the one drawn on stays the same layer.
                    self.active_layer = if active == from {
                        to
                    } else if from < to && (from + 1..=to).contains(&active) {
                        active - 1
                    } else if to < from && (to..from).contains(&active) {
                        active + 1
                    } else {
                        active
                    };
                }
            }
        }
    }
}
