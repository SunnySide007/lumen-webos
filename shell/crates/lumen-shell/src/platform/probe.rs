//! The GPU plan's gate (decision 15): one measurement on the B9 decides
//! whether the Top Shelf heroes become ASTC 4x4 (or ETC2), and it is taken
//! here, in an ES 3 context, by a run launched with `{"probe":"gpu"}`, which
//! measures, logs and leaves without drawing the home screen.
//!
//! * What the context offers: its GL extensions, the compressed formats it
//!   lists (ETC2 is core in ES 3.0, ASTC LDR in ES 3.2), and how many
//!   program binary formats it has (a shader cache needs one).
//! * What a hero costs: a real hero decoded from its PNG, then uploaded as
//!   RGB8 and RGBA8, and payloads of the same size as ETC2 and ASTC 4x4, 6x6
//!   and 8x8, each `ROUNDS` times, timed to `glFinish`. A compressed upload
//!   copies blocks the driver does not read, so random blocks time the same
//!   as encoded ones.
//! * Whether compression bands the gradients with the dither on: for each
//!   encoded hero beside the executable in `art-probe/` (`<hero>.astc` from
//!   `astcenc -cl`, linear, or `<hero>.pkm` ETC2), the hero is drawn from it
//!   and from its PNG, read back, and compared: the mean and largest
//!   difference, and the longest run of one value down the middle column,
//!   which a dither keeps short and banding makes long. Without such files
//!   that part says so and is skipped.
//!
//! Every line starts `probe`. Nothing is written anywhere but the log.

use std::ffi::{CStr, c_void};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::atlas::Uv;
use crate::gl;
use crate::heroes;
use crate::render::{Batch, Renderer, Rgba, Texture, fit_viewport};

const EXTENSIONS: u32 = 0x1F03;
const NUM_COMPRESSED_TEXTURE_FORMATS: u32 = 0x86A2;
const COMPRESSED_TEXTURE_FORMATS: u32 = 0x86A3;
const NUM_PROGRAM_BINARY_FORMATS: u32 = 0x87FE;
const MAX_TEXTURE_SIZE: u32 = 0x0D33;

/// How many times each upload is timed.
const ROUNDS: usize = 7;
/// A hero's size.
const HERO: (u32, u32) = (1920, 640);
/// The most compressed formats listed (Mali lists every ASTC size twice).
const MAX_FORMATS: usize = 256;
/// The largest encoded file read.
const MAX_ENCODED: u64 = 8 << 20;

/// A compressed format: its GL enum, name, block size and bytes a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub gl: u32,
    pub name: &'static str,
    pub block: (u32, u32),
    pub bytes: usize,
}

pub const ETC2_RGB8: Format = Format {
    gl: 0x9274,
    name: "ETC2 RGB8",
    block: (4, 4),
    bytes: 8,
};
pub const ETC2_RGBA8: Format = Format {
    gl: 0x9278,
    name: "ETC2 RGBA8 EAC",
    block: (4, 4),
    bytes: 16,
};
pub const ASTC_4X4: Format = Format {
    gl: 0x93B0,
    name: "ASTC 4x4",
    block: (4, 4),
    bytes: 16,
};
pub const ASTC_5X5: Format = Format {
    gl: 0x93B2,
    name: "ASTC 5x5",
    block: (5, 5),
    bytes: 16,
};
pub const ASTC_6X6: Format = Format {
    gl: 0x93B4,
    name: "ASTC 6x6",
    block: (6, 6),
    bytes: 16,
};
pub const ASTC_8X8: Format = Format {
    gl: 0x93B7,
    name: "ASTC 8x8",
    block: (8, 8),
    bytes: 16,
};

const TIMED: [Format; 5] = [ETC2_RGB8, ETC2_RGBA8, ASTC_4X4, ASTC_6X6, ASTC_8X8];
const KNOWN: [Format; 6] = [
    ETC2_RGB8, ETC2_RGBA8, ASTC_4X4, ASTC_5X5, ASTC_6X6, ASTC_8X8,
];

impl Format {
    /// The bytes of a `w` x `h` image in this format.
    pub fn size(&self, (w, h): (u32, u32)) -> usize {
        w.div_ceil(self.block.0) as usize * h.div_ceil(self.block.1) as usize * self.bytes
    }

    fn by_gl(gl: u32) -> Option<Self> {
        KNOWN.into_iter().find(|f| f.gl == gl)
    }
}

/// An encoded image read from a file: its format, size and blocks.
#[derive(Debug, PartialEq, Eq)]
pub struct Encoded {
    pub format: Format,
    pub size: (u32, u32),
    pub blocks: Vec<u8>,
}

/// Reads astcenc's `.astc` file: a 16-byte header (magic `13 AB A1 5C`,
/// block x, y, z, then x, y, z sizes in 24-bit little endian), then the
/// blocks. 2D only, in a block size the probe knows.
pub fn read_astc(bytes: &[u8]) -> Result<Encoded, String> {
    let header = bytes.get(..16).ok_or("shorter than its header")?;
    if header[..4] != [0x13, 0xAB, 0xA1, 0x5C] {
        return Err("not an .astc file".to_owned());
    }
    let dim = |at: usize| u32::from_le_bytes([header[at], header[at + 1], header[at + 2], 0]);
    let (bx, by, bz) = (u32::from(header[4]), u32::from(header[5]), header[6]);
    let size = (dim(7), dim(10));
    if bz != 1 || dim(13) != 1 {
        return Err("a 3D image".to_owned());
    }
    let format = KNOWN
        .into_iter()
        .find(|f| f.gl >= ASTC_4X4.gl && f.block == (bx, by))
        .ok_or_else(|| format!("ASTC {bx}x{by} blocks, which the probe does not know"))?;
    finish(format, size, &bytes[16..])
}

/// Reads a `.pkm` file (ETC2): `PKM 20`, its type (1 RGB8, 3 RGBA8, big
/// endian), the padded and the real size, then the blocks.
pub fn read_pkm(bytes: &[u8]) -> Result<Encoded, String> {
    let header = bytes.get(..16).ok_or("shorter than its header")?;
    if &header[..6] != b"PKM 20" {
        return Err("not a .pkm file of version 2.0".to_owned());
    }
    let be = |at: usize| u32::from(u16::from_be_bytes([header[at], header[at + 1]]));
    let format = match be(6) {
        1 => ETC2_RGB8,
        3 => ETC2_RGBA8,
        other => return Err(format!("PKM type {other}, which the probe does not know")),
    };
    finish(format, (be(12), be(14)), &bytes[16..])
}

fn finish(format: Format, size: (u32, u32), blocks: &[u8]) -> Result<Encoded, String> {
    let want = format.size(size);
    if blocks.len() < want {
        return Err(format!(
            "{} bytes of blocks for {}x{}, not {want}",
            blocks.len(),
            size.0,
            size.1
        ));
    }
    Ok(Encoded {
        format,
        size,
        blocks: blocks[..want].to_vec(),
    })
}

/// Deterministic noise for the payloads: xorshift32.
fn noise(len: usize, mut seed: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        out.extend_from_slice(&seed.to_le_bytes());
    }
    out.truncate(len);
    out
}

/// The minimum, median and maximum of some times, in ms.
fn spread(times: &mut [Duration]) -> String {
    times.sort_unstable();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    match (times.first(), times.get(times.len() / 2), times.last()) {
        (Some(&lo), Some(&mid), Some(&hi)) => format!(
            "min {:.2}, p50 {:.2}, max {:.2} ms over {}",
            ms(lo),
            ms(mid),
            ms(hi),
            times.len()
        ),
        _ => "not timed".to_owned(),
    }
}

/// The longest run of one value down column `x` of `w`-wide RGBA rows, by
/// green (the channel that bands first on a dark gradient).
pub fn longest_run(rgba: &[u8], w: usize, x: usize) -> usize {
    let column = rgba.chunks_exact(w * 4).map(|row| row[x * 4 + 1]);
    let (mut best, mut run, mut last) = (0, 0, None);
    for v in column {
        run = if Some(v) == last { run + 1 } else { 1 };
        last = Some(v);
        best = best.max(run);
    }
    best
}

/// The mean and largest difference of two RGBA images, colour channels only.
pub fn difference(a: &[u8], b: &[u8]) -> (f64, u8) {
    let (mut sum, mut max, mut n) = (0u64, 0u8, 0u64);
    for (pa, pb) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
        for c in 0..3 {
            let d = pa[c].abs_diff(pb[c]);
            sum += u64::from(d);
            max = max.max(d);
            n += 1;
        }
    }
    (if n == 0 { 0.0 } else { sum as f64 / n as f64 }, max)
}

/// The functions the probe needs beyond the renderer's.
struct Fns {
    get_integerv: unsafe extern "C" fn(u32, *mut i32),
    finish: unsafe extern "C" fn(),
    compressed_tex_image_2d: unsafe extern "C" fn(u32, i32, u32, i32, i32, i32, i32, *const c_void),
}

impl Fns {
    fn load(loader: &dyn Fn(&CStr) -> *const c_void) -> Result<Self, String> {
        let get = |name: &CStr| {
            let ptr = loader(name);
            if ptr.is_null() {
                Err(format!("GL has no {}", name.to_string_lossy()))
            } else {
                Ok(ptr)
            }
        };
        let (a, b, c) = (
            get(c"glGetIntegerv")?,
            get(c"glFinish")?,
            get(c"glCompressedTexImage2D")?,
        );
        // SAFETY: each is the GLES 2.0 prototype of the function it names,
        // non-null, and pointer-sized.
        unsafe {
            Ok(Self {
                get_integerv: std::mem::transmute::<
                    *const c_void,
                    unsafe extern "C" fn(u32, *mut i32),
                >(a),
                finish: std::mem::transmute::<*const c_void, unsafe extern "C" fn()>(b),
                compressed_tex_image_2d: std::mem::transmute::<
                    *const c_void,
                    unsafe extern "C" fn(u32, i32, u32, i32, i32, i32, i32, *const c_void),
                >(c),
            })
        }
    }

    fn integer(&self, name: u32) -> i32 {
        let mut value = 0;
        // SAFETY: each name the probe asks for is a single integer.
        unsafe { (self.get_integerv)(name, &mut value) };
        value
    }

    fn finish(&self) {
        // SAFETY: no arguments; waits for the context's work.
        unsafe { (self.finish)() };
    }
}

/// A texture name of its own, filtered linearly and clamped, bound.
fn new_texture(gl: &gl::Gl) -> u32 {
    let mut name = 0;
    // SAFETY: one name into a valid out-pointer; plain state calls.
    unsafe {
        (gl.gen_textures)(1, &mut name);
        (gl.active_texture)(gl::TEXTURE0);
        (gl.bind_texture)(gl::TEXTURE_2D, name);
        (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR);
        (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR);
        (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE);
        (gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE);
    }
    name
}

/// A compressed texture of `blocks`, or GL's error.
fn compressed(
    fns: &Fns,
    gl: &gl::Gl,
    format: Format,
    size: (u32, u32),
    blocks: &[u8],
) -> Result<u32, String> {
    assert_eq!(blocks.len(), format.size(size), "{}", format.name);
    let before = gl.error();
    let name = new_texture(gl);
    // SAFETY: `blocks` holds exactly the image's blocks (asserted), lives
    // across the call, and GL copies them.
    unsafe {
        (fns.compressed_tex_image_2d)(
            gl::TEXTURE_2D,
            0,
            format.gl,
            size.0 as i32,
            size.1 as i32,
            0,
            blocks.len() as i32,
            blocks.as_ptr().cast(),
        );
    }
    match gl.error() {
        gl::NO_ERROR => Ok(name),
        err => {
            // SAFETY: one texture name.
            unsafe { (gl.delete_textures)(1, &name) };
            Err(format!(
                "GL error {err:#x}{}",
                if before != gl::NO_ERROR {
                    " (one was set before)"
                } else {
                    ""
                }
            ))
        }
    }
}

/// Runs the probe and returns its lines. `art` holds the heroes, `es` is
/// the context's version, `drawable` its size.
pub fn run(
    renderer: &mut Renderer,
    loader: &dyn Fn(&CStr) -> *const c_void,
    art: &Path,
    es: u8,
    drawable: (i32, i32),
) -> Vec<String> {
    let mut lines = Vec::new();
    let fns = match Fns::load(loader) {
        Ok(fns) => fns,
        Err(e) => {
            lines.push(format!("probe: {e}; nothing measured"));
            return lines;
        }
    };
    let gl = renderer.gl();
    let extensions = gl.string(EXTENSIONS);
    let has = |name: &str| extensions.split_whitespace().any(|e| e == name);
    let count = usize::try_from(fns.integer(NUM_COMPRESSED_TEXTURE_FORMATS)).unwrap_or(0);
    // Listed only when GL's count is believable.
    let mut formats = vec![0i32; if count <= MAX_FORMATS { count } else { 0 }];
    if !formats.is_empty() {
        // SAFETY: the array holds every format GL said it would write.
        unsafe { (fns.get_integerv)(COMPRESSED_TEXTURE_FORMATS, formats.as_mut_ptr()) };
    }
    let listed = |f: Format| formats.iter().any(|&g| g as u32 == f.gl);
    let etc2 = if es >= 3 {
        "core"
    } else if listed(ETC2_RGB8) {
        "listed"
    } else if has("GL_OES_compressed_ETC2_RGB8_texture") {
        "extension"
    } else {
        "absent"
    };
    lines.push(format!(
        "probe: ES {es} context ({}); {} GL extensions; ASTC LDR extension {}, ETC2 {etc2}; max texture {}",
        gl.string(gl::VERSION),
        extensions.split_whitespace().count(),
        if has("GL_KHR_texture_compression_astc_ldr") {
            "yes"
        } else {
            "no"
        },
        fns.integer(MAX_TEXTURE_SIZE)
    ));
    let named: Vec<String> = formats
        .iter()
        .map(|&f| {
            let f = f as u32;
            Format::by_gl(f).map_or_else(|| format!("{f:#x}"), |k| format!("{f:#x} {}", k.name))
        })
        .collect();
    lines.push(format!(
        "probe: {count} compressed formats: {}",
        named.join(", ")
    ));
    if es >= 3 {
        lines.push(format!(
            "probe: {} program binary formats",
            fns.integer(NUM_PROGRAM_BINARY_FORMATS)
        ));
    }
    // A real hero: its decode, then its pixels uploaded.
    let hero = art.join("hero-ambient.png");
    let from = Instant::now();
    let layer = match heroes::load(&hero, false) {
        Ok(layer) => layer,
        Err(e) => {
            lines.push(format!("probe: {}: {e}; uploads not timed", hero.display()));
            return lines;
        }
    };
    lines.push(format!(
        "probe: {} decoded in {:.1} ms ({}x{}, {})",
        hero.display(),
        from.elapsed().as_secs_f64() * 1000.0,
        layer.w,
        layer.h,
        if layer.rgb { "RGB" } else { "RGBA" }
    ));
    let rgba: Vec<u8> = if layer.rgb {
        layer
            .pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect()
    } else {
        layer.pixels.clone()
    };
    let size = (layer.w, layer.h);
    for (label, rgb, pixels) in [("RGB8", true, &layer.pixels), ("RGBA8", false, &rgba)] {
        if rgb != layer.rgb && label == "RGB8" {
            continue;
        }
        let mut times = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            let from = Instant::now();
            match renderer.create_texture(size.0, size.1, rgb, pixels) {
                Ok(name) => {
                    fns.finish();
                    times.push(from.elapsed());
                    renderer.delete_texture(name);
                }
                Err(e) => {
                    lines.push(format!("probe upload {label}: {e}"));
                    break;
                }
            }
        }
        lines.push(format!(
            "probe upload {label} {}x{}, {} KB: {}",
            size.0,
            size.1,
            pixels.len() / 1024,
            spread(&mut times)
        ));
    }
    let gl = renderer.gl();
    for format in TIMED {
        let blocks = noise(format.size(HERO), format.gl);
        let mut times = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            let from = Instant::now();
            match compressed(&fns, gl, format, HERO, &blocks) {
                Ok(name) => {
                    fns.finish();
                    times.push(from.elapsed());
                    // SAFETY: one texture name.
                    unsafe { (gl.delete_textures)(1, &name) };
                }
                Err(e) => {
                    lines.push(format!("probe upload {}: refused: {e}", format.name));
                    break;
                }
            }
        }
        if !times.is_empty() {
            lines.push(format!(
                "probe upload {} {}x{}, {} KB: {}",
                format.name,
                HERO.0,
                HERO.1,
                blocks.len() / 1024,
                spread(&mut times)
            ));
        }
    }
    banding(renderer, &fns, art, &layer, drawable, &mut lines);
    lines
}

/// The banding comparison, for each encoded hero in `art-probe/` beside
/// `art/`.
fn banding(
    renderer: &mut Renderer,
    fns: &Fns,
    art: &Path,
    layer: &heroes::Layer,
    drawable: (i32, i32),
    lines: &mut Vec<String>,
) {
    let folder = art.with_file_name("art-probe");
    let Ok(entries) = std::fs::read_dir(&folder) else {
        lines.push(format!(
            "probe banding: no {}; encode heroes there with astcenc -cl to compare",
            folder.display()
        ));
        return;
    };
    renderer.set_dither(true);
    let reference = match renderer.create_texture(layer.w, layer.h, layer.rgb, &layer.pixels) {
        Ok(name) => name,
        Err(e) => {
            lines.push(format!("probe banding: the PNG's texture: {e}"));
            return;
        }
    };
    let png = draw_and_read(renderer, reference, (layer.w, layer.h), drawable);
    renderer.delete_texture(reference);
    let middle = layer.w as usize / 2;
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let read = match ext {
            "astc" => read_astc,
            "pkm" => read_pkm,
            _ => continue,
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = match std::fs::metadata(&path) {
            Ok(m) if m.is_file() && m.len() <= MAX_ENCODED => {
                std::fs::read(&path).map_err(|e| e.to_string())
            }
            Ok(_) => Err(format!("not a file of at most {MAX_ENCODED} bytes")),
            Err(e) => Err(e.to_string()),
        };
        let encoded = match bytes.and_then(|b| read(&b)) {
            Ok(encoded) => encoded,
            Err(e) => {
                lines.push(format!("probe banding {name}: {e}"));
                continue;
            }
        };
        if encoded.size != (layer.w, layer.h) {
            lines.push(format!(
                "probe banding {name}: {}x{}, not the reference's {}x{}",
                encoded.size.0, encoded.size.1, layer.w, layer.h
            ));
            continue;
        }
        let texture = match compressed(
            fns,
            renderer.gl(),
            encoded.format,
            encoded.size,
            &encoded.blocks,
        ) {
            Ok(texture) => texture,
            Err(e) => {
                lines.push(format!("probe banding {name}: refused: {e}"));
                continue;
            }
        };
        let read = draw_and_read(renderer, texture, encoded.size, drawable);
        renderer.delete_texture(texture);
        let (mean, max) = difference(&png, &read);
        lines.push(format!(
            "probe banding {name} ({}) against the PNG, dither on: mean |d| {mean:.2}, max |d| {max}; \
             longest run in the middle column {} px against {} px",
            encoded.format.name,
            longest_run(&read, layer.w as usize, middle),
            longest_run(&png, layer.w as usize, middle)
        ));
    }
}

/// Draws texture `name` over the hero's rows at the top of the screen and
/// reads them back, without showing them.
fn draw_and_read(
    renderer: &Renderer,
    name: u32,
    (w, h): (u32, u32),
    drawable: (i32, i32),
) -> Vec<u8> {
    let mut batch = Batch::default();
    batch.image_from(
        Texture::Own(name),
        0.0,
        0.0,
        w as f32,
        h as f32,
        Uv {
            u0: 0.0,
            v0: 0.0,
            u1: 1.0,
            v1: 1.0,
        },
        Rgba(1.0, 1.0, 1.0, 1.0),
    );
    renderer.draw(&batch, drawable);
    let (vx, vy, vw, vh) = fit_viewport(drawable.0, drawable.1);
    // The hero's rows, in GL's bottom-up coordinates, at the viewport's scale.
    let scale = vw as f32 / crate::render::SCREEN_W;
    let rows = (h as f32 * scale).round() as i32;
    let cols = (w as f32 * scale).round() as i32;
    renderer.gl().read_rgba(vx, vy + vh - rows, cols, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_round_up_to_whole_blocks() {
        assert_eq!(ASTC_4X4.size(HERO), 480 * 160 * 16);
        assert_eq!(
            ASTC_4X4.size(HERO),
            1_228_800,
            "1.23 MB, as research.md says"
        );
        assert_eq!(ASTC_6X6.size(HERO), 320 * 107 * 16);
        assert_eq!(ETC2_RGB8.size(HERO), 614_400);
        assert_eq!(ETC2_RGBA8.size((5, 5)), 4 * 16);
    }

    fn astc_header(bx: u8, by: u8, (w, h): (u32, u32)) -> Vec<u8> {
        let mut v = vec![0x13, 0xAB, 0xA1, 0x5C, bx, by, 1];
        for d in [w, h, 1] {
            v.extend_from_slice(&d.to_le_bytes()[..3]);
        }
        v
    }

    #[test]
    fn reads_astcencs_files() {
        let mut file = astc_header(4, 4, (8, 4));
        file.extend_from_slice(&[7; 32]);
        file.extend_from_slice(&[9; 5]);
        let read = read_astc(&file).unwrap();
        assert_eq!(read.format, ASTC_4X4);
        assert_eq!(read.size, (8, 4));
        assert_eq!(read.blocks, vec![7; 32], "trailing bytes left out");
        let mut short = astc_header(4, 4, (8, 8));
        short.extend_from_slice(&[0; 16]);
        assert!(read_astc(&short).is_err());
        assert!(read_astc(&astc_header(10, 10, (10, 10))).is_err());
        assert!(read_astc(b"not astc at all!").is_err());
        assert!(read_astc(&[0x13]).is_err());
    }

    #[test]
    fn reads_pkm_files() {
        let mut file = b"PKM 20".to_vec();
        file.extend_from_slice(&1u16.to_be_bytes());
        file.extend_from_slice(&8u16.to_be_bytes());
        file.extend_from_slice(&4u16.to_be_bytes());
        file.extend_from_slice(&8u16.to_be_bytes());
        file.extend_from_slice(&4u16.to_be_bytes());
        file.extend_from_slice(&[1; 16]);
        let read = read_pkm(&file).unwrap();
        assert_eq!((read.format, read.size), (ETC2_RGB8, (8, 4)));
        file[7] = 2;
        assert!(read_pkm(&file).is_err(), "type 2 is not known");
    }

    #[test]
    fn noise_is_repeatable_and_sized() {
        assert_eq!(noise(10, 7), noise(10, 7));
        assert_eq!(noise(10, 7).len(), 10);
        assert_ne!(noise(16, 7), noise(16, 8));
    }

    #[test]
    fn a_dither_keeps_runs_short_and_banding_makes_them_long() {
        // Two columns of eight rows: a dithered one and a banded one.
        let rows = |greens: &[u8]| -> Vec<u8> {
            greens
                .iter()
                .flat_map(|&g| [0, g, 0, 255, 0, g, 0, 255])
                .collect()
        };
        let dithered = rows(&[10, 11, 10, 11, 11, 12, 11, 12]);
        let banded = rows(&[10, 10, 10, 10, 11, 11, 11, 11]);
        assert_eq!(longest_run(&dithered, 2, 1), 2);
        assert_eq!(longest_run(&banded, 2, 0), 4);
        let (mean, max) = difference(&dithered, &banded);
        assert_eq!(max, 1);
        assert!(mean > 0.0 && mean < 1.0);
        assert_eq!(difference(&[], &[]), (0.0, 0));
    }

    #[test]
    fn spreads_are_min_median_max() {
        let mut times = [3, 1, 2].map(Duration::from_millis);
        assert_eq!(spread(&mut times), "min 1.00, p50 2.00, max 3.00 ms over 3");
        assert_eq!(spread(&mut []), "not timed");
    }
}
