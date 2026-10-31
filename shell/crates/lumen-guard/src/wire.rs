//! The little of DNS that Guard reads: a query's header and the name and type
//! of its first question. Everything else in a message passes through as bytes.

use std::fmt::{self, Write as _};

/// A DNS header's size; anything shorter is not a DNS message.
pub const HEADER: usize = 12;
/// The largest message Guard carries in one buffer. Queries and UDP answers
/// above it are dropped; TCP answers stream through in pieces of this size.
pub const MAX_MESSAGE: usize = 4096;
/// A name's longest wire form, length octets included (RFC 1035, 2.3.4).
const MAX_NAME: usize = 255;
/// Compression pointers followed before a name counts as a loop.
const MAX_JUMPS: usize = 32;

#[derive(Debug, PartialEq, Eq)]
pub enum WireError {
    /// Shorter than a header.
    Short,
    /// The question runs past the end of the message.
    Truncated,
    /// More compression pointers than a real name needs.
    PointerLoop,
    /// A label type other than a plain length or a pointer.
    BadLabel,
    /// Longer than 255 octets.
    NameTooLong,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Question {
    /// Lower case, in presentation form: labels joined by dots, every octet
    /// other than a letter, digit, `-` or `_` written `\DDD`, the root as `.`.
    pub name: String,
    pub qtype: u16,
}

/// The message's ID, which an answer must repeat.
pub fn id(msg: &[u8]) -> Option<[u8; 2]> {
    Some([*msg.first()?, *msg.get(1)?])
}

/// A header with the QR bit clear: a question, not an answer.
pub fn is_query(msg: &[u8]) -> bool {
    msg.len() >= HEADER && msg[2] & 0x80 == 0
}

/// The first question of a message, or `None` when it has none.
pub fn first_question(msg: &[u8]) -> Result<Option<Question>, WireError> {
    if msg.len() < HEADER {
        return Err(WireError::Short);
    }
    if u16::from_be_bytes([msg[4], msg[5]]) == 0 {
        return Ok(None);
    }
    let (name, end) = read_name(msg, HEADER)?;
    // QTYPE, then QCLASS, which Guard does not need.
    let fixed = msg.get(end..end + 4).ok_or(WireError::Truncated)?;
    Ok(Some(Question {
        name,
        qtype: u16::from_be_bytes([fixed[0], fixed[1]]),
    }))
}

/// Reads a possibly compressed name at `off`; returns it and the offset just
/// past it in the message (past the first pointer, if it used one).
fn read_name(msg: &[u8], mut off: usize) -> Result<(String, usize), WireError> {
    let mut name = String::new();
    let mut end = None;
    let mut jumps = 0;
    // The root label's zero octet.
    let mut wire_len = 1;
    loop {
        let len = usize::from(*msg.get(off).ok_or(WireError::Truncated)?);
        match len & 0xC0 {
            0xC0 => {
                let low = usize::from(*msg.get(off + 1).ok_or(WireError::Truncated)?);
                end.get_or_insert(off + 2);
                jumps += 1;
                if jumps > MAX_JUMPS {
                    return Err(WireError::PointerLoop);
                }
                off = ((len & 0x3F) << 8) | low;
            }
            0 => {
                off += 1;
                if len == 0 {
                    break;
                }
                wire_len += 1 + len;
                if wire_len > MAX_NAME {
                    return Err(WireError::NameTooLong);
                }
                let label = msg.get(off..off + len).ok_or(WireError::Truncated)?;
                if !name.is_empty() {
                    name.push('.');
                }
                push_label(&mut name, label);
                off += len;
            }
            _ => return Err(WireError::BadLabel),
        }
    }
    if name.is_empty() {
        name.push('.');
    }
    Ok((name, end.unwrap_or(off)))
}

/// Names come off the wire: only plain host-name octets are written as they
/// are, so a name is always one word on one line of the log.
fn push_label(name: &mut String, label: &[u8]) {
    for &b in label {
        let b = b.to_ascii_lowercase();
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
            name.push(char::from(b));
        } else {
            let _ = write!(name, "\\{b:03}");
        }
    }
}

/// A query for `name` (plain labels, no escapes) with recursion desired, as
/// `--probe` sends it.
pub fn query(id: u16, name: &str, qtype: u16) -> Vec<u8> {
    let mut msg = Vec::with_capacity(HEADER + name.len() + 6);
    msg.extend_from_slice(&id.to_be_bytes());
    // Flags: RD. One question, no records.
    msg.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.split('.').filter(|l| !l.is_empty()) {
        let label = &label.as_bytes()[..label.len().min(63)];
        msg.push(label.len() as u8);
        msg.extend_from_slice(label);
    }
    msg.push(0);
    msg.extend_from_slice(&qtype.to_be_bytes());
    // Class IN.
    msg.extend_from_slice(&[0, 1]);
    msg
}

/// A query type as the log writes it: its mnemonic, or `TYPE<n>` (RFC 3597).
pub struct Type(pub u16);

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            1 => "A",
            2 => "NS",
            5 => "CNAME",
            6 => "SOA",
            12 => "PTR",
            15 => "MX",
            16 => "TXT",
            28 => "AAAA",
            33 => "SRV",
            35 => "NAPTR",
            43 => "DS",
            48 => "DNSKEY",
            64 => "SVCB",
            65 => "HTTPS",
            255 => "ANY",
            n => return write!(f, "TYPE{n}"),
        };
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(qdcount: u16) -> Vec<u8> {
        let mut h = vec![0xAB, 0xCD, 0x01, 0x00];
        h.extend_from_slice(&qdcount.to_be_bytes());
        h.extend_from_slice(&[0; 6]);
        h
    }

    #[test]
    fn reads_a_plain_question() {
        let msg = query(0x1234, "WWW.Netflix.com", 28);
        assert!(is_query(&msg));
        assert_eq!(id(&msg), Some([0x12, 0x34]));
        let q = first_question(&msg).unwrap().unwrap();
        assert_eq!(q.name, "www.netflix.com");
        assert_eq!(q.qtype, 28);
        assert_eq!(Type(q.qtype).to_string(), "AAAA");
    }

    #[test]
    fn follows_a_compression_pointer() {
        // "www" then a pointer to "example.com", stored after the question.
        let mut msg = header(1);
        msg.extend_from_slice(b"\x03www\xC0\x16");
        msg.extend_from_slice(&[0, 1, 0, 1]);
        assert_eq!(msg.len(), 0x16);
        msg.extend_from_slice(b"\x07example\x03com\x00");
        let q = first_question(&msg).unwrap().unwrap();
        assert_eq!(q.name, "www.example.com");
        // The type is read right after the pointer, not after its target.
        assert_eq!(q.qtype, 1);
    }

    #[test]
    fn rejects_pointer_loops() {
        // A pointer to itself, and two pointers to each other.
        let mut msg = header(1);
        msg.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1]);
        assert_eq!(first_question(&msg), Err(WireError::PointerLoop));
        let mut msg = header(1);
        msg.extend_from_slice(&[0xC0, 14, 0xC0, 12, 0, 1, 0, 1]);
        assert_eq!(first_question(&msg), Err(WireError::PointerLoop));
        // A label that points back at itself grows until it is too long.
        let mut msg = header(1);
        msg.push(63);
        msg.extend_from_slice(&[b'a'; 63]);
        msg.extend_from_slice(&[0xC0, 12]);
        assert_eq!(first_question(&msg), Err(WireError::NameTooLong));
    }

    #[test]
    fn rejects_truncated_input() {
        let full = query(1, "lumen-probe.invalid", 1);
        assert_eq!(first_question(&full[..5]), Err(WireError::Short));
        // Cut inside the name, inside the type, and inside a pointer.
        for cut in [HEADER, HEADER + 3, full.len() - 3, full.len() - 1] {
            assert_eq!(
                first_question(&full[..cut]),
                Err(WireError::Truncated),
                "cut at {cut}"
            );
        }
        let mut msg = header(1);
        msg.push(0xC0);
        assert_eq!(first_question(&msg), Err(WireError::Truncated));
    }

    #[test]
    fn a_message_without_questions_has_none() {
        assert_eq!(first_question(&header(0)), Ok(None));
    }

    #[test]
    fn rejects_extended_label_types() {
        let mut msg = header(1);
        msg.extend_from_slice(&[0x41, 0, 0, 1, 0, 1]);
        assert_eq!(first_question(&msg), Err(WireError::BadLabel));
    }

    #[test]
    fn the_root_and_odd_octets_stay_one_word() {
        let mut msg = header(1);
        msg.extend_from_slice(&[0, 0, 2, 0, 1]);
        assert_eq!(first_question(&msg).unwrap().unwrap().name, ".");
        let mut msg = header(1);
        msg.extend_from_slice(b"\x07a b\nc.d\x03com\x00\x00\x01\x00\x01");
        let name = first_question(&msg).unwrap().unwrap().name;
        assert_eq!(name, "a\\032b\\010c\\046d.com");
        assert!(!name.contains(char::is_whitespace));
    }

    #[test]
    fn answers_are_not_queries() {
        let mut msg = query(7, "a.example", 1);
        msg[2] |= 0x80;
        assert!(!is_query(&msg));
        assert!(!is_query(&msg[..4]));
    }

    #[test]
    fn unknown_types_use_rfc_3597_names() {
        assert_eq!(Type(65).to_string(), "HTTPS");
        assert_eq!(Type(99).to_string(), "TYPE99");
    }
}
