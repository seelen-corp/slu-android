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
}

/// `ApplicationInfo.FLAG_SYSTEM`.
const FLAG_SYSTEM: i32 = 1;

/// `Intent.FLAG_ACTIVITY_NEW_TASK`: required when starting an Activity of
/// another app from the launcher.
const FLAG_ACTIVITY_NEW_TASK: i32 = 0x10000000;
/// `Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED`: brings the app's existing task
/// to the front instead of stacking a new instance, like the stock launcher.
const FLAG_ACTIVITY_RESET_TASK_IF_NEEDED: i32 = 0x00200000;

pub type SubscriptionId = u64;

static NEXT_SUBSCRIPTION_ID: AtomicU64 = AtomicU64::new(1);
type Subscriber = (SubscriptionId, Box<dyn Fn() + Send + 'static>);

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

/// Renders the icon of a launcher Activity into a `size`x`size` RGBA buffer
/// (`size` in physical pixels).
pub fn load_icon(
    package_name: &str,
    activity_name: &str,
    size: u32,
) -> Result<slint::SharedPixelBuffer<slint::Rgba8Pixel>> {
    let ctx = android_ctx::get()?;
    let mut env = ctx.vm.attach_current_thread()?;
    // Own local frame: this runs once per app, so locals must not pile up.
    let result = env.with_local_frame(16, |env| {
        render_icon(
            env,
            ctx.activity.as_obj(),
            package_name,
            activity_name,
            size,
        )
    });
    if result.is_err() && env.exception_check()? {
        // e.g. NameNotFoundException if the app was just uninstalled.
        env.exception_clear()?;
    }
    result
}

fn render_icon(
    env: &mut JNIEnv,
    activity: &JObject,
    package_name: &str,
    activity_name: &str,
    size: u32,
) -> Result<slint::SharedPixelBuffer<slint::Rgba8Pixel>> {
    let side = size as i32;

    let pm = env
        .call_method(
            activity,
            "getPackageManager",
            "()Landroid/content/pm/PackageManager;",
            &[],
        )?
        .l()?;
    let package = env.new_string(package_name)?;
    let class = env.new_string(activity_name)?;
    let component = env.new_object(
        "android/content/ComponentName",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[JValue::Object(&package), JValue::Object(&class)],
    )?;
    let drawable = env
        .call_method(
            &pm,
            "getActivityIcon",
            "(Landroid/content/ComponentName;)Landroid/graphics/drawable/Drawable;",
            &[JValue::Object(&component)],
        )?
        .l()?;

    let config = env
        .get_static_field(
            "android/graphics/Bitmap$Config",
            "ARGB_8888",
            "Landroid/graphics/Bitmap$Config;",
        )?
        .l()?;
    let bitmap = env
        .call_static_method(
            "android/graphics/Bitmap",
            "createBitmap",
            "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;",
            &[
                JValue::Int(side),
                JValue::Int(side),
                JValue::Object(&config),
            ],
        )?
        .l()?;
    let canvas = env.new_object(
        "android/graphics/Canvas",
        "(Landroid/graphics/Bitmap;)V",
        &[JValue::Object(&bitmap)],
    )?;
    env.call_method(
        &drawable,
        "setBounds",
        "(IIII)V",
        &[
            JValue::Int(0),
            JValue::Int(0),
            JValue::Int(side),
            JValue::Int(side),
        ],
    )?;
    env.call_method(
        &drawable,
        "draw",
        "(Landroid/graphics/Canvas;)V",
        &[JValue::Object(&canvas)],
    )?;

    let len = (size * size) as usize;
    let pixels = env.new_int_array(len as i32)?;
    env.call_method(
        &bitmap,
        "getPixels",
        "([IIIIIII)V",
        &[
            JValue::Object(&pixels),
            JValue::Int(0),
            JValue::Int(side),
            JValue::Int(0),
            JValue::Int(0),
            JValue::Int(side),
            JValue::Int(side),
        ],
    )?;
    env.call_method(&bitmap, "recycle", "()V", &[])?;

    let mut argb = vec![0i32; len];
    env.get_int_array_region(&pixels, 0, &mut argb)?;

    // `getPixels` returns non-premultiplied ARGB color ints.
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(size, size);
    for (dst, src) in buffer.make_mut_slice().iter_mut().zip(argb) {
        let c = src as u32;
        *dst = slint::Rgba8Pixel {
            r: (c >> 16) as u8,
            g: (c >> 8) as u8,
            b: c as u8,
            a: (c >> 24) as u8,
        };
    }
    Ok(buffer)
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

#[allow(dead_code)] // public API, not used yet
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
        });
    }

    apps.sort_by_key(|a| a.title.to_lowercase());
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
