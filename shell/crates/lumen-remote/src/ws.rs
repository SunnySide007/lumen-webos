//! WebSocket (RFC 6455), the server's half, by hand: the handshake's
//! accept key, and frames. A client's frames must be masked; a message is
//! at most `MAX_MESSAGE` bytes, fragments included; control frames are at
//! most 125 bytes and never fragmented. Text must be UTF-8. The server
//! sends each line as one unmasked text frame.

use ring::digest;

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// A protocol line (64 KiB) and a newline, with room.
pub const MAX_MESSAGE: usize = 65_536 + 1024;

const CONTINUATION: u8 = 0;
pub const TEXT: u8 = 1;
const BINARY: u8 = 2;
pub const CLOSE: u8 = 8;
pub const PING: u8 = 9;
pub const PONG: u8 = 10;

/// `Sec-WebSocket-Accept` for a client's `Sec-WebSocket-Key`.
pub fn accept_key(key: &str) -> String {
    let d = digest::digest(
        &digest::SHA1_FOR_LEGACY_USE_ONLY,
        format!("{key}{GUID}").as_bytes(),
    );
    base64(d.as_ref())
}

/// A key is 16 bytes in base64: 24 characters ending in "==".
pub fn good_key(key: &str) -> bool {
    key.len() == 24
        && key.ends_with("==")
        && key[..22]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

pub fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(A[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// One frame for the client.
pub fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 10);
    out.push(0x80 | opcode);
    match payload.len() {
        n @ 0..=125 => out.push(n as u8),
        n @ 126..=65_535 => {
            out.push(126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    out
}

/// A close frame with a code and a short reason.
pub fn close_frame(code: u16, reason: &str) -> Vec<u8> {
    let mut payload = code.to_be_bytes().to_vec();
    payload.extend_from_slice(&reason.as_bytes()[..reason.len().min(120)]);
    frame(CLOSE, &payload)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Message {
    Text(String),
    Ping(Vec<u8>),
    /// The client closes; its code, if it gave one.
    Close(Option<u16>),
    /// A pong or an empty continuation: nothing to do.
    Nothing,
}

/// A client's messages, put back together from its frames.
#[derive(Default)]
pub struct Reader {
    partial: Vec<u8>,
    in_text: bool,
}

impl Reader {
    /// The next message in `buf`, and how many bytes it took; `Ok(None)` if
    /// more are needed. An error closes the connection with 1002 (or 1009
    /// for a message too long, 1007 for text that isn't UTF-8).
    /// One frame at a time: a fragment that doesn't end its message is
    /// taken and gives `Nothing`.
    pub fn next(&mut self, buf: &[u8]) -> Result<Option<(Message, usize)>, (u16, &'static str)> {
        let Some((fin, opcode, payload, used)) = raw_frame(buf)? else {
            return Ok(None);
        };
        let message = match opcode {
            TEXT | CONTINUATION => {
                if (opcode == TEXT) == self.in_text {
                    return Err((1002, "a frame out of order"));
                }
                if self.partial.len() + payload.len() > MAX_MESSAGE {
                    return Err((1009, "a message too long"));
                }
                self.partial.extend_from_slice(&payload);
                self.in_text = !fin;
                if !fin {
                    return Ok(Some((Message::Nothing, used)));
                }
                let bytes = std::mem::take(&mut self.partial);
                Message::Text(
                    String::from_utf8(bytes).map_err(|_| (1007, "text that isn't UTF-8"))?,
                )
            }
            BINARY => return Err((1003, "no binary messages")),
            CLOSE => Message::Close(
                (payload.len() >= 2).then(|| u16::from_be_bytes([payload[0], payload[1]])),
            ),
            PING => Message::Ping(payload),
            PONG => Message::Nothing,
            _ => return Err((1002, "an unknown opcode")),
        };
        Ok(Some((message, used)))
    }
}

type Raw = (bool, u8, Vec<u8>, usize);

/// One frame, unmasked: (fin, opcode, payload, bytes used).
fn raw_frame(buf: &[u8]) -> Result<Option<Raw>, (u16, &'static str)> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let (b0, b1) = (buf[0], buf[1]);
    if b0 & 0x70 != 0 {
        return Err((1002, "reserved bits set"));
    }
    let (fin, opcode) = (b0 & 0x80 != 0, b0 & 0x0f);
    if b1 & 0x80 == 0 {
        return Err((1002, "a client frame without a mask"));
    }
    let (len, mut at) = match b1 & 0x7f {
        126 => {
            if buf.len() < 4 {
                return Ok(None);
            }
            (u64::from(u16::from_be_bytes([buf[2], buf[3]])), 4)
        }
        127 => {
            if buf.len() < 10 {
                return Ok(None);
            }
            let mut b = [0u8; 8];
            b.copy_from_slice(&buf[2..10]);
            (u64::from_be_bytes(b), 10)
        }
        n => (u64::from(n), 2),
    };
    if opcode >= 8 && (len > 125 || !fin) {
        return Err((1002, "a bad control frame"));
    }
    if len > MAX_MESSAGE as u64 {
        return Err((1009, "a message too long"));
    }
    let len = len as usize;
    if buf.len() < at + 4 + len {
        return Ok(None);
    }
    let mask = [buf[at], buf[at + 1], buf[at + 2], buf[at + 3]];
    at += 4;
    let payload: Vec<u8> = buf[at..at + len]
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ mask[i % 4])
        .collect();
    Ok(Some((fin, opcode, payload, at + len)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A client's frame, masked.
    fn masked(b0: u8, payload: &[u8]) -> Vec<u8> {
        let mask = [0x37, 0xfa, 0x21, 0x3d];
        let mut out = vec![b0];
        match payload.len() {
            n @ 0..=125 => out.push(0x80 | n as u8),
            n => {
                out.push(0x80 | 126);
                out.extend_from_slice(&(n as u16).to_be_bytes());
            }
        }
        out.extend_from_slice(&mask);
        out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        out
    }

    #[test]
    fn computes_rfc_6455s_example_key() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
        assert!(good_key("dGhlIHNhbXBsZSBub25jZQ=="));
        assert!(!good_key("short") && !good_key("dGhlIHNhbXBsZSBub25jZQ=x"));
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"f"), "Zg==");
    }

    #[test]
    fn reads_a_whole_message() {
        let mut r = Reader::default();
        let one = masked(0x81, b"{\"id\":1}");
        assert_eq!(
            r.next(&one).unwrap(),
            Some((Message::Text("{\"id\":1}".into()), one.len()))
        );
    }

    #[test]
    fn a_ping_between_fragments_comes_out_on_its_own() {
        let mut r = Reader::default();
        let first = masked(0x01, b"hel");
        let ping = masked(0x89, b"p");
        let last = masked(0x80, "lo é".as_bytes());
        // The first fragment gives nothing yet, and is taken.
        let mut buf = first.clone();
        buf.extend(&ping);
        assert_eq!(r.next(&buf).unwrap(), Some((Message::Nothing, first.len())));
        let (m, n) = r.next(&buf[first.len()..]).unwrap().unwrap();
        assert_eq!(m, Message::Ping(b"p".to_vec()));
        assert_eq!(n, ping.len());
        let (m, n) = r.next(&last).unwrap().unwrap();
        assert_eq!((m, n), (Message::Text("hello é".into()), last.len()));
        // Waiting for more: half a frame.
        assert_eq!(r.next(&last[..3]).unwrap(), None);
    }

    #[test]
    fn refuses_bad_frames() {
        let err = |buf: &[u8]| Reader::default().next(buf).unwrap_err().0;
        // No mask.
        assert_eq!(err(&[0x81, 0x01, b'x']), 1002);
        // Binary, unknown opcode, reserved bit.
        assert_eq!(err(&masked(0x82, b"x")), 1003);
        assert_eq!(err(&masked(0x83, b"x")), 1002);
        assert_eq!(err(&masked(0xc1, b"x")), 1002);
        // A continuation with nothing to continue, a fragmented ping.
        assert_eq!(err(&masked(0x80, b"x")), 1002);
        assert_eq!(err(&masked(0x09, b"x")), 1002);
        // Not UTF-8.
        assert_eq!(err(&masked(0x81, &[0xff])), 1007);
        // Too long, by its header alone.
        let mut huge = vec![0x81, 0x80 | 127];
        huge.extend_from_slice(&(1u64 << 32).to_be_bytes());
        assert_eq!(err(&huge), 1009);
        // A close carries its code.
        let close = masked(0x88, &1000u16.to_be_bytes());
        assert_eq!(
            Reader::default().next(&close).unwrap().unwrap().0,
            Message::Close(Some(1000))
        );
    }

    #[test]
    fn writes_frames_with_each_length_form() {
        assert_eq!(frame(TEXT, b"hi"), [0x81, 2, b'h', b'i']);
        let mid = frame(TEXT, &[b'x'; 300]);
        assert_eq!(&mid[..4], &[0x81, 126, 1, 44]);
        let big = frame(TEXT, &vec![b'x'; 70_000]);
        assert_eq!(&big[..2], &[0x81, 127]);
        assert_eq!(big.len(), 70_000 + 10);
        assert_eq!(close_frame(4401, "revoked")[..4], [0x88, 9, 0x11, 0x31]);
    }
}
