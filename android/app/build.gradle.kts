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

// Number of commits up to HEAD. It only grows on master, so it is the
// versionCode of every build (Play requires it to increase on each upload) and
// the 4th component of nightly versions. Needs the full history: CI checks out
// with `fetch-depth: 0`.
val commitNumber: Int = run {
    fun git(vararg args: String): String = providers.exec {
        workingDir = rootProject.file("..")
        commandLine("git", *args)
    }.standardOutput.asText.get().trim()

    if (git("rev-parse", "--is-shallow-repository") == "true") {
        error("shallow git clone: the versionCode needs the full history (fetch-depth: 0)")
    }
    git("rev-list", "--count", "HEAD").toInt()
}

// Releases are "X.Y.Z"; nightlies (SLU_NIGHTLY=true) are "X.Y.Z.<commit number>".
val appVersionName: String =
    if (System.getenv("SLU_NIGHTLY") == "true") "$cargoVersion.$commitNumber" else cargoVersion

// AdMob ids (Settings only, see Ads.java). Not secret: they ship inside the
// APK anyway. Debug builds always use Google's test ids (real ads must never
// be clicked while developing).
object AdMobTestIds {
    const val APP = "ca-app-pub-3940256099942544~3347511713"
    const val BANNER = "ca-app-pub-3940256099942544/9214589741"
    const val REWARDED = "ca-app-pub-3940256099942544/5224354917"
}

object AdMobIds {
    const val APP = "ca-app-pub-4459622307906677~8893870313"
    const val BANNER = "ca-app-pub-4459622307906677/7489289302"
    const val REWARDED = "ca-app-pub-4459622307906677/1519932274"
}

android {
    namespace = "com.seelen.slu_launcher"
    // 36 (Android 16) is the minimum target Google Play accepts for new apps
    // and updates (since 2026-08-31).
    compileSdk = 36

    defaultConfig {
        applicationId = "com.seelen.slu_launcher"
        minSdk = 26
        targetSdk = 36
        versionCode = commitNumber
        versionName = appVersionName
    }

    buildFeatures {
        buildConfig = true
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
        debug {
            manifestPlaceholders["admobAppId"] = AdMobTestIds.APP
            buildConfigField("String", "ADMOB_BANNER_ID", "\"${AdMobTestIds.BANNER}\"")
            buildConfigField("String", "ADMOB_REWARDED_ID", "\"${AdMobTestIds.REWARDED}\"")
        }
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("release")
            manifestPlaceholders["admobAppId"] = AdMobIds.APP
            buildConfigField("String", "ADMOB_BANNER_ID", "\"${AdMobIds.BANNER}\"")
            buildConfigField("String", "ADMOB_REWARDED_ID", "\"${AdMobIds.REWARDED}\"")
        }
    }
}

dependencies {
    // Ads of the Settings screen (Ads.java) and their consent form (GDPR).
    implementation("com.google.android.gms:play-services-ads:23.6.0")
    implementation("com.google.android.ump:user-messaging-platform:3.1.0")
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
