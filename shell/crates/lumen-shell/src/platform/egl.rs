//! EGL, GLES and `wl_egl_window`, loaded at run time, for the Wayland path.
//!
//! On webOS 4 `libEGL.so.1` is a small forwarder onto `libmali`, so the
//! shell looks its functions up through the library's handle, which also
//! searches what it pulls in (research of 2026-11-05, "What LG's driver
//! exposes"). GL's functions come from `libGLESv2` the same way, or from
//! `eglGetProcAddress` where the library lacks one. `wl_egl_window_create`
//! lives in stock Wayland's `libwayland-egl` on the B9 (b9.md, "Platform"),
//! or in the EGL library itself on some Mali builds.
//!
//! The context is ES 2, or ES 3 when asked (`"gles": 3`, and always for the
//! GPU probe), falling back to ES 2 if the driver refuses. The renderer's
//! shaders are GLSL ES 1.00, which an ES 3 context runs unchanged. The
//! config has no alpha where the driver offers one without, so nothing the
//! shell draws shows the compositor through.

use std::ffi::{CStr, c_char, c_void};

use libloading::Library;

pub type Display = *mut c_void;
pub type Config = *mut c_void;
pub type Context = *mut c_void;
pub type Surface = *mut c_void;

const NONE: i32 = 0x3038;
const ALPHA_SIZE: i32 = 0x3021;
const BLUE_SIZE: i32 = 0x3022;
const GREEN_SIZE: i32 = 0x3023;
const RED_SIZE: i32 = 0x3024;
const SURFACE_TYPE: i32 = 0x3033;
const WINDOW_BIT: i32 = 0x0004;
const RENDERABLE_TYPE: i32 = 0x3040;
const OPENGL_ES2_BIT: i32 = 0x0004;
/// `EGL_OPENGL_ES3_BIT_KHR` (EGL_KHR_create_context, core in EGL 1.5).
const OPENGL_ES3_BIT: i32 = 0x0040;
const VENDOR: i32 = 0x3053;
const VERSION: i32 = 0x3054;
const EXTENSIONS: i32 = 0x3055;
const CLIENT_APIS: i32 = 0x308D;
const CONTEXT_CLIENT_VERSION: i32 = 0x3098;
const OPENGL_ES_API: u32 = 0x30A0;
/// The most configs looked at.
const MAX_CONFIGS: usize = 64;

/// EGL's functions.
pub struct Egl {
    get_display: unsafe extern "C" fn(*mut c_void) -> Display,
    initialize: unsafe extern "C" fn(Display, *mut i32, *mut i32) -> u32,
    terminate: unsafe extern "C" fn(Display) -> u32,
    bind_api: unsafe extern "C" fn(u32) -> u32,
    choose_config: unsafe extern "C" fn(Display, *const i32, *mut Config, i32, *mut i32) -> u32,
    get_config_attrib: unsafe extern "C" fn(Display, Config, i32, *mut i32) -> u32,
    create_context: unsafe extern "C" fn(Display, Config, Context, *const i32) -> Context,
    destroy_context: unsafe extern "C" fn(Display, Context) -> u32,
    create_window_surface:
        unsafe extern "C" fn(Display, Config, *mut c_void, *const i32) -> Surface,
    destroy_surface: unsafe extern "C" fn(Display, Surface) -> u32,
    make_current: unsafe extern "C" fn(Display, Surface, Surface, Context) -> u32,
    swap_buffers: unsafe extern "C" fn(Display, Surface) -> u32,
    swap_interval: unsafe extern "C" fn(Display, i32) -> u32,
    get_proc_address: unsafe extern "C" fn(*const c_char) -> *const c_void,
    query_string: unsafe extern "C" fn(Display, i32) -> *const c_char,
    get_error: unsafe extern "C" fn() -> i32,
    /// `libGLESv2`, for GL's functions; `eglGetProcAddress` without it.
    gles: Option<&'static Library>,
    /// `wl_egl_window_create` and `_destroy`.
    window_create: unsafe extern "C" fn(*mut c_void, i32, i32) -> *mut c_void,
    window_destroy: unsafe extern "C" fn(*mut c_void),
    /// Where each came from, for the log.
    pub files: String,
}

/// Opens the first of `names` that loads, kept for the process: EGL's
/// objects point into it.
fn open(names: &[&str]) -> Result<(&'static Library, String), String> {
    let mut tried = Vec::new();
    for &name in names {
        // SAFETY: loading the TV's EGL and GLES libraries runs only their
        // own initialisers, which have no preconditions.
        match unsafe { Library::new(name) } {
            Ok(lib) => return Ok((Box::leak(Box::new(lib)), name.to_owned())),
            Err(e) => tried.push(format!("{name}: {e}")),
        }
    }
    Err(tried.join("; "))
}

/// Copies a function out of `lib`.
///
/// # Safety
/// `T` must be the exact `extern "C"` function-pointer type of `name`.
unsafe fn func<T: Copy>(lib: &Library, file: &str, name: &[u8]) -> Result<T, String> {
    // SAFETY: forwarded from the caller.
    unsafe { lib.get::<T>(name) }.map(|s| *s).map_err(|e| {
        let shown = String::from_utf8_lossy(name.strip_suffix(b"\0").unwrap_or(name));
        format!("{file} has no {shown}: {e}")
    })
}

impl Egl {
    pub fn load() -> Result<&'static Self, String> {
        let (egl, egl_file) = open(&["libEGL.so.1", "libEGL.so", "libmali.so.0", "libmali.so"])?;
        let gles = open(&["libGLESv2.so.2", "libGLESv2.so"]).ok();
        let f = egl_file.as_str();
        // `wl_egl_window_*`: stock Wayland's library, else EGL's own.
        type Create = unsafe extern "C" fn(*mut c_void, i32, i32) -> *mut c_void;
        type Destroy = unsafe extern "C" fn(*mut c_void);
        let from = |lib: &Library, file: &str| -> Result<(Create, Destroy), String> {
            // SAFETY: the prototypes of wayland-egl-core.h.
            unsafe {
                Ok((
                    func(lib, file, b"wl_egl_window_create\0")?,
                    func(lib, file, b"wl_egl_window_destroy\0")?,
                ))
            }
        };
        let window = match open(&["libwayland-egl.so.1", "libwayland-egl.so"]) {
            Ok((lib, file)) => from(lib, &file).map(|fns| (fns, file)),
            Err(e) => Err(e),
        };
        let ((window_create, window_destroy), window_file) = match window {
            Ok(found) => found,
            Err(e) => from(egl, f)
                .map(|fns| (fns, egl_file.clone()))
                .map_err(|own| format!("no wl_egl_window: {e}; {own}"))?,
        };
        let files = format!(
            "EGL {egl_file}, GLES {}, wl_egl_window {window_file}",
            gles.as_ref()
                .map_or("by eglGetProcAddress", |(_, f)| f.as_str())
        );
        // SAFETY: each type below is the EGL 1.4 prototype of the named
        // function (egl.h): EGLBoolean is u32, EGLint i32, EGLenum u32, and
        // the handles pointers.
        let egl = unsafe {
            Self {
                get_display: func(egl, f, b"eglGetDisplay\0")?,
                initialize: func(egl, f, b"eglInitialize\0")?,
                terminate: func(egl, f, b"eglTerminate\0")?,
                bind_api: func(egl, f, b"eglBindAPI\0")?,
                choose_config: func(egl, f, b"eglChooseConfig\0")?,
                get_config_attrib: func(egl, f, b"eglGetConfigAttrib\0")?,
                create_context: func(egl, f, b"eglCreateContext\0")?,
                destroy_context: func(egl, f, b"eglDestroyContext\0")?,
                create_window_surface: func(egl, f, b"eglCreateWindowSurface\0")?,
                destroy_surface: func(egl, f, b"eglDestroySurface\0")?,
                make_current: func(egl, f, b"eglMakeCurrent\0")?,
                swap_buffers: func(egl, f, b"eglSwapBuffers\0")?,
                swap_interval: func(egl, f, b"eglSwapInterval\0")?,
                get_proc_address: func(egl, f, b"eglGetProcAddress\0")?,
                query_string: func(egl, f, b"eglQueryString\0")?,
                get_error: func(egl, f, b"eglGetError\0")?,
                gles: gles.map(|(lib, _)| lib),
                window_create,
                window_destroy,
                files,
            }
        };
        Ok(Box::leak(Box::new(egl)))
    }

    /// A GL function: from `libGLESv2` if it has it, else EGL's lookup.
    pub fn proc_address(&self, name: &CStr) -> *const c_void {
        if let Some(gles) = self.gles {
            // SAFETY: the address is only read as a pointer here; `Gl::load`
            // gives it its prototype.
            if let Ok(sym) = unsafe { gles.get::<*const c_void>(name.to_bytes_with_nul()) } {
                let ptr = *sym;
                if !ptr.is_null() {
                    return ptr;
                }
            }
        }
        // SAFETY: `name` is a C string; a function the driver lacks is null.
        unsafe { (self.get_proc_address)(name.as_ptr()) }
    }

    fn error(&self) -> String {
        // SAFETY: no arguments; reads this thread's last EGL error.
        let code = unsafe { (self.get_error)() };
        format!("EGL error {code:#x}")
    }

    fn string(&self, display: Display, name: i32) -> String {
        // SAFETY: the display is initialised; a string EGL lacks is null.
        let ptr = unsafe { (self.query_string)(display, name) };
        if ptr.is_null() {
            return "-".to_owned();
        }
        // SAFETY: EGL's strings are static and NUL-terminated.
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }
}

/// What the context is and how it was made, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    /// The ES version of the context: 2 or 3.
    pub es: u8,
    /// Whether a swap waits for the display.
    pub vsync: bool,
    /// `EGL <vendor> <version>; client APIs …; N extensions`.
    pub egl_line: String,
    /// The config's sizes and the context asked for.
    pub config_line: String,
}

/// An EGL context drawing to a Wayland surface, current on this thread.
pub struct Window {
    egl: &'static Egl,
    display: Display,
    context: Context,
    surface: Surface,
    native: *mut c_void,
}

impl Window {
    /// A `w` x `h` window on `wl_surface` of `wl_display`, with an ES 3
    /// context if `es3` and the driver gives one, else ES 2, made current.
    ///
    /// # Safety
    /// `wl_display` and `wl_surface` are live Wayland objects that outlive
    /// the window.
    pub unsafe fn open(
        egl: &'static Egl,
        wl_display: *mut c_void,
        wl_surface: *mut c_void,
        size: (i32, i32),
        es3: bool,
    ) -> Result<(Self, Made), String> {
        // SAFETY: a live wl_display is the native display of a Wayland EGL.
        let display = unsafe { (egl.get_display)(wl_display) };
        if display.is_null() {
            return Err(format!("eglGetDisplay: {}", egl.error()));
        }
        let (mut major, mut minor) = (0, 0);
        // SAFETY: the display came from eglGetDisplay; both out-pointers are valid.
        if unsafe { (egl.initialize)(display, &mut major, &mut minor) } == 0 {
            return Err(format!("eglInitialize: {}", egl.error()));
        }
        // From here a failure terminates the display on its way out.
        let fail = |why: String| {
            // SAFETY: the display is initialised and nothing of it is current.
            unsafe { (egl.terminate)(display) };
            why
        };
        let extensions = egl.string(display, EXTENSIONS);
        let egl_line = format!(
            "EGL {} {}; client APIs {}; {} extensions{}",
            egl.string(display, VENDOR),
            egl.string(display, VERSION),
            egl.string(display, CLIENT_APIS),
            extensions.split_whitespace().count(),
            if extensions.contains("EGL_KHR_create_context") {
                ", EGL_KHR_create_context"
            } else {
                ""
            }
        );
        // SAFETY: a plain enum argument.
        if unsafe { (egl.bind_api)(OPENGL_ES_API) } == 0 {
            return Err(fail(format!("eglBindAPI: {}", egl.error())));
        }
        let tries: &[u8] = if es3 { &[3, 2] } else { &[2] };
        let mut refused = Vec::new();
        for &es in tries {
            // SAFETY: the display is initialised; the config and context
            // calls get valid attribute lists ending in EGL_NONE.
            match unsafe { make_context(egl, display, es) } {
                Ok((config, context, config_line)) => {
                    let config_line = if refused.is_empty() {
                        config_line
                    } else {
                        format!("{config_line} ({})", refused.join("; "))
                    };
                    // SAFETY: the surface and context are made from this
                    // display and config; `wl_surface` is live (the caller).
                    let opened =
                        unsafe { surface_for(egl, display, config, context, wl_surface, size) };
                    return match opened {
                        Ok((surface, native)) => {
                            // SAFETY: the display is current with this context.
                            let vsync = unsafe { (egl.swap_interval)(display, 1) } != 0;
                            Ok((
                                Self {
                                    egl,
                                    display,
                                    context,
                                    surface,
                                    native,
                                },
                                Made {
                                    es,
                                    vsync,
                                    egl_line,
                                    config_line,
                                },
                            ))
                        }
                        Err(e) => {
                            // SAFETY: the context is this display's, not current.
                            unsafe { (egl.destroy_context)(display, context) };
                            Err(fail(e))
                        }
                    };
                }
                Err(e) => refused.push(format!("ES {es}: {e}")),
            }
        }
        let refused = refused.join("; ");
        Err(fail(format!("no context ({refused})")))
    }

    /// Shows the frame drawn; false if EGL refused.
    pub fn swap(&self) -> bool {
        // SAFETY: the display and surface are this window's, and current.
        unsafe { (self.egl.swap_buffers)(self.display, self.surface) != 0 }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let egl = self.egl;
        // SAFETY: every handle was made in `open` and is released once, the
        // context made not current first, the native window after the
        // surface that drew to it.
        unsafe {
            (egl.make_current)(
                self.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            (egl.destroy_surface)(self.display, self.surface);
            (egl.destroy_context)(self.display, self.context);
            (egl.window_destroy)(self.native);
            (egl.terminate)(self.display);
        }
    }
}

/// A config and an ES `es` context for it.
///
/// # Safety
/// `display` is initialised and the API bound to ES.
unsafe fn make_context(
    egl: &Egl,
    display: Display,
    es: u8,
) -> Result<(Config, Context, String), String> {
    let renderable = if es >= 3 {
        OPENGL_ES3_BIT
    } else {
        OPENGL_ES2_BIT
    };
    let attribs = [
        SURFACE_TYPE,
        WINDOW_BIT,
        RENDERABLE_TYPE,
        renderable,
        RED_SIZE,
        8,
        GREEN_SIZE,
        8,
        BLUE_SIZE,
        8,
        NONE,
    ];
    let mut configs = [std::ptr::null_mut(); MAX_CONFIGS];
    let mut count = 0;
    // SAFETY: the attribute list ends in EGL_NONE; the array holds
    // MAX_CONFIGS handles and EGL writes at most that many.
    let chose = unsafe {
        (egl.choose_config)(
            display,
            attribs.as_ptr(),
            configs.as_mut_ptr(),
            MAX_CONFIGS as i32,
            &mut count,
        )
    };
    if chose == 0 || count <= 0 {
        return Err(format!("no config ({})", egl.error()));
    }
    let found = &configs[..usize::try_from(count).unwrap_or(0).min(MAX_CONFIGS)];
    let attrib = |config: Config, name: i32| {
        let mut value = 0;
        // SAFETY: the config is one EGL returned for this display.
        unsafe { (egl.get_config_attrib)(display, config, name, &mut value) };
        value
    };
    // The first without alpha, else EGL's first.
    let config = found
        .iter()
        .copied()
        .find(|&c| attrib(c, ALPHA_SIZE) == 0)
        .unwrap_or(found[0]);
    let context_attribs = [CONTEXT_CLIENT_VERSION, i32::from(es), NONE];
    // SAFETY: the config is this display's; no context is shared; the list
    // ends in EGL_NONE.
    let context = unsafe {
        (egl.create_context)(
            display,
            config,
            std::ptr::null_mut(),
            context_attribs.as_ptr(),
        )
    };
    if context.is_null() {
        return Err(egl.error());
    }
    let line = format!(
        "EGL config r{} g{} b{} a{} of {} offered; ES {es} context",
        attrib(config, RED_SIZE),
        attrib(config, GREEN_SIZE),
        attrib(config, BLUE_SIZE),
        attrib(config, ALPHA_SIZE),
        found.len()
    );
    Ok((config, context, line))
}

/// The window surface on `wl_surface`, current with `context`.
///
/// # Safety
/// The display is initialised, the config and context its own, and
/// `wl_surface` a live `wl_surface`.
unsafe fn surface_for(
    egl: &Egl,
    display: Display,
    config: Config,
    context: Context,
    wl_surface: *mut c_void,
    (w, h): (i32, i32),
) -> Result<(Surface, *mut c_void), String> {
    // SAFETY: a live wl_surface (the caller), at the UI's size.
    let native = unsafe { (egl.window_create)(wl_surface, w, h) };
    if native.is_null() {
        return Err("wl_egl_window_create failed".to_owned());
    }
    let attribs = [NONE];
    // SAFETY: the native window was just made for this display's surface.
    let surface = unsafe { (egl.create_window_surface)(display, config, native, attribs.as_ptr()) };
    if surface.is_null() {
        let e = format!("eglCreateWindowSurface: {}", egl.error());
        // SAFETY: made above, and nothing else holds it.
        unsafe { (egl.window_destroy)(native) };
        return Err(e);
    }
    // SAFETY: all three are this display's.
    if unsafe { (egl.make_current)(display, surface, surface, context) } == 0 {
        let e = format!("eglMakeCurrent: {}", egl.error());
        // SAFETY: made above, not current.
        unsafe {
            (egl.destroy_surface)(display, surface);
            (egl.window_destroy)(native);
        }
        return Err(e);
    }
    Ok((surface, native))
}
