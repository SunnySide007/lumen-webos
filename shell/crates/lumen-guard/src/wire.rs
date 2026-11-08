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

/// Response codes Guard sends itself (RFC 1035, 4.1.1).
pub const FORMERR: u8 = 1;
pub const REFUSED: u8 = 5;
/// Record types Guard answers with a sinkhole address.
pub const TYPE_A: u16 = 1;
pub const TYPE_AAAA: u16 = 28;
const CLASS_IN: u16 = 1;

#[derive(Debug, PartialEq, Eq)]
pub struct Question {
    /// Lower case, in presentation form: labels joined by dots, every octet
    /// other than a letter, digit, `-` or `_` written `\DDD`, the root as `.`.
    pub name: String,
    pub qtype: u16,
    pub qclass: u16,
    /// The offset just past QCLASS: the question is `msg[HEADER..end]`.
    pub end: usize,
    /// Whether the name used a compression pointer, so its bytes can't be
    /// copied into an answer as they are.
    pub compressed: bool,
}

/// The message's ID, which an answer must repeat.
pub fn id(msg: &[u8]) -> Option<[u8; 2]> {
    Some([*msg.first()?, *msg.get(1)?])
}

/// A header with the QR bit clear: a question, not an answer.
pub fn is_query(msg: &[u8]) -> bool {
    msg.len() >= HEADER && msg[2] & 0x80 == 0
}

/// The header's OPCODE: 0 is a standard query.
pub fn opcode(msg: &[u8]) -> u8 {
    msg.get(2).map_or(0, |b| (b >> 3) & 0x0F)
}

/// The header's QDCOUNT.
pub fn question_count(msg: &[u8]) -> u16 {
    match msg.get(4..6) {
        Some(n) => u16::from_be_bytes([n[0], n[1]]),
        None => 0,
    }
}

/// The first question of a message, or `None` when it has none.
pub fn first_question(msg: &[u8]) -> Result<Option<Question>, WireError> {
    if msg.len() < HEADER {
        return Err(WireError::Short);
    }
    if question_count(msg) == 0 {
        return Ok(None);
    }
    let (name, end, compressed) = read_name(msg, HEADER)?;
    let fixed = msg.get(end..end + 4).ok_or(WireError::Truncated)?;
    Ok(Some(Question {
        name,
        qtype: u16::from_be_bytes([fixed[0], fixed[1]]),
        qclass: u16::from_be_bytes([fixed[2], fixed[3]]),
        end: end + 4,
        compressed,
    }))
}

/// The header of Guard's own answer to `query`: its ID, QR set, its opcode
/// and RD bit kept, RA set, then `rcode` and the four counts.
fn answer_header(query: &[u8], rcode: u8, qd: u16, an: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER + 64);
    out.extend_from_slice(&query[..2]);
    out.push(0x80 | (query[2] & 0x79));
    out.push(0x80 | (rcode & 0x0F));
    out.extend_from_slice(&qd.to_be_bytes());
    out.extend_from_slice(&an.to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

/// An answer with no question and no records, only `rcode`: for a query
/// Guard won't forward and can't answer with a sinkhole (FORMERR, REFUSED).
pub fn error_answer(query: &[u8], rcode: u8) -> Vec<u8> {
    answer_header(query, rcode, 0, 0)
}

/// Guard's answer for a blocked name, as a hosts-file sinkhole gives it
/// (decision 11): the question repeated, then 0.0.0.0 for A, `::` for AAAA,
/// and no records for any other type (NOERROR, no data), with `ttl`.
/// `q` must be `query`'s first question and not compressed.
pub fn sinkhole_answer(query: &[u8], q: &Question, ttl: u32) -> Vec<u8> {
    let rdata: &[u8] = match (q.qtype, q.qclass) {
        (TYPE_A, CLASS_IN) => &[0; 4],
        (TYPE_AAAA, CLASS_IN) => &[0; 16],
        _ => &[],
    };
    let an = u16::from(!rdata.is_empty());
    let mut out = answer_header(query, 0, 1, an);
    out.extend_from_slice(&query[HEADER..q.end]);
    if an == 1 {
        // The name as a pointer to the question's, at offset 12.
        out.extend_from_slice(&[0xC0, HEADER as u8]);
        out.extend_from_slice(&q.qtype.to_be_bytes());
        out.extend_from_slice(&CLASS_IN.to_be_bytes());
        out.extend_from_slice(&ttl.to_be_bytes());
        out.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        out.extend_from_slice(rdata);
    }
    out
}

/// The longest UDP answer the asker of `query` takes: 512 bytes (RFC 1035),
/// or the size its EDNS record offers (RFC 6891), at most `MAX_MESSAGE`.
/// A TLS answer has no such limit, so a longer one goes back truncated.
pub fn udp_limit(query: &[u8]) -> usize {
    const PLAIN: usize = 512;
    const TYPE_OPT: u16 = 41;
    let count = |at: usize| match query.get(at..at + 2) {
        Some(n) => usize::from(u16::from_be_bytes([n[0], n[1]])),
        None => 0,
    };
    let (qd, an, ns, ar) = (count(4), count(6), count(8), count(10));
    if ar == 0 || query.len() < HEADER {
        return PLAIN;
    }
    let mut off = HEADER;
    for _ in 0..qd {
        let Some(end) = skip_name(query, off) else {
            return PLAIN;
        };
        off = end + 4;
    }
    for i in 0..an + ns + ar {
        let Some(end) = skip_name(query, off) else {
            return PLAIN;
        };
        let Some(fixed) = query.get(end..end + 10) else {
            return PLAIN;
        };
        let rtype = u16::from_be_bytes([fixed[0], fixed[1]]);
        if i >= an + ns && rtype == TYPE_OPT {
            let size = usize::from(u16::from_be_bytes([fixed[2], fixed[3]]));
            return size.clamp(PLAIN, MAX_MESSAGE);
        }
        off = end + 10 + usize::from(u16::from_be_bytes([fixed[8], fixed[9]]));
    }
    PLAIN
}

/// The offset just past the name at `off`, without reading it.
fn skip_name(msg: &[u8], mut off: usize) -> Option<usize> {
    loop {
        let len = usize::from(*msg.get(off)?);
        match len & 0xC0 {
            0xC0 => return Some(off + 2).filter(|end| *end <= msg.len()),
            0 if len == 0 => return Some(off + 1),
            0 => off += 1 + len,
            _ => return None,
        }
    }
}

/// `answer` cut down to its header and question with TC set (RFC 2181,
/// 9), so the asker asks again over TCP; just the header if its question
/// can't be read.
pub fn truncated(answer: &[u8]) -> Vec<u8> {
    let mut out = answer[..HEADER.min(answer.len())].to_vec();
    out.resize(HEADER, 0);
    out[2] |= 0x02;
    out[6..].fill(0);
    match first_question(answer) {
        Ok(Some(q)) if question_count(answer) == 1 => out.extend_from_slice(&answer[HEADER..q.end]),
        _ => out[4..6].fill(0),
    }
    out
}

/// Reads a possibly compressed name at `off`; returns it, the offset just
/// past it in the message (past the first pointer, if it used one), and
/// whether it used one.
fn read_name(msg: &[u8], mut off: usize) -> Result<(String, usize, bool), WireError> {
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
    Ok((name, end.unwrap_or(off), end.is_some()))
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
    fn reads_class_end_and_compression() {
        let msg = query(1, "a.example", 28);
        let q = first_question(&msg).unwrap().unwrap();
        assert_eq!((q.qclass, q.end, q.compressed), (1, msg.len(), false));
        let mut msg = header(1);
        msg.extend_from_slice(b"\x03www\xC0\x16");
        msg.extend_from_slice(&[0, 1, 0, 1]);
        msg.extend_from_slice(b"\x07example\x03com\x00");
        let q = first_question(&msg).unwrap().unwrap();
        assert_eq!((q.end, q.compressed), (0x16, true));
        assert_eq!(opcode(&msg), 0);
        assert_eq!(question_count(&msg), 1);
    }

    #[test]
    fn sinkhole_answers_for_a_aaaa_and_other_types() {
        for (qtype, rdata) in [(TYPE_A, &[0u8; 4][..]), (TYPE_AAAA, &[0; 16]), (65, &[])] {
            let msg = query(0xBEEF, "Ads.Example", qtype);
            let q = first_question(&msg).unwrap().unwrap();
            let a = sinkhole_answer(&msg, &q, 60);
            assert!(!is_query(&a));
            assert_eq!(id(&a), Some([0xBE, 0xEF]));
            // RD kept, RA set, NOERROR.
            assert_eq!((a[2], a[3]), (0x81, 0x80));
            assert_eq!(question_count(&a), 1);
            let an = u16::from_be_bytes([a[6], a[7]]);
            // The question repeated as it came, case and all.
            assert_eq!(&a[HEADER..q.end], &msg[HEADER..q.end]);
            if rdata.is_empty() {
                assert_eq!((an, a.len()), (0, q.end));
                continue;
            }
            assert_eq!(an, 1);
            let rr = &a[q.end..];
            assert_eq!(&rr[..2], &[0xC0, 12]);
            assert_eq!(u16::from_be_bytes([rr[2], rr[3]]), qtype);
            assert_eq!(u16::from_be_bytes([rr[4], rr[5]]), 1);
            assert_eq!(u32::from_be_bytes([rr[6], rr[7], rr[8], rr[9]]), 60);
            assert_eq!(
                usize::from(u16::from_be_bytes([rr[10], rr[11]])),
                rdata.len()
            );
            assert_eq!(&rr[12..], rdata);
            // The answer's own question reads back as the query's.
            assert_eq!(first_question(&a).unwrap().unwrap().name, "ads.example");
        }
    }

    #[test]
    fn another_class_gets_no_records() {
        let mut msg = query(2, "x.example", 1);
        let n = msg.len();
        msg[n - 1] = 3; // CHAOS
        let q = first_question(&msg).unwrap().unwrap();
        let a = sinkhole_answer(&msg, &q, 60);
        assert_eq!(u16::from_be_bytes([a[6], a[7]]), 0);
    }

    #[test]
    fn error_answers_carry_only_the_code() {
        let mut msg = query(0x0102, "x.example", 1);
        msg[2] = 0x29; // opcode 5 (UPDATE), RD
        assert_eq!(opcode(&msg), 5);
        let a = error_answer(&msg, REFUSED);
        assert_eq!(a.len(), HEADER);
        assert_eq!(id(&a), Some([1, 2]));
        assert_eq!(opcode(&a), 5);
        assert_eq!(a[3] & 0x0F, REFUSED);
        assert_eq!(question_count(&a), 0);
        assert!(!is_query(&a));
    }

    /// `msg` with an EDNS OPT record offering `size` bytes over UDP.
    fn with_edns(mut msg: Vec<u8>, size: u16) -> Vec<u8> {
        msg[11] = 1; // ARCOUNT
        msg.push(0); // the root name
        msg.extend_from_slice(&41u16.to_be_bytes());
        msg.extend_from_slice(&size.to_be_bytes());
        msg.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // TTL, RDLENGTH
        msg
    }

    #[test]
    fn the_udp_limit_follows_edns() {
        let plain = query(1, "www.example", 1);
        assert_eq!(udp_limit(&plain), 512);
        assert_eq!(udp_limit(&with_edns(plain.clone(), 1232)), 1232);
        // Below the minimum, or above Guard's buffer: clamped.
        assert_eq!(udp_limit(&with_edns(plain.clone(), 100)), 512);
        assert_eq!(udp_limit(&with_edns(plain.clone(), 65000)), MAX_MESSAGE);
        // A record that runs past the end, or no OPT: the plain limit.
        let mut cut = with_edns(plain.clone(), 1232);
        cut.truncate(cut.len() - 8);
        assert_eq!(udp_limit(&cut), 512);
        let mut not_opt = with_edns(plain, 1232);
        let at = not_opt.len() - 10;
        not_opt[at + 1] = 1;
        assert_eq!(udp_limit(&not_opt), 512);
        assert_eq!(udp_limit(&[0; 4]), 512);
    }

    #[test]
    fn truncated_answers_keep_the_question_and_set_tc() {
        let q = query(0x0A0B, "big.example", 1);
        let mut answer = q.clone();
        answer[2] |= 0x80;
        answer[7] = 3; // ANCOUNT
        answer[11] = 1; // ARCOUNT
        answer.extend(std::iter::repeat_n(0xEE, 900));
        let t = truncated(&answer);
        assert_eq!(&t[..2], &[0x0A, 0x0B]);
        assert!(!is_query(&t));
        assert_ne!(t[2] & 0x02, 0);
        assert_eq!(question_count(&t), 1);
        assert_eq!(&t[6..12], &[0; 6]);
        assert_eq!(&t[HEADER..], &q[HEADER..]);
        // An unreadable question: the header alone.
        let t = truncated(&answer[..HEADER + 3]);
        assert_eq!(t.len(), HEADER);
        assert_eq!(question_count(&t), 0);
    }

    #[test]
    fn unknown_types_use_rfc_3597_names() {
        assert_eq!(Type(65).to_string(), "HTTPS");
        assert_eq!(Type(99).to_string(), "TYPE99");
    }
}
