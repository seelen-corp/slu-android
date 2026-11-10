//! System look the UI follows: the user's accent color and the wallpaper's
//! brightness hint. The light/dark scheme comes from Slint itself
//! (`Palette.color-scheme`).

use jni::JNIEnv;
use jni::objects::{JObject, JValue};
use slint::Color;

use crate::error::Result;
use crate::system_modules::android_ctx;

/// The system accent and its variants, like `--system-accent-*` on the desktop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AccentColors {
    pub color: Color,
    pub light: Color,
    pub lighter: Color,
    pub lightest: Color,
    pub dark: Color,
    pub darker: Color,
    pub darkest: Color,
}

/// API 31 (Android 12): Material You system palette.
const API_S: i32 = 31;

/// `android.R.attr.colorAccent`.
const ATTR_COLOR_ACCENT: i32 = 0x01010435;

/// `WallpaperManager.FLAG_SYSTEM`.
const WALLPAPER_FLAG_SYSTEM: i32 = 1;

/// `WallpaperColors.HINT_SUPPORTS_DARK_TEXT`.
const HINT_SUPPORTS_DARK_TEXT: i32 = 1;

/// The user's accent: the Material You tones (`system_accent1_*`) on Android
/// 12+, or the theme's `colorAccent` (and lighter/darker mixes of it) before.
pub fn accent_colors() -> Result<AccentColors> {
    android_ctx::with_activity(|env, activity| {
        if android_ctx::sdk_int(env)? >= API_S {
            let mut tone =
                |step: u32| system_color(env, activity, &format!("system_accent1_{step}"));
            return Ok(AccentColors {
                lightest: tone(100)?,
                lighter: tone(200)?,
                light: tone(300)?,
                color: tone(500)?,
                dark: tone(600)?,
                darker: tone(700)?,
                darkest: tone(800)?,
            });
        }
        let color = theme_accent(env, activity)?;
        let white = Color::from_rgb_u8(255, 255, 255);
        let black = Color::from_rgb_u8(0, 0, 0);
        Ok(AccentColors {
            color,
            light: color.mix(&white, 0.75),
            lighter: color.mix(&white, 0.5),
            lightest: color.mix(&white, 0.25),
            dark: color.mix(&black, 0.8),
            darker: color.mix(&black, 0.6),
            darkest: color.mix(&black, 0.4),
        })
    })
}

/// Whether dark text is legible over the current wallpaper (it's light). False
/// when unknown (e.g. some live wallpapers).
pub fn wallpaper_supports_dark_text() -> Result<bool> {
    android_ctx::with_activity(|env, activity| {
        let manager = env
            .call_static_method(
                "android/app/WallpaperManager",
                "getInstance",
                "(Landroid/content/Context;)Landroid/app/WallpaperManager;",
                &[JValue::Object(activity)],
            )?
            .l()?;
        let colors = env
            .call_method(
                &manager,
                "getWallpaperColors",
                "(I)Landroid/app/WallpaperColors;",
                &[JValue::Int(WALLPAPER_FLAG_SYSTEM)],
            )?
            .l()?;
        if colors.is_null() {
            return Ok(false);
        }
        if android_ctx::sdk_int(env)? >= API_S {
            let hints = env.call_method(&colors, "getColorHints", "()I", &[])?.i()?;
            return Ok(hints & HINT_SUPPORTS_DARK_TEXT != 0);
        }
        // Before Android 12 the hints aren't public: use the main color.
        let primary = env
            .call_method(
                &colors,
                "getPrimaryColor",
                "()Landroid/graphics/Color;",
                &[],
            )?
            .l()?;
        let luminance = env.call_method(&primary, "luminance", "()F", &[])?.f()?;
        Ok(luminance > 0.5)
    })
}

/// A color of the `android` package resources, e.g. `system_accent1_500`.
fn system_color(env: &mut JNIEnv, activity: &JObject, name: &str) -> Result<Color> {
    let resources = env
        .call_method(
            activity,
            "getResources",
            "()Landroid/content/res/Resources;",
            &[],
        )?
        .l()?;
    let name = env.new_string(name)?;
    let kind = env.new_string("color")?;
    let package = env.new_string("android")?;
    let id = env
        .call_method(
            &resources,
            "getIdentifier",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
            &[
                JValue::Object(&name),
                JValue::Object(&kind),
                JValue::Object(&package),
            ],
        )?
        .i()?;
    if id == 0 {
        return Err("system color not found".into());
    }
    let argb = env
        .call_method(activity, "getColor", "(I)I", &[JValue::Int(id)])?
        .i()?;
    Ok(Color::from_argb_encoded(argb as u32))
}

/// `colorAccent` of the Activity's theme.
fn theme_accent(env: &mut JNIEnv, activity: &JObject) -> Result<Color> {
    let attrs = env.new_int_array(1)?;
    env.set_int_array_region(&attrs, 0, &[ATTR_COLOR_ACCENT])?;
    let array = env
        .call_method(
            activity,
            "obtainStyledAttributes",
            "([I)Landroid/content/res/TypedArray;",
            &[JValue::Object(&attrs)],
        )?
        .l()?;
    let argb = env
        .call_method(
            &array,
            "getColor",
            "(II)I",
            &[JValue::Int(0), JValue::Int(0)],
        )?
        .i()?;
    env.call_method(&array, "recycle", "()V", &[])?;
    if argb == 0 {
        return Err("the theme has no colorAccent".into());
    }
    Ok(Color::from_argb_encoded(argb as u32))
}
