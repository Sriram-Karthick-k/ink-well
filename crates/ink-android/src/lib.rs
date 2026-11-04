//! The Android entry point. The system starts an activity, which loads this library and calls
//! [`android_main`]; from there the app is the same one that runs on a desktop.
//!
//! The activity (`InkActivity`, in `android/java`) passes touch and key input to the functions below, which put it in
//! the queues the app reads; what a touch means is decided there, not here. It also shows the system's pickers for
//! files, which the app asks for through [`Pickers`], and reports what they answer.
//!
//! Everything is left out on other systems, so the crate builds (to nothing) with the rest of the workspace.
#![cfg(target_os = "android")]

use android_activity::AndroidApp;
use ink_app::system_files::{self, Event, Purpose};
use jni::objects::{JByteArray, JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jint};
use jni::{JNIEnv, JavaVM};
use log::LevelFilter;

/// Called by the activity once the app has started. It returns when the app is closed.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(LevelFilter::Info)
            .with_tag("inkwell"),
    );
    // A panic is a crash with nothing to read: put its message where `adb logcat` shows it.
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));
    log::info!("inkwell starting");
    system_files::set_bridge(Box::new(Pickers { app: app.clone() }));
    if let Err(error) = ink_app::run_android(app) {
        log::error!("inkwell stopped: {error}");
    }
}

/// The way to the system's pickers for files: calls on the activity.
struct Pickers {
    app: AndroidApp,
}

impl std::fmt::Debug for Pickers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pickers").finish_non_exhaustive()
    }
}

impl Pickers {
    /// Runs `call` on the activity, from this thread, which is attached to the virtual machine for it.
    fn on_activity(
        &self,
        call: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<()>,
    ) -> Result<(), String> {
        // SAFETY: both pointers come from `android-activity`, which keeps the virtual machine and the activity alive
        // for as long as the app runs, and this is only called while it does.
        let vm =
            unsafe { JavaVM::from_raw(self.app.vm_as_ptr().cast()) }.map_err(|e| e.to_string())?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        // SAFETY: as above. The object is borrowed, not owned: nothing frees it here.
        let activity = unsafe { JObject::from_raw(self.app.activity_as_ptr().cast()) };
        let result = call(&mut env, &activity);
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        result.map_err(|e| e.to_string())
    }
}

impl system_files::Bridge for Pickers {
    fn pick_save(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        self.on_activity(|env, activity| {
            let name = env.new_string(name)?;
            let data = env.byte_array_from_slice(bytes)?;
            env.call_method(
                activity,
                "pickSaveLocation",
                "(Ljava/lang/String;[B)V",
                &[JValue::Object(&name), JValue::Object(&data)],
            )?;
            Ok(())
        })
    }

    fn pick_files(&self, purpose: Purpose) -> Result<(), String> {
        let purpose = match purpose {
            Purpose::Open => 0,
            Purpose::Import => 1,
        };
        self.on_activity(|env, activity| {
            env.call_method(activity, "pickFiles", "(I)V", &[JValue::Int(purpose)])?;
            Ok(())
        })
    }
}

// The functions below are the `native` methods of `app.inkwell.InkActivity`. Java finds them by these names. They
// take the environment and the class first.

/// One sample of one pointer (a finger, a stylus, a mouse).
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_inkwell_InkActivity_nativePointer(
    _env: JNIEnv,
    _class: JClass,
    action: jint,
    device: jint,
    pointer: jint,
    tool: jint,
    x: f32,
    y: f32,
    pressure: f32,
    contact: f32,
    _tilt: f32,
    _orientation: f32,
    buttons: jint,
    time_ms: i64,
) {
    ink_app::push_pointer(
        action, device, pointer, tool, x, y, pressure, contact, buttons, time_ms,
    );
}

/// A key going down or up.
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_inkwell_InkActivity_nativeKey(
    _env: JNIEnv,
    _class: JClass,
    down: jboolean,
    key_code: jint,
    meta: jint,
    unicode: jint,
    repeat: jboolean,
) {
    ink_app::push_key(down != 0, key_code, meta, unicode, repeat != 0);
}

/// Whether the device has a stylus digitizer.
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_inkwell_InkActivity_nativeCapabilities(
    _env: JNIEnv,
    _class: JClass,
    stylus: jboolean,
) {
    ink_app::set_stylus_device(stylus != 0);
}

/// A file was picked.
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_inkwell_InkActivity_nativeFilePicked(
    mut env: JNIEnv,
    _class: JClass,
    purpose: jint,
    name: JString,
    bytes: JByteArray,
) {
    let name: String = env.get_string(&name).map(Into::into).unwrap_or_default();
    let bytes = env.convert_byte_array(&bytes).unwrap_or_default();
    system_files::push(Event::Picked {
        purpose: if purpose == 1 {
            Purpose::Import
        } else {
            Purpose::Open
        },
        name,
        bytes,
    });
}

/// A copy of a drawing was written, or could not be.
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_inkwell_InkActivity_nativeFileSaved(
    mut env: JNIEnv,
    _class: JClass,
    ok: jboolean,
    name: JString,
) {
    let name: String = env.get_string(&name).map(Into::into).unwrap_or_default();
    system_files::push(Event::Saved { ok: ok != 0, name });
}

/// The app is going to the background.
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_inkwell_InkActivity_nativePaused(_env: JNIEnv, _class: JClass) {
    system_files::push_paused();
}

/// The on-screen keyboard is covering this many pixels of the bottom of the window (`0` when it is away).
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_inkwell_InkActivity_nativeKeyboard(
    _env: JNIEnv,
    _class: JClass,
    bottom: jint,
) {
    ink_app::set_keyboard_pixels(bottom as f32);
}

/// Back was pressed: whether the app takes it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_inkwell_InkActivity_nativeBack(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    jboolean::from(ink_app::request_back())
}
