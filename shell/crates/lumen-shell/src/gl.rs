//! The OpenGL ES 2.0 functions the shell uses, resolved through SDL at run time
//! so the same binary code runs on the TV's GLES2 and a PC's desktop GL.

use std::ffi::{CStr, c_char, c_void};

pub const NO_ERROR: u32 = 0;
pub const COLOR_BUFFER_BIT: u32 = 0x4000;
pub const TRIANGLES: u32 = 0x0004;
pub const BLEND: u32 = 0x0BE2;
pub const SRC_ALPHA: u32 = 0x0302;
pub const ONE_MINUS_SRC_ALPHA: u32 = 0x0303;
pub const VENDOR: u32 = 0x1F00;
pub const RENDERER: u32 = 0x1F01;
pub const VERSION: u32 = 0x1F02;
pub const SHADING_LANGUAGE_VERSION: u32 = 0x8B8C;
pub const ARRAY_BUFFER: u32 = 0x8892;
pub const STREAM_DRAW: u32 = 0x88E0;
pub const FLOAT: u32 = 0x1406;
pub const FALSE: u8 = 0;
pub const FRAGMENT_SHADER: u32 = 0x8B30;
pub const VERTEX_SHADER: u32 = 0x8B31;
pub const COMPILE_STATUS: u32 = 0x8B81;
pub const LINK_STATUS: u32 = 0x8B82;
pub const INFO_LOG_LENGTH: u32 = 0x8B84;
pub const RGBA: u32 = 0x1908;
pub const UNSIGNED_BYTE: u32 = 0x1401;

pub struct Gl {
    pub clear_color: unsafe extern "C" fn(f32, f32, f32, f32),
    pub clear: unsafe extern "C" fn(u32),
    pub viewport: unsafe extern "C" fn(i32, i32, i32, i32),
    pub enable: unsafe extern "C" fn(u32),
    pub blend_func: unsafe extern "C" fn(u32, u32),
    pub get_error: unsafe extern "C" fn() -> u32,
    pub get_string: unsafe extern "C" fn(u32) -> *const u8,
    pub create_shader: unsafe extern "C" fn(u32) -> u32,
    pub shader_source: unsafe extern "C" fn(u32, i32, *const *const c_char, *const i32),
    pub compile_shader: unsafe extern "C" fn(u32),
    pub get_shaderiv: unsafe extern "C" fn(u32, u32, *mut i32),
    pub get_shader_info_log: unsafe extern "C" fn(u32, i32, *mut i32, *mut c_char),
    pub delete_shader: unsafe extern "C" fn(u32),
    pub create_program: unsafe extern "C" fn() -> u32,
    pub attach_shader: unsafe extern "C" fn(u32, u32),
    pub link_program: unsafe extern "C" fn(u32),
    pub get_programiv: unsafe extern "C" fn(u32, u32, *mut i32),
    pub get_program_info_log: unsafe extern "C" fn(u32, i32, *mut i32, *mut c_char),
    pub use_program: unsafe extern "C" fn(u32),
    pub get_attrib_location: unsafe extern "C" fn(u32, *const c_char) -> i32,
    pub get_uniform_location: unsafe extern "C" fn(u32, *const c_char) -> i32,
    pub uniform2f: unsafe extern "C" fn(i32, f32, f32),
    pub gen_buffers: unsafe extern "C" fn(i32, *mut u32),
    pub bind_buffer: unsafe extern "C" fn(u32, u32),
    pub buffer_data: unsafe extern "C" fn(u32, isize, *const c_void, u32),
    pub enable_vertex_attrib_array: unsafe extern "C" fn(u32),
    pub vertex_attrib_pointer: unsafe extern "C" fn(u32, i32, u32, u8, i32, *const c_void),
    pub draw_arrays: unsafe extern "C" fn(u32, i32, i32),
    pub read_pixels: unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *mut c_void),
}

/// Resolves one function through `loader`.
///
/// # Safety
/// `T` must be the exact `extern "C"` function-pointer type of `name`.
unsafe fn load_fn<T: Copy>(
    loader: &dyn Fn(&CStr) -> *const c_void,
    name: &CStr,
) -> Result<T, String> {
    let ptr = loader(name);
    if ptr.is_null() {
        return Err(format!("GL has no {}", name.to_string_lossy()));
    }
    assert_eq!(
        size_of::<T>(),
        size_of::<*const c_void>(),
        "T must be a function pointer"
    );
    // SAFETY: non-null, pointer-sized, and the caller vouches for the signature.
    Ok(unsafe { std::mem::transmute_copy::<*const c_void, T>(&ptr) })
}

impl Gl {
    /// Resolves every function, failing on the first one the driver lacks.
    /// Requires a current GL context.
    pub fn load(loader: impl Fn(&CStr) -> *const c_void) -> Result<Self, String> {
        let l: &dyn Fn(&CStr) -> *const c_void = &loader;
        // SAFETY: each type below is the GLES 2.0 prototype of the named function.
        unsafe {
            Ok(Self {
                clear_color: load_fn(l, c"glClearColor")?,
                clear: load_fn(l, c"glClear")?,
                viewport: load_fn(l, c"glViewport")?,
                enable: load_fn(l, c"glEnable")?,
                blend_func: load_fn(l, c"glBlendFunc")?,
                get_error: load_fn(l, c"glGetError")?,
                get_string: load_fn(l, c"glGetString")?,
                create_shader: load_fn(l, c"glCreateShader")?,
                shader_source: load_fn(l, c"glShaderSource")?,
                compile_shader: load_fn(l, c"glCompileShader")?,
                get_shaderiv: load_fn(l, c"glGetShaderiv")?,
                get_shader_info_log: load_fn(l, c"glGetShaderInfoLog")?,
                delete_shader: load_fn(l, c"glDeleteShader")?,
                create_program: load_fn(l, c"glCreateProgram")?,
                attach_shader: load_fn(l, c"glAttachShader")?,
                link_program: load_fn(l, c"glLinkProgram")?,
                get_programiv: load_fn(l, c"glGetProgramiv")?,
                get_program_info_log: load_fn(l, c"glGetProgramInfoLog")?,
                use_program: load_fn(l, c"glUseProgram")?,
                get_attrib_location: load_fn(l, c"glGetAttribLocation")?,
                get_uniform_location: load_fn(l, c"glGetUniformLocation")?,
                uniform2f: load_fn(l, c"glUniform2f")?,
                gen_buffers: load_fn(l, c"glGenBuffers")?,
                bind_buffer: load_fn(l, c"glBindBuffer")?,
                buffer_data: load_fn(l, c"glBufferData")?,
                enable_vertex_attrib_array: load_fn(l, c"glEnableVertexAttribArray")?,
                vertex_attrib_pointer: load_fn(l, c"glVertexAttribPointer")?,
                draw_arrays: load_fn(l, c"glDrawArrays")?,
                read_pixels: load_fn(l, c"glReadPixels")?,
            })
        }
    }

    /// `glGetString(name)` as text, or an empty string if the driver has none.
    pub fn string(&self, name: u32) -> String {
        // SAFETY: glGetString returns null or a static NUL-terminated string.
        let ptr = unsafe { (self.get_string)(name) };
        if ptr.is_null() {
            return String::new();
        }
        // SAFETY: non-null result of glGetString, see above.
        unsafe { CStr::from_ptr(ptr.cast()) }
            .to_string_lossy()
            .into_owned()
    }

    /// Reads a rectangle of the current framebuffer as RGBA, rows bottom to top.
    pub fn read_rgba(&self, x: i32, y: i32, w: i32, h: i32) -> Vec<u8> {
        let mut pixels = vec![0u8; w.max(0) as usize * h.max(0) as usize * 4];
        // SAFETY: the buffer holds exactly w * h RGBA pixels, and RGBA rows of
        // whole pixels always meet the default 4-byte pack alignment.
        unsafe { (self.read_pixels)(x, y, w, h, RGBA, UNSIGNED_BYTE, pixels.as_mut_ptr().cast()) };
        pixels
    }

    pub fn error(&self) -> u32 {
        // SAFETY: no preconditions beyond a current context.
        unsafe { (self.get_error)() }
    }

    pub fn compile_shader(&self, kind: u32, source: &str) -> Result<u32, String> {
        let src_ptr: *const c_char = source.as_ptr().cast();
        let len = i32::try_from(source.len()).map_err(|_| "shader source too long".to_owned())?;
        // SAFETY: the source pointer and length describe `source`, which
        // outlives the call; GL copies it.
        unsafe {
            let shader = (self.create_shader)(kind);
            (self.shader_source)(shader, 1, &src_ptr, &len);
            (self.compile_shader)(shader);
            let mut ok = 0;
            (self.get_shaderiv)(shader, COMPILE_STATUS, &mut ok);
            if ok == 0 {
                let log = self.info_log(shader, self.get_shaderiv, self.get_shader_info_log);
                (self.delete_shader)(shader);
                return Err(format!("shader compile failed: {log}"));
            }
            Ok(shader)
        }
    }

    pub fn link_program(&self, vertex: u32, fragment: u32) -> Result<u32, String> {
        // SAFETY: both shaders are valid, compiled objects from compile_shader.
        unsafe {
            let program = (self.create_program)();
            (self.attach_shader)(program, vertex);
            (self.attach_shader)(program, fragment);
            (self.link_program)(program);
            let mut ok = 0;
            (self.get_programiv)(program, LINK_STATUS, &mut ok);
            if ok == 0 {
                let log = self.info_log(program, self.get_programiv, self.get_program_info_log);
                return Err(format!("program link failed: {log}"));
            }
            Ok(program)
        }
    }

    /// # Safety
    /// `object` must be valid for both `get_iv` and `get_log`.
    unsafe fn info_log(
        &self,
        object: u32,
        get_iv: unsafe extern "C" fn(u32, u32, *mut i32),
        get_log: unsafe extern "C" fn(u32, i32, *mut i32, *mut c_char),
    ) -> String {
        let mut len = 0;
        // SAFETY: forwarded from the caller; `len` is a valid out-pointer.
        unsafe { get_iv(object, INFO_LOG_LENGTH, &mut len) };
        let mut buf = vec![0u8; len.max(1) as usize];
        let mut written = 0;
        // SAFETY: `buf` has room for `len` bytes, which is what GL may write.
        unsafe { get_log(object, len.max(1), &mut written, buf.as_mut_ptr().cast()) };
        buf.truncate(written.max(0) as usize);
        String::from_utf8_lossy(&buf).trim().to_owned()
    }
}
