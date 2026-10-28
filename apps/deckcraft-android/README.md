# DeckCraft for Android

DeckCraft running natively on Android phones and tablets: the same Rust app as the desktop
(`deckcraft-ui-egui` on `deckcraft-engine`), started by a `NativeActivity`.

| | |
|---|---|
| **GPU** | eframe renders through **wgpu** on the GPU: **Vulkan** where the device has it, **OpenGL ES 3** otherwise (`android:hardwareAccelerated`, high-performance adapter). The GPU in use is logged at start (`adb logcat -s DeckCraft`). |
| **Files** | DeckCraft asks for **All files access** (`MANAGE_EXTERNAL_STORAGE`, Android 11+) or the storage permissions (Android 8–10) on first start. File › Open, Insert › Pictures/Audio/Video and Save a Copy/Export use an in-app, touch-sized file browser over shared storage and SD cards. |
| **Sound** | Slide-show sound and media play through AAudio (cpal). |
| **Data** | Preferences and AutoRecover live in the app's private folder, so unsaved work survives Android ending the app in the background. |
| **Requires** | Android 8.0 (API 26) or later, OpenGL ES 3.0; arm64-v8a (phones/tablets) or x86_64 (emulator). |

Not on Android (yet): the TCP control channel, the self-updater, system clipboard images and
opening a file from another app's "Open with".

## Layout

```
apps/deckcraft-android/
├── Cargo.toml            # cdylib `libdeckcraft_android.so`
├── src/
│   ├── lib.rs            # `android_main` entry point
│   ├── android.rs        # shell: logging, GPU setup, Services, eframe runner
│   ├── browser.rs        # in-app file browser (platform-independent, unit-tested)
│   ├── audio.rs          # cpal/AAudio output
│   └── jni_bridge.rs     # one JNI call: MainActivity.requestStorageAccess()
└── android/              # Gradle project (Java NativeActivity, manifest)
```

## Building

You need the Android SDK (platform 35, build-tools 35) and NDK r27c (`27.2.12479018`), JDK 17+,
Gradle 8.9+ and the Rust Android targets and `cargo-ndk`:

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
export ANDROID_HOME=$HOME/Android/Sdk            # Windows: $env:ANDROID_HOME = "C:\Android\Sdk"
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.2.12479018
```

Optionally clone [craft-fonts](https://github.com/storytold/craft-fonts) next to this repository
(`../craft-fonts`) so the presentation fonts are embedded; without it DeckCraft uses Android's
system fonts (`/system/fonts`).

Then, from `apps/deckcraft-android/android`:

```sh
gradle assembleRelease                              # both ABIs
gradle assembleRelease -Pdeckcraft.abis=arm64-v8a   # phones only, faster
adb install -r app/build/outputs/apk/release/app-release.apk
```

Gradle runs `cargo ndk … build --release -p deckcraft-android` itself and packages the library.
The release APK is signed with the debug key so it installs directly; sign it with your own key
before publishing.

On Windows with the `x86_64-pc-windows-gnu` Rust toolchain, put the toolchain's `dlltool.exe` on
`PATH` first (`$(rustc --print sysroot)\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained`).

## Testing

The file browser has host tests: `cargo test -p deckcraft-android`.
