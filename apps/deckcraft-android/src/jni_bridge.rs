//! The one call from Rust into the Java activity: asking for file access again from the file
//! browser ("Grant file access"). `MainActivity.requestStorageAccess()` opens the system's
//! "All files access" page on Android 11+ and the runtime storage prompt before that.
#![allow(unsafe_code)] // raw JNI handles from android-activity; see the SAFETY notes

use jni::JavaVM;
use jni::objects::JObject;
use winit::platform::android::activity::AndroidApp;

/// Ask the activity to show the storage permission screen; failures are logged.
pub fn request_storage_access(app: &AndroidApp) {
    if let Err(e) = call_activity_void(app, "requestStorageAccess") {
        log::warn!("couldn't ask for file access: {e}");
    }
}

/// Call the no-argument `void` method `name` on the running activity.
fn call_activity_void(app: &AndroidApp, name: &str) -> jni::errors::Result<()> {
    // SAFETY: android-activity hands out the process's `JavaVM*`, valid for the life of the
    // process; `JavaVM` doesn't destroy the VM when dropped.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    // SAFETY: a global reference to the running Activity, owned by android-activity and kept
    // alive while the app runs. `JObject` never deletes the reference it wraps.
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    // Permanently: this is eframe's thread, which other JNI users (cpal, ndk-context) share.
    let mut env = vm.attach_current_thread_permanently()?;
    let result = env.call_method(&activity, name, "()V", &[]);
    if result.is_err() && env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    result.map(|_| ())
}
