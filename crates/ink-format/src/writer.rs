//! Writing a document as a `.ink` archive.

use std::io::{Cursor, Write};

use ink_core::Document;
use serde::Serialize;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::error::FormatError;
use crate::extras::Extras;
use crate::reader::{DOCUMENT, EXTRAS, FORMAT, MANIFEST, VERSION};

/// Inputs the writer cannot know on its own.
#[derive(Debug, Clone, Copy)]
pub struct WriteOptions {
    /// The current time in milliseconds since the Unix epoch.
    pub now_unix_ms: i64,
}

#[derive(Serialize)]
struct Manifest<'a> {
    format: &'a str,
    version: u32,
    /// What wrote the file; for a person looking inside it.
    writer: &'a str,
}

/// Writes `document` and its `extras` as the bytes of a `.ink` file. The modification time is now, and the
/// creation time is set if the document has none.
///
/// # Errors
/// A [`FormatError`] if the archive cannot be put together.
pub fn write(
    document: &Document,
    extras: &Extras,
    options: &WriteOptions,
) -> Result<Vec<u8>, FormatError> {
    let mut document = document.clone();
    let now = iso8601_utc(options.now_unix_ms);
    if document.metadata.creation_time.is_empty() {
        document.metadata.creation_time.clone_from(&now);
    }
    document.metadata.modification_time = now;
    document.metadata.format_version = VERSION;

    let manifest = Manifest {
        format: FORMAT,
        version: VERSION,
        writer: concat!("inkwell ", env!("CARGO_PKG_VERSION")),
    };
    let json = |name: &'static str, value: serde_json::Result<Vec<u8>>| {
        value.map_err(|e| FormatError::Json(name, e))
    };

    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let mut add = |name: &'static str, bytes: &[u8]| -> Result<(), FormatError> {
        zip.start_file(name, deflated)?;
        zip.write_all(bytes).map_err(|e| FormatError::Io(name, e))
    };
    add(MANIFEST, &json(MANIFEST, serde_json::to_vec(&manifest))?)?;
    add(DOCUMENT, &json(DOCUMENT, serde_json::to_vec(&document))?)?;
    if !extras.is_empty() {
        add(EXTRAS, &json(EXTRAS, serde_json::to_vec(extras))?)?;
    }
    Ok(zip.finish()?.into_inner())
}

/// Formats Unix milliseconds as ISO 8601 UTC, for example `2026-10-30T20:54:33.371Z`.
#[must_use]
pub fn iso8601_utc(unix_ms: i64) -> String {
    let seconds = unix_ms.div_euclid(1000);
    let millis = unix_ms.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let in_day = seconds.rem_euclid(86_400);
    let (hour, minute, second) = (in_day / 3600, in_day % 3600 / 60, in_day % 60);

    // Civil date from a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_format_as_iso_8601() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso8601_utc(1_793_393_673_371), "2026-10-30T20:54:33.371Z");
        assert_eq!(
            iso8601_utc(1_709_251_199_005),
            "2024-02-29T23:59:59.005Z",
            "leap day"
        );
        assert_eq!(
            iso8601_utc(-1),
            "1969-12-31T23:59:59.999Z",
            "before the epoch"
        );
    }
}
