// DeckCraft for Android: a thin Java NativeActivity around the Rust library
// (`libdeckcraft_android.so`, built by `cargo ndk` from `apps/deckcraft-android`).
//
//   ./gradlew assembleRelease            (or `gradle assembleRelease` without the wrapper)
//   -Pdeckcraft.abis=arm64-v8a           build one ABI only (faster)
//   -Pdeckcraft.skipCargo=true           package .so files already in build/rustJniLibs

plugins {
    id("com.android.application")
}

val repoRoot: File = rootDir.resolve("../../..").canonicalFile
val rustJniLibs: File = layout.buildDirectory.dir("rustJniLibs").get().asFile
val generatedRes: File = layout.buildDirectory.dir("generated/deckcraftRes").get().asFile
val abis: List<String> = (findProperty("deckcraft.abis") as String? ?: "arm64-v8a,x86_64")
    .split(',').map { it.trim() }.filter { it.isNotEmpty() }
val skipCargo: Boolean = (findProperty("deckcraft.skipCargo") as String?).toBoolean()

android {
    namespace = "ai.storyteller.deckcraft"
    compileSdk = 35
    ndkVersion = "27.2.12479018"

    defaultConfig {
        applicationId = "ai.storyteller.deckcraft"
        // 26: AAudio (slide-show sound) and Vulkan 1.0 are both available from Android 8.0.
        minSdk = 26
        // 34 keeps the classic (non edge-to-edge) window, so the UI never sits under the status
        // and navigation bars.
        targetSdk = 34
        versionCode = 1101
        versionName = "0.11.1"
        ndk { abiFilters += abis }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Signed with the debug key so the APK installs as is; use your own key to publish.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDir(rustJniLibs)
            res.srcDir(generatedRes)
        }
    }

    packaging {
        // Compressed in the APK (the library embeds the presentation fonts): a much smaller
        // download, unpacked once at install.
        jniLibs { useLegacyPackaging = true }
    }
}

// The launcher icon comes from the repository's icon set, so the Android build never carries a
// second copy of it.
val copyLauncherIcons by tasks.registering(Copy::class) {
    val icons = repoRoot.resolve("assets/app-icon/hicolor")
    mapOf("mdpi" to 48, "hdpi" to 64, "xhdpi" to 128, "xxhdpi" to 256, "xxxhdpi" to 256).forEach { (density, size) ->
        from(icons.resolve("${size}x$size/apps/ai.storyteller.deckcraft.png")) {
            into("mipmap-$density")
            rename { "ic_launcher.png" }
        }
    }
    into(generatedRes)
}

val cargoBuild by tasks.registering(Exec::class) {
    description = "Builds libdeckcraft_android.so for ${abis.joinToString()} with cargo-ndk"
    workingDir = repoRoot
    val args = mutableListOf("cargo", "ndk", "--platform", "26")
    abis.forEach { args += listOf("-t", it) }
    args += listOf("-o", rustJniLibs.absolutePath, "build", "--release", "-p", "deckcraft-android")
    commandLine(args)
    onlyIf { !skipCargo }
}

tasks.named("preBuild") {
    dependsOn(copyLauncherIcons, cargoBuild)
}
