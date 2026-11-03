//! Errors produced while reading or writing a drawing.

/// Why a file could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("not a drawing: it is not a valid ZIP archive ({0})")]
    Zip(#[from] zip::result::ZipError),
    #[error("not an inkwell drawing: the archive has no `{0}`")]
    MissingEntry(&'static str),
    #[error("not an inkwell drawing: `{0}` says it is something else")]
    NotInk(&'static str),
    #[error(
        "this drawing is from a newer version of inkwell (format {0}); update the app to open it"
    )]
    Newer(u32),
    #[error("`{0}` is larger than {1} bytes")]
    EntryTooLarge(&'static str, u64),
    #[error("I/O error while reading `{0}`: {1}")]
    Io(&'static str, #[source] std::io::Error),
    #[error("`{0}` is damaged: {1}")]
    Json(&'static str, #[source] serde_json::Error),
    #[error("the drawing is not usable: {0}")]
    Structure(&'static str),
}
