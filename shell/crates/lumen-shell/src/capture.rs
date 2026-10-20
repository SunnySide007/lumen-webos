//! Screenshots of the shell's own GL frame, as PNG, using only std.
//!
//! The image is stored uncompressed (zlib "stored" blocks): a 1920x1080 frame
//! is about 6 MB, and in exchange the encoder is small enough to read in one
//! sitting and needs no dependency. On the TV this captures the UI plane only:
//! video sits on a hardware plane GL cannot read.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
const MAX_STORED: usize = 65_535;

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

pub fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65_521;
        b %= 65_521;
    }
    (b << 16) | a
}

fn chunk(out: &mut impl Write, kind: &[u8; 4], data: &[u8]) -> io::Result<()> {
    let len = u32::try_from(data.len()).map_err(|_| io::Error::other("PNG chunk too large"))?;
    out.write_all(&len.to_be_bytes())?;
    out.write_all(kind)?;
    out.write_all(data)?;
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.write_all(&crc32(&crc_input).to_be_bytes())
}

/// A zlib stream of stored (uncompressed) deflate blocks.
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let blocks = raw.len().div_ceil(MAX_STORED).max(1);
    let mut out = Vec::with_capacity(raw.len() + blocks * 5 + 6);
    out.extend_from_slice(&[0x78, 0x01]);
    let mut pieces = raw.chunks(MAX_STORED).peekable();
    if pieces.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(piece) = pieces.next() {
        let last = pieces.peek().is_none();
        let len = piece.len() as u16;
        out.push(u8::from(last));
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(piece);
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// Encodes `rgba` (`width * height * 4` bytes, rows top to bottom) as an RGB PNG.
pub fn encode_png(out: &mut impl Write, width: u32, height: u32, rgba: &[u8]) -> io::Result<()> {
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(io::Error::other(
            "pixel buffer does not match the image size",
        ));
    }
    let mut raw = Vec::with_capacity(height as usize * (1 + width as usize * 3));
    for row in rgba.chunks_exact(width as usize * 4) {
        raw.push(0); // filter: none
        for px in row.as_chunks::<4>().0 {
            raw.extend_from_slice(&px[..3]);
        }
    }
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, RGB, deflate, no filter set, no interlace

    out.write_all(&SIGNATURE)?;
    chunk(out, b"IHDR", &ihdr)?;
    chunk(out, b"IDAT", &zlib_stored(&raw))?;
    chunk(out, b"IEND", &[])
}

/// Writes a GL readback (rows bottom to top) to `path` as a PNG.
pub fn save_gl_frame(
    path: &Path,
    width: u32,
    height: u32,
    bottom_up_rgba: &[u8],
) -> io::Result<()> {
    let row = width as usize * 4;
    let top_down: Vec<u8> = bottom_up_rgba
        .chunks_exact(row)
        .rev()
        .flatten()
        .copied()
        .collect();
    let mut file = BufWriter::new(File::create(path)?);
    encode_png(&mut file, width, height, &top_down)?;
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_match_reference_values() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(&[]), 1);
    }

    #[test]
    fn stored_blocks_split_at_64k_and_mark_the_last() {
        let raw = vec![7u8; MAX_STORED + 10];
        let z = zlib_stored(&raw);
        assert_eq!(&z[..2], &[0x78, 0x01]);
        assert_eq!(z[2], 0, "first block is not final");
        let second = 2 + 5 + MAX_STORED;
        assert_eq!(z[second], 1, "second block is final");
        assert_eq!(&z[second + 1..second + 3], &10u16.to_le_bytes());
        assert_eq!(z.len(), 2 + 5 + MAX_STORED + 5 + 10 + 4);
    }

    #[test]
    fn png_has_signature_header_and_end() {
        let mut out = Vec::new();
        encode_png(&mut out, 2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).expect("encodes");
        assert_eq!(&out[..8], &SIGNATURE);
        assert_eq!(&out[12..16], b"IHDR");
        assert_eq!(&out[16..24], &[0, 0, 0, 2, 0, 0, 0, 1]);
        assert_eq!(&out[out.len() - 8..out.len() - 4], b"IEND");
    }

    #[test]
    fn rejects_a_buffer_of_the_wrong_size() {
        assert!(encode_png(&mut Vec::new(), 2, 2, &[0; 4]).is_err());
    }
}
