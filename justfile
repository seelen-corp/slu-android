set windows-shell := ["powershell.exe", "-NoProfile", "-Command"]

android_home := env_var_or_default("ANDROID_HOME", env_var("LOCALAPPDATA") / "Android" / "Sdk")
avd_name := "slu_launcher_avd"
package := "com.seelen.slu_launcher"
activity := package + "/android.app.NativeActivity"
apk := "android/app/build/outputs/apk/debug/app-debug.apk"
adb := android_home / "platform-tools" / "adb.exe"
emulator := android_home / "emulator" / "emulator.exe"

default:
    @just --list

# build the debug apk
build:
    cd android; .\gradlew.bat assembleDebug

# build the signed release apk
release:
    cd android; .\gradlew.bat assembleRelease

# start the avd emulator
emu:
    & "{{emulator}}" -avd {{avd_name}}

# create the avd (one-time)
avd:
    & "{{android_home}}\cmdline-tools\latest\bin\avdmanager.bat" create avd -n {{avd_name}} -k "system-images;android-34;google_apis;x86_64" -d pixel_6

# install the debug apk on the connected device/emulator
install:
    & "{{adb}}" install -r {{apk}}

# launch the app
start:
    & "{{adb}}" shell am start -n {{activity}}

# uninstall the app
uninstall:
    & "{{adb}}" uninstall {{package}}

# tail app logs
log:
    & "{{adb}}" logcat -s slu_launcher

# install the debug build (build if needed) and launch it
run:
    cd android; .\gradlew.bat installDebug
    & "{{adb}}" shell am start -n {{activity}}
