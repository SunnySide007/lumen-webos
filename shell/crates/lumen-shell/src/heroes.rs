//! Top Shelf heroes (docs/specs/look-and-feel.md, "Phase 4 features"): the
//! PNG files a hero is drawn from, decoded off the frame on a thread of their
//! own and kept as pixels until the home loop says to upload them, and the
//! owner's own art, listed once.
//!
//! A hero comes out ready for its textures. Opaque art (every hero but the
//! ambient one's front layers) loses its alpha and keeps 3 bytes a pixel.
//! The ambient hero's light and ridge layers keep theirs, premultiplied
//! (lane E drew them with straight alpha), cropped to the rows they cover:
//! the light lies over the sky's top two thirds and the ridge over its bottom
//! third, so the three layers of a crossfade out of the ambient hero fit the
//! texture budget with the hero after it.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use lumen_core::apps::AppId;
use lumen_core::art::{Hero, Look};

use crate::home_loop::HeroNews;
use crate::icons::open_regular;

/// The sizes a hero's file may be: the shelf's 1920 x 640, or half that,
/// scaled up when drawn.
pub const SIZES: [(u32, u32); 2] = [(1920, 640), (960, 320)];
/// The largest file read. Lumen's heroes are under 1 MB; the owner's art may
/// be a photograph.
const MAX_FILE: u64 = 8 << 20;
/// The most the decoder may allocate besides the image it decodes into.
const MAX_DECODE: usize = 8 << 20;
/// The longest app ID taken from a file name in the owner's folder.
const MAX_ID: usize = 255;

/// One layer of a hero, ready for a texture of its own.
#[derive(Debug, PartialEq)]
pub struct Layer {
    pub w: u32,
    pub h: u32,
    /// 3 bytes a pixel and opaque, or else 4, premultiplied.
    pub rgb: bool,
    pub pixels: Vec<u8>,
    /// The rows of the hero's frame it covers, top and bottom, as fractions
    /// of the frame's height: (0, 1) for a whole layer.
    pub rows: (f32, f32),
}

impl Layer {
    /// What its texture counts against the texture budget: 4 bytes a pixel,
    /// RGB included, since GL ES 2 cannot say whether a driver keeps RGB in
    /// 3 bytes or pads it to 4.
    pub fn texture_bytes(&self) -> usize {
        self.w as usize * self.h as usize * 4
    }
}

/// Decodes a hero's PNG of any colour type and bit depth. If `keep_alpha`,
/// the layer keeps its alpha, premultiplied, cropped to the rows where it
/// is not clear with one clear row either side for the filtering at its
/// edges; otherwise, and whenever every pixel is opaque, it is laid on black
/// and kept as RGB. Refuses any size but `SIZES`.
pub fn decode(bytes: &[u8], keep_alpha: bool) -> Result<Layer, String> {
    let limits = png::Limits { bytes: MAX_DECODE };
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), limits);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let (w, h) = (reader.info().width, reader.info().height);
    if !SIZES.contains(&(w, h)) {
        return Err(format!("{w}x{h}; a hero is 1920x640 or 960x320"));
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
    let row = |y: usize| &buf[y * frame.line_size..][..w as usize * channels];
    let pixel = |y: usize| row(y).chunks_exact(channels);
    let rgba = |px: &[u8]| match *px {
        [l] => [l, l, l, 255],
        [l, a] => [l, l, l, a],
        [r, g, b] => [r, g, b, 255],
        [r, g, b, a] => [r, g, b, a],
        _ => unreachable!("chunks of 1 to 4"),
    };
    let pre = |c: u8, a: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
    let h_us = h as usize;
    // The rows with anything in them, if the alpha is to be kept.
    let mut lit = None;
    if keep_alpha && channels % 2 == 0 {
        let alpha = |y: usize| pixel(y).map(|px| px[channels - 1]);
        if (0..h_us).any(|y| alpha(y).any(|a| a < 255)) {
            let first = (0..h_us).find(|&y| alpha(y).any(|a| a > 0));
            let last = (0..h_us).rev().find(|&y| alpha(y).any(|a| a > 0));
            let (Some(first), Some(last)) = (first, last) else {
                return Err("nothing in it: every pixel is clear".to_owned());
            };
            lit = Some((first.saturating_sub(1), (last + 2).min(h_us)));
        }
    }
    let (rgb, (top, bottom)) = match lit {
        Some(rows) => (false, rows),
        None => (true, (0, h_us)),
    };
    let mut pixels = Vec::with_capacity(w as usize * (bottom - top) * if rgb { 3 } else { 4 });
    for y in top..bottom {
        for px in pixel(y) {
            let [r, g, b, a] = rgba(px);
            if rgb {
                pixels.extend_from_slice(&[pre(r, a), pre(g, a), pre(b, a)]);
            } else {
                pixels.extend_from_slice(&[pre(r, a), pre(g, a), pre(b, a), a]);
            }
        }
    }
    Ok(Layer {
        w,
        h: (bottom - top) as u32,
        rgb,
        pixels,
        rows: (top as f32 / h as f32, bottom as f32 / h as f32),
    })
}

/// Reads and decodes the PNG at `path`: a regular file of at most `MAX_FILE`
/// bytes, opened without blocking.
pub fn load(path: &Path, keep_alpha: bool) -> Result<Layer, String> {
    let file = open_regular(path, MAX_FILE)?;
    let mut bytes = Vec::new();
    // One byte more than allowed tells a file that grew from one that fits.
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(format!("larger than {MAX_FILE} bytes"));
    }
    decode(&bytes, keep_alpha)
}

/// The apps the owner made art for: each `<app id>.png` in `dir` whose PNG
/// header says it is 1920x640 or 960x320. Also gives a line for each file
/// refused, and why. Files whose names start with a dot are passed over;
/// with no folder there is no art, and nothing to say.
pub fn list_owner(dir: &Path) -> (HashSet<AppId>, Vec<String>) {
    let mut apps = HashSet::new();
    let mut refused = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (apps, refused);
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    for name in names {
        // Debug formatting escapes whatever a file name holds.
        let shown = format!("{name:?}");
        let Some(id) = name.strip_suffix(".png").filter(|id| is_app_id(id)) else {
            refused.push(format!("{shown}: not named <app id>.png"));
            continue;
        };
        match png_size(&dir.join(&name)) {
            Ok(size) if SIZES.contains(&size) => {
                apps.insert(AppId::new(id));
            }
            Ok((w, h)) => refused.push(format!(
                "{shown}: {w}x{h}; the owner's art is 1920x640 or 960x320"
            )),
            Err(e) => refused.push(format!("{shown}: {e}")),
        }
    }
    (apps, refused)
}

/// The characters webOS allows in an app ID, not starting with a dot.
fn is_app_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// A PNG's width and height, from its header alone.
fn png_size(path: &Path) -> Result<(u32, u32), String> {
    let mut head = [0u8; 24];
    open_regular(path, MAX_FILE)?
        .read_exact(&mut head)
        .map_err(|_| "not a PNG".to_owned())?;
    if head[..8] != *b"\x89PNG\r\n\x1a\n" || head[12..16] != *b"IHDR" {
        return Err("not a PNG".to_owned());
    }
    let be = |at: usize| u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    Ok((be(16), be(20)))
}

/// Where a hero's files are.
#[derive(Debug, Clone, PartialEq)]
pub struct Folders {
    /// Lumen's own art.
    pub art: PathBuf,
    /// The owner's, `<app id>.png` each.
    pub owner: PathBuf,
}

impl Folders {
    /// Each of `hero`'s files, back to front, and whether it keeps its
    /// alpha: only a layered hero's layers in front of its first.
    fn files(&self, hero: &Hero, look: &Look) -> Vec<(PathBuf, bool)> {
        let folder = match hero {
            Hero::Owner(_) => &self.owner,
            _ => &self.art,
        };
        let layered = hero.is_layered(look);
        hero.files(look)
            .iter()
            .enumerate()
            .map(|(i, file)| (folder.join(format!("{file}.png")), layered && i > 0))
            .collect()
    }
}

/// What the loader finished.
enum Done {
    Owner(HashSet<AppId>, Vec<String>),
    Hero(Hero, Result<Vec<Layer>, String>, Duration),
}

/// Loads a hero's files in order; the first that fails fails it, named in
/// the error.
fn load_hero(files: &[(PathBuf, bool)]) -> Result<Vec<Layer>, String> {
    files
        .iter()
        .map(|(path, keep_alpha)| {
            load(path, *keep_alpha).map_err(|e| {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                format!("{name}: {e}")
            })
        })
        .collect()
}

/// A thread that lists the owner's art, then loads the heroes asked of it,
/// in order. It ends when its `Heroes` is dropped.
struct Loader {
    requests: Sender<(Hero, Vec<(PathBuf, bool)>)>,
    results: Receiver<Done>,
}

impl Loader {
    fn spawn(owner: PathBuf) -> std::io::Result<Self> {
        let (requests, asked) = mpsc::channel::<(Hero, Vec<(PathBuf, bool)>)>();
        let (done, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("heroes".to_owned())
            .spawn(move || {
                let (apps, refused) = list_owner(&owner);
                if done.send(Done::Owner(apps, refused)).is_err() {
                    return;
                }
                for (hero, files) in asked {
                    let from = Instant::now();
                    let loaded = load_hero(&files);
                    if done.send(Done::Hero(hero, loaded, from.elapsed())).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self { requests, results })
    }
}

/// The heroes asked for and their decoded pixels, until uploaded: loaded on
/// a thread of their own, or at once where there is none (`--fixed-dt`, so
/// the frames are the same each run).
pub struct Heroes {
    folders: Folders,
    look: Look,
    loader: Option<Loader>,
    /// The owner's art has been listed.
    listed: bool,
    /// Asked of the thread and not back.
    asked: Vec<Hero>,
    /// Finished without a thread, not yet told.
    done: Vec<Done>,
    /// Decoded and waiting for the loop to upload them.
    decoded: HashMap<Hero, Vec<Layer>>,
}

impl Heroes {
    /// Heroes loaded at once, the owner's art listed now.
    pub fn at_once(folders: Folders, look: Look) -> Self {
        let (apps, refused) = list_owner(&folders.owner);
        Self {
            folders,
            look,
            loader: None,
            listed: true,
            asked: Vec::new(),
            done: vec![Done::Owner(apps, refused)],
            decoded: HashMap::new(),
        }
    }

    /// Heroes loaded on a thread of their own, which lists the owner's art
    /// first. If the thread cannot start, they load at once instead.
    pub fn threaded(folders: Folders, look: Look) -> Self {
        match Loader::spawn(folders.owner.clone()) {
            Ok(loader) => Self {
                folders,
                look,
                loader: Some(loader),
                listed: false,
                asked: Vec::new(),
                done: Vec::new(),
                decoded: HashMap::new(),
            },
            Err(e) => {
                crate::info!("no hero thread ({e}): heroes load at once");
                Self::at_once(folders, look)
            }
        }
    }

    /// Starts loading `hero`'s files.
    pub fn load(&mut self, hero: &Hero) {
        let files = self.folders.files(hero, &self.look);
        if let Some(loader) = &self.loader {
            if loader.requests.send((hero.clone(), files.clone())).is_ok() {
                self.asked.push(hero.clone());
                return;
            }
            self.thread_stopped();
        }
        let from = Instant::now();
        let loaded = load_hero(&files);
        self.done
            .push(Done::Hero(hero.clone(), loaded, from.elapsed()));
    }

    /// What finished since the last call, without waiting; a decoded hero's
    /// pixels stay here until `take` or `forget`.
    pub fn news(&mut self) -> Vec<HeroNews> {
        while let Some(loader) = &self.loader {
            match loader.results.try_recv() {
                Ok(done) => {
                    match &done {
                        Done::Owner(..) => self.listed = true,
                        Done::Hero(hero, ..) => self.asked.retain(|h| h != hero),
                    }
                    self.done.push(done);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => self.thread_stopped(),
            }
        }
        let mut news = Vec::new();
        for done in std::mem::take(&mut self.done) {
            news.push(match done {
                Done::Owner(apps, refused) => HeroNews::OwnerArt { apps, refused },
                Done::Hero(hero, Ok(layers), took) => {
                    let news = HeroNews::Decoded {
                        hero: hero.clone(),
                        layers: layers.len(),
                        bytes: layers.iter().map(Layer::texture_bytes).sum(),
                        took,
                    };
                    self.decoded.insert(hero, layers);
                    news
                }
                Done::Hero(hero, Err(why), _) => HeroNews::Failed { hero, why },
            });
        }
        news
    }

    /// The thread is gone: what it had not done is done at once, so nothing
    /// waits for it. The owner's art is listed; a hero it had not finished
    /// fails, since loading it here would hold up a frame unasked.
    fn thread_stopped(&mut self) {
        crate::info!("the hero thread stopped: heroes load at once");
        self.loader = None;
        if !self.listed {
            self.listed = true;
            let (apps, refused) = list_owner(&self.folders.owner);
            self.done.push(Done::Owner(apps, refused));
        }
        for hero in self.asked.drain(..) {
            let why = "the hero thread stopped before loading it".to_owned();
            self.done.push(Done::Hero(hero, Err(why), Duration::ZERO));
        }
    }

    /// Takes a decoded hero's layers, to upload.
    pub fn take(&mut self, hero: &Hero) -> Option<Vec<Layer>> {
        self.decoded.remove(hero)
    }

    /// Frees a decoded hero's pixels, unused.
    pub fn forget(&mut self, hero: &Hero) {
        self.decoded.remove(hero);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The art lane E drew, in the checkout.
    fn art() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/art")
    }

    fn png_bytes(
        w: u32,
        h: u32,
        color: png::ColorType,
        px: &dyn Fn(u32, u32) -> Vec<u8>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(color);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().unwrap();
            let data: Vec<u8> = (0..h)
                .flat_map(|y| (0..w).flat_map(move |x| px(x, y)))
                .collect();
            writer.write_image_data(&data).unwrap();
        }
        out
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumen-heroes-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_opaque_hero_is_kept_as_rgb() {
        let layer = load(&art().join("hero-plex.png"), false).unwrap();
        assert_eq!((layer.w, layer.h, layer.rgb), (1920, 640, true));
        assert_eq!(layer.pixels.len(), 1920 * 640 * 3);
        assert_eq!(layer.rows, (0.0, 1.0));
        assert_eq!(layer.texture_bytes(), 1920 * 640 * 4, "counted at 4 bytes");
        let half = load(&art().join("hero-plex-half.png"), false).unwrap();
        assert_eq!((half.w, half.h), (960, 320));
    }

    #[test]
    fn the_ambient_layers_are_cropped_premultiplied_and_fit_the_budget() {
        let files = Folders {
            art: art(),
            owner: art(),
        }
        .files(&Hero::Ambient, &Look::FULL);
        let layers = load_hero(&files).unwrap();
        let [bg, mid, fg] = &layers[..] else {
            panic!("three layers")
        };
        // The sky is opaque, whatever its file's alpha channel says.
        assert_eq!((bg.rgb, bg.h, bg.rows), (true, 640, (0.0, 1.0)));
        // The light lies on rows 0 to 423 and the ridge on 456 to 639 (lane
        // E's files, 2026-10-31), each with a clear row either side.
        assert_eq!((mid.rgb, mid.h), (false, 425));
        assert_eq!(mid.rows, (0.0, 425.0 / 640.0));
        assert_eq!((fg.rgb, fg.h), (false, 185));
        assert_eq!(fg.rows, (455.0 / 640.0, 1.0));
        for layer in [mid, fg] {
            for px in layer.pixels.as_chunks::<4>().0 {
                assert!(px[..3].iter().all(|&c| c <= px[3]), "premultiplied: {px:?}");
            }
        }
        let bytes: usize = layers.iter().map(Layer::texture_bytes).sum();
        assert_eq!(bytes, 9_600_000);
        assert_eq!(bytes, crate::home_loop::fake::AMBIENT_BYTES);
        // Flat, it is the one picture.
        let flat = Folders {
            art: art(),
            owner: art(),
        }
        .files(
            &Hero::Ambient,
            &Look {
                parallax: false,
                ..Look::FULL
            },
        );
        assert_eq!(flat.len(), 1);
        assert_eq!(
            load_hero(&flat).unwrap()[0].texture_bytes(),
            crate::home_loop::fake::FLAT_BYTES
        );
    }

    #[test]
    fn every_hero_the_lookup_names_is_there_at_both_sizes() {
        let folders = Folders {
            art: art(),
            owner: art(),
        };
        let mut heroes: Vec<Hero> = lumen_core::art::NAMED
            .iter()
            .map(|&(_, name)| Hero::Named(name))
            .collect();
        heroes.extend((0..lumen_core::style::TILE_PALETTE.len()).map(Hero::Palette));
        heroes.push(Hero::Ambient);
        for half in [false, true] {
            for parallax in [false, true] {
                let look = Look {
                    half,
                    parallax,
                    ..Look::FULL
                };
                for hero in &heroes {
                    for (path, _) in folders.files(hero, &look) {
                        let size = png_size(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
                        assert_eq!(size, SIZES[usize::from(half)], "{path:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn straight_alpha_is_premultiplied_and_clear_rows_are_cut() {
        // Rows 0-2 clear (RGB left in, as lane E's light layer has), row 3
        // half-lit red, the rest clear.
        let bytes = png_bytes(960, 320, png::ColorType::Rgba, &|_, y| match y {
            3 => vec![255, 0, 0, 128],
            _ => vec![9, 9, 9, 0],
        });
        let layer = decode(&bytes, true).unwrap();
        assert_eq!((layer.rgb, layer.h), (false, 3), "rows 2 to 4");
        assert_eq!(layer.rows, (2.0 / 320.0, 5.0 / 320.0));
        assert_eq!(&layer.pixels[..4], &[0, 0, 0, 0], "clear RGB goes");
        assert_eq!(&layer.pixels[960 * 4..960 * 4 + 4], &[128, 0, 0, 128]);
        // Without its alpha, the same layer is laid on black.
        let flat = decode(&bytes, false).unwrap();
        assert_eq!((flat.rgb, flat.h), (true, 320));
        assert_eq!(&flat.pixels[960 * 3 * 3..][..3], &[128, 0, 0]);
    }

    #[test]
    fn a_layer_with_nothing_in_it_or_of_another_size_is_refused() {
        let clear = png_bytes(960, 320, png::ColorType::Rgba, &|_, _| vec![0, 0, 0, 0]);
        assert!(decode(&clear, true).unwrap_err().contains("clear"));
        let odd = png_bytes(1280, 720, png::ColorType::Rgb, &|_, _| vec![1, 2, 3]);
        let err = decode(&odd, false).unwrap_err();
        assert!(err.contains("1280x720"), "{err}");
        assert!(decode(b"not a png", false).is_err());
        let grey = png_bytes(960, 320, png::ColorType::Grayscale, &|_, _| vec![77]);
        assert_eq!(&decode(&grey, false).unwrap().pixels[..3], &[77, 77, 77]);
    }

    #[test]
    fn the_owners_art_is_listed_by_app_id_and_checked_for_size() {
        let dir = scratch("owner");
        let rgb = |w, h| png_bytes(w, h, png::ColorType::Rgb, &|_, _| vec![1, 2, 3]);
        std::fs::write(dir.join("netflix.png"), rgb(1920, 640)).unwrap();
        std::fs::write(dir.join("com.webos.app.hdmi1.png"), rgb(960, 320)).unwrap();
        std::fs::write(dir.join("hulu.png"), rgb(1920, 1080)).unwrap();
        std::fs::write(dir.join("notes.txt"), b"hi").unwrap();
        std::fs::write(dir.join("bad name.png"), rgb(960, 320)).unwrap();
        std::fs::write(dir.join("junk.png"), b"not a png at all, honestly").unwrap();
        std::fs::write(dir.join(".DS_Store"), b"x").unwrap();
        let (apps, refused) = list_owner(&dir);
        let mut ids: Vec<&str> = apps.iter().map(AppId::as_str).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["com.webos.app.hdmi1", "netflix"]);
        assert_eq!(
            refused,
            [
                "\"bad name.png\": not named <app id>.png",
                "\"hulu.png\": 1920x1080; the owner's art is 1920x640 or 960x320",
                "\"junk.png\": not a PNG",
                "\"notes.txt\": not named <app id>.png",
            ]
        );
        let (none, said) = list_owner(&dir.join("missing"));
        assert!(none.is_empty() && said.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Takes the news until a hero comes, decoded or failed.
    fn wait_for_hero(heroes: &mut Heroes) -> Vec<HeroNews> {
        let started = Instant::now();
        let mut all = Vec::new();
        loop {
            let news = heroes.news();
            let done = news
                .iter()
                .any(|n| matches!(n, HeroNews::Decoded { .. } | HeroNews::Failed { .. }));
            all.extend(news);
            if done {
                return all;
            }
            assert!(
                started.elapsed().as_secs() < 30,
                "the loader never answered"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn the_thread_lists_the_owners_art_then_loads_what_is_asked() {
        let owner = scratch("threaded");
        let rgb = png_bytes(960, 320, png::ColorType::Rgb, &|_, _| vec![1, 2, 3]);
        std::fs::write(owner.join("netflix.png"), rgb).unwrap();
        let folders = Folders {
            art: art(),
            owner: owner.clone(),
        };
        let look = Look {
            half: true,
            ..Look::FULL
        };
        let mut heroes = Heroes::threaded(folders, look);
        let hero = Hero::Owner(AppId::new("netflix"));
        heroes.load(&hero);
        let news = wait_for_hero(&mut heroes);
        let HeroNews::OwnerArt { apps, refused } = &news[0] else {
            panic!("the owner's art first: {news:?}")
        };
        assert!(apps.contains(&AppId::new("netflix")) && refused.is_empty());
        let Some(HeroNews::Decoded {
            hero: got,
            layers,
            bytes,
            ..
        }) = news.last()
        else {
            panic!("{news:?}")
        };
        assert_eq!((got, *layers, *bytes), (&hero, 1, 960 * 320 * 4));
        let layers = heroes.take(&hero).expect("its pixels wait");
        assert_eq!(layers[0].pixels.len(), 960 * 320 * 3);
        assert!(heroes.take(&hero).is_none(), "taken once");
        let _ = std::fs::remove_dir_all(&owner);
    }

    #[test]
    fn a_missing_or_corrupt_file_fails_its_hero_naming_the_file() {
        let art = scratch("broken");
        std::fs::write(art.join("hero-plex.png"), b"\x89PNG\r\n\x1a\nnot really").unwrap();
        let folders = Folders {
            art: art.clone(),
            owner: art.join("none"),
        };
        let mut heroes = Heroes::at_once(folders, Look::FULL);
        heroes.load(&Hero::Named("hero-plex"));
        heroes.load(&Hero::Palette(1));
        let news = heroes.news();
        assert!(matches!(&news[0], HeroNews::OwnerArt { apps, .. } if apps.is_empty()));
        let why = |n: &HeroNews| match n {
            HeroNews::Failed { why, .. } => why.clone(),
            other => panic!("{other:?}"),
        };
        assert!(why(&news[1]).starts_with("hero-plex.png: "), "{news:?}");
        assert!(
            why(&news[2]).starts_with("hero-palette-1.png: "),
            "{news:?}"
        );
        assert!(heroes.news().is_empty(), "told once");
        let _ = std::fs::remove_dir_all(&art);
    }
}
