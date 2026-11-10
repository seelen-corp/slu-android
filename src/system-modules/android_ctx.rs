//! Shared Android context (JavaVM + Activity + ClassLoader), used by other
//! system-modules (`apps`, `activity`) to make JNI calls back into the
//! Activity that hosts this native library.

use std::sync::OnceLock;

use jni::objects::{GlobalRef, JObject};
use jni::{JNIEnv, JavaVM};
use slint::android::AndroidApp;

use crate::error::Result;

pub struct AndroidCtx {
    pub vm: JavaVM,
    pub activity: GlobalRef,
    pub class_loader: GlobalRef,
}

// Safety: the JavaVM and GlobalRef are both designed to be shared across
// threads; JNI itself requires attaching per-thread before use.
unsafe impl Send for AndroidCtx {}
unsafe impl Sync for AndroidCtx {}

static CTX: OnceLock<AndroidCtx> = OnceLock::new();

/// Must be called once from `android_main`, before any other function in
/// this module or in `apps`/`activity`.
pub fn init(app: &AndroidApp) -> Result<()> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) }?;
    let activity_obj = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };

    let mut env = vm.attach_current_thread()?;
    let activity = env.new_global_ref(&activity_obj)?;

    let class_loader_obj = env
        .call_method(
            activity.as_obj(),
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )?
        .l()?;
    let class_loader = env.new_global_ref(&class_loader_obj)?;
    drop(env);

    CTX.set(AndroidCtx {
        vm,
        activity,
        class_loader,
    })
    .ok();
    Ok(())
}

pub fn get() -> Result<&'static AndroidCtx> {
    CTX.get()
        .ok_or_else(|| "android_ctx::init must be called before use".into())
}

/// Runs `f` with a JNI env attached to the current thread and the Activity, in
/// its own local frame. If it fails because Java threw (e.g.
/// `ActivityNotFoundException`), the exception is cleared: a pending exception
/// would make every later JNI call on this thread fail.
pub fn with_activity<T>(f: impl FnOnce(&mut JNIEnv, &JObject) -> Result<T>) -> Result<T> {
    let ctx = get()?;
    let mut env = ctx.vm.attach_current_thread()?;
    let result = env.with_local_frame(32, |env| f(env, ctx.activity.as_obj()));
    if result.is_err() && env.exception_check()? {
        env.exception_describe()?;
        env.exception_clear()?;
    }
    result
}

/// `android.os.Build.VERSION.SDK_INT`.
pub fn sdk_int(env: &mut JNIEnv) -> Result<i32> {
    Ok(env
        .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
        .i()?)
}
