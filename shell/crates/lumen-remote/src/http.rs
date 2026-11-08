//! HTTP/1.1, the little of it a browser needs here: GET and HEAD for the
//! page, POST for the pairing form, and the WebSocket upgrade. Written by
//! hand and strict: a head of at most 8 KiB, a body of at most 2 KiB with a
//! Content-Length (no chunked bodies), one Host, no folded headers.

pub const MAX_HEAD: usize = 8 << 10;
pub const MAX_BODY: usize = 2 << 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    /// HTTP/1.0 closes after the reply unless it asks otherwise.
    pub http11: bool,
    /// Names in lower case, in order.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Whether a comma-separated header holds `token`, in any case.
    pub fn has_token(&self, name: &str, token: &str) -> bool {
        self.headers
            .iter()
            .filter(|(n, _)| n == name)
            .flat_map(|(_, v)| v.split(','))
            .any(|t| t.trim().eq_ignore_ascii_case(token))
    }

    pub fn keep_alive(&self) -> bool {
        if self.has_token("connection", "close") {
            return false;
        }
        self.http11 || self.has_token("connection", "keep-alive")
    }

    /// A cookie's value, from any Cookie header.
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n == "cookie")
            .flat_map(|(_, v)| v.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.trim())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parse {
    /// More bytes are needed.
    More,
    /// A request, and how many bytes of the buffer it took.
    Done(Request, usize),
    /// Refused with this status; the connection closes after it.
    Bad(u16, &'static str),
}

fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

pub fn parse(buf: &[u8]) -> Parse {
    let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if buf.len() > MAX_HEAD {
            Parse::Bad(431, "the request's head is too long")
        } else {
            Parse::More
        };
    };
    if end > MAX_HEAD {
        return Parse::Bad(431, "the request's head is too long");
    }
    let Ok(head) = std::str::from_utf8(&buf[..end]) else {
        return Parse::Bad(400, "the request's head is not text");
    };
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Parse::Bad(400, "a bad request line");
    };
    let http11 = match version {
        "HTTP/1.1" => true,
        "HTTP/1.0" => false,
        _ => return Parse::Bad(505, "HTTP/1.1 only"),
    };
    if !is_token(method) {
        return Parse::Bad(400, "a bad method");
    }
    if !target.starts_with('/') || target.bytes().any(|b| b <= b' ' || b >= 0x7f) {
        return Parse::Bad(400, "a bad target");
    }
    // The query is ignored: nothing here reads one.
    let path = target.split(['?', '#']).next().unwrap_or("/").to_owned();
    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Parse::Bad(400, "a header without a colon");
        };
        if !is_token(name) {
            // Folded lines start with a space; they are not a token.
            return Parse::Bad(400, "a bad header name");
        }
        if value.bytes().any(|b| (b < b' ' && b != b'\t') || b == 0x7f) {
            return Parse::Bad(400, "a control character in a header");
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
    }
    let count = |n: &str| headers.iter().filter(|(h, _)| h == n).count();
    if count("host") != 1 {
        return Parse::Bad(400, "one Host header is needed");
    }
    if count("transfer-encoding") > 0 {
        return Parse::Bad(501, "no chunked bodies");
    }
    let length = match count("content-length") {
        0 => 0,
        1 => {
            let v = &headers
                .iter()
                .find(|(h, _)| h == "content-length")
                .unwrap()
                .1;
            match v.parse::<usize>() {
                Ok(n) if v.bytes().all(|b| b.is_ascii_digit()) => n,
                _ => return Parse::Bad(400, "a bad Content-Length"),
            }
        }
        _ => return Parse::Bad(400, "more than one Content-Length"),
    };
    if length > MAX_BODY {
        return Parse::Bad(413, "the body is too long");
    }
    let start = end + 4;
    if buf.len() < start + length {
        return Parse::More;
    }
    Parse::Done(
        Request {
            method: method.to_owned(),
            path,
            http11,
            headers,
            body: buf[start..start + length].to_vec(),
        },
        start + length,
    )
}

/// `application/x-www-form-urlencoded`: the value of `name`, decoded.
pub fn form_value(body: &[u8], name: &str) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    for pair in text.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if decode(k)? == name {
            return decode(v);
        }
    }
    None
}

fn decode(s: &str) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// A reply. Every one carries the same safety headers.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

pub fn reason(status: u16) -> &'static str {
    match status {
        101 => "Switching Protocols",
        200 => "OK",
        303 => "See Other",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Content Too Large",
        421 => "Misdirected Request",
        426 => "Upgrade Required",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "Error",
    }
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type", content_type.to_owned())],
            body,
        }
    }

    /// A short plain-text reply.
    pub fn text(status: u16, text: &str) -> Self {
        let mut r = Self::new(
            status,
            "text/plain; charset=utf-8",
            format!("{text}\n").into_bytes(),
        );
        r.headers.push(("Cache-Control", "no-store".to_owned()));
        r
    }

    pub fn redirect(to: &str) -> Self {
        let mut r = Self::text(303, "See Other");
        r.headers.push(("Location", to.to_owned()));
        r
    }

    pub fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    /// The bytes on the wire; HEAD gets the head only.
    pub fn bytes(&self, head_only: bool, keep_alive: bool) -> Vec<u8> {
        let mut out = format!("HTTP/1.1 {} {}\r\n", self.status, reason(self.status));
        for (name, value) in &self.headers {
            out.push_str(&format!("{name}: {value}\r\n"));
        }
        if self.status != 101 {
            out.push_str(&format!("Content-Length: {}\r\n", self.body.len()));
            out.push_str(if keep_alive {
                "Connection: keep-alive\r\n"
            } else {
                "Connection: close\r\n"
            });
        }
        out.push_str(
            "X-Content-Type-Options: nosniff\r\n\
             Referrer-Policy: no-referrer\r\n\
             X-Frame-Options: DENY\r\n\
             Cross-Origin-Opener-Policy: same-origin\r\n\
             Cross-Origin-Resource-Policy: same-origin\r\n\r\n",
        );
        let mut bytes = out.into_bytes();
        if !head_only {
            bytes.extend_from_slice(&self.body);
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(raw: &str) -> Request {
        match parse(raw.as_bytes()) {
            Parse::Done(r, n) => {
                assert_eq!(n, raw.len());
                r
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reads_a_get_and_a_post() {
        let r = done(
            "GET /assets/a.js?v=1 HTTP/1.1\r\nHost: tv:8443\r\nCookie: a=1; __Host-lumen=abc\r\nConnection: keep-alive, Upgrade\r\n\r\n",
        );
        assert_eq!(
            (r.method.as_str(), r.path.as_str()),
            ("GET", "/assets/a.js")
        );
        assert_eq!(r.header("host"), Some("tv:8443"));
        assert_eq!(r.cookie("__Host-lumen"), Some("abc"));
        assert!(r.has_token("connection", "upgrade") && r.keep_alive());
        let r = done(
            "POST /pair HTTP/1.1\r\nHost: tv\r\nContent-Length: 23\r\n\r\ncode=123456&name=A+b%21",
        );
        assert_eq!(form_value(&r.body, "name").as_deref(), Some("A b!"));
        let body = b"code=123456&name=Pixel+9%20Pro";
        assert_eq!(form_value(body, "code").as_deref(), Some("123456"));
        assert_eq!(form_value(body, "name").as_deref(), Some("Pixel 9 Pro"));
        assert_eq!(form_value(body, "x"), None);
        assert_eq!(form_value(b"code=%zz", "code"), None);
        assert!(!done("GET / HTTP/1.0\r\nHost: tv\r\n\r\n").keep_alive());
    }

    #[test]
    fn waits_for_the_whole_request() {
        assert_eq!(parse(b"GET / HTTP/1.1\r\nHost: tv\r\n"), Parse::More);
        assert_eq!(
            parse(b"POST /pair HTTP/1.1\r\nHost: tv\r\nContent-Length: 5\r\n\r\nab"),
            Parse::More
        );
        // A pipelined second request stays in the buffer.
        let two = b"GET / HTTP/1.1\r\nHost: tv\r\n\r\nGET /b HTTP/1.1\r\n";
        let Parse::Done(_, n) = parse(two) else {
            panic!()
        };
        assert_eq!(&two[n..], b"GET /b HTTP/1.1\r\n");
    }

    #[test]
    fn refuses_what_it_does_not_take() {
        let bad = |raw: &str| match parse(raw.as_bytes()) {
            Parse::Bad(status, _) => status,
            other => panic!("{raw:?}: {other:?}"),
        };
        assert_eq!(bad("GET / HTTP/1.1\r\n\r\n"), 400);
        assert_eq!(bad("GET / HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n"), 400);
        assert_eq!(bad("GET / HTTP/2\r\nHost: a\r\n\r\n"), 505);
        assert_eq!(bad("GET http://x/ HTTP/1.1\r\nHost: a\r\n\r\n"), 400);
        assert_eq!(bad("GET / HTTP/1.1\r\nHost: a\r\n folded\r\n\r\n"), 400);
        assert_eq!(
            bad("POST / HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked\r\n\r\n"),
            501
        );
        assert_eq!(
            bad("POST / HTTP/1.1\r\nHost: a\r\nContent-Length: 99999\r\n\r\n"),
            413
        );
        assert_eq!(
            bad("POST / HTTP/1.1\r\nHost: a\r\nContent-Length: +5\r\n\r\n"),
            400
        );
        assert_eq!(
            bad("POST / HTTP/1.1\r\nHost: a\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n"),
            400
        );
        assert_eq!(bad("GET / HTTP/1.1\r\nHost: a\u{1}\r\n\r\n"), 400);
        let long = format!(
            "GET / HTTP/1.1\r\nHost: a\r\nX: {}\r\n\r\n",
            "y".repeat(MAX_HEAD)
        );
        assert_eq!(bad(&long), 431);
        assert_eq!(bad(&"x".repeat(MAX_HEAD + 1)), 431);
    }

    #[test]
    fn every_reply_carries_the_safety_headers() {
        let r = Response::text(404, "no").bytes(false, true);
        let text = String::from_utf8(r).unwrap();
        assert!(text.starts_with("HTTP/1.1 404 Not Found\r\n"));
        for h in [
            "X-Content-Type-Options: nosniff",
            "X-Frame-Options: DENY",
            "Content-Length: 3",
            "Connection: keep-alive",
        ] {
            assert!(text.contains(h), "{h}");
        }
        assert!(text.ends_with("\r\n\r\nno\n"));
        let head = Response::text(200, "body").bytes(true, false);
        assert!(String::from_utf8(head).unwrap().ends_with("\r\n\r\n"));
    }
}
