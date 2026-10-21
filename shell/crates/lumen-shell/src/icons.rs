//! App icons: PNG files named by the launcher, decoded once, premultiplied,
//! shrunk to the size a tile draws them and kept in the atlas. On the TV the
//! paths will come from other apps' appinfo (milestone B3), so a file is read
//! only if it is a small regular file, and decoded only within fixed limits;
//! any failure leaves the tile with its letter instead.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use crate::atlas::{Atlas, Uv};

/// webOS icons are 80 or 130 px square; anything past this is not an icon.
pub const MAX_SIDE: u32 = 512;
/// The largest an icon is kept, a side: a tile draws it at most 138 px (120
/// lifted by 1.15), so bigger ones are halved until they fit. This keeps
/// dozens of icons in the atlas and about 100 KB of memory each.
pub const MAX_KEPT: u32 = 160;
/// The largest file read. A webOS icon is a few kilobytes; a 512 px one fits
/// unless it is stored uncompressed.
const MAX_FILE: u64 = 1 << 20;
/// The most the decoder may allocate.
const MAX_DECODE: usize = 4 << 20;

/// A decoded image: `w` x `h` premultiplied RGBA, rows top to bottom.
#[derive(Debug, PartialEq)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Decodes a PNG of any colour type and bit depth into premultiplied RGBA.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let limits = png::Limits { bytes: MAX_DECODE };
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), limits);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let (w, h) = (reader.info().width, reader.info().height);
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!(
            "{w}x{h} is not an icon (at most {MAX_SIDE} px a side)"
        ));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| format!("{w}x{h} is too big to decode"))?;
    let mut buf = vec![0; size];
    let frame = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("palette not expanded".to_owned()),
    };
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for row in buf.chunks(frame.line_size).take(h as usize) {
        for px in row[..w as usize * channels].chunks_exact(channels) {
            let [r, g, b, a] = match *px {
                [l] => [l, l, l, 255],
                [l, a] => [l, l, l, a],
                [r, g, b] => [r, g, b, 255],
                [r, g, b, a] => [r, g, b, a],
                _ => unreachable!("chunks of 1 to 4"),
            };
            let pre = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
            rgba.extend_from_slice(&[pre(r), pre(g), pre(b), a]);
        }
    }
    Ok(Image { w, h, rgba })
}

/// Halves `image`, averaging each 2x2 block of premultiplied pixels (fewer at
/// an odd edge), until neither side is over `max`.
pub fn shrink(mut image: Image, max: u32) -> Image {
    while image.w.max(image.h) > max {
        let (w, h) = (image.w.div_ceil(2), image.h.div_ceil(2));
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0u32; 4];
                let mut n = 0;
                for (sx, sy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = (2 * x + sx, 2 * y + sy);
                    if sx < image.w && sy < image.h {
                        let at = ((sy * image.w + sx) * 4) as usize;
                        for (s, &v) in sum.iter_mut().zip(&image.rgba[at..at + 4]) {
                            *s += u32::from(v);
                        }
                        n += 1;
                    }
                }
                rgba.extend(sum.map(|s| ((s + n / 2) / n) as u8));
            }
        }
        image = Image { w, h, rgba };
    }
    image
}

/// Reads, decodes and shrinks the PNG at `path`.
pub fn load(path: &Path) -> Result<Image, String> {
    let file = open_regular(path)?;
    let mut bytes = Vec::new();
    // One byte more than allowed tells a file that grew from one that fits.
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(format!("larger than {MAX_FILE} bytes"));
    }
    Ok(shrink(decode(&bytes)?, MAX_KEPT))
}

/// Opens `path` only if it is a regular file of at most `MAX_FILE` bytes. On
/// Linux the open itself cannot block: a FIFO or a device would otherwise
/// hang the shell before its type could be checked.
fn open_regular(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        /// `O_NONBLOCK` on Linux for ARM and x86.
        const O_NONBLOCK: i32 = 0o4000;
        options.custom_flags(O_NONBLOCK);
    }
    let file = options.open(path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".to_owned());
    }
    if meta.len() > MAX_FILE {
        return Err(format!("larger than {MAX_FILE} bytes"));
    }
    Ok(file)
}

enum Entry {
    /// Decoded, and placed in the atlas of the given generation, if at all.
    Ready {
        image: Image,
        placed: Option<(Uv, u32)>,
    },
    Failed,
}

/// Icons by path, each loaded the first time a tile asks for it.
#[derive(Default)]
pub struct Icons {
    entries: HashMap<PathBuf, Entry>,
}

impl Icons {
    /// Where `path`'s icon sits in the atlas and its size in pixels, loading
    /// and placing it if need be. `None` if it cannot be loaded (logged once)
    /// or does not fit even in an empty atlas.
    pub fn get(&mut self, path: &Path, atlas: &mut Atlas) -> Option<(Uv, u32, u32)> {
        if !self.entries.contains_key(path) {
            let entry = match load(path) {
                Ok(image) => Entry::Ready {
                    image,
                    placed: None,
                },
                Err(e) => {
                    // Debug formatting escapes whatever a third-party path holds.
                    crate::info!("icon {path:?}: {e}; the tile shows its letter");
                    Entry::Failed
                }
            };
            self.entries.insert(path.to_owned(), entry);
        }
        let Some(Entry::Ready { image, placed }) = self.entries.get_mut(path) else {
            return None;
        };
        let generation = atlas.generation();
        if placed.is_none_or(|(_, at)| at != generation) {
            *placed = place(image, atlas).map(|uv| (uv, atlas.generation()));
        }
        placed.map(|(uv, _)| (uv, image.w, image.h))
    }
}

/// Places `image`, emptying the atlas once if it is full.
fn place(image: &Image, atlas: &mut Atlas) -> Option<Uv> {
    atlas.insert(image.w, image.h, &image.rgba).or_else(|| {
        atlas.clear();
        atlas.insert(image.w, image.h, &image.rgba)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::apps::{Launcher, MockLauncher};

    /// A PNG of `w` x `h` pixels of `color` in the given colour type.
    fn png_bytes(w: u32, h: u32, color: png::ColorType, px: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(color);
            enc.set_depth(png::BitDepth::Eight);
            if color == png::ColorType::Indexed {
                enc.set_palette(vec![0, 0, 0, 255, 0, 0]);
                enc.set_trns(vec![255, 64]);
            }
            let mut writer = enc.write_header().unwrap();
            let data: Vec<u8> = px
                .iter()
                .copied()
                .cycle()
                .take(px.len() * (w * h) as usize)
                .collect();
            writer.write_image_data(&data).unwrap();
        }
        out
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-icons-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
        let path = temp_dir().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn rgba_is_premultiplied() {
        let bytes = png_bytes(2, 1, png::ColorType::Rgba, &[255, 128, 0, 128]);
        let image = decode(&bytes).unwrap();
        assert_eq!((image.w, image.h), (2, 1));
        assert_eq!(image.rgba, [128, 64, 0, 128, 128, 64, 0, 128]);
    }

    #[test]
    fn every_colour_type_comes_out_as_rgba() {
        let grey = decode(&png_bytes(1, 1, png::ColorType::Grayscale, &[200])).unwrap();
        assert_eq!(grey.rgba, [200, 200, 200, 255]);
        let grey_alpha =
            decode(&png_bytes(1, 1, png::ColorType::GrayscaleAlpha, &[200, 0])).unwrap();
        assert_eq!(grey_alpha.rgba, [0, 0, 0, 0]);
        let rgb = decode(&png_bytes(1, 1, png::ColorType::Rgb, &[1, 2, 3])).unwrap();
        assert_eq!(rgb.rgba, [1, 2, 3, 255]);
        // Palette entry 1 is red with a quarter alpha (tRNS 64).
        let indexed = decode(&png_bytes(1, 1, png::ColorType::Indexed, &[1])).unwrap();
        assert_eq!(indexed.rgba, [64, 0, 0, 64]);
    }

    #[test]
    fn rows_are_read_top_to_bottom() {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 1, 2);
            enc.set_color(png::ColorType::Grayscale);
            let mut writer = enc.write_header().unwrap();
            writer.write_image_data(&[10, 20]).unwrap();
        }
        assert_eq!(
            decode(&out).unwrap().rgba,
            [10, 10, 10, 255, 20, 20, 20, 255]
        );
    }

    #[test]
    fn junk_and_oversized_images_are_refused() {
        assert!(decode(b"not a png").is_err());
        assert!(decode(&[]).is_err());
        let wide = png_bytes(MAX_SIDE + 1, 1, png::ColorType::Grayscale, &[0]);
        let err = decode(&wide).unwrap_err();
        assert!(err.contains("not an icon"), "{err}");
        let mut cut = png_bytes(4, 4, png::ColorType::Rgb, &[1, 2, 3]);
        cut.truncate(cut.len() - 20);
        assert!(decode(&cut).is_err(), "a truncated file");
    }

    #[test]
    fn shrinking_halves_until_it_fits_and_averages_blocks() {
        let image = Image {
            w: 3,
            h: 2,
            rgba: [[0, 0, 0, 0], [200, 200, 200, 200], [10, 20, 30, 40]]
                .repeat(2)
                .concat(),
        };
        let small = shrink(image, 2);
        assert_eq!((small.w, small.h), (2, 1));
        // A full block of two clear and two white pixels; a half block at the edge.
        assert_eq!(small.rgba, [100, 100, 100, 100, 10, 20, 30, 40]);
        let kept = shrink(
            Image {
                w: 1,
                h: 1,
                rgba: vec![1, 2, 3, 4],
            },
            2,
        );
        assert_eq!(kept.rgba, [1, 2, 3, 4], "small enough already");
    }

    #[test]
    fn big_icons_are_kept_at_most_max_kept_a_side() {
        let png = png_bytes(MAX_SIDE, 300, png::ColorType::Rgb, &[9, 9, 9]);
        let image = load(&temp_file("big-icon.png", &png)).unwrap();
        assert_eq!((image.w, image.h), (128, 75));
        assert_eq!(image.rgba.len(), 128 * 75 * 4);
        let small = load(&temp_file(
            "small-icon.png",
            &png_bytes(130, 130, png::ColorType::Rgb, &[9, 9, 9]),
        ))
        .unwrap();
        assert_eq!(
            (small.w, small.h),
            (130, 130),
            "a webOS icon is kept as it is"
        );
    }

    #[test]
    fn files_that_are_missing_too_big_or_not_files_are_refused() {
        assert!(load(Path::new("/no/such/icon.png")).is_err());
        let big = temp_file("big.png", &vec![0; MAX_FILE as usize + 1]);
        let err = load(&big).unwrap_err();
        assert!(err.contains("larger than"), "{err}");
        assert!(load(&temp_dir()).is_err(), "a folder");
        let good = temp_file(
            "good.png",
            &png_bytes(3, 3, png::ColorType::Rgb, &[9, 9, 9]),
        );
        assert_eq!(load(&good).unwrap().w, 3);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn fifos_and_devices_are_refused_without_blocking() {
        let fifo = temp_dir().join("icon.fifo");
        let _ = std::fs::remove_file(&fifo);
        let made = std::process::Command::new("mkfifo").arg(&fifo).status();
        assert!(made.is_ok_and(|s| s.success()), "mkfifo");
        // Without O_NONBLOCK this open would wait for a writer forever.
        assert_eq!(load(&fifo).unwrap_err(), "not a regular file");
        assert_eq!(
            load(Path::new("/dev/zero")).unwrap_err(),
            "not a regular file"
        );
        assert_eq!(load(&temp_dir()).unwrap_err(), "not a regular file");
    }

    #[test]
    fn the_mock_launchers_icons_are_all_there() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/mock-icons");
        let apps = MockLauncher::default().with_icons_in(&dir).list().unwrap();
        let mut named = 0;
        for app in &apps {
            if let Some(path) = &app.icon {
                let image = load(path).unwrap_or_else(|e| panic!("{}: {e}", app.title));
                assert_eq!((image.w, image.h), (128, 128), "{}", app.title);
                named += 1;
            }
        }
        assert_eq!(named, 10);
    }

    #[test]
    fn an_icon_is_placed_once_and_again_after_the_atlas_is_emptied() {
        let path = temp_file(
            "icon.png",
            &png_bytes(8, 8, png::ColorType::Rgba, &[1, 2, 3, 255]),
        );
        let (mut icons, mut atlas) = (Icons::default(), Atlas::default());
        let (_, w, h) = icons.get(&path, &mut atlas).expect("loads");
        assert_eq!((w, h), (8, 8));
        assert_eq!(atlas.take_uploads().len(), 1);
        assert!(icons.get(&path, &mut atlas).is_some());
        assert!(atlas.take_uploads().is_empty(), "cached");
        atlas.clear();
        assert!(icons.get(&path, &mut atlas).is_some());
        assert_eq!(atlas.take_uploads().len(), 1, "placed again");
    }

    #[test]
    fn a_bad_icon_fails_once_and_stays_failed() {
        let path = temp_file("bad.png", b"junk");
        let (mut icons, mut atlas) = (Icons::default(), Atlas::default());
        assert!(icons.get(&path, &mut atlas).is_none());
        std::fs::write(&path, png_bytes(2, 2, png::ColorType::Rgb, &[1, 1, 1])).unwrap();
        assert!(icons.get(&path, &mut atlas).is_none(), "not read again");
    }
}
