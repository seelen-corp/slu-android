//! Installed-apps system module: get the current list, and subscribe to
//! changes (install/uninstall/update) via a JNI-registered BroadcastReceiver.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString, JValue};

use crate::error::Result;
use crate::system_modules::android_ctx;

/// The Java package/class of the BroadcastReceiver declared in
/// `android/app/src/main/java/com/seelen/slu_launcher/PackageChangeReceiver.java`.
const RECEIVER_CLASS: &str = "com/seelen/slu_launcher/PackageChangeReceiver";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    pub package_name: String,
    /// Fully-qualified class name of the launcher Activity. A package may
    /// expose several launcher entries, so this (not just the package) is
    /// what identifies the item.
    pub activity_name: String,
    pub title: String,
    /// False for system apps, which the user can't uninstall.
    pub uninstallable: bool,
    /// `ApplicationInfo.category` (`CATEGORY_GAME` also for apps flagged as
    /// games), declared by the app itself; `CATEGORY_UNDEFINED` (-1) if not.
    pub category: i32,
}

/// `ApplicationInfo.FLAG_SYSTEM`.
const FLAG_SYSTEM: i32 = 1;
/// `ApplicationInfo.FLAG_IS_GAME` (deprecated in favor of the category, still
/// set by older games).
const FLAG_IS_GAME: i32 = 1 << 25;
/// `ApplicationInfo.CATEGORY_GAME`.
pub const CATEGORY_GAME: i32 = 0;
/// `ApplicationInfo.CATEGORY_SOCIAL`.
pub const CATEGORY_SOCIAL: i32 = 4;
/// `PackageManager.MATCH_DEFAULT_ONLY`.
const MATCH_DEFAULT_ONLY: i32 = 0x0001_0000;
/// Package of the system's "choose an app" dialog, what `resolveActivity`
/// returns when there's no default handler.
const RESOLVER_PACKAGE: &str = "android";

/// A job usually done by one app, picked by the user or preinstalled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Phone,
    Messages,
    Browser,
    Camera,
    Gallery,
    Clock,
}

/// `Intent.FLAG_ACTIVITY_NEW_TASK`: required when starting an Activity of
/// another app from the launcher.
const FLAG_ACTIVITY_NEW_TASK: i32 = 0x10000000;
/// `Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED`: brings the app's existing task
/// to the front instead of stacking a new instance, like the stock launcher.
const FLAG_ACTIVITY_RESET_TASK_IF_NEEDED: i32 = 0x00200000;

pub type SubscriptionId = u64;

static NEXT_SUBSCRIPTION_ID: AtomicU64 = AtomicU64::new(1);
type Subscriber = (SubscriptionId, Box<dyn Fn(&PackageChange) + Send + 'static>);

/// A change of the installed-apps set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageChange {
    /// A package was just installed (not an update of an installed one).
    Installed(String),
    /// Anything else: uninstall, update, enable/disable...
    Other,
}

static SUBSCRIBERS: Mutex<Vec<Subscriber>> = Mutex::new(Vec::new());

/// Must be called once from `android_main`, after `android_ctx::init`.
/// Registers the BroadcastReceiver used to detect package changes.
pub fn init() -> Result<()> {
    let ctx = android_ctx::get()?;
    let mut env = ctx.vm.attach_current_thread()?;
    register_package_receiver(&mut env, ctx.activity.as_obj(), ctx.class_loader.as_obj())?;
    Ok(())
}

/// Synchronously queries the PackageManager for every launchable app for the
/// current user, sorted by title.
pub fn get_installed_apps() -> Result<Vec<AppInfo>> {
    let ctx = android_ctx::get()?;
    let mut env = ctx.vm.attach_current_thread()?;
    query_launchable_apps(&mut env, ctx.activity.as_obj())
}

/// Starts the given launcher Activity (as returned by `get_installed_apps`)
/// the same way the system launcher does.
pub fn launch(package_name: &str, activity_name: &str) -> Result<()> {
    let ctx = android_ctx::get()?;
    let mut env = ctx.vm.attach_current_thread()?;

    let intent_class = env.find_class("android/content/Intent")?;
    let action = env.new_string("android.intent.action.MAIN")?;
    let intent = env.new_object(
        intent_class,
        "(Ljava/lang/String;)V",
        &[JValue::Object(&action)],
    )?;
    let category = env.new_string("android.intent.category.LAUNCHER")?;
    env.call_method(
        &intent,
        "addCategory",
        "(Ljava/lang/String;)Landroid/content/Intent;",
        &[JValue::Object(&category)],
    )?;

    let package = env.new_string(package_name)?;
    let class = env.new_string(activity_name)?;
    env.call_method(
        &intent,
        "setClassName",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
        &[JValue::Object(&package), JValue::Object(&class)],
    )?;
    env.call_method(
        &intent,
        "addFlags",
        "(I)Landroid/content/Intent;",
        &[JValue::Int(
            FLAG_ACTIVITY_NEW_TASK | FLAG_ACTIVITY_RESET_TASK_IF_NEEDED,
        )],
    )?;

    start_activity(&mut env, ctx.activity.as_obj(), &intent)
}

/// Opens the system "App info" screen of the given package.
pub fn open_app_info(package_name: &str) -> Result<()> {
    let ctx = android_ctx::get()?;
    let mut env = ctx.vm.attach_current_thread()?;
    let intent = package_intent(
        &mut env,
        "android.settings.APPLICATION_DETAILS_SETTINGS",
        package_name,
    )?;
    env.call_method(
        &intent,
        "addFlags",
        "(I)Landroid/content/Intent;",
        &[JValue::Int(FLAG_ACTIVITY_NEW_TASK)],
    )?;
    start_activity(&mut env, ctx.activity.as_obj(), &intent)
}

/// Asks the system to uninstall the given package. Android shows its own
/// confirmation dialog; the app list refreshes through the package receiver.
/// Needs `REQUEST_DELETE_PACKAGES` in the manifest.
pub fn uninstall(package_name: &str) -> Result<()> {
    let ctx = android_ctx::get()?;
    let mut env = ctx.vm.attach_current_thread()?;
    let intent = package_intent(&mut env, "android.intent.action.DELETE", package_name)?;
    start_activity(&mut env, ctx.activity.as_obj(), &intent)
}

/// Package of the app that does `role`: the default one if the user (or the
/// system) picked it, otherwise the first that can. `None` if no app can.
pub fn role_package(role: Role) -> Result<Option<String>> {
    android_ctx::with_activity(|env, activity| {
        let pm = env
            .call_method(
                activity,
                "getPackageManager",
                "()Landroid/content/pm/PackageManager;",
                &[],
            )?
            .l()?;
        for intent in role_intents(env, role)? {
            if let Some(package) = resolve_package(env, &pm, &intent)? {
                return Ok(Some(package));
            }
        }
        Ok(None)
    })
}

/// Intents only handled by apps doing `role`, most reliable first.
fn role_intents<'local>(env: &mut JNIEnv<'local>, role: Role) -> Result<Vec<JObject<'local>>> {
    const MAIN: &str = "android.intent.action.MAIN";
    const VIEW: &str = "android.intent.action.VIEW";
    let intents = match role {
        Role::Phone => vec![role_intent(
            env,
            "android.intent.action.DIAL",
            None,
            None,
            None,
        )?],
        Role::Messages => vec![
            role_intent(
                env,
                "android.intent.action.SENDTO",
                Some("smsto:"),
                None,
                None,
            )?,
            role_intent(
                env,
                MAIN,
                None,
                None,
                Some("android.intent.category.APP_MESSAGING"),
            )?,
        ],
        Role::Browser => vec![
            role_intent(
                env,
                VIEW,
                Some("https://"),
                None,
                Some("android.intent.category.BROWSABLE"),
            )?,
            role_intent(
                env,
                MAIN,
                None,
                None,
                Some("android.intent.category.APP_BROWSER"),
            )?,
        ],
        Role::Camera => vec![
            role_intent(
                env,
                "android.media.action.STILL_IMAGE_CAMERA",
                None,
                None,
                None,
            )?,
            role_intent(env, "android.media.action.IMAGE_CAPTURE", None, None, None)?,
        ],
        Role::Gallery => vec![
            role_intent(
                env,
                MAIN,
                None,
                None,
                Some("android.intent.category.APP_GALLERY"),
            )?,
            role_intent(
                env,
                VIEW,
                Some("content://media/external/images/media/1"),
                Some("image/*"),
                None,
            )?,
        ],
        // `AlarmClock.ACTION_SHOW_ALARMS`, then `ACTION_SET_ALARM`.
        Role::Clock => vec![
            role_intent(env, "android.intent.action.SHOW_ALARMS", None, None, None)?,
            role_intent(env, "android.intent.action.SET_ALARM", None, None, None)?,
        ],
    };
    Ok(intents)
}

/// `new Intent(action)` with an optional data URI, MIME type and category.
fn role_intent<'local>(
    env: &mut JNIEnv<'local>,
    action: &str,
    data: Option<&str>,
    mime: Option<&str>,
    category: Option<&str>,
) -> Result<JObject<'local>> {
    let action = env.new_string(action)?;
    let intent = env.new_object(
        "android/content/Intent",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&action)],
    )?;
    let uri = match data {
        Some(data) => {
            let data = env.new_string(data)?;
            env.call_static_method(
                "android/net/Uri",
                "parse",
                "(Ljava/lang/String;)Landroid/net/Uri;",
                &[JValue::Object(&data)],
            )?
            .l()?
        }
        None => JObject::null(),
    };
    if let Some(mime) = mime {
        let mime = env.new_string(mime)?;
        env.call_method(
            &intent,
            "setDataAndType",
            "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&uri), JValue::Object(&mime)],
        )?;
    } else if !uri.is_null() {
        env.call_method(
            &intent,
            "setData",
            "(Landroid/net/Uri;)Landroid/content/Intent;",
            &[JValue::Object(&uri)],
        )?;
    }
    if let Some(category) = category {
        let category = env.new_string(category)?;
        env.call_method(
            &intent,
            "addCategory",
            "(Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&category)],
        )?;
    }
    Ok(intent)
}

/// The default handler of `intent`, or else the first one (never the system's
/// "choose an app" dialog).
fn resolve_package(env: &mut JNIEnv, pm: &JObject, intent: &JObject) -> Result<Option<String>> {
    let default = env
        .call_method(
            pm,
            "resolveActivity",
            "(Landroid/content/Intent;I)Landroid/content/pm/ResolveInfo;",
            &[JValue::Object(intent), JValue::Int(MATCH_DEFAULT_ONLY)],
        )?
        .l()?;
    if !default.is_null()
        && let Some(package) = activity_package(env, &default)?
    {
        return Ok(Some(package));
    }
    // Category intents (e.g. `APP_GALLERY`) don't match as default ones.
    let list = env
        .call_method(
            pm,
            "queryIntentActivities",
            "(Landroid/content/Intent;I)Ljava/util/List;",
            &[JValue::Object(intent), JValue::Int(0)],
        )?
        .l()?;
    let size = env.call_method(&list, "size", "()I", &[])?.i()?;
    for i in 0..size {
        let info = env
            .call_method(&list, "get", "(I)Ljava/lang/Object;", &[JValue::Int(i)])?
            .l()?;
        if let Some(package) = activity_package(env, &info)? {
            return Ok(Some(package));
        }
    }
    Ok(None)
}

/// Package of a `ResolveInfo`'s activity, unless it's the system resolver.
fn activity_package(env: &mut JNIEnv, resolve_info: &JObject) -> Result<Option<String>> {
    let activity_info = env
        .get_field(
            resolve_info,
            "activityInfo",
            "Landroid/content/pm/ActivityInfo;",
        )?
        .l()?;
    if activity_info.is_null() {
        return Ok(None);
    }
    let package: JString = env
        .get_field(&activity_info, "packageName", "Ljava/lang/String;")?
        .l()?
        .into();
    let package: String = env.get_string(&package)?.into();
    Ok((package != RESOLVER_PACKAGE).then_some(package))
}

/// `new Intent(action, Uri.parse("package:" + package_name))`.
fn package_intent<'local>(
    env: &mut JNIEnv<'local>,
    action: &str,
    package_name: &str,
) -> Result<JObject<'local>> {
    let uri_str = env.new_string(format!("package:{package_name}"))?;
    let uri = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[JValue::Object(&uri_str)],
        )?
        .l()?;
    let action = env.new_string(action)?;
    let intent = env.new_object(
        "android/content/Intent",
        "(Ljava/lang/String;Landroid/net/Uri;)V",
        &[JValue::Object(&action), JValue::Object(&uri)],
    )?;
    Ok(intent)
}

fn start_activity(env: &mut JNIEnv, activity: &JObject, intent: &JObject) -> Result<()> {
    env.call_method(
        activity,
        "startActivity",
        "(Landroid/content/Intent;)V",
        &[JValue::Object(intent)],
    )?;
    Ok(())
}

/// Registers a callback invoked whenever the installed-apps set changes
/// (install, uninstall, or update), with what changed. The callback runs on
/// the JNI/broadcast thread, not the UI thread — dispatch UI updates
/// yourself, e.g. via `slint::invoke_from_event_loop`.
pub fn subscribe<F>(callback: F) -> SubscriptionId
where
    F: Fn(&PackageChange) + Send + 'static,
{
    let id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
    SUBSCRIBERS.lock().unwrap().push((id, Box::new(callback)));
    id
}

#[allow(dead_code)] // public API, not used yet
pub fn unsubscribe(id: SubscriptionId) {
    SUBSCRIBERS
        .lock()
        .unwrap()
        .retain(|(sub_id, _)| *sub_id != id);
}

fn notify_subscribers(change: &PackageChange) {
    for (_, callback) in SUBSCRIBERS.lock().unwrap().iter() {
        callback(change);
    }
}

/// `env.find_class` only searches the boot classloader, which never contains
/// our own app classes when called from a thread attached outside of a
/// normal Java-originated call stack (as is the case here, and in any
/// broadcast/background thread). App-defined classes must instead be
/// resolved via the Activity's own ClassLoader.
fn load_app_class<'local>(
    env: &mut JNIEnv<'local>,
    class_loader: &JObject,
    binary_name: &str,
) -> Result<JClass<'local>> {
    let dotted_name = binary_name.replace('/', ".");
    let name = env.new_string(dotted_name)?;
    let class = env
        .call_method(
            class_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?;
    Ok(class.into())
}

fn register_package_receiver(
    env: &mut JNIEnv,
    activity: &JObject,
    class_loader: &JObject,
) -> Result<()> {
    let receiver_class = load_app_class(env, class_loader, RECEIVER_CLASS)?;
    let receiver = env.new_object(receiver_class, "()V", &[])?;

    let filter_class = env.find_class("android/content/IntentFilter")?;
    let filter = env.new_object(filter_class, "()V", &[])?;

    for action in [
        "android.intent.action.PACKAGE_ADDED",
        "android.intent.action.PACKAGE_REMOVED",
        "android.intent.action.PACKAGE_REPLACED",
    ] {
        let action_str = env.new_string(action)?;
        env.call_method(
            &filter,
            "addAction",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&action_str)],
        )?;
    }
    let scheme = env.new_string("package")?;
    env.call_method(
        &filter,
        "addDataScheme",
        "(Ljava/lang/String;)V",
        &[JValue::Object(&scheme)],
    )?;

    env.call_method(
        activity,
        "registerReceiver",
        "(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;)Landroid/content/Intent;",
        &[JValue::Object(&receiver), JValue::Object(&filter)],
    )?;
    Ok(())
}

fn query_launchable_apps(env: &mut JNIEnv, activity: &JObject) -> Result<Vec<AppInfo>> {
    let pm = env
        .call_method(
            activity,
            "getPackageManager",
            "()Landroid/content/pm/PackageManager;",
            &[],
        )?
        .l()?;

    let intent_class = env.find_class("android/content/Intent")?;
    let action = env.new_string("android.intent.action.MAIN")?;
    let intent = env.new_object(
        intent_class,
        "(Ljava/lang/String;)V",
        &[JValue::Object(&action)],
    )?;

    let category = env.new_string("android.intent.category.LAUNCHER")?;
    env.call_method(
        &intent,
        "addCategory",
        "(Ljava/lang/String;)Landroid/content/Intent;",
        &[JValue::Object(&category)],
    )?;

    let list = env
        .call_method(
            &pm,
            "queryIntentActivities",
            "(Landroid/content/Intent;I)Ljava/util/List;",
            &[JValue::Object(&intent), JValue::Int(0)],
        )?
        .l()?;

    let size = env.call_method(&list, "size", "()I", &[])?.i()?;

    let mut apps = Vec::with_capacity(size.max(0) as usize);
    for i in 0..size {
        let resolve_info = env
            .call_method(&list, "get", "(I)Ljava/lang/Object;", &[JValue::Int(i)])?
            .l()?;

        let activity_info = env
            .get_field(
                &resolve_info,
                "activityInfo",
                "Landroid/content/pm/ActivityInfo;",
            )?
            .l()?;
        let package_name_obj = env
            .get_field(&activity_info, "packageName", "Ljava/lang/String;")?
            .l()?;
        let package_name_jstr: JString = package_name_obj.into();
        let package_name: String = env.get_string(&package_name_jstr)?.into();
        let activity_name_obj = env
            .get_field(&activity_info, "name", "Ljava/lang/String;")?
            .l()?;
        let activity_name_jstr: JString = activity_name_obj.into();
        let activity_name: String = env.get_string(&activity_name_jstr)?.into();
        let application_info = env
            .get_field(
                &activity_info,
                "applicationInfo",
                "Landroid/content/pm/ApplicationInfo;",
            )?
            .l()?;
        let flags = env.get_field(&application_info, "flags", "I")?.i()?;
        let uninstallable = flags & FLAG_SYSTEM == 0;
        let category = match flags & FLAG_IS_GAME {
            0 => env.get_field(&application_info, "category", "I")?.i()?,
            _ => CATEGORY_GAME,
        };

        let label = env
            .call_method(
                &resolve_info,
                "loadLabel",
                "(Landroid/content/pm/PackageManager;)Ljava/lang/CharSequence;",
                &[JValue::Object(&pm)],
            )?
            .l()?;
        let title_obj = env
            .call_method(&label, "toString", "()Ljava/lang/String;", &[])?
            .l()?;
        let title_jstr: JString = title_obj.into();
        let title: String = env.get_string(&title_jstr)?.into();

        apps.push(AppInfo {
            package_name,
            activity_name,
            title,
            uninstallable,
            category,
        });
    }

    apps.sort_by_key(|a| a.title.to_lowercase());
    Ok(apps)
}

/// Called from `PackageChangeReceiver.onReceive` via JNI whenever a package
/// is added, removed, or replaced. `installed_package` is the package just
/// installed (not updated), or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_seelen_slu_1launcher_PackageChangeReceiver_nativeOnPackageChanged(
    mut env: JNIEnv,
    _class: JClass,
    installed_package: JString,
) {
    let change = if installed_package.is_null() {
        PackageChange::Other
    } else {
        match env.get_string(&installed_package) {
            Ok(package) => PackageChange::Installed(package.into()),
            Err(err) => {
                log::error!("failed to read the installed package: {err}");
                PackageChange::Other
            }
        }
    };
    notify_subscribers(&change);
}
