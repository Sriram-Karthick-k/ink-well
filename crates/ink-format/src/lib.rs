//! The `.ink` drawing file: a ZIP archive of JSON, read into and written from the [`ink_core`] document model.
//!
//! The layout is in `docs/FORMAT.md`. Reading never panics on bad input: a file that is damaged, hostile, too big
//! or not a drawing at all gives a [`FormatError`].

mod error;
mod extras;
mod reader;
mod writer;

pub use error::FormatError;
pub use extras::Extras;
pub use reader::{Drawing, read};
pub use writer::{WriteOptions, iso8601_utc, write};

/// The extension of a drawing file, without the dot.
pub const EXTENSION: &str = "ink";
