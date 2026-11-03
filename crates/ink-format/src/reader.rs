//! Reading a `.ink` file.

use std::io::{Cursor, Read, Seek};

use ink_core::Document;
use serde::Deserialize;
use zip::ZipArchive;
use zip::result::ZipError;

use crate::error::FormatError;
use crate::extras::Extras;

/// What `manifest.json` says the file is.
pub(crate) const FORMAT: &str = "ink";
/// The version of the layout this code reads and writes. A file of a higher one is refused.
pub(crate) const VERSION: u32 = 1;

pub(crate) const MANIFEST: &str = "manifest.json";
pub(crate) const DOCUMENT: &str = "document.json";
pub(crate) const EXTRAS: &str = "extras.json";

/// The most each part may hold once unpacked, so that a small file cannot ask for all the memory there is.
#[derive(Debug, Clone, Copy)]
struct Limits {
    manifest: u64,
    document: u64,
    extras: u64,
}

const LIMITS: Limits = Limits {
    manifest: 64 * 1024,
    document: 512 * 1024 * 1024,
    extras: 16 * 1024 * 1024,
};

#[derive(Debug, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
}

/// A drawing as read: its layers and strokes, and what it keeps besides.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    pub document: Document,
    pub extras: Extras,
}

/// Reads the bytes of a `.ink` file.
///
/// # Errors
/// A [`FormatError`] says why the bytes are not a drawing this version can open.
pub fn read(bytes: &[u8]) -> Result<Drawing, FormatError> {
    read_with(bytes, LIMITS)
}

fn read_with(bytes: &[u8], limits: Limits) -> Result<Drawing, FormatError> {
    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    let manifest_bytes =
        part(&mut zip, MANIFEST, limits.manifest)?.ok_or(FormatError::MissingEntry(MANIFEST))?;
    let manifest: Manifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| FormatError::Json(MANIFEST, e))?;
    if manifest.format != FORMAT {
        return Err(FormatError::NotInk(MANIFEST));
    }
    if manifest.version > VERSION {
        return Err(FormatError::Newer(manifest.version));
    }
    let document_bytes =
        part(&mut zip, DOCUMENT, limits.document)?.ok_or(FormatError::MissingEntry(DOCUMENT))?;
    let document: Document =
        serde_json::from_slice(&document_bytes).map_err(|e| FormatError::Json(DOCUMENT, e))?;
    if !document.layers.iter().any(|l| l.content) {
        return Err(FormatError::Structure("it has no layer to draw on"));
    }
    // What a drawing keeps besides its strokes is not worth losing the strokes for.
    let extras = part(&mut zip, EXTRAS, limits.extras)?
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    Ok(Drawing { document, extras })
}

/// The unpacked bytes of the entry `name`, or `None` if the archive has no such entry.
fn part<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &'static str,
    limit: u64,
) -> Result<Option<Vec<u8>>, FormatError> {
    let file = match zip.by_name(name) {
        Ok(file) => file,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if file.size() > limit {
        return Err(FormatError::EntryTooLarge(name, limit));
    }
    // The size the archive declares can be a lie: read no more than the limit, and one byte more to notice.
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| FormatError::Io(name, e))?;
    if bytes.len() as u64 > limit {
        return Err(FormatError::EntryTooLarge(name, limit));
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::*;

    /// An archive with these entries, stored as they are.
    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            zip.start_file(*name, SimpleFileOptions::default())
                .expect("entry");
            zip.write_all(bytes).expect("bytes");
        }
        zip.finish().expect("archive").into_inner()
    }

    const MANIFEST_OK: &[u8] = br#"{"format":"ink","version":1}"#;

    #[test]
    fn something_that_is_not_an_archive_is_refused() {
        assert!(matches!(read(b""), Err(FormatError::Zip(_))));
        assert!(matches!(read(b"just some text"), Err(FormatError::Zip(_))));
    }

    #[test]
    fn an_archive_of_something_else_is_not_a_drawing() {
        let other = archive(&[("notes.txt", b"x"), ("data.json", b"{}")]);
        assert!(matches!(
            read(&other),
            Err(FormatError::MissingEntry(MANIFEST))
        ));
        let wrong = archive(&[(MANIFEST, br#"{"format":"other","version":1}"#)]);
        assert!(matches!(read(&wrong), Err(FormatError::NotInk(MANIFEST))));
        let damaged = archive(&[(MANIFEST, b"not json")]);
        assert!(matches!(
            read(&damaged),
            Err(FormatError::Json(MANIFEST, _))
        ));
    }

    #[test]
    fn a_newer_version_is_refused_with_its_number() {
        let newer = archive(&[(MANIFEST, br#"{"format":"ink","version":99}"#)]);
        assert!(matches!(read(&newer), Err(FormatError::Newer(99))));
    }

    #[test]
    fn a_missing_or_damaged_document_is_refused() {
        let none = archive(&[(MANIFEST, MANIFEST_OK)]);
        assert!(matches!(
            read(&none),
            Err(FormatError::MissingEntry(DOCUMENT))
        ));
        let damaged = archive(&[(MANIFEST, MANIFEST_OK), (DOCUMENT, b"{\"layers\": 3}")]);
        assert!(matches!(
            read(&damaged),
            Err(FormatError::Json(DOCUMENT, _))
        ));
    }

    #[test]
    fn a_part_over_its_limit_is_refused_whatever_the_archive_says() {
        let big = archive(&[(MANIFEST, MANIFEST_OK), (DOCUMENT, &[b' '; 400])]);
        let tight = Limits {
            document: 100,
            ..LIMITS
        };
        assert!(matches!(
            read_with(&big, tight),
            Err(FormatError::EntryTooLarge(DOCUMENT, 100))
        ));
    }
}
