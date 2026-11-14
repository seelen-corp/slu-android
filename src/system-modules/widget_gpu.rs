//! GPU frames of the widgets (Android 10+, `widgets`). `WidgetHost` draws each
//! widget with a `HardwareRenderer` into an `ImageReader` and hands over each
//! frame as a `HardwareBuffer` (`push`): no pixel goes through the CPU.
//!
//! Here, on Slint's thread, a buffer becomes a texture of Slint's OpenGL
//! context (`take`). It's imported as an `EGLImage` in a context of our own
//! that shares textures with Slint's, so Skia's GL state is never touched, and
//! drawn once into a texture of the widget: Skia takes borrowed textures as
//! unpremultiplied, and HWUI draws premultiplied, so that pass converts it.
//! A fence makes Slint's context wait (on the GPU) for that pass, and the
//! buffer goes back to Java once the GPU has read it (`before_rendering`).
//!
//! The textures live as long as Slint's context: `teardown` drops them all, and
//! the widgets are drawn again once there's a new one (`setup`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::num::NonZeroU32;
use std::ptr::null_mut;
use std::sync::Mutex;

use slint::{BorrowedOpenGLTextureBuilder, BorrowedOpenGLTextureOrigin, Image};

use crate::error::Result;
use crate::system_modules::widgets;

#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
mod ffi {
    use std::ffi::{c_char, c_void};

    pub type EGLDisplay = *mut c_void;
    pub type EGLContext = *mut c_void;
    pub type EGLSurface = *mut c_void;
    pub type EGLConfig = *mut c_void;
    pub type EGLImageKHR = *mut c_void;
    pub type EGLSyncKHR = *mut c_void;
    pub type EGLClientBuffer = *mut c_void;
    pub type EGLint = i32;
    pub type EGLBoolean = u32;
    pub type EGLenum = u32;
    pub type GLuint = u32;
    pub type GLint = i32;
    pub type GLenum = u32;
    pub type GLsizei = i32;

    pub const EGL_NONE: EGLint = 0x3038;
    pub const EGL_CONFIG_ID: EGLint = 0x3028;
    pub const EGL_EXTENSIONS: EGLint = 0x3055;
    pub const EGL_HEIGHT: EGLint = 0x3056;
    pub const EGL_WIDTH: EGLint = 0x3057;
    pub const EGL_DRAW: EGLint = 0x3059;
    pub const EGL_READ: EGLint = 0x305A;
    pub const EGL_CONTEXT_CLIENT_VERSION: EGLint = 0x3098;
    pub const EGL_IMAGE_PRESERVED_KHR: EGLint = 0x30D2;
    pub const EGL_NATIVE_BUFFER_ANDROID: EGLenum = 0x3140;
    pub const EGL_SYNC_FENCE_KHR: EGLenum = 0x30F9;
    pub const EGL_SYNC_FLUSH_COMMANDS_BIT_KHR: EGLint = 0x0001;
    pub const EGL_CONDITION_SATISFIED_KHR: EGLint = 0x30F6;
    pub const EGL_FOREVER_KHR: u64 = u64::MAX;
    pub const EGL_TRUE: EGLint = 1;

    pub const GL_TEXTURE_2D: GLenum = 0x0DE1;
    pub const GL_TEXTURE_EXTERNAL_OES: GLenum = 0x8D65;
    pub const GL_TEXTURE0: GLenum = 0x84C0;
    pub const GL_TEXTURE_MAG_FILTER: GLenum = 0x2800;
    pub const GL_TEXTURE_MIN_FILTER: GLenum = 0x2801;
    pub const GL_TEXTURE_WRAP_S: GLenum = 0x2802;
    pub const GL_TEXTURE_WRAP_T: GLenum = 0x2803;
    pub const GL_NEAREST: GLint = 0x2600;
    pub const GL_LINEAR: GLint = 0x2601;
    pub const GL_CLAMP_TO_EDGE: GLint = 0x812F;
    pub const GL_RGBA: GLenum = 0x1908;
    pub const GL_UNSIGNED_BYTE: GLenum = 0x1401;
    pub const GL_FLOAT: GLenum = 0x1406;
    pub const GL_FRAMEBUFFER: GLenum = 0x8D40;
    pub const GL_COLOR_ATTACHMENT0: GLenum = 0x8CE0;
    pub const GL_FRAMEBUFFER_COMPLETE: GLenum = 0x8CD5;
    pub const GL_VERTEX_SHADER: GLenum = 0x8B31;
    pub const GL_FRAGMENT_SHADER: GLenum = 0x8B30;
    pub const GL_COMPILE_STATUS: GLenum = 0x8B81;
    pub const GL_LINK_STATUS: GLenum = 0x8B82;
    pub const GL_TRIANGLE_STRIP: GLenum = 0x0005;

    #[repr(C)]
    pub struct AHardwareBuffer {
        _private: [u8; 0],
    }

    #[cfg_attr(target_os = "android", link(name = "EGL"))]
    unsafe extern "C" {
        pub fn eglGetCurrentDisplay() -> EGLDisplay;
        pub fn eglGetCurrentContext() -> EGLContext;
        pub fn eglGetCurrentSurface(readdraw: EGLint) -> EGLSurface;
        pub fn eglQueryContext(
            dpy: EGLDisplay,
            ctx: EGLContext,
            attribute: EGLint,
            value: *mut EGLint,
        ) -> EGLBoolean;
        pub fn eglChooseConfig(
            dpy: EGLDisplay,
            attrib_list: *const EGLint,
            configs: *mut EGLConfig,
            config_size: EGLint,
            num_config: *mut EGLint,
        ) -> EGLBoolean;
        pub fn eglCreateContext(
            dpy: EGLDisplay,
            config: EGLConfig,
            share_context: EGLContext,
            attrib_list: *const EGLint,
        ) -> EGLContext;
        pub fn eglDestroyContext(dpy: EGLDisplay, ctx: EGLContext) -> EGLBoolean;
        pub fn eglCreatePbufferSurface(
            dpy: EGLDisplay,
            config: EGLConfig,
            attrib_list: *const EGLint,
        ) -> EGLSurface;
        pub fn eglDestroySurface(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean;
        pub fn eglMakeCurrent(
            dpy: EGLDisplay,
            draw: EGLSurface,
            read: EGLSurface,
            ctx: EGLContext,
        ) -> EGLBoolean;
        pub fn eglQueryString(dpy: EGLDisplay, name: EGLint) -> *const c_char;
        pub fn eglGetProcAddress(name: *const c_char) -> *mut c_void;
        pub fn eglGetError() -> EGLint;
    }

    #[cfg_attr(target_os = "android", link(name = "GLESv2"))]
    unsafe extern "C" {
        pub fn glGenTextures(n: GLsizei, textures: *mut GLuint);
        pub fn glDeleteTextures(n: GLsizei, textures: *const GLuint);
        pub fn glBindTexture(target: GLenum, texture: GLuint);
        pub fn glActiveTexture(texture: GLenum);
        pub fn glTexParameteri(target: GLenum, pname: GLenum, param: GLint);
        pub fn glTexImage2D(
            target: GLenum,
            level: GLint,
            internalformat: GLint,
            width: GLsizei,
            height: GLsizei,
            border: GLint,
            format: GLenum,
            ty: GLenum,
            pixels: *const c_void,
        );
        pub fn glGenFramebuffers(n: GLsizei, framebuffers: *mut GLuint);
        pub fn glDeleteFramebuffers(n: GLsizei, framebuffers: *const GLuint);
        pub fn glBindFramebuffer(target: GLenum, framebuffer: GLuint);
        pub fn glFramebufferTexture2D(
            target: GLenum,
            attachment: GLenum,
            textarget: GLenum,
            texture: GLuint,
            level: GLint,
        );
        pub fn glCheckFramebufferStatus(target: GLenum) -> GLenum;
        pub fn glViewport(x: GLint, y: GLint, width: GLsizei, height: GLsizei);
        pub fn glCreateShader(ty: GLenum) -> GLuint;
        pub fn glShaderSource(
            shader: GLuint,
            count: GLsizei,
            string: *const *const c_char,
            length: *const GLint,
        );
        pub fn glCompileShader(shader: GLuint);
        pub fn glGetShaderiv(shader: GLuint, pname: GLenum, params: *mut GLint);
        pub fn glGetShaderInfoLog(
            shader: GLuint,
            max_length: GLsizei,
            length: *mut GLsizei,
            info_log: *mut c_char,
        );
        pub fn glDeleteShader(shader: GLuint);
        pub fn glCreateProgram() -> GLuint;
        pub fn glAttachShader(program: GLuint, shader: GLuint);
        pub fn glBindAttribLocation(program: GLuint, index: GLuint, name: *const c_char);
        pub fn glLinkProgram(program: GLuint);
        pub fn glGetProgramiv(program: GLuint, pname: GLenum, params: *mut GLint);
        pub fn glDeleteProgram(program: GLuint);
        pub fn glUseProgram(program: GLuint);
        pub fn glGetUniformLocation(program: GLuint, name: *const c_char) -> GLint;
        pub fn glUniform1i(location: GLint, v0: GLint);
        pub fn glUniform2f(location: GLint, v0: f32, v1: f32);
        pub fn glVertexAttribPointer(
            index: GLuint,
            size: GLint,
            ty: GLenum,
            normalized: u8,
            stride: GLsizei,
            pointer: *const c_void,
        );
        pub fn glEnableVertexAttribArray(index: GLuint);
        pub fn glDrawArrays(mode: GLenum, first: GLint, count: GLsizei);
        pub fn glFlush();
    }

    #[cfg_attr(target_os = "android", link(name = "android"))]
    unsafe extern "C" {
        pub fn AHardwareBuffer_fromHardwareBuffer(
            env: *mut jni::sys::JNIEnv,
            hardware_buffer: jni::sys::jobject,
        ) -> *mut AHardwareBuffer;
        pub fn AHardwareBuffer_acquire(buffer: *mut AHardwareBuffer);
        pub fn AHardwareBuffer_release(buffer: *mut AHardwareBuffer);
    }

    // Extensions, loaded with `eglGetProcAddress`.
    pub type GetNativeClientBuffer =
        unsafe extern "C" fn(buffer: *const AHardwareBuffer) -> EGLClientBuffer;
    pub type CreateImage = unsafe extern "C" fn(
        dpy: EGLDisplay,
        ctx: EGLContext,
        target: EGLenum,
        buffer: EGLClientBuffer,
        attrib_list: *const EGLint,
    ) -> EGLImageKHR;
    pub type DestroyImage = unsafe extern "C" fn(dpy: EGLDisplay, image: EGLImageKHR) -> EGLBoolean;
    pub type CreateSync = unsafe extern "C" fn(
        dpy: EGLDisplay,
        ty: EGLenum,
        attrib_list: *const EGLint,
    ) -> EGLSyncKHR;
    pub type DestroySync = unsafe extern "C" fn(dpy: EGLDisplay, sync: EGLSyncKHR) -> EGLBoolean;
    pub type ClientWaitSync = unsafe extern "C" fn(
        dpy: EGLDisplay,
        sync: EGLSyncKHR,
        flags: EGLint,
        timeout: u64,
    ) -> EGLint;
    pub type WaitSync =
        unsafe extern "C" fn(dpy: EGLDisplay, sync: EGLSyncKHR, flags: EGLint) -> EGLint;
    pub type ImageTargetTexture = unsafe extern "C" fn(target: GLenum, image: EGLImageKHR);
}

use ffi::*;

/// A frame of a widget: a `HardwareBuffer` held (acquired) until it's given
/// back to Java (`release`).
pub struct Buffer {
    buffer: *mut AHardwareBuffer,
    id: u32,
    seq: i32,
    width: u32,
    height: u32,
}

// Safety: an AHardwareBuffer reference can be used and released from any
// thread.
unsafe impl Send for Buffer {}

impl Buffer {
    /// Takes a reference to the buffer of a `HardwareBuffer` (valid during
    /// the call that gave it).
    ///
    /// # Safety
    /// `env` and `hardware_buffer` must be a valid JNI env and a live
    /// `android.hardware.HardwareBuffer` of it.
    pub unsafe fn acquire(
        env: *mut jni::sys::JNIEnv,
        hardware_buffer: jni::sys::jobject,
        id: u32,
        seq: i32,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        let buffer = unsafe { AHardwareBuffer_fromHardwareBuffer(env, hardware_buffer) };
        if buffer.is_null() {
            return None;
        }
        unsafe { AHardwareBuffer_acquire(buffer) };
        Some(Self {
            buffer,
            id,
            seq,
            width,
            height,
        })
    }

    /// Gives it back to Java (`WidgetHost`), which can draw into it again.
    fn release(self) {
        unsafe { AHardwareBuffer_release(self.buffer) };
        if let Err(err) = widgets::frame_done(self.id, self.seq) {
            log::error!("failed to give back a widget frame: {err}");
        }
    }
}

/// The latest frame of each widget not shown yet. Filled on the Android UI
/// thread, taken on Slint's.
static PENDING: Mutex<Option<HashMap<u32, Buffer>>> = Mutex::new(None);

/// Keeps `buffer` as the next frame of its widget (`take`); one not taken yet
/// goes back to Java.
pub fn push(buffer: Buffer) {
    let replaced = PENDING.lock().ok().and_then(|mut pending| {
        pending
            .get_or_insert_with(HashMap::new)
            .insert(buffer.id, buffer)
    });
    if let Some(replaced) = replaced {
        replaced.release();
    }
}

fn take_pending(id: u32) -> Option<Buffer> {
    PENDING.lock().ok()?.as_mut()?.remove(&id)
}

/// A frame drawn by `convert`: the GPU may still be reading its buffer.
struct InFlight {
    sync: EGLSyncKHR,
    buffer: Buffer,
    /// Slint's context waits for it (`before_rendering`).
    waited: bool,
}

struct Ext {
    get_native_client_buffer: GetNativeClientBuffer,
    create_image: CreateImage,
    destroy_image: DestroyImage,
    create_sync: CreateSync,
    destroy_sync: DestroySync,
    client_wait_sync: ClientWaitSync,
    /// `EGL_KHR_wait_sync`; without it, the CPU waits instead.
    wait_sync: Option<WaitSync>,
    image_target_texture: ImageTargetTexture,
}

impl Ext {
    fn load() -> Result<Self> {
        unsafe fn load(name: &CStr) -> Result<*mut c_void> {
            let function = unsafe { eglGetProcAddress(name.as_ptr()) };
            if function.is_null() {
                return Err(format!("missing {name:?}").into());
            }
            Ok(function)
        }
        // Safety: each name matches the type it's turned into.
        unsafe {
            Ok(Self {
                get_native_client_buffer: std::mem::transmute::<*mut c_void, GetNativeClientBuffer>(
                    load(c"eglGetNativeClientBufferANDROID")?,
                ),
                create_image: std::mem::transmute::<*mut c_void, CreateImage>(load(
                    c"eglCreateImageKHR",
                )?),
                destroy_image: std::mem::transmute::<*mut c_void, DestroyImage>(load(
                    c"eglDestroyImageKHR",
                )?),
                create_sync: std::mem::transmute::<*mut c_void, CreateSync>(load(
                    c"eglCreateSyncKHR",
                )?),
                destroy_sync: std::mem::transmute::<*mut c_void, DestroySync>(load(
                    c"eglDestroySyncKHR",
                )?),
                client_wait_sync: std::mem::transmute::<*mut c_void, ClientWaitSync>(load(
                    c"eglClientWaitSyncKHR",
                )?),
                wait_sync: load(c"eglWaitSyncKHR")
                    .ok()
                    .map(|f| std::mem::transmute::<*mut c_void, WaitSync>(f)),
                image_target_texture: std::mem::transmute::<*mut c_void, ImageTargetTexture>(load(
                    c"glEGLImageTargetTexture2DOES",
                )?),
            })
        }
    }
}

/// Our context (sharing textures with Slint's) and what the conversion needs.
struct Gl {
    ctx: EGLContext,
    /// A 1x1 pbuffer, or none with `EGL_KHR_surfaceless_context`.
    surface: EGLSurface,
    ext: Ext,
    program: GLuint,
    size_location: GLint,
    /// Bound to each buffer (`GL_TEXTURE_EXTERNAL_OES`).
    source: GLuint,
    framebuffer: GLuint,
}

const VERTEX_SHADER: &CStr = c"attribute vec2 position;
void main() { gl_Position = vec4(position, 0.0, 1.0); }";

/// Same pixel as the buffer (`gl_FragCoord`, nearest), unpremultiplied.
const FRAGMENT_SHADER: &CStr = c"#extension GL_OES_EGL_image_external : require
precision highp float;
uniform samplerExternalOES source;
uniform vec2 size;
void main() {
    vec4 color = texture2D(source, gl_FragCoord.xy / size);
    gl_FragColor = color.a > 0.0 ? vec4(color.rgb / color.a, color.a) : vec4(0.0);
}";

const QUAD: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];

/// What's current on this thread, to put it back.
struct Current {
    display: EGLDisplay,
    draw: EGLSurface,
    read: EGLSurface,
    ctx: EGLContext,
}

impl Current {
    fn get() -> Self {
        unsafe {
            Self {
                display: eglGetCurrentDisplay(),
                draw: eglGetCurrentSurface(EGL_DRAW),
                read: eglGetCurrentSurface(EGL_READ),
                ctx: eglGetCurrentContext(),
            }
        }
    }

    fn restore(&self, display: EGLDisplay) {
        unsafe {
            if self.ctx.is_null() {
                eglMakeCurrent(display, null_mut(), null_mut(), null_mut());
            } else {
                eglMakeCurrent(self.display, self.draw, self.read, self.ctx);
            }
        }
    }
}

#[derive(Default)]
struct State {
    /// Slint's display and context, between `setup` and `teardown`.
    display: EGLDisplay,
    slint_ctx: EGLContext,
    /// Created on the first frame.
    gl: Option<Gl>,
    /// Creating it failed: the widgets went back to bitmaps.
    failed: bool,
    /// The texture shown for each widget.
    textures: HashMap<u32, GLuint>,
    in_flight: Vec<InFlight>,
}

thread_local! {
    static STATE: RefCell<State> = Default::default();
}

impl State {
    fn ready(&self) -> bool {
        !self.display.is_null() && !self.failed
    }

    fn create_gl(&self) -> Result<Gl> {
        let display = self.display;
        unsafe {
            let ext = Ext::load()?;
            let mut config_id = 0;
            let mut version = 0;
            eglQueryContext(display, self.slint_ctx, EGL_CONFIG_ID, &mut config_id);
            eglQueryContext(
                display,
                self.slint_ctx,
                EGL_CONTEXT_CLIENT_VERSION,
                &mut version,
            );
            let attributes = [EGL_CONFIG_ID, config_id, EGL_NONE];
            let mut config = null_mut();
            let mut count = 0;
            if eglChooseConfig(display, attributes.as_ptr(), &mut config, 1, &mut count) == 0
                || count < 1
            {
                return Err(format!("no EGL config {config_id} ({:#x})", eglGetError()).into());
            }
            let attributes = [EGL_CONTEXT_CLIENT_VERSION, version.max(2), EGL_NONE];
            let ctx = eglCreateContext(display, config, self.slint_ctx, attributes.as_ptr());
            if ctx.is_null() {
                return Err(format!("can't create a shared context ({:#x})", eglGetError()).into());
            }
            let extensions = eglQueryString(display, EGL_EXTENSIONS);
            let surfaceless = !extensions.is_null()
                && CStr::from_ptr(extensions)
                    .to_bytes()
                    .split(|&b| b == b' ')
                    .any(|name| name == b"EGL_KHR_surfaceless_context");
            let surface = if surfaceless {
                null_mut()
            } else {
                let attributes = [EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE];
                let surface = eglCreatePbufferSurface(display, config, attributes.as_ptr());
                if surface.is_null() {
                    eglDestroyContext(display, ctx);
                    return Err(format!("can't create a pbuffer ({:#x})", eglGetError()).into());
                }
                surface
            };

            let current = Current::get();
            if eglMakeCurrent(display, surface, surface, ctx) == 0 {
                let error = eglGetError();
                if !surface.is_null() {
                    eglDestroySurface(display, surface);
                }
                eglDestroyContext(display, ctx);
                return Err(format!("can't make the shared context current ({error:#x})").into());
            }
            let resources = create_resources();
            current.restore(display);
            let (program, size_location, source, framebuffer) = match resources {
                Ok(resources) => resources,
                Err(err) => {
                    if !surface.is_null() {
                        eglDestroySurface(display, surface);
                    }
                    eglDestroyContext(display, ctx);
                    return Err(err);
                }
            };
            Ok(Gl {
                ctx,
                surface,
                ext,
                program,
                size_location,
                source,
                framebuffer,
            })
        }
    }

    /// Runs `f` with our context current, then puts back what was current.
    fn with_gl<T>(&mut self, f: impl FnOnce(&Gl, &mut HashMap<u32, GLuint>) -> T) -> Option<T> {
        let gl = self.gl.as_ref()?;
        let current = Current::get();
        unsafe {
            if eglMakeCurrent(self.display, gl.surface, gl.surface, gl.ctx) == 0 {
                log::error!(
                    "can't make the widget context current ({:#x})",
                    eglGetError()
                );
                return None;
            }
        }
        let result = f(gl, &mut self.textures);
        current.restore(self.display);
        Some(result)
    }

    /// Draws `buffer` into a new texture of its widget (unpremultiplied).
    fn convert(&mut self, buffer: &Buffer) -> Option<(GLuint, EGLSyncKHR)> {
        let display = self.display;
        let result = self.with_gl(|gl, textures| unsafe {
            let client_buffer = (gl.ext.get_native_client_buffer)(buffer.buffer);
            let attributes = [EGL_IMAGE_PRESERVED_KHR, EGL_TRUE, EGL_NONE];
            let image = (gl.ext.create_image)(
                display,
                null_mut(),
                EGL_NATIVE_BUFFER_ANDROID,
                client_buffer,
                attributes.as_ptr(),
            );
            if image.is_null() {
                return Err(format!(
                    "can't import a widget frame ({:#x})",
                    eglGetError()
                ));
            }
            glActiveTexture(GL_TEXTURE0);
            glBindTexture(GL_TEXTURE_EXTERNAL_OES, gl.source);
            (gl.ext.image_target_texture)(GL_TEXTURE_EXTERNAL_OES, image);

            let mut texture = 0;
            glGenTextures(1, &mut texture);
            glBindTexture(GL_TEXTURE_2D, texture);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
            let (width, height) = (buffer.width as GLsizei, buffer.height as GLsizei);
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGBA as GLint,
                width,
                height,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                std::ptr::null(),
            );
            glBindTexture(GL_TEXTURE_2D, 0);
            glBindFramebuffer(GL_FRAMEBUFFER, gl.framebuffer);
            glFramebufferTexture2D(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                texture,
                0,
            );
            if glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE {
                glDeleteTextures(1, &texture);
                (gl.ext.destroy_image)(display, image);
                return Err("widget texture not renderable".to_owned());
            }
            glViewport(0, 0, width, height);
            glUseProgram(gl.program);
            glUniform2f(gl.size_location, width as f32, height as f32);
            glVertexAttribPointer(0, 2, GL_FLOAT, 0, 0, QUAD.as_ptr().cast());
            glEnableVertexAttribArray(0);
            glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
            glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, 0, 0);
            // The texture keeps what it needs of the image.
            (gl.ext.destroy_image)(display, image);

            let sync = (gl.ext.create_sync)(display, EGL_SYNC_FENCE_KHR, std::ptr::null());
            glFlush();
            // The previous frame: Slint stops drawing it now, and GL keeps it
            // alive until the GPU is done with it.
            if let Some(old) = textures.insert(buffer.id, texture) {
                glDeleteTextures(1, &old);
            }
            Ok((texture, sync))
        })?;
        match result {
            Ok(converted) => Some(converted),
            Err(err) => {
                log::error!("{err}");
                None
            }
        }
    }

    /// Gives back the buffers the GPU is done with (all of them if `wait`).
    fn release_done(&mut self, wait: bool) {
        let Some(gl) = self.gl.as_ref() else {
            return;
        };
        let display = self.display;
        let timeout = if wait { EGL_FOREVER_KHR } else { 0 };
        let mut i = 0;
        while i < self.in_flight.len() {
            let sync = self.in_flight[i].sync;
            let done = sync.is_null()
                || unsafe {
                    (gl.ext.client_wait_sync)(
                        display,
                        sync,
                        EGL_SYNC_FLUSH_COMMANDS_BIT_KHR,
                        timeout,
                    )
                } == EGL_CONDITION_SATISFIED_KHR;
            if done {
                let frame = self.in_flight.swap_remove(i);
                if !frame.sync.is_null() {
                    unsafe { (gl.ext.destroy_sync)(display, frame.sync) };
                }
                frame.buffer.release();
            } else {
                i += 1;
            }
        }
    }
}

/// Program, its `size` uniform, the source texture and the framebuffer, in
/// the current (our) context.
unsafe fn create_resources() -> Result<(GLuint, GLint, GLuint, GLuint)> {
    unsafe fn shader(ty: GLenum, source: &CStr) -> Result<GLuint> {
        unsafe {
            let shader = glCreateShader(ty);
            let sources = [source.as_ptr()];
            glShaderSource(shader, 1, sources.as_ptr(), std::ptr::null());
            glCompileShader(shader);
            let mut ok = 0;
            glGetShaderiv(shader, GL_COMPILE_STATUS, &mut ok);
            if ok == 0 {
                let mut log = [0 as c_char; 512];
                glGetShaderInfoLog(shader, log.len() as GLsizei, null_mut(), log.as_mut_ptr());
                glDeleteShader(shader);
                return Err(format!(
                    "widget shader: {}",
                    CStr::from_ptr(log.as_ptr()).to_string_lossy()
                )
                .into());
            }
            Ok(shader)
        }
    }
    unsafe {
        let vertex = shader(GL_VERTEX_SHADER, VERTEX_SHADER)?;
        let fragment = match shader(GL_FRAGMENT_SHADER, FRAGMENT_SHADER) {
            Ok(fragment) => fragment,
            Err(err) => {
                glDeleteShader(vertex);
                return Err(err);
            }
        };
        let program = glCreateProgram();
        glAttachShader(program, vertex);
        glAttachShader(program, fragment);
        glBindAttribLocation(program, 0, c"position".as_ptr());
        glLinkProgram(program);
        glDeleteShader(vertex);
        glDeleteShader(fragment);
        let mut ok = 0;
        glGetProgramiv(program, GL_LINK_STATUS, &mut ok);
        if ok == 0 {
            glDeleteProgram(program);
            return Err("can't link the widget shader".into());
        }
        glUseProgram(program);
        glUniform1i(glGetUniformLocation(program, c"source".as_ptr()), 0);
        let size_location = glGetUniformLocation(program, c"size".as_ptr());

        let mut source = 0;
        glGenTextures(1, &mut source);
        glBindTexture(GL_TEXTURE_EXTERNAL_OES, source);
        glTexParameteri(GL_TEXTURE_EXTERNAL_OES, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        glTexParameteri(GL_TEXTURE_EXTERNAL_OES, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        glTexParameteri(GL_TEXTURE_EXTERNAL_OES, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
        glTexParameteri(GL_TEXTURE_EXTERNAL_OES, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
        let mut framebuffer = 0;
        glGenFramebuffers(1, &mut framebuffer);
        Ok((program, size_location, source, framebuffer))
    }
}

/// Slint's context was created (rendering notifier, with it current).
pub fn setup() {
    STATE.with_borrow_mut(|state| unsafe {
        state.display = eglGetCurrentDisplay();
        state.slint_ctx = eglGetCurrentContext();
    });
}

/// Before Slint draws a frame (rendering notifier, with its context current):
/// it waits for the frames converted since, and the buffers the GPU is done
/// with go back to Java.
pub fn before_rendering() {
    STATE.with_borrow_mut(|state| {
        let Some(gl) = state.gl.as_ref() else {
            return;
        };
        for frame in state.in_flight.iter_mut().filter(|frame| !frame.waited) {
            frame.waited = true;
            if frame.sync.is_null() {
                continue;
            }
            unsafe {
                match gl.ext.wait_sync {
                    Some(wait_sync) => {
                        wait_sync(state.display, frame.sync, 0);
                    }
                    None => {
                        (gl.ext.client_wait_sync)(
                            state.display,
                            frame.sync,
                            EGL_SYNC_FLUSH_COMMANDS_BIT_KHR,
                            EGL_FOREVER_KHR,
                        );
                    }
                }
            }
        }
        state.release_done(false);
    });
}

/// Slint's context is going away (rendering notifier, with it current): so do
/// the textures. Shown frames must be dropped (`lib.rs`).
pub fn teardown() {
    STATE.with_borrow_mut(|state| {
        state.release_done(true);
        let display = state.display;
        state.with_gl(|gl, textures| unsafe {
            for (_, texture) in textures.drain() {
                glDeleteTextures(1, &texture);
            }
            glDeleteTextures(1, &gl.source);
            glDeleteFramebuffers(1, &gl.framebuffer);
            glDeleteProgram(gl.program);
        });
        if let Some(gl) = state.gl.take() {
            unsafe {
                if !gl.surface.is_null() {
                    eglDestroySurface(display, gl.surface);
                }
                eglDestroyContext(display, gl.ctx);
            }
        }
        state.textures.clear();
        state.display = null_mut();
        state.slint_ctx = null_mut();
    });
}

/// The new frame of a widget, if there's one and Slint's context is ready
/// (otherwise it stays pending until `setup`).
pub fn take(id: u32) -> Option<Image> {
    STATE.with_borrow_mut(|state| {
        if !state.ready() {
            return None;
        }
        let buffer = take_pending(id)?;
        if state.gl.is_none() {
            match state.create_gl() {
                Ok(gl) => state.gl = Some(gl),
                Err(err) => {
                    log::error!("widget GPU frames unavailable, using bitmaps: {err}");
                    state.failed = true;
                    buffer.release();
                    drop_pending();
                    if let Err(err) = widgets::use_bitmaps() {
                        log::error!("failed to switch the widgets to bitmaps: {err}");
                    }
                    return None;
                }
            }
        }
        let Some((texture, sync)) = state.convert(&buffer) else {
            buffer.release();
            return None;
        };
        let size = (buffer.width, buffer.height).into();
        state.in_flight.push(InFlight {
            sync,
            buffer,
            waited: false,
        });
        let texture = NonZeroU32::new(texture)?;
        // Safety: the texture lives in Slint's share group until it's replaced
        // (`convert`), removed or torn down, and then it's no longer shown.
        let image = unsafe {
            BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(texture, size)
                .origin(BorrowedOpenGLTextureOrigin::TopLeft)
                .build()
        };
        Some(image)
    })
}

fn drop_pending() {
    let pending: Vec<Buffer> = PENDING
        .lock()
        .ok()
        .and_then(|mut pending| pending.take())
        .map(|pending| pending.into_values().collect())
        .unwrap_or_default();
    for buffer in pending {
        buffer.release();
    }
}

/// A widget removed from Home: its texture and pending frame go.
pub fn remove(id: u32) {
    if let Some(buffer) = take_pending(id) {
        buffer.release();
    }
    STATE.with_borrow_mut(|state| {
        if let Some(texture) = state.textures.get(&id).copied() {
            state.with_gl(|_, textures| unsafe {
                textures.remove(&id);
                glDeleteTextures(1, &texture);
            });
        }
    });
}
