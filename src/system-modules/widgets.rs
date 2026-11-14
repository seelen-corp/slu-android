//! Android app widgets on Home (`config::HomeWidget`). The system side lives
//! in `WidgetHost.java`: it binds and configures them, keeps their views in an
//! invisible container and draws each one whenever it changes: on the GPU into
//! a `HardwareBuffer` (Android 10+, `nativeOnWidgetBuffer`, `widget_gpu`), or
//! into a bitmap (`nativeOnWidgetFrame`). Home shows those frames like a
//! picture and forwards the touches on them (`touch`).

use std::collections::HashMap;
use std::sync::Mutex;

use jni::JNIEnv;
use jni::objects::{JByteArray, JByteBuffer, JClass, JObject, JObjectArray, JString, JValue};
use slint::{Rgba8Pixel, SharedPixelBuffer};

use crate::error::Result;
use crate::system_modules::{android_ctx, widget_gpu};

/// A widget provider, as shown in the picker.
pub struct WidgetProvider {
    /// Flattened component name (`package/class`).
    pub component: String,
    pub label: String,
    pub app_label: String,
    /// Minimum size (dp, = Slint logical px).
    pub min_width: f32,
    pub min_height: f32,
    /// Default size in cells, given by the provider (API 31+).
    pub target_span: Option<(u32, u32)>,
    /// Premultiplied RGBA.
    pub preview: Option<SharedPixelBuffer<Rgba8Pixel>>,
}

impl WidgetProvider {
    /// Cells it takes by default: the provider's own, or as many as its minimum
    /// size needs. A cell gives `cell` (width, height) minus `inset` on each
    /// side; rows are `gap` apart.
    pub fn default_span(&self, cell: (f32, f32), gap: f32, inset: f32) -> (u32, u32) {
        if let Some(span) = self.target_span {
            return span;
        }
        let cells = |min: f32, size: f32, gap: f32| {
            // n cells: n * size + (n - 1) * gap - 2 * inset >= min
            (((min + 2.0 * inset + gap) / (size + gap).max(1.0)).ceil() as u32).max(1)
        };
        (
            cells(self.min_width, cell.0, 0.0),
            cells(self.min_height, cell.1, gap),
        )
    }
}

type FrameListener = Box<dyn Fn(u32) + Send + 'static>;
type AddedListener = Box<dyn Fn(u32, String) + Send + 'static>;

/// The last frame of each widget, until the UI takes it (`take_frame`).
static FRAMES: Mutex<Option<HashMap<u32, SharedPixelBuffer<Rgba8Pixel>>>> = Mutex::new(None);
static FRAME_LISTENER: Mutex<Option<FrameListener>> = Mutex::new(None);
static ADDED_LISTENER: Mutex<Option<AddedListener>> = Mutex::new(None);

/// `on_frame(id)` runs (on the Android UI thread) when a widget has a new frame
/// (`take_frame`, or `widget_gpu::take`). Replaces the previous listener.
pub fn subscribe_frames(on_frame: impl Fn(u32) + Send + 'static) {
    if let Ok(mut listener) = FRAME_LISTENER.lock() {
        *listener = Some(Box::new(on_frame));
    }
}

/// `on_added(id, provider)` runs (on the Android UI thread) once a widget asked
/// for with `add` is bound and configured. Replaces the previous listener.
pub fn subscribe_added(on_added: impl Fn(u32, String) + Send + 'static) {
    if let Ok(mut listener) = ADDED_LISTENER.lock() {
        *listener = Some(Box::new(on_added));
    }
}

/// The latest frame of a widget, if there's a new one since the last call.
pub fn take_frame(id: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    FRAMES.lock().ok()?.as_mut()?.remove(&id)
}

/// Every installed widget provider (slow: renders their previews).
pub fn providers() -> Result<Vec<WidgetProvider>> {
    android_ctx::with_activity(|env, activity| {
        let array = env
            .call_method(activity, "widgetProviders", "()[Ljava/lang/Object;", &[])?
            .l()?;
        let array = JObjectArray::from(array);
        let len = env.get_array_length(&array)?;
        let mut providers = Vec::with_capacity(len as usize);
        for i in 0..len {
            let provider = env.get_object_array_element(&array, i)?;
            match read_provider(env, &provider) {
                Ok(provider) => providers.push(provider),
                Err(err) => log::error!("failed to read a widget provider: {err}"),
            }
            env.delete_local_ref(provider)?;
        }
        Ok(providers)
    })
}

fn string_field(env: &mut JNIEnv, object: &JObject, name: &str) -> Result<String> {
    let value = env.get_field(object, name, "Ljava/lang/String;")?.l()?;
    if value.is_null() {
        return Ok(String::new());
    }
    let value = JString::from(value);
    let text: String = env.get_string(&value)?.into();
    env.delete_local_ref(value)?;
    Ok(text)
}

fn read_provider(env: &mut JNIEnv, provider: &JObject) -> Result<WidgetProvider> {
    let int = |env: &mut JNIEnv, name: &str| -> Result<i32> {
        Ok(env.get_field(provider, name, "I")?.i()?)
    };
    let float = |env: &mut JNIEnv, name: &str| -> Result<f32> {
        Ok(env.get_field(provider, name, "F")?.f()?)
    };
    let (columns, rows) = (int(env, "targetColumns")?, int(env, "targetRows")?);
    let (width, height) = (int(env, "previewWidth")?, int(env, "previewHeight")?);
    let preview = env.get_field(provider, "preview", "[B")?.l()?;
    let preview = if preview.is_null() || width <= 0 || height <= 0 {
        None
    } else {
        let bytes = env.convert_byte_array(JByteArray::from(preview))?;
        pixel_buffer(width as u32, height as u32, &bytes)
    };
    Ok(WidgetProvider {
        component: string_field(env, provider, "component")?,
        label: string_field(env, provider, "label")?,
        app_label: string_field(env, provider, "appLabel")?,
        min_width: float(env, "minWidth")?,
        min_height: float(env, "minHeight")?,
        target_span: (columns > 0 && rows > 0).then_some((columns as u32, rows as u32)),
        preview,
    })
}

/// `width` x `height` RGBA pixels from `bytes` (`None` if the size is wrong).
fn pixel_buffer(width: u32, height: u32, bytes: &[u8]) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
    let target = buffer.make_mut_bytes();
    if target.len() != bytes.len() {
        return None;
    }
    target.copy_from_slice(bytes);
    Some(buffer)
}

/// Starts adding a widget of a provider (`WidgetProvider::component`): the
/// system may ask the user to allow it, and the widget may open its settings
/// first. Once it's ready, the `subscribe_added` listener runs.
pub fn add(component: &str) -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        let component = env.new_string(component)?;
        env.call_method(
            activity,
            "addWidget",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&component)],
        )?;
        Ok(())
    })
}

/// Lays out a widget of Home at `width` x `height` physical pixels; its frames
/// follow.
pub fn show(id: u32, width: i32, height: i32) -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        env.call_method(
            activity,
            "showWidget",
            "(III)V",
            &[
                JValue::Int(id as i32),
                JValue::Int(width),
                JValue::Int(height),
            ],
        )?;
        Ok(())
    })
}

/// Deletes a widget removed from Home. Called on Slint's thread.
pub fn remove(id: u32) -> Result<()> {
    widget_gpu::remove(id);
    if let Ok(mut frames) = FRAMES.lock()
        && let Some(frames) = frames.as_mut()
    {
        frames.remove(&id);
    }
    android_ctx::with_activity(|env, activity| {
        env.call_method(activity, "removeWidget", "(I)V", &[JValue::Int(id as i32)])?;
        Ok(())
    })
}

/// Deletes the widgets of the launcher that are not on Home (`keep`), e.g. left
/// by an add that never finished.
pub fn prune(keep: &[u32]) -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        let ids: Vec<i32> = keep.iter().map(|&id| id as i32).collect();
        let array = env.new_int_array(ids.len() as i32)?;
        env.set_int_array_region(&array, 0, &ids)?;
        env.call_method(activity, "pruneWidgets", "([I)V", &[JValue::Object(&array)])?;
        Ok(())
    })
}

/// A touch on a widget (a `MotionEvent` action: down, up, move or cancel), at
/// physical pixels of the widget.
pub fn touch(id: u32, action: i32, x: f32, y: f32) -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        env.call_method(
            activity,
            "widgetTouch",
            "(IIFF)V",
            &[
                JValue::Int(id as i32),
                JValue::Int(action),
                JValue::Float(x),
                JValue::Float(y),
            ],
        )?;
        Ok(())
    })
}

/// Java may draw into the buffer of frame `seq` of a widget again
/// (`nativeOnWidgetBuffer`). Any thread.
pub fn frame_done(id: u32, seq: i32) -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        env.call_method(
            activity,
            "widgetFrameDone",
            "(II)V",
            &[JValue::Int(id as i32), JValue::Int(seq)],
        )?;
        Ok(())
    })
}

/// Draws every widget again, e.g. once their textures were lost
/// (`widget_gpu::teardown`).
pub fn redraw() -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        env.call_method(activity, "redrawWidgets", "()V", &[])?;
        Ok(())
    })
}

/// The widgets go back to bitmap frames: GPU frames can't be shown here.
pub fn use_bitmaps() -> Result<()> {
    android_ctx::with_activity(|env, activity| {
        env.call_method(activity, "useWidgetBitmaps", "()V", &[])?;
        Ok(())
    })
}

/// Called from `WidgetHost` (Android UI thread) with a new frame of a widget,
/// drawn on the GPU: Rust holds the buffer (`widget_gpu`) until `frame_done`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_seelen_slu_1launcher_WidgetHost_nativeOnWidgetBuffer(
    env: JNIEnv,
    _class: JClass,
    id: jni::sys::jint,
    seq: jni::sys::jint,
    width: jni::sys::jint,
    height: jni::sys::jint,
    buffer: JObject,
) {
    let id = id as u32;
    if width <= 0 || height <= 0 {
        give_back(id, seq);
        return;
    }
    // Safety: a live HardwareBuffer of this call's env.
    let buffer = unsafe {
        widget_gpu::Buffer::acquire(
            env.get_raw(),
            buffer.as_raw(),
            id,
            seq,
            width as u32,
            height as u32,
        )
    };
    let Some(buffer) = buffer else {
        log::error!("failed to read a widget buffer");
        give_back(id, seq);
        return;
    };
    widget_gpu::push(buffer);
    if let Ok(listener) = FRAME_LISTENER.lock()
        && let Some(listener) = listener.as_ref()
    {
        listener(id);
    }
}

fn give_back(id: u32, seq: i32) {
    if let Err(err) = frame_done(id, seq) {
        log::error!("failed to give back a widget frame: {err}");
    }
}

/// Called from `WidgetHost` (Android UI thread) once a widget is bound and
/// configured.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_seelen_slu_1launcher_WidgetHost_nativeOnWidgetAdded(
    mut env: JNIEnv,
    _class: JClass,
    id: jni::sys::jint,
    provider: JString,
) {
    let provider: String = match env.get_string(&provider) {
        Ok(provider) => provider.into(),
        Err(err) => {
            log::error!("failed to read the widget provider: {err}");
            return;
        }
    };
    if let Ok(listener) = ADDED_LISTENER.lock()
        && let Some(listener) = listener.as_ref()
    {
        listener(id as u32, provider);
    }
}

/// Called from `WidgetHost` (Android UI thread) with a new image of a widget:
/// `width` x `height` premultiplied RGBA pixels, only valid during the call.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_seelen_slu_1launcher_WidgetHost_nativeOnWidgetFrame(
    env: JNIEnv,
    _class: JClass,
    id: jni::sys::jint,
    width: jni::sys::jint,
    height: jni::sys::jint,
    pixels: JByteBuffer,
) {
    if width <= 0 || height <= 0 {
        return;
    }
    let bytes = match (
        env.get_direct_buffer_address(&pixels),
        env.get_direct_buffer_capacity(&pixels),
    ) {
        // Safety: a direct buffer of `len` bytes, alive (and not written by
        // Java) during this call.
        (Ok(ptr), Ok(len)) if !ptr.is_null() => unsafe { std::slice::from_raw_parts(ptr, len) },
        _ => {
            log::error!("failed to read a widget frame");
            return;
        }
    };
    let Some(frame) = pixel_buffer(width as u32, height as u32, bytes) else {
        log::error!("widget frame of the wrong size");
        return;
    };
    let id = id as u32;
    if let Ok(mut frames) = FRAMES.lock() {
        frames.get_or_insert_with(HashMap::new).insert(id, frame);
    }
    if let Ok(listener) = FRAME_LISTENER.lock()
        && let Some(listener) = listener.as_ref()
    {
        listener(id);
    }
}
