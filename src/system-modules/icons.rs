//! App icons: rendered from the launcher Activity's drawable, in the shape the
//! user picked (`config::IconStyle`).
//!
//! - Adaptive icons (Android 8+) are two full-bleed layers: the system shape is
//!   applied by Android; any other shape is applied here to the unmasked layers.
//! - Legacy icons (one image) are wrapped like adaptive ones, so every icon
//!   gets the same shape: an icon drawn on a solid base shape (a circle, a
//!   rounded square...) is scaled to fill the shape over the base's color; any
//!   other icon is scaled down onto a white background (as Launcher3 does).
//!   The user can also keep them as they are.

use std::sync::Mutex;

use jni::JNIEnv;
use jni::objects::{JObject, JValue};
use slint::{Rgba8Pixel, SharedPixelBuffer};

use crate::error::Result;
use crate::system_modules::android_ctx;
use crate::system_modules::config::{IconShape, IconStyle, LegacyIcons};

/// `Build.VERSION_CODES.O` (adaptive icons).
const API_O: i32 = 26;
const ADAPTIVE_ICON_CLASS: &str = "android/graphics/drawable/AdaptiveIconDrawable";
const COLOR_DRAWABLE_CLASS: &str = "android/graphics/drawable/ColorDrawable";

/// Exponent of the `IconShape::Squircle` superellipse (`|x|^n + |y|^n = 1`).
const SQUIRCLE_EXPONENT: f32 = 4.0;
/// Radius of the small (bottom-right) corner of `IconShape::Teardrop`, relative
/// to the icon side; the others are fully round (same as Android's teardrop).
const TEARDROP_CORNER: f32 = 0.15;

/// Legacy icon without a base shape: its content fills this fraction of the
/// icon side (fits inside every shape).
const LEGACY_SCALE: f32 = 0.7;
/// Legacy icon without a base shape: background behind it. Icon content (not a
/// UI color): the same white Launcher3 uses to wrap legacy icons.
const LEGACY_BACKGROUND: Rgba8Pixel = Rgba8Pixel {
    r: 255,
    g: 255,
    b: 255,
    a: 255,
};
/// A legacy icon has a base shape if its opaque pixels fill at least this
/// fraction of their bounding box (a circle fills ~0.785)...
const BASE_MIN_FILL: f32 = 0.75;
/// ...and its outline has a single color: mean deviation per channel (0-255).
const BASE_MAX_EDGE_DEVIATION: f32 = 24.0;
/// Alpha from which a pixel counts as part of the icon (soft shadows don't).
const SOLID_ALPHA: u8 = 128;

/// The system icon mask at the last requested size (it doesn't change while
/// the launcher runs).
static SYSTEM_MASK: Mutex<Option<(u32, Vec<u8>)>> = Mutex::new(None);

/// Renders the icon of a launcher Activity into a `size`x`size` RGBA buffer
/// (`size` in physical pixels).
pub fn load_icon(
    package_name: &str,
    activity_name: &str,
    size: u32,
    style: IconStyle,
) -> Result<SharedPixelBuffer<Rgba8Pixel>> {
    let pixels = android_ctx::with_activity(|env, activity| {
        let drawable = activity_icon(env, activity, package_name, activity_name)?;
        let surface = Surface::new(env, size)?;
        let result = render(env, &surface, &drawable, style);
        surface.recycle(env)?;
        result
    })?;
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(size, size);
    buffer.make_mut_slice().copy_from_slice(&pixels);
    Ok(buffer)
}

/// The icon shape alone (`size`x`size`, physical pixels): opaque inside, with
/// the same anti-aliased edge as the icons. White, to be tinted by the UI
/// (folders are drawn in the shape of the app icons).
pub fn load_shape(size: u32, style: IconStyle) -> Result<SharedPixelBuffer<Rgba8Pixel>> {
    let mask = if style.shape == IconShape::System {
        android_ctx::with_activity(|env, _| {
            // Before Android 8 there is no system shape (icons keep their own).
            if android_ctx::sdk_int(env)? < API_O {
                let circle = IconStyle {
                    shape: IconShape::Circle,
                    ..style
                };
                return Ok(custom_shape_mask(size, circle));
            }
            let surface = Surface::new(env, size)?;
            let result = shape_mask(env, &surface, style);
            surface.recycle(env)?;
            result
        })?
    } else {
        custom_shape_mask(size, style)
    };
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(size, size);
    for (pixel, coverage) in buffer.make_mut_slice().iter_mut().zip(mask) {
        *pixel = Rgba8Pixel {
            a: coverage,
            ..LEGACY_BACKGROUND
        };
    }
    Ok(buffer)
}

fn activity_icon<'local>(
    env: &mut JNIEnv<'local>,
    activity: &JObject,
    package_name: &str,
    activity_name: &str,
) -> Result<JObject<'local>> {
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
    Ok(env
        .call_method(
            &pm,
            "getActivityIcon",
            "(Landroid/content/ComponentName;)Landroid/graphics/drawable/Drawable;",
            &[JValue::Object(&component)],
        )?
        .l()?)
}

fn render(
    env: &mut JNIEnv,
    surface: &Surface,
    drawable: &JObject,
    style: IconStyle,
) -> Result<Vec<Rgba8Pixel>> {
    let size = surface.size;
    let side = size as i32;
    let has_adaptive_icons = android_ctx::sdk_int(env)? >= API_O;
    let adaptive = has_adaptive_icons && env.is_instance_of(drawable, ADAPTIVE_ICON_CLASS)?;

    if adaptive {
        if style.shape == IconShape::System {
            surface.draw(env, drawable, (0, 0, side, side))?;
            return surface.read(env);
        }
        // Same layout as `AdaptiveIconDrawable`: the layers are 1.5x the visible
        // area (108dp vs 72dp), centered.
        let extra = side / 4;
        for getter in ["getBackground", "getForeground"] {
            let layer = env
                .call_method(
                    drawable,
                    getter,
                    "()Landroid/graphics/drawable/Drawable;",
                    &[],
                )?
                .l()?;
            if !layer.is_null() {
                surface.draw(env, &layer, (-extra, -extra, side + extra, side + extra))?;
            }
        }
        let mut pixels = surface.read(env)?;
        apply_mask(&mut pixels, &shape_mask(env, surface, style)?);
        return Ok(pixels);
    }

    // Legacy icon.
    surface.draw(env, drawable, (0, 0, side, side))?;
    let original = surface.read(env)?;
    // Before Android 8 there is no system shape to wrap them in.
    let keep = style.legacy == LegacyIcons::Original
        || (style.shape == IconShape::System && !has_adaptive_icons);
    if keep {
        return Ok(original);
    }
    let Some(layout) = legacy_layout(&original, size) else {
        return Ok(original);
    };
    surface.clear(env)?;
    surface.draw(env, drawable, layout.bounds)?;
    let mut pixels = surface.read(env)?;
    fill_background(&mut pixels, layout.background);
    apply_mask(&mut pixels, &shape_mask(env, surface, style)?);
    Ok(pixels)
}

/// Coverage (0-255) of the icon shape, per pixel.
fn shape_mask(env: &mut JNIEnv, surface: &Surface, style: IconStyle) -> Result<Vec<u8>> {
    let size = surface.size;
    if style.shape != IconShape::System {
        return Ok(custom_shape_mask(size, style));
    }
    let mut cache = SYSTEM_MASK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((cached_size, mask)) = cache.as_ref()
        && *cached_size == size
    {
        return Ok(mask.clone());
    }
    // An adaptive icon with an opaque background: its alpha is the system mask.
    let opaque = env.new_object(
        COLOR_DRAWABLE_CLASS,
        "(I)V",
        &[JValue::Int(0xFF00_0000_u32 as i32)],
    )?;
    let transparent = env.new_object(COLOR_DRAWABLE_CLASS, "(I)V", &[JValue::Int(0)])?;
    let icon = env.new_object(
        ADAPTIVE_ICON_CLASS,
        "(Landroid/graphics/drawable/Drawable;Landroid/graphics/drawable/Drawable;)V",
        &[JValue::Object(&opaque), JValue::Object(&transparent)],
    )?;
    let side = size as i32;
    surface.clear(env)?;
    surface.draw(env, &icon, (0, 0, side, side))?;
    let mask: Vec<u8> = surface.read(env)?.iter().map(|p| p.a).collect();
    *cache = Some((size, mask.clone()));
    Ok(mask)
}

/// Coverage (0-255) of a shape drawn by the launcher (not `IconShape::System`),
/// anti-aliased over one pixel.
fn custom_shape_mask(size: u32, style: IconStyle) -> Vec<u8> {
    let half = size as f32 / 2.0;
    let side = size as f32;
    (0..size * size)
        .map(|i| {
            let x = (i % size) as f32 + 0.5 - half;
            let y = (i / size) as f32 + 0.5 - half;
            // Signed distance to the edge in pixels (positive inside); approximate
            // for the squircle (exact enough for the anti-aliasing).
            let distance = match style.shape {
                IconShape::System => half,
                IconShape::Circle => rounded_square_distance(x, y, half, half),
                IconShape::Squircle => {
                    let n = SQUIRCLE_EXPONENT;
                    let r = ((x.abs() / half).powf(n) + (y.abs() / half).powf(n)).powf(1.0 / n);
                    (1.0 - r) * half
                }
                IconShape::Teardrop => {
                    let radius = if x > 0.0 && y > 0.0 {
                        TEARDROP_CORNER * side
                    } else {
                        half
                    };
                    rounded_square_distance(x, y, half, radius)
                }
                IconShape::RoundedSquare => {
                    let radius = (style.corner_radius as f32 / 100.0 * side).min(half);
                    rounded_square_distance(x, y, half, radius)
                }
            };
            ((distance + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect()
}

/// Signed distance from `(x, y)` (relative to the center) to the edge of a
/// square of half side `half` with corners of `radius`; positive inside.
fn rounded_square_distance(x: f32, y: f32, half: f32, radius: f32) -> f32 {
    let qx = x.abs() - (half - radius);
    let qy = y.abs() - (half - radius);
    let outside = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0);
    radius - outside
}

fn apply_mask(pixels: &mut [Rgba8Pixel], mask: &[u8]) {
    for (pixel, coverage) in pixels.iter_mut().zip(mask) {
        pixel.a = ((pixel.a as u32 * *coverage as u32 + 127) / 255) as u8;
    }
}

/// Puts the (non-premultiplied) pixels over an opaque background.
fn fill_background(pixels: &mut [Rgba8Pixel], background: Rgba8Pixel) {
    let blend =
        |fg: u8, bg: u8, a: u32| ((fg as u32 * a + bg as u32 * (255 - a) + 127) / 255) as u8;
    for pixel in pixels {
        let a = pixel.a as u32;
        *pixel = Rgba8Pixel {
            r: blend(pixel.r, background.r, a),
            g: blend(pixel.g, background.g, a),
            b: blend(pixel.b, background.b, a),
            a: 255,
        };
    }
}

/// Where to draw a legacy icon to wrap it, and the background behind it.
#[derive(Debug, PartialEq)]
struct LegacyLayout {
    /// Drawable bounds (left, top, right, bottom).
    bounds: (i32, i32, i32, i32),
    background: Rgba8Pixel,
}

/// `None` if the icon is empty.
fn legacy_layout(pixels: &[Rgba8Pixel], size: u32) -> Option<LegacyLayout> {
    let solid = |x: u32, y: u32| pixels[(y * size + x) as usize].a >= SOLID_ALPHA;

    // Bounding box of the solid pixels.
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0, 0);
    let mut filled = 0u32;
    for y in 0..size {
        for x in 0..size {
            if solid(x, y) {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
                filled += 1;
            }
        }
    }
    if filled == 0 {
        return None;
    }
    let (width, height) = (max_x - min_x + 1, max_y - min_y + 1);
    let fill = filled as f32 / (width * height) as f32;

    // Outline: solid pixels next to a non-solid one (or the canvas edge).
    let mut outline: Vec<Rgba8Pixel> = Vec::new();
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if !solid(x, y) {
                continue;
            }
            let edge = x == 0
                || y == 0
                || x == size - 1
                || y == size - 1
                || !solid(x - 1, y)
                || !solid(x + 1, y)
                || !solid(x, y - 1)
                || !solid(x, y + 1);
            if edge {
                outline.push(pixels[(y * size + x) as usize]);
            }
        }
    }
    let base = (fill >= BASE_MIN_FILL)
        .then(|| uniform_color(&outline))
        .flatten();

    // A base shape fills the icon; anything else is scaled down to fit.
    let side = size as f32;
    let (target, background) = match base {
        Some(color) => (side, color),
        None => (side * LEGACY_SCALE, LEGACY_BACKGROUND),
    };
    let scale = target / width.max(height) as f32;
    let center_x = (min_x + max_x + 1) as f32 / 2.0;
    let center_y = (min_y + max_y + 1) as f32 / 2.0;
    let left = side / 2.0 - center_x * scale;
    let top = side / 2.0 - center_y * scale;
    let extent = side * scale;
    Some(LegacyLayout {
        bounds: (
            left.round() as i32,
            top.round() as i32,
            (left + extent).round() as i32,
            (top + extent).round() as i32,
        ),
        background,
    })
}

/// The mean color of `pixels` (opaque), if they are all about the same color.
fn uniform_color(pixels: &[Rgba8Pixel]) -> Option<Rgba8Pixel> {
    if pixels.is_empty() {
        return None;
    }
    let n = pixels.len() as f32;
    let mean =
        |channel: fn(&Rgba8Pixel) -> u8| pixels.iter().map(|p| channel(p) as f32).sum::<f32>() / n;
    let channels: [fn(&Rgba8Pixel) -> u8; 3] = [|p| p.r, |p| p.g, |p| p.b];
    let means = channels.map(mean);
    let deviation = channels
        .iter()
        .zip(means)
        .map(|(channel, m)| {
            pixels
                .iter()
                .map(|p| (channel(p) as f32 - m).abs())
                .sum::<f32>()
                / n
        })
        .fold(0.0, f32::max);
    (deviation <= BASE_MAX_EDGE_DEVIATION).then(|| Rgba8Pixel {
        r: means[0].round() as u8,
        g: means[1].round() as u8,
        b: means[2].round() as u8,
        a: 255,
    })
}

/// A `size`x`size` ARGB bitmap and a canvas drawing on it.
struct Surface<'local> {
    size: u32,
    bitmap: JObject<'local>,
    canvas: JObject<'local>,
}

impl<'local> Surface<'local> {
    fn new(env: &mut JNIEnv<'local>, size: u32) -> Result<Self> {
        let side = size as i32;
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
        Ok(Self {
            size,
            bitmap,
            canvas,
        })
    }

    /// Draws `drawable` in `bounds` (left, top, right, bottom).
    fn draw(
        &self,
        env: &mut JNIEnv,
        drawable: &JObject,
        bounds: (i32, i32, i32, i32),
    ) -> Result<()> {
        let (left, top, right, bottom) = bounds;
        env.call_method(
            drawable,
            "setBounds",
            "(IIII)V",
            &[
                JValue::Int(left),
                JValue::Int(top),
                JValue::Int(right),
                JValue::Int(bottom),
            ],
        )?;
        env.call_method(
            drawable,
            "draw",
            "(Landroid/graphics/Canvas;)V",
            &[JValue::Object(&self.canvas)],
        )?;
        Ok(())
    }

    /// Makes every pixel transparent.
    fn clear(&self, env: &mut JNIEnv) -> Result<()> {
        env.call_method(&self.bitmap, "eraseColor", "(I)V", &[JValue::Int(0)])?;
        Ok(())
    }

    /// The pixels, non-premultiplied, row by row.
    fn read(&self, env: &mut JNIEnv) -> Result<Vec<Rgba8Pixel>> {
        let side = self.size as i32;
        let len = (self.size * self.size) as usize;
        let array = env.new_int_array(len as i32)?;
        env.call_method(
            &self.bitmap,
            "getPixels",
            "([IIIIIII)V",
            &[
                JValue::Object(&array),
                JValue::Int(0),
                JValue::Int(side),
                JValue::Int(0),
                JValue::Int(0),
                JValue::Int(side),
                JValue::Int(side),
            ],
        )?;
        let mut argb = vec![0i32; len];
        env.get_int_array_region(&array, 0, &mut argb)?;
        env.delete_local_ref(array)?;
        // `getPixels` returns non-premultiplied ARGB color ints.
        Ok(argb
            .into_iter()
            .map(|c| {
                let c = c as u32;
                Rgba8Pixel {
                    r: (c >> 16) as u8,
                    g: (c >> 8) as u8,
                    b: c as u8,
                    a: (c >> 24) as u8,
                }
            })
            .collect())
    }

    fn recycle(&self, env: &mut JNIEnv) -> Result<()> {
        env.call_method(&self.bitmap, "recycle", "()V", &[])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAR: Rgba8Pixel = Rgba8Pixel {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    const RED: Rgba8Pixel = Rgba8Pixel {
        r: 200,
        g: 20,
        b: 20,
        a: 255,
    };
    const BLUE: Rgba8Pixel = Rgba8Pixel {
        r: 20,
        g: 20,
        b: 200,
        a: 255,
    };

    fn style(shape: IconShape, corner_radius: u32) -> IconStyle {
        IconStyle {
            shape,
            corner_radius,
            legacy: LegacyIcons::Wrap,
        }
    }

    /// A circle of `color` with a `dot` in the middle, inside `size`.
    fn circle_icon(size: u32, radius: f32, color: Rgba8Pixel, dot: Rgba8Pixel) -> Vec<Rgba8Pixel> {
        let half = size as f32 / 2.0;
        (0..size * size)
            .map(|i| {
                let x = (i % size) as f32 + 0.5 - half;
                let y = (i / size) as f32 + 0.5 - half;
                let d = x.hypot(y);
                if d < radius / 4.0 {
                    dot
                } else if d < radius {
                    color
                } else {
                    CLEAR
                }
            })
            .collect()
    }

    #[test]
    fn masks() {
        let size = 100;
        let at = |mask: &[u8], x: u32, y: u32| mask[(y * size + x) as usize];
        let circle = custom_shape_mask(size, style(IconShape::Circle, 0));
        assert_eq!(at(&circle, 50, 50), 255);
        assert_eq!(at(&circle, 1, 1), 0);
        let square = custom_shape_mask(size, style(IconShape::RoundedSquare, 0));
        assert_eq!(at(&square, 0, 0), 255);
        let rounded = custom_shape_mask(size, style(IconShape::RoundedSquare, 50));
        assert_eq!(rounded, circle);
        let teardrop = custom_shape_mask(size, style(IconShape::Teardrop, 0));
        assert_eq!(at(&teardrop, 1, 1), 0);
        assert_eq!(at(&teardrop, 93, 93), 255);
        assert_eq!(at(&circle, 93, 93), 0);
        let squircle = custom_shape_mask(size, style(IconShape::Squircle, 0));
        assert_eq!(at(&squircle, 1, 1), 0);
        assert_eq!(at(&squircle, 10, 10), 255);
    }

    #[test]
    fn legacy_with_base_fills_the_shape() {
        let icon = circle_icon(100, 40.0, RED, BLUE);
        let layout = legacy_layout(&icon, 100).unwrap();
        assert_eq!(layout.background, RED);
        // The 80px circle grows to 100px.
        assert_eq!(layout.bounds, (-13, -13, 113, 113));
    }

    #[test]
    fn legacy_without_base_is_scaled_down() {
        // A thin cross: no base shape.
        let size = 100;
        let icon: Vec<_> = (0..size * size)
            .map(|i| {
                let (x, y) = (i % size, i / size);
                if (45..55).contains(&x) || (45..55).contains(&y) {
                    BLUE
                } else {
                    CLEAR
                }
            })
            .collect();
        let layout = legacy_layout(&icon, size).unwrap();
        assert_eq!(layout.background, LEGACY_BACKGROUND);
        assert_eq!(layout.bounds, (15, 15, 85, 85));
    }

    #[test]
    fn empty_legacy() {
        assert_eq!(legacy_layout(&[CLEAR; 16], 4), None);
    }
}
