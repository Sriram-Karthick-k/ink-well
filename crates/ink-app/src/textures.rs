//! Loads stamp textures for the pens that can use them, when a folder of them is provided.
//!
//! No textures are shipped. They are read at run time from a folder, found in this order: the `--textures` option
//! or the setting, the `INKWELL_TEXTURES` environment variable, and a `textures` folder next to the program or in
//! the current folder. The file names are those of [`MaskKind::file_name`]. Without textures, the pencils and the
//! airbrush draw plain soft stamps.

use std::path::{Path, PathBuf};

use ink_brush::MaskKind;

/// The folder looked for beside the program and in the current folder when none is given.
const DEFAULT_FOLDER: &str = "textures";

/// A square single-channel texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    pub kind: MaskKind,
    /// Pixels on a side.
    pub size: i32,
    pub pixels: Vec<u8>,
}

/// The folder to load textures from, if there is one.
#[must_use]
pub fn find_folder(explicit: Option<&Path>) -> Option<PathBuf> {
    let from_env = std::env::var_os("INKWELL_TEXTURES").map(PathBuf::from);
    if let Some(folder) = explicit.map(Path::to_owned).or(from_env) {
        return folder.is_dir().then_some(folder);
    }
    let beside_program = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(DEFAULT_FOLDER)));
    let here = std::env::current_dir()
        .ok()
        .map(|dir| dir.join(DEFAULT_FOLDER));
    beside_program
        .into_iter()
        .chain(here)
        .find(|candidate| candidate.is_dir())
}

/// Loads every stamp texture found in `folder`. Files that are missing, unreadable or not square are skipped.
#[must_use]
pub fn load_masks(folder: &Path) -> Vec<Mask> {
    MaskKind::TEXTURED
        .into_iter()
        .filter_map(|kind| {
            let path = folder.join(kind.file_name()?);
            let image = image::open(path).ok()?.into_luma8();
            (image.width() == image.height() && image.width() > 0).then(|| Mask {
                kind,
                size: image.width() as i32,
                pixels: image.into_raw(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("inkwell-textures-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp folder");
        dir
    }

    fn write_gray(path: &Path, width: u32, height: u32) {
        image::GrayImage::from_fn(width, height, |x, y| image::Luma([(x + y) as u8]))
            .save(path)
            .expect("written");
    }

    #[test]
    fn square_textures_are_loaded_by_kind() {
        let dir = temp_folder("square");
        write_gray(&dir.join("airbrush.png"), 8, 8);
        write_gray(&dir.join("hard-pencil.png"), 16, 16);
        let masks = load_masks(&dir);
        let kinds: Vec<MaskKind> = masks.iter().map(|m| m.kind).collect();
        assert_eq!(kinds, [MaskKind::Airbrush, MaskKind::HardPencil]);
        assert_eq!(masks[1].size, 16);
        assert_eq!(masks[1].pixels.len(), 16 * 16);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_non_square_or_missing_texture_is_skipped() {
        let dir = temp_folder("skipped");
        write_gray(&dir.join("soft-pencil.png"), 8, 4);
        assert_eq!(load_masks(&dir), Vec::<Mask>::new());
        assert_eq!(load_masks(&dir.join("nowhere")), Vec::<Mask>::new());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_explicit_folder_that_does_not_exist_finds_nothing() {
        assert_eq!(find_folder(Some(Path::new("/definitely/not/here"))), None);
    }
}
