//! What the system tells the app about the screen it is on, where that differs between platforms.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use eframe::egui::{Color32, Context, Id};

/// Whether the controls are laid out for a finger: Android always, and a computer when asked (to check the layout in
/// screenshots).
static TOUCH_FIRST: AtomicBool = AtomicBool::new(cfg!(target_os = "android"));

/// `true` on a device that is used by touch first: the buttons are bigger, there is no hovering, and nothing is
/// shown as a key on a keyboard.
#[must_use]
pub fn touch_first() -> bool {
    TOUCH_FIRST.load(Ordering::Relaxed)
}

/// Lays the controls out for a finger whatever the system is.
pub fn set_touch_first(on: bool) {
    TOUCH_FIRST.store(on, Ordering::Relaxed);
}

/// The color the strips under the system bars are painted, so their icons read on it: the same as the top bar.
pub const SYSTEM_BAR: Color32 = Color32::from_rgb(0x15, 0x14, 0x12);

/// The activity the app runs in, kept for asking about the screen.
#[cfg(target_os = "android")]
static ACTIVITY: std::sync::OnceLock<android_activity::AndroidApp> = std::sync::OnceLock::new();

/// Remembers the activity the app runs in.
#[cfg(target_os = "android")]
pub fn remember(app: android_activity::AndroidApp) {
    let _ = ACTIVITY.set(app);
}

/// How much of each edge of the window the system covers (the status bar, the navigation bar, a camera cutout, the
/// keyboard), in pixels: left, top, right and bottom. `None` where the platform does not say, or before it knows.
#[cfg(target_os = "android")]
#[must_use]
pub fn covered_pixels() -> Option<[f32; 4]> {
    let app = ACTIVITY.get()?;
    let window = app.native_window()?;
    let (width, height) = (window.width(), window.height());
    // The part of the window the content should be in, in window pixels. Zero until the system has said.
    let content = app.content_rect();
    if content.right <= content.left || content.bottom <= content.top {
        return None;
    }
    Some([
        content.left.max(0) as f32,
        content.top.max(0) as f32,
        (width - content.right).max(0) as f32,
        (height - content.bottom).max(0) as f32,
    ])
}

#[cfg(not(target_os = "android"))]
#[must_use]
pub fn covered_pixels() -> Option<[f32; 4]> {
    None
}

/// Shows the on-screen keyboard while a text field wants typing, and puts it away when none does. It is asked every frame
/// and only acts when the answer changes, or when the person touches the screen with the keyboard wanted and not there
/// (they put it away with its own button, and touch the field to have it back). A computer has a keyboard of its own:
/// nothing happens.
///
/// The keyboard is asked for outright and not as something implied: a native view is not an editor, and the system
/// refuses to show a keyboard for one that was only implied.
#[cfg(target_os = "android")]
pub fn soft_keyboard(wanted: bool, touched: bool) {
    static SHOWN: AtomicBool = AtomicBool::new(false);
    let changed = SHOWN.swap(wanted, Ordering::Relaxed) != wanted;
    let asked_back = wanted && touched && keyboard_pixels() < 1.0;
    if !changed && !asked_back {
        return;
    }
    if let Some(app) = ACTIVITY.get() {
        if wanted {
            app.show_soft_input(false);
        } else {
            app.hide_soft_input(false);
        }
    }
}

#[cfg(not(target_os = "android"))]
pub fn soft_keyboard(_wanted: bool, _touched: bool) {}

/// How much of the bottom of the window the on-screen keyboard covers, in pixels, as the bits of an `f32`.
static KEYBOARD_PIXELS: AtomicU32 = AtomicU32::new(0);

/// The on-screen keyboard now covers `pixels` of the bottom of the window (`0.0` when it is away). May be called from any
/// thread: the system says so on its own.
pub fn set_keyboard_pixels(pixels: f32) {
    let pixels = if pixels.is_finite() {
        pixels.max(0.0)
    } else {
        0.0
    };
    if KEYBOARD_PIXELS.swap(pixels.to_bits(), Ordering::Relaxed) != pixels.to_bits() {
        crate::touch_input::wake();
    }
}

/// How much of the bottom of the window the on-screen keyboard covers, in pixels.
#[must_use]
pub(crate) fn keyboard_pixels() -> f32 {
    f32::from_bits(KEYBOARD_PIXELS.load(Ordering::Relaxed))
}

/// Remembers how tall the keyboard is in points, for this frame, so what must clear it can ask.
pub(crate) fn set_keyboard(ctx: &Context, points: f32) {
    ctx.data_mut(|d| d.insert_temp(Id::unique("keyboard_points"), points));
}

/// How far up from the bottom of the area the app lays out in a sheet must be lifted to clear the on-screen keyboard, in
/// points: the keyboard's height, less what the system bars already keep clear. `0.0` where there is no keyboard to
/// clear (a computer has one of its own).
#[must_use]
pub fn keyboard_lift(ctx: &Context) -> f32 {
    let keyboard = ctx
        .data(|d| d.get_temp::<f32>(Id::unique("keyboard_points")))
        .unwrap_or(0.0);
    let already_clear = ctx.viewport_rect().bottom() - ctx.content_rect().bottom();
    (keyboard - already_clear).max(0.0)
}

/// Whether the app would do something with a press of Back (see `back`), kept for the system to ask about.
static CAN_GO_BACK: AtomicBool = AtomicBool::new(false);
/// A press of Back that the app has been asked to act on.
static BACK_PRESSED: AtomicBool = AtomicBool::new(false);

/// Says whether the app would do something with a press of Back. Called every frame.
pub fn set_can_go_back(can: bool) {
    CAN_GO_BACK.store(can, Ordering::Relaxed);
}

/// The system's Back was pressed. Returns `true` when the app takes it, and `false` when there is nothing left in the
/// app to go back out of and the system should put the app away. May be called from any thread.
#[must_use]
pub fn request_back() -> bool {
    if !CAN_GO_BACK.load(Ordering::Relaxed) {
        return false;
    }
    BACK_PRESSED.store(true, Ordering::Relaxed);
    crate::touch_input::wake();
    true
}

/// `true` once for each press of Back that was taken.
#[must_use]
pub(crate) fn take_back() -> bool {
    BACK_PRESSED.swap(false, Ordering::Relaxed)
}
