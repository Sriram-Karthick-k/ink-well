//! What can be done with the drawings and folders of the Library: make a folder, rename, duplicate, move, delete and bring
//! back. The desktop's page and the touch interface's page are two ways of asking for these, and both ask here, so a
//! drawing is renamed, moved and deleted the same way whichever page was used.

use std::path::{Path, PathBuf};

use eframe::egui::Context;

use crate::app::InkApp;
use crate::library;

/// The name of a drawing (without its extension) or folder.
#[must_use]
pub fn name_of(path: &Path) -> String {
    let name = if path.is_dir() {
        path.file_name()
    } else {
        path.file_stem()
    };
    name.and_then(|n| n.to_str())
        .unwrap_or("Untitled")
        .to_owned()
}

impl InkApp {
    /// Makes a folder in the one shown and asks for its name right away.
    pub(crate) fn library_new_folder(&mut self, ctx: &Context) {
        match library::create_folder(&self.library.current, "") {
            Ok(path) => {
                let name = name_of(&path);
                self.refresh_library(ctx);
                self.library.renaming = Some((path, name));
            }
            Err(e) => self.notify(ctx, e),
        }
    }

    /// Asks for a new name for a drawing or folder.
    pub(crate) fn library_ask_rename(&mut self, path: PathBuf) {
        let name = name_of(&path);
        self.library.menu = None;
        self.library.renaming = Some((path, name));
    }

    /// Renames a drawing or folder. A name that cannot be used keeps the question open with what was typed.
    pub(crate) fn library_rename(&mut self, ctx: &Context, path: &Path, name: String) {
        self.flush_autosave(ctx);
        match library::rename_item(path, &name) {
            Ok(new_path) => {
                self.follow_move(path, &new_path);
                self.notify(ctx, "Renamed");
                self.refresh_library(ctx);
            }
            Err(e) => {
                self.library.renaming = Some((path.to_owned(), name));
                self.notify(ctx, e);
            }
        }
    }

    /// Makes a copy of a drawing next to it.
    pub(crate) fn library_duplicate(&mut self, ctx: &Context, path: &Path) {
        self.library.menu = None;
        self.flush_autosave(ctx);
        match library::duplicate_note(path) {
            Ok(_) => self.notify(ctx, "Duplicated"),
            Err(e) => self.notify(ctx, e),
        }
        self.refresh_library(ctx);
    }

    /// Moves a drawing or folder into another folder.
    pub(crate) fn library_move_into(&mut self, ctx: &Context, path: &Path, target: &Path) {
        self.library.move_picker = None;
        self.flush_autosave(ctx);
        match library::move_item(path, target) {
            Ok(new_path) => {
                if new_path != path {
                    self.follow_move(path, &new_path);
                    let place = if target == self.library.root {
                        "Library".to_owned()
                    } else {
                        name_of(target)
                    };
                    self.notify(ctx, format!("Moved to \u{201c}{place}\u{201d}"));
                }
                self.refresh_library(ctx);
            }
            Err(e) => self.notify(ctx, e),
        }
    }

    /// Puts a drawing or folder in the bin, where it can be brought back for a while.
    pub(crate) fn library_delete(&mut self, ctx: &Context, path: &Path) {
        self.library.menu = None;
        let now = ctx.input(|i| i.time);
        let root = self.library.root.clone();
        match library::delete_item(path, &root, now) {
            Ok(deleted) => {
                // Drawings that were open in tabs go with their files.
                self.close_tabs_under(ctx, path);
                self.library.deleted = Some(deleted);
            }
            Err(e) => self.notify(ctx, e),
        }
        self.refresh_library(ctx);
    }

    /// Brings back what was deleted last.
    pub(crate) fn library_undo_delete(&mut self, ctx: &Context) {
        if let Some(deleted) = self.library.deleted.take() {
            match library::restore_item(&deleted) {
                Ok(_) => self.notify(
                    ctx,
                    format!("Brought back \u{201c}{}\u{201d}", deleted.name),
                ),
                Err(e) => self.notify(ctx, e),
            }
            self.refresh_library(ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;

    fn temp_folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("inkwell-ops-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("folder");
        dir
    }

    /// An app whose Library is the folder `root`, with one drawing named `Plan` in it.
    fn app_with_plan(name: &str) -> (InkApp, PathBuf, Context) {
        let root = temp_folder(name);
        let mut app = InkApp::blank(Options::default());
        app.config.files.library_folder = Some(root.display().to_string());
        app.library.root = root.clone();
        app.library.current = root.clone();
        let bytes = InkApp::blank(Options::default()).encode().expect("bytes");
        std::fs::write(root.join("Plan.ink"), bytes).expect("drawing");
        (app, root, Context::default())
    }

    #[test]
    fn the_name_of_a_drawing_has_no_extension_and_a_folder_keeps_its_dots() {
        assert_eq!(name_of(Path::new("a/Plan.ink")), "Plan");
        let root = temp_folder("names");
        let folder = root.join("v1.2");
        std::fs::create_dir_all(&folder).expect("folder");
        assert_eq!(name_of(&folder), "v1.2");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_folder_is_made_where_the_page_is_and_is_asked_a_name() {
        let (mut app, root, ctx) = app_with_plan("ops-folder");
        app.library_new_folder(&ctx);
        let (path, _) = app.library.renaming.clone().expect("asked a name");
        assert!(path.is_dir() && path.starts_with(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renaming_moves_the_file_and_a_bad_name_keeps_the_question_open() {
        let (mut app, root, ctx) = app_with_plan("ops-rename");
        let plan = root.join("Plan.ink");
        app.library_rename(&ctx, &plan, "Sketch".to_owned());
        assert!(root.join("Sketch.ink").exists() && !plan.exists());
        let sketch = root.join("Sketch.ink");
        app.library_rename(&ctx, &sketch, "   ".to_owned());
        assert!(sketch.exists(), "an empty name changes nothing");
        assert_eq!(
            app.library.renaming.as_ref().map(|(p, _)| p.clone()),
            Some(sketch),
            "and the question stays open"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicating_makes_a_second_file_and_moving_puts_one_in_a_folder() {
        let (mut app, root, ctx) = app_with_plan("ops-copy-move");
        let plan = root.join("Plan.ink");
        app.library_duplicate(&ctx, &plan);
        assert_eq!(
            std::fs::read_dir(&root)
                .expect("dir")
                .filter(|e| e.as_ref().is_ok_and(|e| e.path().extension().is_some()))
                .count(),
            2
        );
        let folder = root.join("Work");
        std::fs::create_dir_all(&folder).expect("folder");
        app.library_move_into(&ctx, &plan, &folder);
        assert!(folder.join("Plan.ink").exists() && !plan.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_deleted_drawing_goes_away_and_comes_back() {
        let (mut app, root, ctx) = app_with_plan("ops-delete");
        let plan = root.join("Plan.ink");
        app.library_delete(&ctx, &plan);
        assert!(!plan.exists());
        assert!(app.library.deleted.is_some(), "it can be brought back");
        app.library_undo_delete(&ctx);
        assert!(plan.exists());
        assert!(app.library.deleted.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
