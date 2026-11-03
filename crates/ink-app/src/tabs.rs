//! Open drawings as tabs, like the tabs of a browser or Figma.
//!
//! The drawing on the screen lives in the fields of [`InkApp`] (its editor, view, path and so on), so the canvas
//! and every panel read it without knowing about tabs. The other open drawings are parked in `tabs` as a
//! [`Doc`], and changing tab swaps the live fields with a parked one. Each drawing therefore keeps its own undo
//! history, zoom, layer and unsaved changes. The Library is not a tab of this list: it is a page over whichever
//! drawing is live, and the strip shows it as a first, fixed tab.

use std::path::{Path, PathBuf};

use eframe::egui;
use ink_core::{Editor, IdGen};

use crate::app::InkApp;
use crate::files;
use crate::guides::GuideState;
use crate::render::StrokeCache;
use crate::state::ViewState;

/// Everything that belongs to one open drawing.
#[derive(Debug)]
pub(crate) struct Doc {
    pub id: u64,
    pub editor: Editor,
    pub view: ViewState,
    pub active_layer: usize,
    pub fit_pending: bool,
    pub guides: Vec<ink_core::guide::Guide>,
    pub artboard: Option<ink_core::artboard::Artboard>,
    pub saved_extras: ink_format::Extras,
    pub path: Option<PathBuf>,
    pub saved_revision: u64,
    pub cache: StrokeCache,
    pub autosave_seen: u64,
    pub autosave_at: f64,
}

/// One tab. `doc` is `None` for the tab whose drawing is the live one.
#[derive(Debug)]
pub(crate) struct Tab {
    pub id: u64,
    pub doc: Option<Doc>,
}

/// What the strip shows about a tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabLabel {
    pub name: String,
    /// There are changes that are not in the file.
    pub dirty: bool,
}

/// `path` as it is after the file or folder at `from` was moved to `to`; `None` if it was not affected.
pub(crate) fn rebase(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    path.strip_prefix(from).ok().map(|rest| to.join(rest))
}

impl InkApp {
    /// Moves the live drawing out of the app's fields, leaving blanks behind.
    fn take_doc(&mut self) -> Doc {
        Doc {
            id: self.doc_id,
            editor: std::mem::replace(&mut self.editor, Editor::blank(IdGen::new(1))),
            view: std::mem::take(&mut self.view),
            active_layer: std::mem::take(&mut self.active_layer),
            fit_pending: std::mem::take(&mut self.fit_pending),
            guides: std::mem::take(&mut self.guides.list),
            artboard: self.artboard.take(),
            saved_extras: std::mem::take(&mut self.saved_extras),
            path: self.path.take(),
            saved_revision: std::mem::take(&mut self.saved_revision),
            cache: std::mem::take(&mut self.cache),
            autosave_seen: std::mem::take(&mut self.autosave_seen),
            autosave_at: std::mem::take(&mut self.autosave_at),
        }
    }

    /// Makes `doc` the live drawing. A stroke or erase in progress is dropped.
    fn put_doc(&mut self, doc: Doc) {
        self.doc_id = doc.id;
        self.editor = doc.editor;
        self.view = doc.view;
        self.active_layer = doc.active_layer;
        self.fit_pending = doc.fit_pending;
        self.guides = GuideState {
            list: doc.guides,
            ..GuideState::default()
        };
        self.artboard = doc.artboard;
        self.saved_extras = doc.saved_extras;
        self.path = doc.path;
        self.saved_revision = doc.saved_revision;
        self.cache = doc.cache;
        self.autosave_seen = doc.autosave_seen;
        self.autosave_at = doc.autosave_at;
        self.builder = None;
        self.eraser = None;
        self.selection.clear();
        self.gesture = None;
        self.ui.renaming = None;
    }

    fn next_tab_id(&mut self) -> u64 {
        let id = self.next_tab;
        self.next_tab += 1;
        id
    }

    /// Stores the live drawing in its tab so another can take its place. A drawing that saves by itself is
    /// saved first, so nothing is lost by leaving it.
    fn park_live(&mut self, ctx: &egui::Context) {
        self.flush_autosave(ctx);
        self.abandon_gesture();
        if let Some(i) = self.active.take() {
            let doc = self.take_doc();
            self.tabs[i].doc = Some(doc);
        }
    }

    /// Adds a tab for a drawing that is about to be put in the live fields, and makes it the active one.
    fn push_tab(&mut self, ctx: &egui::Context) {
        self.park_live(ctx);
        let id = self.next_tab_id();
        self.doc_id = id;
        self.tabs.push(Tab { id, doc: None });
        self.active = Some(self.tabs.len() - 1);
        self.ui.library_open = false;
        self.ui.close_popovers();
    }

    /// Opens a new tab with an empty drawing, in the folder the library is showing.
    pub(crate) fn new_tab(&mut self, ctx: &egui::Context) {
        self.push_tab(ctx);
        self.start_note();
    }

    /// Opens a new tab with a drawing read from `.ink` bytes (for example a file dropped on the window).
    ///
    /// # Errors
    /// The bytes are not a drawing; no tab is opened.
    pub(crate) fn open_bytes_in_tab(
        &mut self,
        ctx: &egui::Context,
        bytes: &[u8],
    ) -> Result<(), ink_format::FormatError> {
        let drawing = ink_format::read(bytes)?;
        self.push_tab(ctx);
        self.open_drawing(drawing);
        Ok(())
    }

    /// Opens the file at `path` in a tab. If a tab already has it, that tab comes to the front instead.
    pub(crate) fn open_in_tab(&mut self, ctx: &egui::Context, path: &Path) -> bool {
        if let Some(i) = self.tab_of(path) {
            self.switch_tab(ctx, i);
            return true;
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.notify(ctx, format!("Could not open {}: {e}", path.display()));
                return false;
            }
        };
        if let Err(e) = self.open_bytes_in_tab(ctx, &bytes) {
            self.notify(ctx, format!("Could not open {}: {e}", path.display()));
            return false;
        }
        self.path = Some(path.to_owned());
        let name = self.document_name();
        self.notify(ctx, format!("Opened {name}"));
        true
    }

    /// Brings tab `i` to the front and closes the library page.
    pub(crate) fn switch_tab(&mut self, ctx: &egui::Context, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.ui.library_open = false;
        self.ui.close_popovers();
        if self.active == Some(i) {
            return;
        }
        self.park_live(ctx);
        if let Some(doc) = self.tabs[i].doc.take() {
            self.put_doc(doc);
        }
        self.active = Some(i);
    }

    /// The next tab (`1`) or the one before (`-1`), wrapping around.
    pub(crate) fn cycle_tab(&mut self, ctx: &egui::Context, step: isize) {
        let count = self.tabs.len() as isize;
        if count == 0 {
            return;
        }
        let from = self.active.map_or(0, |a| a as isize);
        let to = (from + step).rem_euclid(count);
        self.switch_tab(ctx, to as usize);
    }

    /// Tab number `n` counting from 1; 9 always means the last one, as in a browser.
    pub(crate) fn goto_tab(&mut self, ctx: &egui::Context, n: usize) {
        let target = if n >= 9 {
            self.tabs.len().checked_sub(1)
        } else {
            n.checked_sub(1)
        };
        if let Some(i) = target.filter(|i| *i < self.tabs.len()) {
            self.switch_tab(ctx, i);
        }
    }

    /// What the strip shows for tab `i`.
    pub(crate) fn tab_label(&self, i: usize) -> TabLabel {
        match &self.tabs[i].doc {
            Some(doc) => TabLabel {
                name: files::drawing_name(doc.path.as_deref()),
                dirty: doc.editor.revision() != doc.saved_revision
                    || ink_format::Extras::of(doc.artboard, &doc.guides, doc.editor.marks())
                        != doc.saved_extras,
            },
            None => TabLabel {
                name: self.document_name(),
                dirty: self.is_dirty(),
            },
        }
    }

    /// The path of the drawing in tab `i`, if it has a file.
    fn tab_path(&self, i: usize) -> Option<&Path> {
        match &self.tabs[i].doc {
            Some(doc) => doc.path.as_deref(),
            None => self.path.as_deref(),
        }
    }

    /// The tab that has the file at `path` open.
    pub(crate) fn tab_of(&self, path: &Path) -> Option<usize> {
        (0..self.tabs.len()).find(|i| self.tab_path(*i) == Some(path))
    }

    /// `true` if the file at `path` is open in a tab.
    pub(crate) fn is_open(&self, path: &Path) -> bool {
        self.tab_of(path).is_some()
    }

    /// Closes tab `i`. A drawing with changes that would be lost needs the request twice; one that saves by
    /// itself is saved and closed at once.
    pub(crate) fn close_tab(&mut self, ctx: &egui::Context, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        if self.active == Some(i) {
            self.flush_autosave(ctx);
        }
        let label = self.tab_label(i);
        if label.dirty && self.config.files.confirm_discard {
            let revision = match &self.tabs[i].doc {
                Some(doc) => doc.editor.revision(),
                None => self.editor.revision(),
            };
            let key = (self.tabs[i].id, revision);
            if self.armed_close != Some(key) {
                self.armed_close = Some(key);
                self.notify(
                    ctx,
                    format!(
                        "\u{201c}{}\u{201d} has unsaved changes. Close it again to discard them, or save first (Ctrl+S).",
                        label.name
                    ),
                );
                return;
            }
        }
        self.armed_close = None;
        self.remove_tab(ctx, i, true);
    }

    /// Takes tab `i` away. `save` says whether the drawing may still be saved on its way out.
    fn remove_tab(&mut self, ctx: &egui::Context, i: usize, save: bool) {
        let was_live = self.active == Some(i);
        if was_live {
            if save {
                self.flush_autosave(ctx);
            }
            drop(self.take_doc());
            self.active = None;
        }
        self.tabs.remove(i);
        if let Some(a) = self.active
            && a > i
        {
            self.active = Some(a - 1);
        }
        if !was_live {
            return;
        }
        if self.tabs.is_empty() {
            // Nothing left open: the Library is all there is to show.
            self.new_document();
            self.ui.library_open = true;
            return;
        }
        let next = i.min(self.tabs.len() - 1);
        if let Some(doc) = self.tabs[next].doc.take() {
            self.put_doc(doc);
        }
        self.active = Some(next);
    }

    /// Closes every tab and starts over with nothing open, for when the app starts on the Library.
    pub(crate) fn clear_tabs(&mut self) {
        self.tabs.clear();
        self.active = None;
        self.new_document();
    }

    /// Moves tab `from` to position `to`, keeping the active tab active.
    pub(crate) fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = self.active.map(|a| {
            if a == from {
                to
            } else if from < a && a <= to {
                a - 1
            } else if to <= a && a < from {
                a + 1
            } else {
                a
            }
        });
    }

    /// Tells every open drawing that the file or folder at `from` is now at `to`.
    pub(crate) fn follow_move(&mut self, from: &Path, to: &Path) {
        if let Some(path) = self.path.as_deref().and_then(|p| rebase(p, from, to)) {
            self.path = Some(path);
        }
        for tab in &mut self.tabs {
            if let Some(doc) = &mut tab.doc
                && let Some(path) = doc.path.as_deref().and_then(|p| rebase(p, from, to))
            {
                doc.path = Some(path);
            }
        }
    }

    /// Closes the tabs whose files are at or under `path`, because they were deleted. Nothing is saved.
    pub(crate) fn close_tabs_under(&mut self, ctx: &egui::Context, path: &Path) {
        let doomed: Vec<usize> = (0..self.tabs.len())
            .filter(|i| self.tab_path(*i).is_some_and(|p| p.starts_with(path)))
            .collect();
        for i in doomed.into_iter().rev() {
            self.remove_tab(ctx, i, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::Context;
    use ink_core::{Color, Pen, Point, StrokeBuilder};

    use crate::Options;

    use super::*;

    fn app() -> InkApp {
        InkApp::blank(Options::default())
    }

    fn draw(app: &mut InkApp) {
        let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.1);
        for i in 0..10 {
            b.push(Point::new(i as f32 * 5.0, 0.0), None, f64::from(i) * 0.01);
        }
        let stroke = b.with_id(app.editor.next_id()).finish();
        app.editor.commit_stroke(app.active_layer, stroke);
    }

    fn strokes(app: &InkApp) -> usize {
        app.editor
            .document()
            .layers
            .iter()
            .map(|l| l.strokes.len())
            .sum()
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("inkwell-tabs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp folder");
        dir
    }

    #[test]
    fn every_tab_keeps_its_own_drawing_view_and_history() {
        let ctx = Context::default();
        let mut app = app();
        draw(&mut app);
        app.view.zoom = 2.0;
        app.new_tab(&ctx);
        assert_eq!((app.tabs.len(), app.active), (2, Some(1)));
        assert_eq!(strokes(&app), 0, "a new tab is empty");
        draw(&mut app);
        draw(&mut app);
        app.view.zoom = 0.5;

        app.switch_tab(&ctx, 0);
        assert_eq!(strokes(&app), 1);
        assert!((app.view.zoom - 2.0).abs() < 1e-6);
        assert!(app.editor.undo(), "its own history");
        assert!(!app.editor.can_undo());

        app.switch_tab(&ctx, 1);
        assert_eq!(strokes(&app), 2);
        assert!((app.view.zoom - 0.5).abs() < 1e-6);
        assert!(app.editor.can_undo());
    }

    #[test]
    fn labels_show_the_name_and_unsaved_changes_of_every_tab() {
        let ctx = Context::default();
        let mut app = app();
        draw(&mut app);
        app.new_tab(&ctx);
        assert_eq!(
            app.tab_label(0),
            TabLabel {
                name: "Untitled".to_owned(),
                dirty: true
            },
            "a parked tab keeps its mark"
        );
        assert!(!app.tab_label(1).dirty);
        draw(&mut app);
        assert!(app.tab_label(1).dirty);
    }

    #[test]
    fn closing_the_active_tab_brings_up_its_neighbor() {
        let ctx = Context::default();
        let mut app = app();
        app.config.files.confirm_discard = false;
        app.new_tab(&ctx);
        app.new_tab(&ctx);
        draw(&mut app);
        draw(&mut app);
        app.switch_tab(&ctx, 1);
        draw(&mut app);
        app.close_tab(&ctx, 1);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active, Some(1), "the tab that was to its right");
        assert_eq!(strokes(&app), 2);
        app.close_tab(&ctx, 1);
        assert_eq!(
            app.active,
            Some(0),
            "and with none to the right, the one to its left"
        );
    }

    #[test]
    fn closing_a_tab_before_the_active_one_keeps_the_active_one() {
        let ctx = Context::default();
        let mut app = app();
        app.config.files.confirm_discard = false;
        app.new_tab(&ctx);
        app.new_tab(&ctx);
        draw(&mut app);
        app.close_tab(&ctx, 0);
        assert_eq!(app.active, Some(1));
        assert_eq!(strokes(&app), 1);
    }

    #[test]
    fn closing_the_last_tab_leaves_only_the_library() {
        let ctx = Context::default();
        let mut app = app();
        app.config.files.confirm_discard = false;
        app.close_tab(&ctx, 0);
        assert!(app.tabs.is_empty());
        assert_eq!(app.active, None);
        assert!(app.ui.library_open);
    }

    #[test]
    fn unsaved_work_that_does_not_save_itself_needs_a_second_request_to_close() {
        let ctx = Context::default();
        let mut app = app();
        draw(&mut app);
        app.close_tab(&ctx, 0);
        assert_eq!(app.tabs.len(), 1, "the first request only warns");
        assert!(app.message.is_some());
        app.close_tab(&ctx, 0);
        assert!(app.tabs.is_empty());
    }

    #[test]
    fn a_drawing_that_saves_itself_closes_at_once_and_is_saved() {
        let dir = folder("autosave");
        let ctx = Context::default();
        let mut app = app();
        app.config.files.library_folder = Some(dir.display().to_string());
        app.start_note();
        let path = app.path.clone().expect("a place in the library");
        draw(&mut app);
        app.new_tab(&ctx);
        assert!(path.exists(), "parking saved it");
        app.close_tab(&ctx, 0);
        assert_eq!(app.tabs.len(), 1, "closed without a warning");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_a_file_that_is_already_open_switches_to_its_tab() {
        let dir = folder("reopen");
        let ctx = Context::default();
        let mut app = app();
        let file = dir.join("A.ink");
        InkApp::blank(Options::default())
            .write_file(&file)
            .expect("written");
        assert!(app.open_in_tab(&ctx, &file));
        assert_eq!(app.tabs.len(), 2);
        app.new_tab(&ctx);
        assert_eq!(app.tabs.len(), 3);
        assert!(app.open_in_tab(&ctx, &file));
        assert_eq!(app.tabs.len(), 3, "no second copy");
        assert_eq!(app.path.as_deref(), Some(file.as_path()));
        assert!(app.is_open(&file));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_cannot_be_read_opens_no_tab() {
        let dir = folder("unreadable");
        let ctx = Context::default();
        let mut app = app();
        let file = dir.join("Broken.ink");
        std::fs::write(&file, "garbage").expect("file");
        assert!(!app.open_in_tab(&ctx, &file));
        assert_eq!(app.tabs.len(), 1);
        assert!(app.message.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tabs_can_be_reordered_without_losing_track_of_the_active_one() {
        let ctx = Context::default();
        let mut app = app();
        for _ in 0..3 {
            app.new_tab(&ctx);
        }
        app.switch_tab(&ctx, 1);
        let active_id = app.tabs[1].id;
        app.move_tab(1, 3);
        assert_eq!(app.tabs[3].id, active_id);
        assert_eq!(app.active, Some(3));
        app.move_tab(0, 2);
        assert_eq!(app.tabs[app.active.expect("active")].id, active_id);
        assert_eq!(app.tabs.len(), 4);
    }

    #[test]
    fn keyboard_helpers_cycle_and_jump() {
        let ctx = Context::default();
        let mut app = app();
        for _ in 0..3 {
            app.new_tab(&ctx);
        }
        assert_eq!(app.active, Some(3));
        app.cycle_tab(&ctx, 1);
        assert_eq!(app.active, Some(0), "wraps around");
        app.cycle_tab(&ctx, -1);
        assert_eq!(app.active, Some(3));
        app.goto_tab(&ctx, 2);
        assert_eq!(app.active, Some(1));
        app.goto_tab(&ctx, 9);
        assert_eq!(app.active, Some(3), "9 is the last tab");
        app.goto_tab(&ctx, 7);
        assert_eq!(
            app.active,
            Some(3),
            "a tab that is not there changes nothing"
        );
    }

    #[test]
    fn moved_and_deleted_files_are_followed_by_their_tabs() {
        let dir = folder("follow");
        let ctx = Context::default();
        let mut app = app();
        let old = dir.join("Old");
        let file = old.join("A.ink");
        std::fs::create_dir_all(&old).expect("folder");
        InkApp::blank(Options::default())
            .write_file(&file)
            .expect("written");
        assert!(app.open_in_tab(&ctx, &file));
        app.new_tab(&ctx);
        let moved = dir.join("New");
        app.follow_move(&old, &moved);
        assert_eq!(app.tab_of(&moved.join("A.ink")), Some(1));
        app.close_tabs_under(&ctx, &moved);
        assert_eq!(app.tabs.len(), 2);
        assert!(!app.is_open(&moved.join("A.ink")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rebasing_only_touches_what_is_under_the_moved_path() {
        let from = Path::new("/a/b");
        let to = Path::new("/c");
        assert_eq!(
            rebase(Path::new("/a/b/x/y.ink"), from, to),
            Some(PathBuf::from("/c/x/y.ink"))
        );
        assert_eq!(rebase(from, from, to), Some(PathBuf::from("/c")));
        assert_eq!(rebase(Path::new("/a/bc/x"), from, to), None);
    }
}
