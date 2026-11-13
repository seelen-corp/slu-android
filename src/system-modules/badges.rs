//! Notification badges: how many notifications each app has, as reported by
//! `NotificationBadgeService` (a `NotificationListenerService`, Java). The
//! system only binds it once the user allows the launcher's notification
//! access (`open_access_settings`).

use std::collections::HashMap;
use std::sync::Mutex;

use jni::JNIEnv;
use jni::objects::{JClass, JIntArray, JObjectArray, JString, JValue};

use crate::error::Result;
use crate::system_modules::android_ctx;

/// Notifications by package. Empty while the service is not connected.
static COUNTS: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);

type Listener = Box<dyn Fn() + Send + 'static>;

static LISTENER: Mutex<Option<Listener>> = Mutex::new(None);

/// Notifications of `package_name` (0 without notification access).
pub fn count(package_name: &str) -> u32 {
    COUNTS
        .lock()
        .ok()
        .and_then(|counts| counts.as_ref()?.get(package_name).copied())
        .unwrap_or(0)
}

/// `on_change` runs (on the Java thread that reports them) whenever the counts
/// change. Replaces the previous listener.
pub fn subscribe(on_change: impl Fn() + Send + 'static) {
    if let Ok(mut listener) = LISTENER.lock() {
        *listener = Some(Box::new(on_change));
    }
}

/// The user allowed the launcher's notification access (the listener service
/// is in `Settings.Secure` "enabled_notification_listeners").
pub fn access_granted() -> Result<bool> {
    android_ctx::with_activity(|env, activity| {
        let package: String = {
            let name = env
                .call_method(activity, "getPackageName", "()Ljava/lang/String;", &[])?
                .l()?;
            env.get_string(&JString::from(name))?.into()
        };
        let resolver = env
            .call_method(
                activity,
                "getContentResolver",
                "()Landroid/content/ContentResolver;",
                &[],
            )?
            .l()?;
        let key = env.new_string("enabled_notification_listeners")?;
        let value = env
            .call_static_method(
                "android/provider/Settings$Secure",
                "getString",
                "(Landroid/content/ContentResolver;Ljava/lang/String;)Ljava/lang/String;",
                &[JValue::Object(&resolver), JValue::Object(&key)],
            )?
            .l()?;
        if value.is_null() {
            return Ok(false);
        }
        let value: String = env.get_string(&JString::from(value))?.into();
        // Flattened component names (`package/class`), separated by `:`.
        let prefix = format!("{package}/");
        Ok(value.split(':').any(|component| component.starts_with(&prefix)))
    })
}

fn set_counts(counts: Option<HashMap<String, u32>>) {
    let changed = match COUNTS.lock() {
        Ok(mut current) if *current != counts => {
            *current = counts;
            true
        }
        _ => false,
    };
    if changed && let Ok(listener) = LISTENER.lock() {
        if let Some(listener) = listener.as_ref() {
            listener();
        }
    }
}

fn read_counts(
    env: &mut JNIEnv,
    packages: &JObjectArray,
    counts: &JIntArray,
) -> Result<HashMap<String, u32>> {
    let len = env.get_array_length(packages)?;
    let mut values = vec![0; len as usize];
    env.get_int_array_region(counts, 0, &mut values)?;
    let mut map = HashMap::with_capacity(values.len());
    for (i, count) in values.into_iter().enumerate() {
        let package = env.get_object_array_element(packages, i as i32)?;
        let package: String = env.get_string(&JString::from(package))?.into();
        if count > 0 {
            map.insert(package, count as u32);
        }
    }
    Ok(map)
}

/// Called from `NotificationBadgeService` (Android UI thread) with the
/// notifications of every package that has some.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_seelen_slu_1launcher_NotificationBadgeService_nativeOnBadgesChanged(
    mut env: JNIEnv,
    _class: JClass,
    packages: JObjectArray,
    counts: JIntArray,
) {
    match read_counts(&mut env, &packages, &counts) {
        Ok(counts) => set_counts(Some(counts)),
        Err(err) => log::error!("failed to read the notification badges: {err}"),
    }
}
