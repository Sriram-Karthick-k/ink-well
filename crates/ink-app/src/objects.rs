//! The objects a person can put on a drawing: the shapes the app provides, and the ones they have made themselves.
//!
//! The app's shapes are always the same. A person's own are made from strokes they selected (see
//! [`ink_core::shapes::Shape::from_strokes`]) and are kept in a file of their own beside the settings, so they are there
//! in every drawing and the next time the app starts.

use std::path::{Path, PathBuf};

use ink_core::Stroke;
use ink_core::shapes::{self, Shape};

/// What the objects are called in the file.
const FILE: &str = "objects.json";

/// The longest a name can be.
pub const NAME_LIMIT: usize = 40;

/// All the objects there are.
#[derive(Debug, Clone)]
pub struct Objects {
    built_in: Vec<Shape>,
    custom: Vec<Shape>,
    /// The person's objects changed since they were last written.
    changed: bool,
}

impl Default for Objects {
    fn default() -> Self {
        Self {
            built_in: shapes::built_in(),
            custom: Vec::new(),
            changed: false,
        }
    }
}

/// Where the objects are kept, beside the settings file at `config`.
#[must_use]
pub fn file_beside(config: &Path) -> PathBuf {
    config.with_file_name(FILE)
}

/// A name with its ends trimmed and cut to [`NAME_LIMIT`], or `None` when nothing is left.
fn tidy(name: &str) -> Option<String> {
    let name: String = name.trim().chars().take(NAME_LIMIT).collect();
    (!name.is_empty()).then_some(name)
}

impl Objects {
    /// The shapes the app provides, in the order they are listed.
    #[must_use]
    pub fn built_in(&self) -> &[Shape] {
        &self.built_in
    }

    /// The person's own objects, the oldest first.
    #[must_use]
    pub fn custom(&self) -> &[Shape] {
        &self.custom
    }

    /// The object with this id, whoever made it.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Shape> {
        self.built_in
            .iter()
            .chain(&self.custom)
            .find(|shape| shape.id == id)
    }

    /// `true` for an object the person made, which they can rename and delete.
    #[must_use]
    pub fn is_custom(&self, id: &str) -> bool {
        self.custom.iter().any(|shape| shape.id == id)
    }

    /// Reads the person's objects from `path`. A file that is not there is no objects; one that cannot be read leaves
    /// them as they were.
    ///
    /// # Errors
    /// The message says what went wrong.
    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let found: Vec<Shape> =
                    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
                self.custom = found
                    .into_iter()
                    .filter(|shape| {
                        !shape.paths.is_empty() && !self.built_in.iter().any(|b| b.id == shape.id)
                    })
                    .collect();
                self.changed = false;
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Writes the person's objects to `path` through a temporary file.
    ///
    /// # Errors
    /// The message says what went wrong.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        }
        let text = serde_json::to_string(&self.custom).map_err(|e| e.to_string())?;
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".tmp");
        let temporary = PathBuf::from(temporary);
        std::fs::write(&temporary, text).map_err(|e| format!("{}: {e}", temporary.display()))?;
        std::fs::rename(&temporary, path).map_err(|e| {
            let _ = std::fs::remove_file(&temporary);
            format!("{}: {e}", path.display())
        })
    }

    /// Whether the objects changed since the last call, so they are written once for a change.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Makes an object of `strokes` called `name`, and returns its id; `None` when there is nothing to make it of.
    pub fn add(&mut self, name: &str, strokes: &[&Stroke]) -> Option<String> {
        let name = tidy(name).unwrap_or_else(|| format!("Object {}", self.custom.len() + 1));
        let id = self.next_id();
        let shape = Shape::from_strokes(&id, &name, strokes)?;
        self.custom.push(shape);
        self.changed = true;
        Some(id)
    }

    fn next_id(&self) -> String {
        let used = self
            .custom
            .iter()
            .filter_map(|s| s.id.strip_prefix("mine-")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("mine-{}", used + 1)
    }

    /// Gives the object `id` the name `name`. Only the person's own can be renamed.
    pub fn rename(&mut self, id: &str, name: &str) -> bool {
        let Some(name) = tidy(name) else {
            return false;
        };
        match self.custom.iter_mut().find(|s| s.id == id) {
            Some(shape) if shape.name != name => {
                shape.name = name;
                self.changed = true;
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    /// Deletes the object `id`. Only the person's own can be deleted.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.custom.len();
        self.custom.retain(|s| s.id != id);
        let gone = self.custom.len() != before;
        self.changed |= gone;
        gone
    }
}

#[cfg(test)]
mod tests {
    use ink_core::{Color, IdGen, Pen, Point, StrokeBuilder};

    use super::*;

    fn stroke(ids: &mut IdGen, to: (f32, f32)) -> Stroke {
        let mut builder = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.0);
        builder.replace_with(vec![Point::new(0.0, 0.0), Point::new(to.0, to.1)]);
        builder.with_id(ids.next_id()).finish()
    }

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("inkwell-objects-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn it_starts_with_the_apps_shapes_and_none_of_the_persons() {
        let objects = Objects::default();
        assert!(objects.built_in().len() >= 20);
        assert_eq!(objects.custom().len(), 0);
        assert!(objects.get("circle").is_some());
        assert!(objects.get("nothing").is_none());
    }

    #[test]
    fn an_object_made_of_strokes_is_found_by_its_id_and_is_the_persons() {
        let mut ids = IdGen::new(1);
        let a = stroke(&mut ids, (100.0, 50.0));
        let mut objects = Objects::default();
        let id = objects.add("My line", &[&a]).expect("made");
        assert_eq!(objects.get(&id).map(|s| s.name.as_str()), Some("My line"));
        assert!(objects.is_custom(&id));
        assert!(!objects.is_custom("circle"));
    }

    #[test]
    fn two_objects_never_share_an_id_even_after_one_is_deleted() {
        let mut ids = IdGen::new(1);
        let a = stroke(&mut ids, (100.0, 50.0));
        let mut objects = Objects::default();
        let first = objects.add("A", &[&a]).expect("made");
        let second = objects.add("B", &[&a]).expect("made");
        assert_ne!(first, second);
        assert!(objects.remove(&first));
        let third = objects.add("C", &[&a]).expect("made");
        assert_ne!(third, second);
    }

    #[test]
    fn a_name_is_trimmed_cut_and_never_empty() {
        let mut ids = IdGen::new(1);
        let a = stroke(&mut ids, (10.0, 10.0));
        let mut objects = Objects::default();
        let id = objects.add("   ", &[&a]).expect("made");
        assert_eq!(objects.get(&id).map(|s| s.name.as_str()), Some("Object 1"));
        assert!(!objects.rename(&id, "  "), "a blank name is refused");
        assert!(objects.rename(&id, &format!("  {}  ", "x".repeat(100))));
        assert_eq!(
            objects.get(&id).map(|s| s.name.chars().count()),
            Some(NAME_LIMIT)
        );
    }

    #[test]
    fn the_apps_shapes_can_be_neither_renamed_nor_deleted() {
        let mut objects = Objects::default();
        assert!(!objects.rename("circle", "Ball"));
        assert!(!objects.remove("circle"));
        assert!(objects.get("circle").is_some());
        assert!(!objects.take_changed());
    }

    #[test]
    fn nothing_to_make_an_object_of_makes_none() {
        let mut objects = Objects::default();
        assert!(objects.add("Empty", &[]).is_none());
        assert_eq!(objects.custom().len(), 0);
        assert!(!objects.take_changed());
    }

    #[test]
    fn a_change_is_reported_once() {
        let mut ids = IdGen::new(1);
        let a = stroke(&mut ids, (10.0, 10.0));
        let mut objects = Objects::default();
        objects.add("A", &[&a]);
        assert!(objects.take_changed());
        assert!(!objects.take_changed());
    }

    #[test]
    fn the_persons_objects_are_there_again_after_they_are_written_and_read() {
        let dir = folder("roundtrip");
        let path = file_beside(&dir.join("config.json"));
        let mut ids = IdGen::new(1);
        let a = stroke(&mut ids, (100.0, 50.0));
        let mut objects = Objects::default();
        let id = objects.add("Kept", &[&a]).expect("made");
        objects.save(&path).expect("written");
        let mut again = Objects::default();
        again.load(&path).expect("read");
        assert_eq!(again.get(&id), objects.get(&id));
        assert!(!again.take_changed(), "reading is not a change");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_there_is_no_objects_and_one_that_is_broken_is_an_error() {
        let dir = folder("missing");
        let mut objects = Objects::default();
        assert!(objects.load(&dir.join("objects.json")).is_ok());
        std::fs::create_dir_all(&dir).expect("folder");
        std::fs::write(dir.join("objects.json"), "{ not json").expect("written");
        assert!(objects.load(&dir.join("objects.json")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stored_object_cannot_take_the_id_of_one_of_the_apps_shapes() {
        let dir = folder("clash");
        std::fs::create_dir_all(&dir).expect("folder");
        let mut ids = IdGen::new(1);
        let a = stroke(&mut ids, (100.0, 50.0));
        let imposter = Shape::from_strokes("circle", "Not a circle", &[&a]).expect("made");
        let text = serde_json::to_string(&vec![imposter]).expect("written");
        std::fs::write(dir.join("objects.json"), text).expect("written");
        let mut objects = Objects::default();
        objects.load(&dir.join("objects.json")).expect("read");
        assert_eq!(objects.custom().len(), 0);
        assert_eq!(
            objects.get("circle").map(|s| s.name.as_str()),
            Some("Circle")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
