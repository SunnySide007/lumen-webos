//! One texture for everything the shell draws from pixels: app icons and the
//! glyphs of its text. Keeping both in one 2048x1024 RGBA texture (8 MB) keeps
//! a frame to one draw call. This side decides where things go and holds the
//! pixels waiting to be uploaded; `render::Renderer` owns the GL texture.

pub const ATLAS_W: u32 = 2048;
pub const ATLAS_H: u32 = 1024;
/// Transparent pixels around each item, so linear filtering at its edge never
/// picks up a neighbour.
const PAD: u32 = 1;

/// Where an item sits, in texture coordinates (0 to 1, v down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Uv {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

/// Premultiplied RGBA pixels waiting to go to the GL texture at (x, y).
#[derive(Debug)]
pub struct Upload {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// A row of items of about the same height (a "shelf"), filled left to right.
#[derive(Debug)]
struct Shelf {
    y: u32,
    h: u32,
    x: u32,
}

#[derive(Debug, Default)]
pub struct Atlas {
    shelves: Vec<Shelf>,
    pending: Vec<Upload>,
    generation: u32,
}

impl Atlas {
    /// Changes whenever the atlas is emptied: anything placed before must be
    /// placed again.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Places a `w` x `h` image of premultiplied RGBA, or `None` when it does
    /// not fit in what is left (or at all).
    pub fn insert(&mut self, w: u32, h: u32, rgba: &[u8]) -> Option<Uv> {
        assert_eq!(
            rgba.len(),
            (w * h * 4) as usize,
            "{w}x{h} needs {} bytes",
            w * h * 4
        );
        let (pw, ph) = (w + 2 * PAD, h + 2 * PAD);
        let (x, y) = self.place(pw, ph)?;
        let mut padded = vec![0; (pw * ph * 4) as usize];
        let row = (w * 4) as usize;
        for (r, src) in rgba.chunks_exact(row.max(1)).enumerate().take(h as usize) {
            let at = (((r as u32 + PAD) * pw + PAD) * 4) as usize;
            padded[at..at + row].copy_from_slice(src);
        }
        self.pending.push(Upload {
            x,
            y,
            w: pw,
            h: ph,
            rgba: padded,
        });
        Some(Uv {
            u0: (x + PAD) as f32 / ATLAS_W as f32,
            v0: (y + PAD) as f32 / ATLAS_H as f32,
            u1: (x + PAD + w) as f32 / ATLAS_W as f32,
            v1: (y + PAD + h) as f32 / ATLAS_H as f32,
        })
    }

    /// The top-left corner for a `w` x `h` cell: on the snuggest shelf with
    /// room, else on a new shelf below the last.
    fn place(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > ATLAS_W || h > ATLAS_H {
            return None;
        }
        // A shelf taller than a third again wastes too much of its height.
        let best = self
            .shelves
            .iter_mut()
            .filter(|s| s.h >= h && s.h <= h + h / 3 + 2 && s.x + w <= ATLAS_W)
            .min_by_key(|s| s.h);
        if let Some(shelf) = best {
            let x = shelf.x;
            shelf.x += w;
            return Some((x, shelf.y));
        }
        let y = self.shelves.last().map_or(0, |s| s.y + s.h);
        if y + h > ATLAS_H {
            return None;
        }
        self.shelves.push(Shelf { y, h, x: w });
        Some((0, y))
    }

    /// Empties the atlas. What the texture still holds is never sampled again.
    pub fn clear(&mut self) {
        self.shelves.clear();
        self.pending.clear();
        self.generation = self.generation.wrapping_add(1);
    }

    /// The pixels placed since the last call, for the renderer to upload.
    pub fn take_uploads(&mut self) -> Vec<Upload> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32) -> Vec<u8> {
        vec![255; (w * h * 4) as usize]
    }

    #[test]
    fn an_item_is_uploaded_with_a_transparent_border() {
        let mut atlas = Atlas::default();
        let uv = atlas.insert(2, 1, &solid(2, 1)).expect("fits");
        let uploads = atlas.take_uploads();
        assert_eq!(uploads.len(), 1);
        let up = &uploads[0];
        assert_eq!((up.x, up.y, up.w, up.h), (0, 0, 4, 3));
        let alpha: Vec<u8> = up.rgba.chunks(4).map(|p| p[3]).collect();
        assert_eq!(alpha, [0, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 0]);
        assert_eq!(uv.u0, 1.0 / ATLAS_W as f32);
        assert_eq!(uv.u1, 3.0 / ATLAS_W as f32);
        assert_eq!((uv.v0, uv.v1), (1.0 / ATLAS_H as f32, 2.0 / ATLAS_H as f32));
        assert!(
            atlas.take_uploads().is_empty(),
            "each upload is handed over once"
        );
    }

    #[test]
    fn items_of_a_height_share_a_shelf_and_taller_ones_start_another() {
        let mut atlas = Atlas::default();
        atlas.insert(10, 20, &solid(10, 20)).unwrap();
        atlas.insert(10, 18, &solid(10, 18)).unwrap();
        atlas.insert(10, 60, &solid(10, 60)).unwrap();
        let at: Vec<(u32, u32)> = atlas.take_uploads().iter().map(|u| (u.x, u.y)).collect();
        assert_eq!(at, [(0, 0), (12, 0), (0, 22)]);
    }

    #[test]
    fn items_never_overlap() {
        let mut atlas = Atlas::default();
        let mut placed = Vec::new();
        for i in 0..400u32 {
            let (w, h) = (8 + i * 7 % 120, 8 + i * 13 % 130);
            if atlas.insert(w, h, &solid(w, h)).is_none() {
                break;
            }
        }
        for up in atlas.take_uploads() {
            assert!(up.x + up.w <= ATLAS_W && up.y + up.h <= ATLAS_H);
            placed.push(up);
        }
        assert!(placed.len() > 50);
        for (i, a) in placed.iter().enumerate() {
            for b in &placed[i + 1..] {
                let apart =
                    a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn a_full_atlas_refuses_until_cleared() {
        let mut atlas = Atlas::default();
        let big = ATLAS_W - 2;
        let tall = ATLAS_H / 2 - 2;
        assert!(atlas.insert(big, tall, &solid(big, tall)).is_some());
        assert!(atlas.insert(big, tall, &solid(big, tall)).is_some());
        assert!(atlas.insert(1, 1, &solid(1, 1)).is_none());
        let before = atlas.generation();
        atlas.clear();
        assert_ne!(atlas.generation(), before);
        assert!(
            atlas.take_uploads().is_empty(),
            "cleared uploads are dropped"
        );
        assert!(atlas.insert(1, 1, &solid(1, 1)).is_some());
    }

    #[test]
    fn an_item_bigger_than_the_atlas_never_fits() {
        let mut atlas = Atlas::default();
        assert!(atlas.insert(ATLAS_W, 1, &solid(ATLAS_W, 1)).is_none());
    }

    #[test]
    fn an_empty_item_still_gets_a_place() {
        let mut atlas = Atlas::default();
        assert!(atlas.insert(0, 0, &[]).is_some());
    }
}
