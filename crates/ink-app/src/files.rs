//! Opening and saving documents, and the unsaved-changes bookkeeping.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui;
use ink_format::WriteOptions;

use crate::app::InkApp;
use crate::library;
use ink_format::Extras;

/// How long after the last change a note saves itself.
const AUTOSAVE_SECONDS: f64 = 1.5;
/// How long a status message stays on screen.
pub(crate) const MESSAGE_SECONDS: f64 = 6.0;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// The name of a drawing: its file name without the extension, or "Untitled" if it has no file yet.
pub(crate) fn drawing_name(path: Option<&Path>) -> String {
    path.and_then(Path::file_stem)
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_owned()
}

impl InkApp {
    /// `true` when the document differs from what was last saved or opened.
    pub(crate) fn is_dirty(&self) -> bool {
        self.editor.revision() != self.saved_revision || self.extras() != self.saved_extras
    }

    /// The artboard, guides, locks and groups the drawing has now.
    pub(crate) fn extras(&self) -> Extras {
        Extras::of(self.artboard, &self.guides.list, self.editor.marks())
    }

    /// Puts what was kept beside a drawing on the drawing.
    pub(crate) fn take_extras(&mut self, extras: &Extras) {
        self.artboard = extras.artboard;
        self.guides.list.clone_from(&extras.guides);
        let marks = extras.marks();
        self.editor.edit_marks(|m| *m = marks);
    }

    pub(crate) fn document_name(&self) -> String {
        drawing_name(self.path.as_deref())
    }

    /// The window title, with a dot after the name while there are unsaved changes.
    pub(crate) fn title(&self) -> String {
        if self.ui.library_open {
            return "Library \u{2014} inkwell".to_owned();
        }
        let mark = if self.is_dirty() { " \u{2022}" } else { "" };
        format!("{}{mark} \u{2014} inkwell", self.document_name())
    }

    /// Shows `text` in the top bar for a few seconds.
    pub(crate) fn notify(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        self.message = Some((text.into(), ctx.input(|i| i.time)));
    }

    /// The status message to show at time `now`, unless it has expired.
    pub(crate) fn current_message(&self, now: f64) -> Option<&str> {
        self.message
            .as_ref()
            .filter(|(_, shown)| now - shown < MESSAGE_SECONDS)
            .map(|(text, _)| text.as_str())
    }

    /// Starts a fresh note. With autosave it gets its place in the notes folder at once, but nothing is written
    /// until something is drawn, so an empty note leaves no file behind.
    pub(crate) fn start_note(&mut self) {
        self.new_document();
        if self.config.files.autosave {
            self.path = Some(library::new_note_path(&self.drawing_folder()));
        }
    }

    /// The folder a new drawing goes in: the one the Library is showing, or else the top of the library.
    pub(crate) fn drawing_folder(&self) -> PathBuf {
        let root = self.library_folder();
        let shown = &self.library.current;
        if !shown.as_os_str().is_empty() && shown.starts_with(&root) {
            shown.clone()
        } else {
            root
        }
    }

    /// The folder drawings are kept in, with the folders inside it.
    pub(crate) fn library_folder(&self) -> PathBuf {
        self.config.library_folder()
    }

    /// `true` when the open drawing is a note in the notes folder and saving by itself is on.
    pub(crate) fn autosaves(&self) -> bool {
        self.config.files.autosave
            && self
                .path
                .as_deref()
                .is_some_and(|path| path.starts_with(self.library_folder()))
    }

    /// Saves the open note now if it saves by itself and has unsaved changes. Only a failure is announced.
    pub(crate) fn flush_autosave(&mut self, ctx: &egui::Context) {
        if !self.is_dirty() || !self.autosaves() {
            return;
        }
        if let Some(path) = self.path.clone()
            && let Err(e) = self.write_file(&path)
        {
            self.notify(ctx, format!("Could not save {}: {e}", path.display()));
        }
    }

    /// Saves a note a moment after the last change, but never while a pen or button is down.
    pub(crate) fn autosave_tick(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let revision = self.editor.revision();
        // A change to what is kept beside the drawing counts as a change, so it is saved a moment after too.
        let extras_changed = self.extras() != self.autosave_extras;
        if revision != self.autosave_seen || extras_changed {
            self.autosave_seen = revision;
            self.autosave_extras = self.extras();
            self.autosave_at = now;
        }
        if !self.is_dirty() || !self.autosaves() {
            return;
        }
        if now - self.autosave_at >= AUTOSAVE_SECONDS && !ctx.input(|i| i.pointer.any_down()) {
            self.flush_autosave(ctx);
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(400));
        }
    }

    /// Opens the notes page.
    pub(crate) fn open_library(&mut self, ctx: &egui::Context) {
        self.flush_autosave(ctx);
        self.ui.close_popovers();
        self.ui.library_open = true;
        self.refresh_library(ctx);
    }

    /// Leaves the notes page for the note that is open.
    pub(crate) fn close_library(&mut self) {
        self.ui.library_open = false;
        self.library.menu = None;
        self.library.move_picker = None;
        self.library.renaming = None;
        self.library.drag = None;
    }

    /// Reads the notes folder again in the background.
    pub(crate) fn refresh_library(&mut self, ctx: &egui::Context) {
        let ctx = ctx.clone();
        let folder = self.library_folder();
        self.library.refresh(folder, move || ctx.request_repaint());
    }

    /// Opens a drawing from the library in a tab. If it is already open, its tab comes to the front.
    pub(crate) fn open_note(&mut self, ctx: &egui::Context, path: &Path) {
        self.open_in_tab(ctx, path);
    }

    /// Copies chosen files into the notes folder, leaving the originals as they are.
    #[cfg(not(target_os = "android"))]
    pub(crate) fn import_notes(&mut self, ctx: &egui::Context) {
        let Some(files) = rfd::FileDialog::new()
            .add_filter("inkwell drawing", &["ink"])
            .pick_files()
        else {
            return;
        };
        let results = library::import_notes(&files, &self.drawing_folder());
        let imported = results.iter().filter(|r| r.is_ok()).count();
        if let Some(Err(e)) = results.iter().find(|r| r.is_err()) {
            self.notify(ctx, e.clone());
        } else {
            let plural = if imported == 1 { "" } else { "s" };
            self.notify(ctx, format!("Imported {imported} note{plural}"));
        }
        self.refresh_library(ctx);
    }

    #[cfg(target_os = "android")]
    pub(crate) fn import_notes(&mut self, ctx: &egui::Context) {
        self.notify(ctx, "Importing files is not available on Android yet.");
    }

    /// Asks which file to open.
    #[cfg(not(target_os = "android"))]
    pub(crate) fn request_open(&mut self, ctx: &egui::Context) {
        let picked = rfd::FileDialog::new()
            .add_filter("inkwell drawing", &["ink"])
            .pick_file();
        if let Some(path) = picked {
            self.open_in_tab(ctx, &path);
        }
    }

    #[cfg(target_os = "android")]
    pub(crate) fn request_open(&mut self, ctx: &egui::Context) {
        self.notify(ctx, "Opening files is not available on Android yet.");
    }

    /// Saves to the current file, or asks where to save a drawing that has none.
    pub(crate) fn save(&mut self, ctx: &egui::Context) {
        match self.path.clone() {
            Some(path) => self.save_to(ctx, &path),
            None => self.save_as(ctx),
        }
    }

    /// Asks where to save.
    #[cfg(not(target_os = "android"))]
    pub(crate) fn save_as(&mut self, ctx: &egui::Context) {
        let picked = rfd::FileDialog::new()
            .add_filter("inkwell drawing", &["ink"])
            .set_file_name(format!("{}.ink", self.document_name()))
            .save_file();
        if let Some(mut path) = picked {
            if path.extension().is_none() {
                path.set_extension("ink");
            }
            self.save_to(ctx, &path);
        }
    }

    #[cfg(target_os = "android")]
    pub(crate) fn save_as(&mut self, ctx: &egui::Context) {
        self.notify(ctx, "Saving files is not available on Android yet.");
    }

    fn save_to(&mut self, ctx: &egui::Context, path: &Path) {
        match self.write_file(path) {
            Ok(()) => {
                let name = self.document_name();
                self.notify(ctx, format!("Saved {name}"));
            }
            Err(e) => self.notify(ctx, format!("Could not save {}: {e}", path.display())),
        }
    }

    /// Writes the drawing to `path` through a temporary file, so a failure cannot damage an existing file.
    pub(crate) fn write_file(&mut self, path: &Path) -> Result<(), String> {
        if let Some(folder) = path.parent().filter(|f| !f.as_os_str().is_empty()) {
            std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        }
        let extras = self.extras();
        let bytes = ink_format::write(
            self.editor.document(),
            &extras,
            &WriteOptions {
                now_unix_ms: now_ms(),
            },
        )
        .map_err(|e| e.to_string())?;
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".tmp");
        let temporary = PathBuf::from(temporary);
        std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, path).map_err(|e| {
            let _ = std::fs::remove_file(&temporary);
            e.to_string()
        })?;
        self.path = Some(path.to_owned());
        self.saved_revision = self.editor.revision();
        self.saved_extras = extras;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ink_core::{Color, Pen, Point, StrokeBuilder};

    use crate::Options;

    use super::*;

    fn app() -> InkApp {
        InkApp::blank(Options::default())
    }

    fn draw_something(app: &mut InkApp) {
        let mut builder = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.5);
        for i in 0..20 {
            builder.push(
                Point::new(i as f32 * 5.0, (i as f32).sin() * 10.0),
                None,
                f64::from(i) * 0.01,
            );
        }
        let stroke = builder.with_id(app.editor.next_id()).finish();
        app.editor.commit_stroke(app.active_layer, stroke);
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("inkwell-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir.join(name)
    }

    #[test]
    fn a_fresh_drawing_is_clean_and_an_edit_makes_it_dirty() {
        let mut app = app();
        assert!(!app.is_dirty());
        draw_something(&mut app);
        assert!(app.is_dirty());
        assert!(app.title().contains('\u{2022}'));
    }

    #[test]
    fn saving_then_opening_restores_the_drawing_and_clears_the_dirty_mark() {
        let mut app = app();
        draw_something(&mut app);
        let path = temp_path("roundtrip.ink");
        app.write_file(&path).expect("saved");
        assert!(!app.is_dirty());
        assert!(!app.title().contains('\u{2022}'));
        assert_eq!(app.document_name(), "roundtrip");

        let mut reopened = InkApp::blank(Options::default());
        let bytes = std::fs::read(&path).expect("file exists");
        reopened.open_bytes(&bytes).expect("readable");
        let strokes: usize = reopened
            .editor
            .document()
            .layers
            .iter()
            .map(|l| l.strokes.len())
            .sum();
        assert_eq!(strokes, 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn saving_leaves_no_temporary_file_behind() {
        let mut app = app();
        let path = temp_path("clean.ink");
        app.write_file(&path).expect("saved");
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".tmp");
        assert!(!Path::new(&temporary).exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn status_messages_expire() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.notify(&ctx, "hello");
        assert_eq!(app.current_message(0.0), Some("hello"));
        assert_eq!(app.current_message(MESSAGE_SECONDS + 1.0), None);
    }
}
