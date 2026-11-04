//! The system's own pickers for files, and what they report.
//!
//! On a computer a file dialog blocks until it is answered. On Android the picker is another screen that answers later
//! (the app is paused while it is up), and the file it picks is not a path the app may open: it is a stream the system
//! hands over. So the app asks for a picker through a [`Bridge`] (the Android entry point implements it, with the
//! platform's calls), and the answers come back as [`Event`]s in a queue the app reads once a frame.
//!
//! The bridge and the queue are on every system, so what the app does with an answer can be tested on a computer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// What a picked file is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Open a drawing: it is copied into the Library and opened in a tab.
    Open,
    /// Import drawings: they are copied into the Library.
    Import,
}

/// Something the system reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A file was picked, with its name and everything in it.
    Picked {
        purpose: Purpose,
        name: String,
        bytes: Vec<u8>,
    },
    /// A copy of the drawing was written where the person chose, or could not be.
    Saved { ok: bool, name: String },
}

/// What the app can ask the system for.
pub trait Bridge: Send + Sync {
    /// Opens the picker for where to save `bytes` as `name`. The answer comes back as [`Event::Saved`].
    ///
    /// # Errors
    /// The picker could not be opened; the message says why.
    fn pick_save(&self, name: &str, bytes: &[u8]) -> Result<(), String>;

    /// Opens the picker for files to open or import. Each file comes back as an [`Event::Picked`].
    ///
    /// # Errors
    /// The picker could not be opened; the message says why.
    fn pick_files(&self, purpose: Purpose) -> Result<(), String>;
}

static BRIDGE: OnceLock<Box<dyn Bridge>> = OnceLock::new();
static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());
static PAUSED: AtomicBool = AtomicBool::new(false);

/// How many answers may wait for the app to read them.
const MOST_WAITING: usize = 64;

/// Gives the app its way to the system's pickers. Only the first is kept.
pub fn set_bridge(bridge: Box<dyn Bridge>) {
    let _ = BRIDGE.set(bridge);
}

/// Asks for where to save a copy of a drawing.
///
/// # Errors
/// There is no picker, or it could not be opened.
#[cfg_attr(
    not(target_os = "android"),
    allow(
        dead_code,
        reason = "only Android asks the system for pickers; a computer has its own dialogs"
    )
)]
pub(crate) fn pick_save(name: &str, bytes: &[u8]) -> Result<(), String> {
    BRIDGE
        .get()
        .ok_or_else(|| "This device has no file picker for the app.".to_owned())?
        .pick_save(name, bytes)
}

/// Asks for files to open or import.
///
/// # Errors
/// There is no picker, or it could not be opened.
#[cfg_attr(
    not(target_os = "android"),
    allow(
        dead_code,
        reason = "only Android asks the system for pickers; a computer has its own dialogs"
    )
)]
pub(crate) fn pick_files(purpose: Purpose) -> Result<(), String> {
    BRIDGE
        .get()
        .ok_or_else(|| "This device has no file picker for the app.".to_owned())?
        .pick_files(purpose)
}

/// Reports something the system did. It may be called from any thread.
pub fn push(event: Event) {
    if let Ok(mut waiting) = EVENTS.lock() {
        if waiting.len() >= MOST_WAITING {
            waiting.remove(0);
        }
        waiting.push(event);
    }
    crate::touch_input::wake();
}

/// Reports that the app is going to the background, where it may be stopped without a word: whatever is unsaved has to
/// be written now.
pub fn push_paused() {
    PAUSED.store(true, Ordering::Relaxed);
    crate::touch_input::wake();
}

/// What the system reported since the last time, oldest first.
#[must_use]
pub(crate) fn take() -> Vec<Event> {
    EVENTS
        .lock()
        .map(|mut waiting| std::mem::take(&mut *waiting))
        .unwrap_or_default()
}

/// `true` once after the app was sent to the background.
#[must_use]
pub(crate) fn take_paused() -> bool {
    PAUSED.swap(false, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_bridge_asking_for_a_picker_says_so_instead_of_doing_nothing() {
        // The bridge is set once for the whole program, and no test here sets it.
        let error = pick_save("A.ink", b"x").expect_err("no picker");
        assert!(error.contains("no file picker"), "{error}");
        assert!(pick_files(Purpose::Open).is_err());
    }
}
