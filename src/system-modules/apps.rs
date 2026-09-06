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
    pub title: String,
}

pub type SubscriptionId = u64;

static NEXT_SUBSCRIPTION_ID: AtomicU64 = AtomicU64::new(1);
static SUBSCRIBERS: Mutex<Vec<(SubscriptionId, Box<dyn Fn() + Send + 'static>)>> =
    Mutex::new(Vec::new());

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

/// Registers a callback invoked whenever the installed-apps set changes
/// (install, uninstall, or update). The callback runs on the JNI/broadcast
/// thread, not the UI thread — dispatch UI updates yourself, e.g. via
/// `slint::invoke_from_event_loop`.
pub fn subscribe<F>(callback: F) -> SubscriptionId
where
    F: Fn() + Send + 'static,
{
    let id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
    SUBSCRIBERS.lock().unwrap().push((id, Box::new(callback)));
    id
}

pub fn unsubscribe(id: SubscriptionId) {
    SUBSCRIBERS
        .lock()
        .unwrap()
        .retain(|(sub_id, _)| *sub_id != id);
}

fn notify_subscribers() {
    for (_, callback) in SUBSCRIBERS.lock().unwrap().iter() {
        callback();
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
            title,
        });
    }

    apps.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(apps)
}

/// Called from `PackageChangeReceiver.onReceive` via JNI whenever a package
/// is added, removed, or replaced.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_seelen_slu_1launcher_PackageChangeReceiver_nativeOnPackageChanged(
    _env: JNIEnv,
    _class: JClass,
) {
    notify_subscribers();
}
