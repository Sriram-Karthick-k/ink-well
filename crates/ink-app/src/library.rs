//! The library: the drawings and folders on disk, what they look like, and the file operations on them.
//!
//! Drawings are `.ink` files, kept in a library folder and in folders inside it, as deep as the user likes.
//! The folders are read on a background thread, one drawing at a time, so the page fills in while it is open and
//! never freezes the window; a drawing whose file has not changed since the last scan is not read again.
//! Deleting moves a drawing or folder into a hidden `.deleted` folder so it can be brought back.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{SystemTime, UNIX_EPOCH};

use ink_core::Document;

use crate::color_panel::parse_hex;

/// The file extension of a note.
pub const EXTENSION: &str = ink_format::EXTENSION;
/// Where deleted drawings and folders wait, inside the library folder.
const DELETED_FOLDER: &str = ".deleted";
/// A preview never carries more strokes or points than this, so a huge drawing stays cheap to show.
const MAX_PREVIEW_STROKES: usize = 3000;
const MAX_PREVIEW_POINTS: usize = 48;
/// A region keeps its corners and curves: points are only dropped that are within this many canvas units of the line
/// between those kept, and then no more than this many are kept.
const REGION_TOLERANCE: f32 = 0.3;
const MAX_REGION_POINTS: usize = 400;

/// One stroke of a preview, in canvas units with y pointing up.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewStroke {
    pub color: [u8; 4],
    pub width: f32,
    pub points: Vec<[f32; 2]>,
    /// For a region (a shape's fill, the letters of some words): the triangles of its inside, `x, y` pairs in canvas units,
    /// three pairs to a triangle. Empty for a line.
    pub fill: Vec<f32>,
}

/// A small picture of a note: its paper and a thinned-out copy of its strokes.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub paper: [u8; 3],
    /// Left, bottom, right and top of everything drawn, in canvas units.
    pub bounds: [f32; 4],
    pub strokes: Vec<PreviewStroke>,
}

impl Default for Preview {
    fn default() -> Self {
        Self {
            paper: [0xF9, 0xF9, 0xF9],
            bounds: [0.0, 0.0, 1.0, 1.0],
            strokes: Vec::new(),
        }
    }
}

/// Builds the preview of `document`: visible layers only, strokes and points thinned to a fixed budget.
#[must_use]
pub fn preview_of(document: &Document) -> Preview {
    let paper =
        parse_hex(&document.metadata.background_solid_color).unwrap_or(Preview::default().paper);
    let total: usize = document
        .layers
        .iter()
        .filter(|l| l.content && l.visible)
        .map(|l| l.strokes.len())
        .sum();
    let stroke_step = total.div_ceil(MAX_PREVIEW_STROKES).max(1);
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let mut strokes = Vec::new();
    for stroke in document
        .layers
        .iter()
        .filter(|l| l.content && l.visible)
        .flat_map(|l| l.strokes.iter())
        .step_by(stroke_step)
    {
        let all = stroke.geometry();
        if all.is_empty() {
            continue;
        }
        let region = stroke.pen == ink_core::Pen::Fill && all.len() >= 3;
        let mut points: Vec<[f32; 2]> = if region {
            // A shape's fill and the letters of words are drawn as what they are, with their corners: thinning them evenly
            // would round the star off and turn the letters to mush.
            let whole: Vec<[f32; 2]> = all.iter().map(|p| [p.x, p.y]).collect();
            let mut kept = ink_core::shapes::simplify(&whole, REGION_TOLERANCE);
            let step = kept.len().div_ceil(MAX_REGION_POINTS).max(1);
            if step > 1 {
                kept = kept.into_iter().step_by(step).collect();
            }
            kept
        } else {
            let step = all.len().div_ceil(MAX_PREVIEW_POINTS).max(1);
            let mut thinned: Vec<[f32; 2]> = all.iter().step_by(step).map(|p| [p.x, p.y]).collect();
            if let Some(last) = all.last()
                && thinned.last() != Some(&[last.x, last.y])
            {
                thinned.push([last.x, last.y]);
            }
            thinned
        };
        let fill = if region {
            let polygon: Vec<ink_core::Point> = points
                .iter()
                .map(|p| ink_core::Point::new(p[0], p[1]))
                .collect();
            ink_brush::fill::triangulate(&polygon)
        } else {
            Vec::new()
        };
        if region && fill.is_empty() {
            // Nothing inside (all on a line): it is a line, as it was before regions.
            points.dedup();
        }
        let width = stroke.width.unwrap_or(2.0);
        for [x, y] in &points {
            x0 = x0.min(x - width / 2.0);
            y0 = y0.min(y - width / 2.0);
            x1 = x1.max(x + width / 2.0);
            y1 = y1.max(y + width / 2.0);
        }
        let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        strokes.push(PreviewStroke {
            color: [
                channel(stroke.color.r),
                channel(stroke.color.g),
                channel(stroke.color.b),
                channel(stroke.color.a),
            ],
            width,
            points,
            fill,
        });
    }
    let bounds = if x0 <= x1 && y0 <= y1 {
        [x0, y0, x1, y1]
    } else {
        Preview::default().bounds
    };
    Preview {
        paper,
        bounds,
        strokes,
    }
}

/// A note in the library.
#[derive(Debug, Clone)]
pub struct Note {
    pub path: PathBuf,
    /// The file name without its extension.
    pub name: String,
    pub modified: SystemTime,
    pub preview: Arc<Preview>,
    pub strokes: usize,
    /// The file could not be read as a drawing; it is still listed so it can be renamed or deleted.
    pub unreadable: bool,
}

fn load_note(path: &Path, modified: SystemTime) -> Note {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_owned();
    let document = std::fs::read(path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| ink_format::read(&bytes).map_err(|e| e.to_string()))
        .map(|drawing| drawing.document);
    match document {
        Ok(document) => Note {
            path: path.to_owned(),
            name,
            modified,
            strokes: document
                .layers
                .iter()
                .filter(|l| l.content)
                .map(|l| l.strokes.len())
                .sum(),
            preview: Arc::new(preview_of(&document)),
            unreadable: false,
        },
        Err(_) => Note {
            path: path.to_owned(),
            name,
            modified,
            strokes: 0,
            preview: Arc::new(Preview::default()),
            unreadable: true,
        },
    }
}

enum Message {
    Folder(PathBuf),
    Loaded(Note),
    Unchanged(PathBuf),
    Done,
}

/// How the library is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Recent,
    Name,
}

/// A drawing or folder that was deleted and can still be brought back.
#[derive(Debug, Clone)]
pub struct Deleted {
    pub name: String,
    original: PathBuf,
    moved_to: PathBuf,
    is_dir: bool,
    /// When it was deleted, in egui time, for the undo message to expire.
    pub at: f64,
}

/// A folder as the page shows it: its name, how much is inside, and a few drawings to peek at.
#[derive(Debug, Clone)]
pub struct FolderCard {
    pub path: PathBuf,
    pub name: String,
    /// Every drawing inside, in folders below this one as well.
    pub drawings: usize,
    /// The newest drawings inside, for the pictures on the folder.
    pub previews: Vec<Arc<Preview>>,
}

/// The menu of a drawing or folder, and where it hangs on the screen, in points.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemMenu {
    pub path: PathBuf,
    pub at: [f32; 2],
}

/// The list of folders an item can be moved to.
#[derive(Debug, Clone, PartialEq)]
pub struct MovePicker {
    pub path: PathBuf,
    pub at: [f32; 2],
}

/// The library page's state: what is in the library folder and what the page is showing of it.
pub struct Library {
    /// The folder everything lives in.
    pub root: PathBuf,
    /// The folder the page is showing; the root or a folder below it.
    pub current: PathBuf,
    notes: Vec<Note>,
    folders: Vec<PathBuf>,
    receiver: Option<Receiver<Message>>,
    seen: Vec<PathBuf>,
    seen_folders: Vec<PathBuf>,
    pub query: String,
    pub sort: Sort,
    pub menu: Option<ItemMenu>,
    pub move_picker: Option<MovePicker>,
    /// The drawing or folder being renamed and the name typed so far.
    pub renaming: Option<(PathBuf, String)>,
    pub deleted: Option<Deleted>,
    /// The drawing or folder being dragged to another folder.
    pub drag: Option<PathBuf>,
}

impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Library")
            .field("root", &self.root)
            .field("current", &self.current)
            .field("drawings", &self.notes.len())
            .field("folders", &self.folders.len())
            .finish_non_exhaustive()
    }
}

impl Default for Library {
    fn default() -> Self {
        Self {
            root: PathBuf::new(),
            current: PathBuf::new(),
            notes: Vec::new(),
            folders: Vec::new(),
            receiver: None,
            seen: Vec::new(),
            seen_folders: Vec::new(),
            query: String::new(),
            sort: Sort::Recent,
            menu: None,
            move_picker: None,
            renaming: None,
            deleted: None,
            drag: None,
        }
    }
}

/// `true` for a `.ink` file.
fn is_drawing(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// `true` for a name that is hidden, such as the `.deleted` folder.
fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

impl Library {
    /// Starts reading `root` and everything below it in the background. `repaint` is called whenever
    /// something new has arrived.
    pub fn refresh(&mut self, root: PathBuf, repaint: impl Fn() + Send + 'static) {
        if self.root != root {
            self.notes.clear();
            self.folders.clear();
            self.root.clone_from(&root);
            self.current.clone_from(&root);
        }
        if !self.current.starts_with(&root) {
            self.current.clone_from(&root);
        }
        self.seen.clear();
        self.seen_folders.clear();
        let known: HashMap<PathBuf, SystemTime> = self
            .notes
            .iter()
            .map(|n| (n.path.clone(), n.modified))
            .collect();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        std::thread::spawn(move || {
            let mut files: Vec<(PathBuf, SystemTime)> = Vec::new();
            let mut folders: Vec<PathBuf> = Vec::new();
            let mut pending = vec![root];
            while let Some(dir) = pending.pop() {
                for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                    let path = entry.path();
                    let Ok(meta) = entry.metadata() else {
                        continue;
                    };
                    if meta.is_dir() {
                        if !is_hidden(&entry.file_name()) {
                            folders.push(path.clone());
                            pending.push(path);
                        }
                    } else if meta.is_file()
                        && is_drawing(&path)
                        && let Ok(modified) = meta.modified()
                    {
                        files.push((path, modified));
                    }
                }
            }
            for folder in folders {
                if sender.send(Message::Folder(folder)).is_err() {
                    return;
                }
            }
            files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
            for (path, modified) in files {
                let message = if known.get(&path) == Some(&modified) {
                    Message::Unchanged(path)
                } else {
                    Message::Loaded(load_note(&path, modified))
                };
                if sender.send(message).is_err() {
                    return;
                }
                repaint();
            }
            let _ = sender.send(Message::Done);
            repaint();
        });
    }

    /// Takes in what the background read has found. Returns `true` if what is shown may have changed.
    pub fn poll(&mut self) -> bool {
        let Some(receiver) = &self.receiver else {
            return false;
        };
        let mut changed = false;
        while let Ok(message) = receiver.try_recv() {
            match message {
                Message::Folder(path) => {
                    self.seen_folders.push(path.clone());
                    if !self.folders.contains(&path) {
                        self.folders.push(path);
                        changed = true;
                    }
                }
                Message::Loaded(note) => {
                    self.seen.push(note.path.clone());
                    match self.notes.iter_mut().find(|n| n.path == note.path) {
                        Some(existing) => *existing = note,
                        None => self.notes.push(note),
                    }
                    changed = true;
                }
                Message::Unchanged(path) => self.seen.push(path),
                Message::Done => {
                    let (notes, folders) = (self.notes.len(), self.folders.len());
                    let seen = std::mem::take(&mut self.seen);
                    let seen_folders = std::mem::take(&mut self.seen_folders);
                    self.notes.retain(|n| seen.contains(&n.path));
                    self.folders.retain(|f| seen_folders.contains(f));
                    changed |= notes != self.notes.len() || folders != self.folders.len();
                    self.receiver = None;
                    self.keep_current_folder();
                    break;
                }
            }
        }
        changed
    }

    /// If the folder being shown is gone, shows the nearest one above it that is not.
    fn keep_current_folder(&mut self) {
        while self.current != self.root && !self.folders.contains(&self.current) {
            if !self.current.pop() {
                self.current.clone_from(&self.root);
            }
        }
    }

    /// `true` while the folders are still being read.
    #[must_use]
    pub fn is_scanning(&self) -> bool {
        self.receiver.is_some()
    }

    /// How many drawings there are in the whole library.
    #[must_use]
    pub fn count(&self) -> usize {
        self.notes.len()
    }

    /// `true` while the page shows the library folder itself.
    #[must_use]
    pub fn at_root(&self) -> bool {
        self.current == self.root
    }

    /// `true` while there is something typed in the search field.
    #[must_use]
    pub fn is_searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// The title of the page: "Library" at the top, otherwise the name of the folder shown.
    #[must_use]
    pub fn title(&self) -> String {
        if self.at_root() {
            "Library".to_owned()
        } else {
            self.current
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Library")
                .to_owned()
        }
    }

    /// The way from the library folder down to the folder shown, each step with the folder it leads to. The
    /// first is "Library"; the last is the folder shown.
    #[must_use]
    pub fn breadcrumbs(&self) -> Vec<(String, PathBuf)> {
        let mut steps = vec![("Library".to_owned(), self.root.clone())];
        if let Ok(rest) = self.current.strip_prefix(&self.root) {
            let mut at = self.root.clone();
            for part in rest.components() {
                at.push(part);
                steps.push((part.as_os_str().to_string_lossy().into_owned(), at.clone()));
            }
        }
        steps
    }

    /// Shows `folder`, if it is the library folder or one below it.
    pub fn go_to(&mut self, folder: PathBuf) {
        if folder == self.root || self.folders.contains(&folder) {
            self.current = folder;
        }
    }

    /// The folder card for `path`.
    fn card_for(&self, path: &Path) -> FolderCard {
        let mut inside: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.path.starts_with(path))
            .collect();
        inside.sort_by_key(|n| std::cmp::Reverse(n.modified));
        let drawings = inside.len();
        let previews = inside
            .iter()
            .filter(|n| !n.unreadable && !n.preview.strokes.is_empty())
            .take(3)
            .map(|n| Arc::clone(&n.preview))
            .collect();
        FolderCard {
            path: path.to_owned(),
            name: path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Folder")
                .to_owned(),
            drawings,
            previews,
        }
    }

    /// The folders directly inside the folder shown, by name.
    #[must_use]
    pub fn folders_here(&self) -> Vec<FolderCard> {
        let mut cards: Vec<FolderCard> = self
            .folders
            .iter()
            .filter(|f| f.parent() == Some(self.current.as_path()))
            .map(|f| self.card_for(f))
            .collect();
        cards.sort_by_key(|c| c.name.to_lowercase());
        cards
    }

    /// The drawings directly inside the folder shown, in the chosen order.
    #[must_use]
    pub fn notes_here(&self) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.path.parent() == Some(self.current.as_path()))
            .collect();
        self.sort_notes(&mut notes);
        notes
    }

    fn sort_notes(&self, notes: &mut [&Note]) {
        match self.sort {
            Sort::Recent => notes.sort_by_key(|n| std::cmp::Reverse(n.modified)),
            Sort::Name => notes.sort_by_key(|n| n.name.to_lowercase()),
        }
    }

    /// The drawings edited most recently anywhere in the library, newest first.
    #[must_use]
    pub fn recent(&self, limit: usize) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self.notes.iter().collect();
        notes.sort_by_key(|n| std::cmp::Reverse(n.modified));
        notes.truncate(limit);
        notes
    }

    /// Folders and drawings anywhere in the library whose names contain the search text.
    #[must_use]
    pub fn search_results(&self) -> (Vec<FolderCard>, Vec<&Note>) {
        let query = self.query.trim().to_lowercase();
        let mut folders: Vec<FolderCard> = self
            .folders
            .iter()
            .map(|f| self.card_for(f))
            .filter(|c| c.name.to_lowercase().contains(&query))
            .collect();
        folders.sort_by_key(|c| c.name.to_lowercase());
        let mut notes: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| n.name.to_lowercase().contains(&query))
            .collect();
        self.sort_notes(&mut notes);
        (folders, notes)
    }

    /// Where `path` is, written for people: "Work / Meetings", or `None` for the library folder itself.
    #[must_use]
    pub fn location_of(&self, path: &Path) -> Option<String> {
        let parent = path.parent()?.strip_prefix(&self.root).ok()?;
        let parts: Vec<String> = parent
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        (!parts.is_empty()).then(|| parts.join(" / "))
    }

    /// Every folder an item could be moved to, the library folder first, as paths with how deep they are.
    #[must_use]
    pub fn folder_choices(&self) -> Vec<(PathBuf, usize)> {
        let mut folders: Vec<&PathBuf> = self.folders.iter().collect();
        folders.sort_by_key(|f| f.to_string_lossy().to_lowercase());
        let mut choices = vec![(self.root.clone(), 0)];
        for folder in folders {
            let depth = folder
                .strip_prefix(&self.root)
                .map_or(1, |rest| rest.components().count());
            choices.push((folder.clone(), depth));
        }
        choices
    }
}

/// `true` if `item` may be moved into `target`: not into the folder it is in, and a folder not into itself.
#[must_use]
pub fn can_move_into(item: &Path, target: &Path) -> bool {
    item.parent() != Some(target) && !(item.is_dir() && target.starts_with(item))
}

// File operations

/// A file name made from what the user typed: characters Windows and others refuse are dropped, and so are
/// spaces and dots at the ends. `None` if nothing is left.
#[must_use]
pub fn sanitize_name(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c))
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    let name: String = trimmed.chars().take(80).collect();
    (!name.is_empty()).then_some(name)
}

/// `folder/stem.ink`, or with a number after the name if that is taken.
#[must_use]
pub fn unique_path(folder: &Path, stem: &str) -> PathBuf {
    let mut candidate = folder.join(format!("{stem}.{EXTENSION}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = folder.join(format!("{stem} {n}.{EXTENSION}"));
        n += 1;
    }
    candidate
}

/// `parent/name`, or with a number after the name if that is taken.
#[must_use]
pub fn unique_dir(parent: &Path, name: &str) -> PathBuf {
    let mut candidate = parent.join(name);
    let mut n = 2;
    while candidate.exists() {
        candidate = parent.join(format!("{name} {n}"));
        n += 1;
    }
    candidate
}

/// Where the next new drawing goes. Nothing is written until the drawing has something in it.
#[must_use]
pub fn new_note_path(folder: &Path) -> PathBuf {
    unique_path(folder, "Untitled")
}

/// Makes a folder inside `parent`. Without a usable name it is called "New folder".
///
/// # Errors
/// The message says what went wrong, in words for the user.
pub fn create_folder(parent: &Path, raw_name: &str) -> Result<PathBuf, String> {
    let name = sanitize_name(raw_name).unwrap_or_else(|| "New folder".to_owned());
    let path = unique_dir(parent, &name);
    std::fs::create_dir_all(&path).map_err(|e| format!("Could not make the folder: {e}"))?;
    Ok(path)
}

/// Renames a drawing or a folder. Fails if the name is empty or something there already has it.
///
/// # Errors
/// The message says what went wrong, in words for the user.
pub fn rename_item(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let name = sanitize_name(new_name).ok_or("It needs a name.")?;
    let parent = path.parent().ok_or("That has no folder.")?;
    let target = if path.is_dir() {
        parent.join(&name)
    } else {
        parent.join(format!("{name}.{EXTENSION}"))
    };
    let only_case_differs = target
        .to_string_lossy()
        .eq_ignore_ascii_case(&path.to_string_lossy());
    if target.exists() && target != path && !only_case_differs {
        return Err(format!(
            "\u{201c}{name}\u{201d} is already taken in this folder."
        ));
    }
    std::fs::rename(path, &target).map_err(|e| format!("Could not rename: {e}"))?;
    Ok(target)
}

/// Copies a drawing next to itself, as "name copy".
///
/// # Errors
/// The message says what went wrong, in words for the user.
pub fn duplicate_note(path: &Path) -> Result<PathBuf, String> {
    let folder = path.parent().ok_or("That drawing has no folder.")?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");
    let target = unique_path(folder, &format!("{stem} copy"));
    std::fs::copy(path, &target).map_err(|e| format!("Could not duplicate: {e}"))?;
    Ok(target)
}

/// Moves a drawing or folder into `target`, under the next free name if it is taken there. Moving something to
/// where it already is changes nothing.
///
/// # Errors
/// The message says what went wrong, in words for the user.
pub fn move_item(path: &Path, target: &Path) -> Result<PathBuf, String> {
    if path.parent() == Some(target) {
        return Ok(path.to_owned());
    }
    if path.is_dir() && target.starts_with(path) {
        return Err("A folder can\u{2019}t go inside itself.".to_owned());
    }
    let destination = if path.is_dir() {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Folder");
        unique_dir(target, name)
    } else {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled");
        unique_path(target, stem)
    };
    std::fs::rename(path, &destination).map_err(|e| format!("Could not move: {e}"))?;
    Ok(destination)
}

/// Moves a drawing or folder into the `.deleted` folder at the top of the library, from where
/// [`restore_item`] can bring it back.
///
/// # Errors
/// The message says what went wrong, in words for the user.
pub fn delete_item(path: &Path, root: &Path, now: f64) -> Result<Deleted, String> {
    let bin = root.join(DELETED_FOLDER);
    std::fs::create_dir_all(&bin).map_err(|e| format!("Could not delete: {e}"))?;
    let is_dir = path.is_dir();
    let (name, moved_to) = if is_dir {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Folder");
        (name.to_owned(), unique_dir(&bin, name))
    } else {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled");
        (stem.to_owned(), unique_path(&bin, stem))
    };
    std::fs::rename(path, &moved_to).map_err(|e| format!("Could not delete: {e}"))?;
    Ok(Deleted {
        name,
        original: path.to_owned(),
        moved_to,
        is_dir,
        at: now,
    })
}

/// Brings a deleted drawing or folder back, under its old name or the next free one.
///
/// # Errors
/// The message says what went wrong, in words for the user.
pub fn restore_item(deleted: &Deleted) -> Result<PathBuf, String> {
    let parent = deleted.original.parent().ok_or("That has no folder.")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("Could not restore: {e}"))?;
    let target = if deleted.is_dir {
        unique_dir(parent, &deleted.name)
    } else {
        unique_path(parent, &deleted.name)
    };
    std::fs::rename(&deleted.moved_to, &target).map_err(|e| format!("Could not restore: {e}"))?;
    Ok(target)
}

/// Copies files into `folder`, leaving the originals alone. Each result is the new path or why it failed.
#[must_use]
#[cfg_attr(
    target_os = "android",
    allow(
        dead_code,
        reason = "Android imports through the system picker, which hands over bytes: see `import_bytes`"
    )
)]
pub fn import_notes(files: &[PathBuf], folder: &Path) -> Vec<Result<PathBuf, String>> {
    if let Err(e) = std::fs::create_dir_all(folder) {
        return vec![Err(format!("Could not create the folder: {e}"))];
    }
    files
        .iter()
        .map(|file| {
            let stem = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Imported");
            let target = unique_path(folder, stem);
            std::fs::copy(file, &target)
                .map(|_| target)
                .map_err(|e| format!("Could not import {}: {e}", file.display()))
        })
        .collect()
}

/// Puts a drawing that arrived as bytes (from the system's file picker, which hands over a stream and not a path) in
/// `folder` under its own name, or a numbered one if that is taken.
///
/// # Errors
/// The bytes are not a drawing, or the file could not be written; the message says which, in words for the user.
pub fn import_bytes(name: &str, bytes: &[u8], folder: &Path) -> Result<PathBuf, String> {
    ink_format::read(bytes).map_err(|_| format!("{name} is not an inkwell drawing."))?;
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(sanitize_name)
        .unwrap_or_else(|| "Imported".to_owned());
    std::fs::create_dir_all(folder).map_err(|e| format!("Could not create the folder: {e}"))?;
    let target = unique_path(folder, &stem);
    std::fs::write(&target, bytes).map_err(|e| format!("Could not import {name}: {e}"))?;
    Ok(target)
}

// Dates

/// The year, month and day of a day number counted from 1970-01-01 (proleptic Gregorian calendar).
#[must_use]
pub fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// When a drawing was last edited, in words: "Just now", "5 min ago", "Yesterday", or a date.
#[must_use]
pub fn format_age(now: SystemTime, then: SystemTime) -> String {
    let Ok(elapsed) = now.duration_since(then) else {
        return "Just now".to_owned();
    };
    let secs = elapsed.as_secs();
    match secs {
        0..=59 => "Just now".to_owned(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        86_400..=172_799 => "Yesterday".to_owned(),
        172_800..=604_799 => format!("{} days ago", secs / 86_400),
        _ => {
            let days = then
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| (d.as_secs() / 86_400) as i64);
            let (y, m, d) = civil_date(days);
            format!("{y}-{m:02}-{d:02}")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ink_core::{Color, Editor, IdGen, Pen, Point, StrokeBuilder};
    use ink_format::WriteOptions;

    use super::*;

    /// The time `seconds` ago.
    fn seconds_ago(now: SystemTime, seconds: u64) -> SystemTime {
        now.checked_sub(Duration::from_secs(seconds))
            .unwrap_or(UNIX_EPOCH)
    }

    fn temp_folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("inkwell-library-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp folder");
        dir
    }

    /// Writes a drawing with `strokes` horizontal lines.
    fn write_note(folder: &Path, name: &str, strokes: usize) -> PathBuf {
        std::fs::create_dir_all(folder).expect("folder");
        let mut editor = Editor::blank(IdGen::new(7));
        for s in 0..strokes {
            let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.1);
            for i in 0..12 {
                b.push(
                    Point::new(i as f32 * 10.0, s as f32 * 20.0),
                    None,
                    f64::from(i) * 0.01,
                );
            }
            let stroke = b.with_id(editor.next_id()).finish();
            editor.commit_stroke(0, stroke);
        }
        let bytes = ink_format::write(
            editor.document(),
            &ink_format::Extras::default(),
            &WriteOptions {
                now_unix_ms: 1_700_000_000_000,
            },
        )
        .expect("written");
        let path = folder.join(format!("{name}.{EXTENSION}"));
        std::fs::write(&path, bytes).expect("file");
        path
    }

    #[test]
    fn a_drawing_that_arrived_as_bytes_is_put_in_the_folder_under_its_own_name() {
        let root = temp_folder("bytes");
        let bytes = std::fs::read(write_note(&root.join("source"), "Sketch", 2)).expect("bytes");
        let target = root.join("library");
        let first = import_bytes("Sketch.ink", &bytes, &target).expect("imported");
        assert_eq!(first, target.join("Sketch.ink"));
        let second = import_bytes("Sketch.ink", &bytes, &target).expect("imported again");
        assert_eq!(
            second,
            target.join("Sketch 2.ink"),
            "a taken name gets a number"
        );
        assert_eq!(std::fs::read(&first).expect("read"), bytes);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bytes_that_are_not_a_drawing_are_refused_and_leave_nothing_behind() {
        let root = temp_folder("not-a-drawing");
        let target = root.join("library");
        let error = import_bytes("Notes.txt", b"hello", &target).expect_err("refused");
        assert!(
            error.contains("Notes.txt is not an inkwell drawing"),
            "{error}"
        );
        assert!(!target.exists(), "nothing was made");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_name_the_system_gives_with_nothing_usable_in_it_becomes_imported() {
        let root = temp_folder("nameless");
        let bytes = std::fs::read(write_note(&root.join("source"), "A", 1)).expect("bytes");
        let target = root.join("library");
        let path = import_bytes("...", &bytes, &target).expect("imported");
        assert_eq!(path, target.join("Imported.ink"));
        let _ = std::fs::remove_dir_all(&root);
    }

    fn age(path: &Path, seconds: u64) {
        let file = std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open");
        file.set_modified(seconds_ago(SystemTime::now(), seconds))
            .expect("time");
    }

    fn scan(library: &mut Library, root: &Path) {
        library.refresh(root.to_owned(), || {});
        for _ in 0..500 {
            library.poll();
            if !library.is_scanning() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the scan did not finish");
    }

    fn names(notes: &[&Note]) -> Vec<String> {
        notes.iter().map(|n| n.name.clone()).collect()
    }

    #[test]
    fn a_scan_finds_drawings_and_folders_at_every_depth() {
        let root = temp_folder("scan");
        write_note(&root, "Top", 1);
        write_note(&root.join("Work"), "Plan", 2);
        write_note(&root.join("Work").join("Meetings"), "Monday", 1);
        std::fs::create_dir_all(root.join("Empty")).expect("folder");
        std::fs::create_dir_all(root.join(".hidden")).expect("folder");
        write_note(&root.join(".hidden"), "Secret", 1);
        std::fs::write(root.join("readme.txt"), "not a drawing").expect("file");
        let mut library = Library::default();
        scan(&mut library, &root);
        assert_eq!(library.count(), 3, "the hidden folder is not scanned");
        let folders: Vec<String> = library
            .folders_here()
            .iter()
            .map(|f| f.name.clone())
            .collect();
        assert_eq!(folders, ["Empty", "Work"], "empty folders count, by name");
        let work = library
            .folders_here()
            .into_iter()
            .find(|f| f.name == "Work")
            .expect("Work");
        assert_eq!(work.drawings, 2, "drawings in the folders below count too");
        assert_eq!(names(&library.notes_here()), ["Top"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_page_moves_down_into_folders_and_back_with_breadcrumbs() {
        let root = temp_folder("walk");
        write_note(&root.join("Work").join("Meetings"), "Monday", 1);
        let mut library = Library::default();
        scan(&mut library, &root);
        assert!(library.at_root());
        assert_eq!(library.title(), "Library");
        library.go_to(root.join("Work"));
        library.go_to(root.join("Work").join("Meetings"));
        assert_eq!(library.title(), "Meetings");
        let steps: Vec<String> = library.breadcrumbs().into_iter().map(|(n, _)| n).collect();
        assert_eq!(steps, ["Library", "Work", "Meetings"]);
        assert_eq!(names(&library.notes_here()), ["Monday"]);
        library.go_to(root.join("nowhere"));
        assert_eq!(library.title(), "Meetings", "an unknown folder is ignored");
        library.go_to(library.breadcrumbs()[0].1.clone());
        assert!(library.at_root());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_that_vanishes_while_shown_sends_the_page_up() {
        let root = temp_folder("vanish");
        write_note(&root.join("A").join("B"), "X", 1);
        let mut library = Library::default();
        scan(&mut library, &root);
        library.go_to(root.join("A"));
        library.go_to(root.join("A").join("B"));
        std::fs::remove_dir_all(root.join("A").join("B")).expect("removed");
        scan(&mut library, &root);
        assert_eq!(
            library.current,
            root.join("A"),
            "the nearest folder that is left"
        );
        std::fs::remove_dir_all(root.join("A")).expect("removed");
        scan(&mut library, &root);
        assert!(library.at_root());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn recent_drawings_come_from_every_folder_newest_first() {
        let root = temp_folder("recent");
        let a = write_note(&root, "Old", 1);
        let b = write_note(&root.join("Deep").join("Deeper"), "New", 1);
        let c = write_note(&root.join("Deep"), "Middle", 1);
        age(&a, 9000);
        age(&b, 10);
        age(&c, 500);
        let mut library = Library::default();
        scan(&mut library, &root);
        assert_eq!(names(&library.recent(10)), ["New", "Middle", "Old"]);
        assert_eq!(names(&library.recent(2)), ["New", "Middle"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn search_looks_in_every_folder_ignoring_case_and_says_where_things_are() {
        let root = temp_folder("search");
        write_note(&root, "Banana", 1);
        write_note(&root.join("Work"), "banana split", 1);
        write_note(&root.join("Work"), "Cherry", 1);
        std::fs::create_dir_all(root.join("Bananas")).expect("folder");
        let mut library = Library::default();
        scan(&mut library, &root);
        library.sort = Sort::Name;
        library.query = "BANANA".to_owned();
        assert!(library.is_searching());
        let (folders, notes) = library.search_results();
        assert_eq!(folders.len(), 1);
        assert_eq!(names(&notes), ["Banana", "banana split"]);
        assert_eq!(
            library.location_of(&notes[0].path),
            None,
            "at the top of the library"
        );
        assert_eq!(library.location_of(&notes[1].path).as_deref(), Some("Work"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn drawings_sort_by_name_or_by_time() {
        let root = temp_folder("sort");
        let banana = write_note(&root, "banana", 1);
        write_note(&root, "Apple", 1);
        write_note(&root, "cherry", 1);
        age(&banana, 100_000);
        let mut library = Library::default();
        scan(&mut library, &root);
        library.sort = Sort::Name;
        assert_eq!(names(&library.notes_here()), ["Apple", "banana", "cherry"]);
        library.sort = Sort::Recent;
        assert_eq!(
            names(&library.notes_here()).last().map(String::as_str),
            Some("banana")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_scan_drops_vanished_drawings_and_keeps_unchanged_ones() {
        let root = temp_folder("rescan");
        let keep = write_note(&root, "Keep", 1);
        let gone = write_note(&root, "Gone", 1);
        let mut library = Library::default();
        scan(&mut library, &root);
        let preview_of_keep = |library: &Library| {
            library
                .notes_here()
                .into_iter()
                .find(|n| n.path == keep)
                .map(|n| Arc::clone(&n.preview))
                .expect("Keep is listed")
        };
        let first = preview_of_keep(&library);
        std::fs::remove_file(&gone).expect("removed");
        scan(&mut library, &root);
        assert_eq!(library.count(), 1);
        assert!(
            Arc::ptr_eq(&first, &preview_of_keep(&library)),
            "an unchanged drawing is not read again"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_that_is_not_a_drawing_is_listed_as_unreadable() {
        let root = temp_folder("broken");
        std::fs::write(root.join("Broken.ink"), "garbage").expect("file");
        let mut library = Library::default();
        scan(&mut library, &root);
        let notes = library.notes_here();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].unreadable);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn previews_are_thinned_and_framed() {
        let mut editor = Editor::blank(IdGen::new(3));
        let mut b = StrokeBuilder::new(Pen::Pen, Some(4.0), Color::BLACK, 1.0, 0.0);
        for i in 0..400 {
            b.push(
                Point::new(i as f32, (i % 7) as f32),
                None,
                f64::from(i) * 0.01,
            );
        }
        let stroke = b.with_id(editor.next_id()).finish();
        editor.commit_stroke(0, stroke);
        let preview = preview_of(editor.document());
        assert_eq!(preview.strokes.len(), 1);
        assert!(preview.strokes[0].points.len() <= MAX_PREVIEW_POINTS + 1);
        let last = preview.strokes[0].points.last().expect("points");
        assert!(
            (last[0] - 399.0).abs() < 1.0,
            "the end of the stroke is kept"
        );
        assert!(
            preview.bounds[0] < 0.0 && preview.bounds[2] > 399.0,
            "padded by the width"
        );
        let blank = preview_of(Editor::blank(IdGen::new(1)).document());
        assert_eq!(blank.bounds, Preview::default().bounds);
    }

    #[test]
    fn names_are_cleaned_and_made_unique() {
        assert_eq!(
            sanitize_name("  My: note / v2? ").as_deref(),
            Some("My note  v2")
        );
        assert_eq!(sanitize_name("...").as_deref(), None);
        assert_eq!(sanitize_name("").as_deref(), None);
        let folder = temp_folder("unique");
        assert_eq!(
            unique_path(&folder, "Untitled"),
            folder.join("Untitled.ink")
        );
        std::fs::write(folder.join("Untitled.ink"), "x").expect("file");
        assert_eq!(
            unique_path(&folder, "Untitled"),
            folder.join("Untitled 2.ink")
        );
        std::fs::create_dir_all(folder.join("Work")).expect("folder");
        assert_eq!(unique_dir(&folder, "Work"), folder.join("Work 2"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn folders_are_created_with_a_free_name() {
        let root = temp_folder("create");
        let first = create_folder(&root, "").expect("default name");
        assert_eq!(first, root.join("New folder"));
        assert_eq!(
            create_folder(&root, "").expect("again"),
            root.join("New folder 2")
        );
        assert_eq!(
            create_folder(&root, " Work: 2025 ").expect("cleaned"),
            root.join("Work 2025")
        );
        assert!(first.is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renaming_works_for_drawings_and_folders_and_refuses_taken_or_empty_names() {
        let root = temp_folder("rename");
        let a = write_note(&root, "A", 1);
        write_note(&root, "B", 1);
        assert!(rename_item(&a, "B").is_err());
        assert!(rename_item(&a, "  ").is_err());
        let renamed = rename_item(&a, "Gamma").expect("renamed");
        assert_eq!(renamed, root.join("Gamma.ink"));
        assert!(renamed.exists() && !a.exists());
        assert_eq!(rename_item(&renamed, "Gamma").expect("to itself"), renamed);
        let folder = create_folder(&root, "Old").expect("folder");
        std::fs::create_dir_all(root.join("Taken")).expect("folder");
        assert!(rename_item(&folder, "Taken").is_err());
        let moved = rename_item(&folder, "New").expect("folder renamed");
        assert_eq!(moved, root.join("New"));
        assert!(moved.is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicating_makes_a_copy_with_its_own_name() {
        let root = temp_folder("duplicate");
        let a = write_note(&root, "Plan", 1);
        let first = duplicate_note(&a).expect("copy");
        let second = duplicate_note(&a).expect("another");
        assert_eq!(first, root.join("Plan copy.ink"));
        assert_eq!(second, root.join("Plan copy 2.ink"));
        assert_eq!(
            std::fs::read(&a).expect("a"),
            std::fs::read(&first).expect("copy")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn items_move_between_folders_without_overwriting_anything() {
        let root = temp_folder("move");
        let a = write_note(&root, "Plan", 1);
        let work = create_folder(&root, "Work").expect("folder");
        write_note(&work, "Plan", 2);
        let moved = move_item(&a, &work).expect("moved");
        assert_eq!(
            moved,
            work.join("Plan 2.ink"),
            "the name in the folder was taken"
        );
        assert!(!a.exists() && moved.exists());
        assert_eq!(move_item(&moved, &work).expect("already there"), moved);

        let inner = create_folder(&work, "Inner").expect("folder");
        assert!(move_item(&work, &inner).is_err(), "not into itself");
        assert!(move_item(&work, &work).is_err());
        let up = move_item(&inner, &root).expect("a folder can move up");
        assert_eq!(up, root.join("Inner"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_sensible_moves_are_offered() {
        let root = temp_folder("choices");
        let work = create_folder(&root, "Work").expect("folder");
        let inner = create_folder(&work, "Inner").expect("folder");
        let item = write_note(&work, "A", 1);
        assert!(!can_move_into(&item, &work), "it is already there");
        assert!(can_move_into(&item, &root));
        assert!(can_move_into(&item, &inner));
        assert!(!can_move_into(&work, &work), "not into itself");
        assert!(
            !can_move_into(&work, &inner),
            "nor into something inside it"
        );
        assert!(can_move_into(&inner, &root));
        let mut library = Library::default();
        scan(&mut library, &root);
        let choices = library.folder_choices();
        assert_eq!(choices[0], (root.clone(), 0));
        assert_eq!(choices[1], (work, 1));
        assert_eq!(choices[2], (inner, 2));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_deleted_drawing_or_folder_can_be_brought_back_even_if_its_name_was_reused() {
        let root = temp_folder("delete");
        let a = write_note(&root, "Idea", 1);
        let deleted = delete_item(&a, &root, 0.0).expect("deleted");
        assert!(!a.exists());
        assert!(root.join(DELETED_FOLDER).is_dir());
        write_note(&root, "Idea", 0);
        let back = restore_item(&deleted).expect("restored");
        assert_eq!(back, root.join("Idea 2.ink"));

        let folder = create_folder(&root, "Work").expect("folder");
        write_note(&folder, "Inside", 1);
        let gone = delete_item(&folder, &root, 0.0).expect("folder deleted");
        assert!(!folder.exists());
        let again = restore_item(&gone).expect("folder restored");
        assert_eq!(again, folder);
        assert!(folder.join("Inside.ink").exists(), "with what was in it");

        let mut library = Library::default();
        scan(&mut library, &root);
        assert_eq!(library.count(), 3, "the bin is not part of the library");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn importing_copies_and_leaves_the_original() {
        let source = temp_folder("import-from");
        let folder = temp_folder("import-to");
        let original = write_note(&source, "Sketch", 1);
        write_note(&folder, "Sketch", 1);
        let missing = source.join("missing.ink");
        let results = import_notes(&[original.clone(), missing], &folder);
        assert_eq!(
            results[0].as_ref().expect("copied"),
            &folder.join("Sketch 2.ink")
        );
        assert!(results[1].is_err());
        assert!(original.exists());
        let _ = std::fs::remove_dir_all(&source);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn dates_are_civil_and_ages_are_in_words() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(19_723), (2024, 1, 1));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let age = |s| format_age(now, seconds_ago(now, s));
        assert_eq!(age(5), "Just now");
        assert_eq!(age(300), "5 min ago");
        assert_eq!(age(7_200), "2 h ago");
        assert_eq!(age(100_000), "Yesterday");
        assert_eq!(age(300_000), "3 days ago");
        assert_eq!(age(3_000_000), "2023-10-11");
        assert_eq!(
            format_age(UNIX_EPOCH, now),
            "Just now",
            "a time in the future is not negative"
        );
    }

    fn region(editor: &mut Editor, points: Vec<Point>) {
        let mut b = StrokeBuilder::new(Pen::Fill, None, Color::new(1.0, 0.8, 0.0, 1.0), 1.0, 0.0);
        b.replace_with(points);
        let stroke = b.with_id(editor.next_id()).finish();
        editor.commit_stroke(0, stroke);
    }

    #[test]
    fn a_region_in_a_preview_carries_the_triangles_of_its_inside() {
        let mut editor = Editor::blank(IdGen::new(5));
        region(
            &mut editor,
            vec![
                Point::new(0.0, 0.0),
                Point::new(100.0, 0.0),
                Point::new(100.0, 100.0),
                Point::new(0.0, 100.0),
            ],
        );
        let preview = preview_of(editor.document());
        let fill = &preview.strokes[0].fill;
        assert!(!fill.is_empty(), "a filled shape is filled");
        assert!((ink_brush::fill::area(fill) - 10_000.0).abs() < 1.0);
    }

    #[test]
    fn a_line_in_a_preview_has_no_fill() {
        let mut editor = Editor::blank(IdGen::new(5));
        let mut b = StrokeBuilder::new(Pen::Pen, Some(3.0), Color::BLACK, 1.0, 0.0);
        b.replace_with(vec![
            Point::new(0.0, 0.0),
            Point::new(50.0, 0.0),
            Point::new(50.0, 50.0),
        ]);
        let stroke = b.with_id(editor.next_id()).finish();
        editor.commit_stroke(0, stroke);
        assert_eq!(preview_of(editor.document()).strokes[0].fill.len(), 0);
    }

    #[test]
    fn the_corners_of_a_star_survive_the_preview_and_a_hole_stays_a_hole() {
        let star = ink_core::shapes::built_in_named("star").expect("star");
        let mut editor = Editor::blank(IdGen::new(5));
        let mut ids = IdGen::new(9);
        let style = ink_core::shapes::Style {
            pen: Pen::Pen,
            width: Some(3.0),
            color: Color::BLACK,
            fill: Some(Color::new(1.0, 0.8, 0.0, 1.0)),
            outline: true,
        };
        for stroke in star.to_strokes(Point::new(0.0, 0.0), 200.0, &style, &mut || ids.next_id()) {
            editor.commit_stroke(0, stroke);
        }
        let preview = preview_of(editor.document());
        assert!(!preview.strokes[0].fill.is_empty(), "the fill comes first");
        // Ten corners, and the first is repeated at the end that closes the outline.
        assert!(
            preview.strokes[0].points.len() >= 10,
            "{}",
            preview.strokes[0].points.len()
        );
        // A ring with a hole in it: bridged, so the inside is the ring and not the whole disc.
        let mut editor = Editor::blank(IdGen::new(6));
        let ring: Vec<Point> = (0..=40)
            .map(|i| {
                let a = i as f32 / 40.0 * std::f32::consts::TAU;
                Point::new(100.0 * a.cos(), 100.0 * a.sin())
            })
            .chain((0..=40).map(|i| {
                let a = i as f32 / 40.0 * std::f32::consts::TAU;
                Point::new(50.0 * a.cos(), 50.0 * a.sin())
            }))
            .chain(std::iter::once(Point::new(100.0, 0.0)))
            .collect();
        region(&mut editor, ring);
        let area = ink_brush::fill::area(&preview_of(editor.document()).strokes[0].fill);
        let disc = std::f32::consts::PI * 100.0 * 100.0;
        assert!(
            area < 0.9 * disc,
            "the hole is not filled: {area} of {disc}"
        );
    }
}
