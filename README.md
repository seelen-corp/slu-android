# Seelen UI Launcher for Android

Android launcher written 100% in Rust (`android-activity` + `ndk` +
`ndk-context`, no Kotlin/Java). The crate lives at the repo root; `android/`
is a standard Gradle project that cross-compiles it with `cargo-ndk` and
packages it into an APK.

```
slu-android/
├── Cargo.toml, src/lib.rs   # the Rust crate
└── android/                 # the Gradle project
    ├── app/                 # Android application module
    ├── gradlew, gradlew.bat # Gradle wrapper (no local Gradle install needed)
    └── release.keystore     # dev signing key (gitignored)
```

## Requirements

- Rust targets: `aarch64-linux-android`, `x86_64-linux-android`
  (`rustup target add aarch64-linux-android x86_64-linux-android`)
- `cargo-ndk` (`cargo install cargo-ndk`)
- Android SDK + NDK, with `ANDROID_HOME` and `ANDROID_NDK_HOME` set
- JDK 21 (JDK 25/the Android Studio JBR does **not** work — AGP's `d8`/R8
  doesn't support its class file version)

```powershell
$env:JAVA_HOME = "C:\Program Files\Eclipse Adoptium\jdk-21.0.12.101-hotspot"
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
$env:ANDROID_NDK_HOME = "$env:ANDROID_HOME\ndk\26.3.11579264"
```

## Build

```powershell
cd android
.\gradlew.bat assembleDebug      # fast, debuggable
.\gradlew.bat assembleRelease    # optimized, signed with release.keystore
```

This runs `cargo ndk` against the crate at the repo root, drops the
resulting `.so` files into `android/app/src/main/jniLibs/`, then has Gradle
package and sign the APK. Output:

- `android/app/build/outputs/apk/debug/app-debug.apk`
- `android/app/build/outputs/apk/release/app-release.apk`

## Run

If you have a physical device with USB debugging enabled, plug it in and
skip straight to `installDebug` below. Otherwise, use an emulator (AVD):

### 1. Create the AVD (optional, one-time)

```powershell
& "$env:ANDROID_HOME\emulator\emulator.exe" -list-avds   # check if one already exists
& "$env:ANDROID_HOME\cmdline-tools\latest\bin\avdmanager.bat" create avd `
  -n slu_launcher_avd `
  -k "system-images;android-34;google_apis;x86_64" `
  -d pixel_6
```

(Install the system image first with `sdkmanager` if it's missing:
`sdkmanager "system-images;android-34;google_apis;x86_64"`.)

### 2. Start the emulator (optional if a device is already connected)

```powershell
& "$env:ANDROID_HOME\emulator\emulator.exe" -avd slu_launcher_avd
```

### 3. Install and launch

```powershell
.\gradlew.bat installDebug
& "$env:ANDROID_HOME\platform-tools\adb.exe" shell am start -n com.seelen.slu_launcher/android.app.NativeActivity
```

Logs:

```
adb logcat -s slu_launcher
```

### Troubleshooting

If `installDebug` fails with `INSTALL_FAILED_UPDATE_INCOMPATIBLE`, the device
already has a build installed with a different signing key (e.g. you
previously installed a release build). Uninstall it first, then reinstall:

```powershell
& "$env:ANDROID_HOME\platform-tools\adb.exe" uninstall com.seelen.slu_launcher
.\gradlew.bat installDebug
```
