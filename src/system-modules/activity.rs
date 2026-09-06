//! Helpers for identifying which Android Activity subclass launched this
//! process, and for starting other Activities declared in the manifest. A
//! single `android_main` backs every `<activity>` in the manifest, so this
//! is how the caller picks which Slint window to show.

use jni::objects::{JString, JValue};

use crate::error::Result;
use crate::system_modules::android_ctx;

const PACKAGE: &str = "com.seelen.slu_launcher";

/// `Intent.FLAG_ACTIVITY_NEW_TASK`. Required for `SettingsActivity` to start
/// in its own task (its `taskAffinity` in the manifest differs from Home's)
/// instead of being pushed onto whichever task is currently in front.
const FLAG_ACTIVITY_NEW_TASK: i32 = 0x10000000;

/// The simple class name of the Activity that launched this process (e.g.
/// "NativeActivity" or "SettingsActivity").
pub fn current_class_name() -> Result<String> {
    let ctx = android_ctx::get()?;
    let mut env = ctx.vm.attach_current_thread()?;

    let class = env
        .call_method(
            ctx.activity.as_obj(),
            "getClass",
            "()Ljava/lang/Class;",
            &[],
        )?
        .l()?;
    let name_obj = env
        .call_method(&class, "getSimpleName", "()Ljava/lang/String;", &[])?
        .l()?;
    let name: JString = name_obj.into();
    Ok(env.get_string(&name)?.into())
}

/// Starts `SettingsActivity` via an explicit Intent. Since it's a regular
/// (non-HOME) task, it shows up in the Recents/multitasking view.
pub fn open_settings() -> Result<()> {
    let ctx = android_ctx::get()?;
    let mut env = ctx.vm.attach_current_thread()?;

    let intent_class = env.find_class("android/content/Intent")?;
    let intent = env.new_object(intent_class, "()V", &[])?;

    let component_class = env.find_class("android/content/ComponentName")?;
    let package_name = env.new_string(PACKAGE)?;
    let class_name = env.new_string(format!("{PACKAGE}.SettingsActivity"))?;
    let component = env.new_object(
        component_class,
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[JValue::Object(&package_name), JValue::Object(&class_name)],
    )?;

    env.call_method(
        &intent,
        "setComponent",
        "(Landroid/content/ComponentName;)Landroid/content/Intent;",
        &[JValue::Object(&component)],
    )?;
    env.call_method(
        &intent,
        "addFlags",
        "(I)Landroid/content/Intent;",
        &[JValue::Int(FLAG_ACTIVITY_NEW_TASK)],
    )?;
    env.call_method(
        ctx.activity.as_obj(),
        "startActivity",
        "(Landroid/content/Intent;)V",
        &[JValue::Object(&intent)],
    )?;
    Ok(())
}
