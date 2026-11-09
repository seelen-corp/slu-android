//! System locale, used to pick the translation file from `i18n/`.

use jni::objects::JString;

use crate::error::Result;
use crate::system_modules::android_ctx;

/// The device locale as a BCP 47 tag (e.g. "en-US", "es-419", "zh-CN").
pub fn system_locale() -> Result<String> {
    let ctx = android_ctx::get()?;
    let mut env = ctx.vm.attach_current_thread()?;

    let locale = env
        .call_static_method(
            "java/util/Locale",
            "getDefault",
            "()Ljava/util/Locale;",
            &[],
        )?
        .l()?;
    let tag_obj = env
        .call_method(&locale, "toLanguageTag", "()Ljava/lang/String;", &[])?
        .l()?;
    let tag: JString = tag_obj.into();
    Ok(env.get_string(&tag)?.into())
}
