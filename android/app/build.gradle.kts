plugins {
    id("com.android.application")
}

android {
    namespace = "com.seelen.slu_launcher"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.seelen.slu_launcher"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
    }

    signingConfigs {
        create("release") {
            storeFile = rootProject.file("release.keystore")
            storePassword = "slu_launcher"
            keyAlias = "slu_launcher"
            keyPassword = "slu_launcher"
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
        "-P", "${android.compileSdk}",
        "-o", jniLibsDir.asFile.absolutePath,
        "build",
    )
    if (isRelease) args += "--release"

    commandLine(args)
}

tasks.named("preBuild") {
    dependsOn(cargoNdkBuild)
}
