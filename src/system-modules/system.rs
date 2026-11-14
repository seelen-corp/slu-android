//! System actions of the launcher: wallpaper picker, default launcher settings,
//! notification shade, haptics, system bar contrast.

use jni::JNIEnv;
use jni::objects::{JObject, JValue};

use crate::error::Result;
use crate::system_modules::android_ctx;

/// `Intent.FLAG_ACTIVITY_NEW_TASK`.
const FLAG_ACTIVITY_NEW_TASK: i32 = 0x10000000;

/// Opens the system wallpaper picker (`Intent.ACTION_SET_WALLPAPER`).
pub fn open_wallpaper_picker() -> Result<()> {
    start_action("android.intent.action.SET_WALLPAPER")
}

/// Opens `url` in the browser (`Intent.ACTION_VIEW`).
pub fn open_url(url: &str) -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        let url = env.new_string(url)?;
        let uri = env
            .call_static_method(
                "android/net/Uri",
                "parse",
                "(Ljava/lang/String;)Landroid/net/Uri;",
                &[JValue::Object(&url)],
            )?
            .l()?;
        let intent = new_intent(env, "android.intent.action.VIEW")?;
        env.call_method(
            &intent,
            "setData",
            "(Landroid/net/Uri;)Landroid/content/Intent;",
            &[JValue::Object(&uri)],
        )?;
        env.call_method(
            &intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(FLAG_ACTIVITY_NEW_TASK)],
        )?;
        env.call_method(
            activity,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(&intent)],
        )?;
        Ok(())
    })
}

/// Opens the system "Default home app" settings
/// (`Settings.ACTION_HOME_SETTINGS`).
pub fn open_default_launcher_settings() -> Result<()> {
    start_action("android.settings.HOME_SETTINGS")
}

/// Opens the system "Notification access" settings, where the user allows the
/// launcher to read notifications (badges, see `badges.rs`)
/// (`Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS`).
pub fn open_notification_access_settings() -> Result<()> {
    start_action("android.settings.ACTION_NOTIFICATION_LISTENER_SETTINGS")
}

/// Pulls down the notification shade. `StatusBarManager.expandNotificationsPanel`
/// is not public API, but it's allowed for apps (with `EXPAND_STATUS_BAR`) and
/// it's what launchers use for the swipe down on Home.
pub fn expand_notifications() -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        let name = env.new_string("statusbar")?;
        let service = env
            .call_method(
                activity,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::Object(&name)],
            )?
            .l()?;
        if service.is_null() {
            return Err("no statusbar service".into());
        }
        env.call_method(&service, "expandNotificationsPanel", "()V", &[])?;
        Ok(())
    })
}

/// Long-press haptic feedback (`SluNativeActivity.performLongPressHaptic`).
pub fn long_press_haptic() -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        env.call_method(activity, "performLongPressHaptic", "()V", &[])?;
        Ok(())
    })
}

/// Dark system bar icons (light content behind the bars) or light ones
/// (`SluNativeActivity.setLightSystemBars`).
pub fn set_light_system_bars(light: bool) -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        env.call_method(
            activity,
            "setLightSystemBars",
            "(Z)V",
            &[JValue::Bool(light.into())],
        )?;
        Ok(())
    })
}

fn start_action(action: &str) -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        let intent = new_intent(env, action)?;
        env.call_method(
            &intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(FLAG_ACTIVITY_NEW_TASK)],
        )?;
        env.call_method(
            activity,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(&intent)],
        )?;
        Ok(())
    })
}

fn new_intent<'local>(env: &mut JNIEnv<'local>, action: &str) -> Result<JObject<'local>> {
    let action = env.new_string(action)?;
    Ok(env.new_object(
        "android/content/Intent",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&action)],
    )?)
}
