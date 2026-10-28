//! DeckCraft for Android.
//!
//! The same [`deckcraft_ui_egui::SlideApp`] as the desktop and web apps, run by eframe inside a
//! `NativeActivity` (`ai.storyteller.deckcraft.MainActivity`, see `android/`). Rendering goes
//! through wgpu on the GPU: Vulkan where the device has it, OpenGL ES 3 otherwise.
//!
//! Differences from the desktop app:
//! - File › Open, Insert › Pictures/Audio/Video and Save a Copy/Export use an in-app, touch-sized
//!   file browser over shared storage ([`browser`]); files arrive through `Services::inbox`, as on
//!   the web. It needs "All files access", which the activity asks for on first start;
//! - preferences, AutoRecover and the log live in the app's private data folder; the log goes to
//!   logcat (`adb logcat -s DeckCraft`);
//! - no TCP control channel, self-updater or system clipboard images.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

pub mod browser;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
mod audio;
#[cfg(target_os = "android")]
mod jni_bridge;

/// Entry point `NativeActivity` calls (through android-activity) on its own native thread.
#[cfg(target_os = "android")]
#[allow(unsafe_code)] // the symbol must be unmangled for android-activity to find it
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android::run(app);
}
