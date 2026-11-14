plugins {
    id("com.android.application")
}

// Cargo.toml is the single source of truth for the app version (the release
// workflow checks the pushed vX.Y.Z tag against it).
val cargoVersion: String = run {
    val pkg = rootProject.file("../Cargo.toml").readText()
        .substringAfter("[package]").substringBefore("\n[")
    Regex("""(?m)^version\s*=\s*"([^"]+)"""").find(pkg)?.groupValues?.get(1)
        ?: error("version not found in Cargo.toml [package]")
}
val cargoVersionCode: Int = run {
    val (major, minor, patch) = cargoVersion.substringBefore('-').split('.').map { it.toInt() }
    major * 10000 + minor * 100 + patch
}

android {
    namespace = "com.seelen.slu_launcher"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.seelen.slu_launcher"
        minSdk = 26
        targetSdk = 34
        versionCode = cargoVersionCode
        versionName = cargoVersion
    }

    signingConfigs {
        create("release") {
            // CI injects the production key via env vars; locally it falls
            // back to the gitignored dev keystore.
            storeFile = System.getenv("SLU_KEYSTORE_PATH")?.let { file(it) }
                ?: rootProject.file("release.keystore")
            storePassword = System.getenv("SLU_KEYSTORE_PASSWORD") ?: "slu_launcher"
            keyAlias = System.getenv("SLU_KEY_ALIAS") ?: "slu_launcher"
            keyPassword = System.getenv("SLU_KEY_PASSWORD") ?: "slu_launcher"
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("release")
        }
    }
}

// Builds the Rust crate (the repo root, one level above android/) into a
// .so per ABI with cargo-ndk and drops them straight into src/main/jniLibs,
// where AGP picks them up automatically like any other prebuilt native
// library.
val jniLibsDir = layout.projectDirectory.dir("src/main/jniLibs")

val cargoNdkBuild = tasks.register<Exec>("cargoNdkBuild") {
    workingDir = rootProject.file("..")

    // Mirrors the variant currently being assembled so debug/release Rust
    // optimizations match the Android build type (rust-android-gradle uses
    // this same taskNames-sniffing trick instead of full per-variant wiring).
    val isRelease = gradle.startParameter.taskNames.any { it.contains("Release") }

    val args = mutableListOf(
        "cargo", "ndk",
        "-t", "arm64-v8a",
        "-t", "x86_64",
        // Native code links against minSdk, so NDK symbols newer than it fail
        // the build instead of crashing on old devices.
        "-P", "${android.defaultConfig.minSdk}",
        "-o", jniLibsDir.asFile.absolutePath,
        "build",
    )
    if (isRelease) args += "--release"

    // Slint's build script compiles Java against the android.jar of the -P
    // platform, which isn't installed: point it to the compileSdk one.
    val sdkDir = androidComponents.sdkComponents.sdkDirectory.get().asFile
    environment("ANDROID_JAR", sdkDir.resolve("platforms/android-${android.compileSdk}/android.jar").absolutePath)

    commandLine(args)
}

tasks.named("preBuild") {
    dependsOn(cargoNdkBuild)
}
