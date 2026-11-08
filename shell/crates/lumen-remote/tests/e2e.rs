//! lumen-remote as a process, in front of `lumen-ctl serve --link web
//! --bus fake`, reached over TLS by a client that trusts only the test
//! authority (tests/fixtures, made by tools/remote/ca.sh). The PC is a
//! second lumen-ctl, `--link ssh`, on the same devices folder, as on the
//! TV. Nothing here touches a network beyond 127.0.0.1, luna or a TV.
//!
//! As root (the build container), lumen-remote really drops root: into an
//! empty folder, as uid 61101; one test checks it through /proc.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(8);

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// lumen-ctl, as `cargo test --workspace` (make check) builds it beside
/// this test's binary, or `LUMEN_CTL_BIN`. A cargo build from here would
/// wait forever on the lock the running `cargo test` holds.
fn lumen_ctl() -> PathBuf {
    if let Ok(bin) = std::env::var("LUMEN_CTL_BIN") {
        return PathBuf::from(bin);
    }
    // target/debug/deps/e2e-… → target/debug/lumen-ctl
    let exe = std::env::current_exe().unwrap();
    let bin = exe.parent().unwrap().parent().unwrap().join("lumen-ctl");
    assert!(
        bin.exists(),
        "no {}: run cargo test --workspace, or cargo build -p lumen-ctl first",
        bin.display()
    );
    bin
}

fn is_root() -> bool {
    // SAFETY: geteuid takes nothing and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Server {
    child: Child,
    port: u16,
    dir: Dir,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(name: &str, cert: &str, allow: &str) -> Server {
    let dir = std::env::temp_dir().join(format!("lumen-remote-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("web/assets")).unwrap();
    std::fs::create_dir_all(dir.join("state")).unwrap();
    std::fs::write(
        dir.join("web/index.html"),
        "<!doctype html><title>Lumen Remote</title>",
    )
    .unwrap();
    std::fs::write(dir.join("web/assets/app-1a2b.js"), "console.log(1)").unwrap();
    use std::os::unix::fs::PermissionsExt;
    for (from, to, mode) in [
        (format!("{cert}.crt"), "tls.crt", 0o644),
        (format!("{cert}.key"), "tls.key", 0o600),
    ] {
        std::fs::copy(fixtures().join(from), dir.join(to)).unwrap();
        std::fs::set_permissions(dir.join(to), std::fs::Permissions::from_mode(mode)).unwrap();
    }
    let state = dir.join("state");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lumen-remote"));
    cmd.args([
        "--listen",
        "127.0.0.1:0",
        "--allow",
        allow,
        "--name",
        "127.0.0.1",
        "--name",
        "localhost",
    ])
    .arg("--cert")
    .arg(dir.join("tls.crt"))
    .arg("--key")
    .arg(dir.join("tls.key"))
    .arg("--web")
    .arg(dir.join("web"))
    .arg("--ctl")
    .arg(lumen_ctl())
    .arg("--log")
    .arg(dir.join("remote.log"))
    .arg("--jail")
    .arg(dir.join("empty"));
    for a in ["--bus", "fake", "--alive-ms", "500", "--state"] {
        cmd.arg("--ctl-arg").arg(a);
    }
    cmd.arg("--ctl-arg").arg(&state);
    cmd.arg("--ctl-arg")
        .arg("--log")
        .arg("--ctl-arg")
        .arg(dir.join("ctl.log"));
    cmd.arg("--ctl-arg")
        .arg("--config")
        .arg("--ctl-arg")
        .arg(dir.join("none"));
    if !is_root() {
        cmd.arg("--no-drop");
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let Some(addr) = line.trim().strip_prefix("lumen-remote: listening on ") else {
        let mut err = String::new();
        let _ = child.stderr.take().unwrap().read_to_string(&mut err);
        panic!("lumen-remote did not start: {line:?} {err}");
    };
    let port = addr.rsplit(':').next().unwrap().parse().unwrap();
    let server = Server {
        child,
        port,
        dir: Dir(dir),
    };
    // Wait for lumen-ctl's hello to reach the hub.
    thread::sleep(Duration::from_millis(300));
    server
}

impl Server {
    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.0.join("remote.log")).unwrap_or_default()
    }
}

// ------------------------------------------------------------------ TLS

fn client_config() -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    for c in CertificateDer::pem_file_iter(fixtures().join("lumen-ca.crt")).unwrap() {
        roots.add(c.unwrap()).unwrap();
    }
    Arc::new(
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

type Tls = StreamOwned<ClientConnection, TcpStream>;

fn connect_as(port: u16, name: ServerName<'static>) -> std::io::Result<Tls> {
    let sock = TcpStream::connect(("127.0.0.1", port))?;
    sock.set_read_timeout(Some(WAIT))?;
    let conn = ClientConnection::new(client_config(), name).map_err(std::io::Error::other)?;
    let mut tls = StreamOwned::new(conn, sock);
    // Finish the handshake now, so its errors show here.
    while tls.conn.is_handshaking() {
        tls.conn.complete_io(&mut tls.sock)?;
    }
    Ok(tls)
}

fn connect(port: u16) -> Tls {
    connect_as(
        port,
        ServerName::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST).into()),
    )
    .unwrap()
}

struct Reply {
    status: u16,
    head: String,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().find_map(|l| {
            let (n, v) = l.split_once(':')?;
            n.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
        })
    }
}

/// One request on its own connection.
fn request(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Reply {
    let mut tls = connect(port);
    let mut raw = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    if !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("host")) {
        raw.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (n, v) in headers {
        raw.push_str(&format!("{n}: {v}\r\n"));
    }
    if !body.is_empty() {
        raw.push_str(&format!(
            "Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    raw.push_str("\r\n");
    raw.push_str(body);
    tls.write_all(raw.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = tls.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Reply {
        status,
        head: head.to_owned(),
        body: body.to_owned(),
    }
}

fn origin(port: u16) -> String {
    format!("https://127.0.0.1:{port}")
}

fn get(port: u16, path: &str, cookie: Option<&str>) -> Reply {
    let c = cookie.map(|t| format!("__Host-lumen={t}"));
    let headers: Vec<(&str, &str)> = c.iter().map(|c| ("Cookie", c.as_str())).collect();
    request(port, "GET", path, &headers, "")
}

fn post_code(port: u16, code: &str) -> Reply {
    let o = origin(port);
    request(
        port,
        "POST",
        "/pair",
        &[("Origin", &o)],
        &format!("code={code}&name=Pixel+9"),
    )
}

// ------------------------------------------------------------ WebSocket

struct Ws {
    tls: Tls,
    buf: Vec<u8>,
    /// What `reply` passed over on its way to the reply.
    seen: Vec<Value>,
}

fn ws_open(port: u16, token: &str, origin_header: &str) -> Result<Ws, Reply> {
    let mut tls = connect(port);
    let raw = format!(
        "GET /ctl HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\
         Origin: {origin_header}\r\nCookie: __Host-lumen={token}\r\n\r\n"
    );
    tls.write_all(raw.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let mut byte = [0u8; 4096];
    let end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        let n = tls.read(&mut byte).unwrap_or(0);
        assert!(
            n > 0,
            "no reply to the upgrade: {}",
            String::from_utf8_lossy(&buf)
        );
        buf.extend_from_slice(&byte[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let status: u16 = head.split(' ').nth(1).unwrap().parse().unwrap();
    if status != 101 {
        return Err(Reply {
            status,
            head,
            body: String::new(),
        });
    }
    assert!(
        head.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
        "{head}"
    );
    let rest = buf[end + 4..].to_vec();
    Ok(Ws {
        tls,
        buf: rest,
        seen: Vec::new(),
    })
}

enum Frame {
    Text(Value),
    Close(u16),
}

impl Ws {
    fn send(&mut self, text: &str) {
        let mask = [1u8, 2, 3, 4];
        let p = text.as_bytes();
        let mut f = vec![0x81];
        if p.len() < 126 {
            f.push(0x80 | p.len() as u8);
        } else {
            f.push(0x80 | 126);
            f.extend_from_slice(&(p.len() as u16).to_be_bytes());
        }
        f.extend_from_slice(&mask);
        f.extend(p.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.tls.write_all(&f).unwrap();
    }

    fn frame(&mut self) -> Frame {
        let deadline = Instant::now() + WAIT;
        loop {
            if self.buf.len() >= 2 {
                let len0 = (self.buf[1] & 0x7f) as usize;
                let (len, at) = match len0 {
                    126 if self.buf.len() >= 4 => {
                        (u16::from_be_bytes([self.buf[2], self.buf[3]]) as usize, 4)
                    }
                    127 if self.buf.len() >= 10 => {
                        let mut b = [0u8; 8];
                        b.copy_from_slice(&self.buf[2..10]);
                        (u64::from_be_bytes(b) as usize, 10)
                    }
                    126 | 127 => (usize::MAX, 0),
                    n => (n, 2),
                };
                if len != usize::MAX && self.buf.len() >= at + len {
                    let opcode = self.buf[0] & 0x0f;
                    let payload: Vec<u8> = self.buf[at..at + len].to_vec();
                    self.buf.drain(..at + len);
                    match opcode {
                        1 => return Frame::Text(serde_json::from_slice(&payload).unwrap()),
                        8 => return Frame::Close(u16::from_be_bytes([payload[0], payload[1]])),
                        _ => continue,
                    }
                }
            }
            assert!(Instant::now() < deadline, "no frame in time");
            let mut b = [0u8; 16384];
            match self.tls.read(&mut b) {
                Ok(0) => panic!("the WebSocket ended without a close"),
                Ok(n) => self.buf.extend_from_slice(&b[..n]),
                Err(e) => panic!("reading the WebSocket: {e}"),
            }
        }
    }

    /// The next text message that isn't `alive`.
    fn next(&mut self) -> Value {
        for _ in 0..20 {
            match self.frame() {
                Frame::Text(v) if v["event"] == json!("alive") => {}
                Frame::Text(v) => return v,
                Frame::Close(code) => panic!("closed with {code}"),
            }
        }
        panic!("only alive for 20 frames");
    }

    fn reply(&mut self, id: u32) -> Value {
        for _ in 0..50 {
            let v = self.next();
            if v["id"] == json!(id) {
                return v;
            }
            self.seen.push(v);
        }
        panic!("no reply {id}; saw {:?}", self.seen);
    }

    fn closed(&mut self) -> u16 {
        loop {
            if let Frame::Close(code) = self.frame() {
                return code;
            }
        }
    }
}

// ------------------------------------------------------------- the PC

/// The PC's session: lumen-ctl `--link ssh` on the same devices folder.
struct Pc {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Value>,
    next: u32,
}

impl Drop for Pc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Pc {
    fn start(server: &Server) -> Self {
        let d = &server.dir.0;
        let mut child = Command::new(lumen_ctl())
            .args(["serve", "--bus", "fake", "--state"])
            .arg(d.join("state"))
            .arg("--log")
            .arg(d.join("pc.log"))
            .arg("--config")
            .arg(d.join("none"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(serde_json::from_str(&line).unwrap()).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take().unwrap();
        Self {
            child,
            stdin,
            lines,
            next: 0,
        }
    }

    fn ask(&mut self, verb: &str, args: Value) -> Value {
        self.next += 1;
        let line = json!({ "id": self.next, "verb": verb, "args": args }).to_string();
        writeln!(self.stdin, "{line}").unwrap();
        loop {
            let v = self
                .lines
                .recv_timeout(WAIT)
                .expect("no reply from the PC's lumen-ctl");
            if v["id"] == json!(self.next) {
                assert_eq!(v["ok"], json!(true), "{verb}: {v}");
                return v["result"].clone();
            }
        }
    }

    fn open_pairing(&mut self) -> String {
        self.ask("pair.open", json!({}))["code"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

/// Pairs a phone; its token, from the cookie.
fn pair(server: &Server, pc: &mut Pc) -> String {
    let code = pc.open_pairing();
    let r = post_code(server.port, &code);
    assert_eq!(r.status, 303, "{}", r.head);
    let cookie = r.header("set-cookie").expect("a cookie");
    cookie
        .strip_prefix("__Host-lumen=")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

// --------------------------------------------------------------- tests

#[test]
fn an_unpaired_browser_gets_only_the_pairing_page() {
    let s = start("unpaired", "server", "127.0.0.0/8");
    let p = s.port;
    let page = get(p, "/", None);
    assert_eq!(page.status, 200);
    assert!(
        page.body.contains("<form method=\"post\" action=\"/pair\""),
        "{}",
        page.body
    );
    assert!(
        page.header("content-security-policy")
            .unwrap()
            .starts_with("default-src 'none'")
    );
    assert_eq!(page.header("cache-control").as_deref(), Some("no-store"));
    // Nothing of the front-end, and no WebSocket.
    for path in ["/index.html", "/assets/app-1a2b.js", "/anything"] {
        let r = get(p, path, None);
        assert_eq!(
            (r.status, r.header("location").as_deref()),
            (303, Some("/")),
            "{path}"
        );
        assert!(!r.body.contains("console.log"));
    }
    assert_eq!(get(p, "/whoami", None).status, 401);
    let err = ws_open(p, &"0".repeat(64), &origin(p))
        .err()
        .expect("refused");
    assert_eq!(err.status, 401);
    // A cookie that isn't a paired device's is no better, and is cleared.
    let r = get(p, "/", Some(&"ab".repeat(32)));
    assert!(r.body.contains("Pair this phone"));
    assert!(r.header("set-cookie").unwrap().contains("Max-Age=0"));
    // With pairing closed, the right shape of code gets a plain answer.
    let r = post_code(p, "123456");
    assert!(r.body.contains("Pairing is closed"), "{}", r.body);
    let r = post_code(p, "12ab");
    assert!(r.body.contains("The code is six digits."));
}

#[test]
fn a_paired_phone_controls_the_tv_within_its_role() {
    let s = start("paired", "server", "127.0.0.0/8");
    let mut pc = Pc::start(&s);
    let p = s.port;
    let code = pc.open_pairing();
    let r = post_code(p, &code);
    assert_eq!(
        (r.status, r.header("location").as_deref()),
        (303, Some("/"))
    );
    let cookie = r.header("set-cookie").unwrap();
    for attr in ["Path=/", "Secure", "HttpOnly", "SameSite=Strict"] {
        assert!(cookie.contains(attr), "{cookie}");
    }
    assert!(cookie.starts_with("__Host-lumen="));
    let token = cookie[13..77].to_owned();
    // The front-end now, with its policy.
    let page = get(p, "/", Some(&token));
    assert!(page.body.contains("<title>Lumen Remote</title>"));
    assert!(
        page.header("content-security-policy")
            .unwrap()
            .contains("connect-src 'self'")
    );
    let js = get(p, "/assets/app-1a2b.js", Some(&token));
    assert_eq!(
        js.header("content-type").as_deref(),
        Some("text/javascript; charset=utf-8")
    );
    assert!(js.header("cache-control").unwrap().contains("immutable"));
    assert_eq!(get(p, "/missing.js", Some(&token)).status, 404);
    let me: Value = serde_json::from_str(&get(p, "/whoami", Some(&token)).body).unwrap();
    assert_eq!(
        (me["name"].as_str(), me["role"].as_str()),
        (Some("Pixel 9"), Some("remote"))
    );

    let mut ws = ws_open(p, &token, &origin(p)).ok().expect("open");
    let hello = ws.next();
    assert_eq!(hello["event"], json!("hello"));
    let d = &hello["data"];
    assert_eq!(d["role"], json!("remote"));
    assert_eq!(d["device"]["name"], json!("Pixel 9"));
    let verbs: Vec<&str> = d["verbs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(verbs.contains(&"volume.set") && verbs.contains(&"launch"));
    assert!(!verbs.contains(&"privacy.state") && !verbs.contains(&"toast"));
    assert!(
        !verbs
            .iter()
            .any(|v| v.starts_with("pair.") || v.starts_with("devices."))
    );
    // The state follows the hello.
    let names: Vec<Value> = (0..5).map(|_| ws.next()["event"].clone()).collect();
    assert_eq!(
        names,
        [
            json!("power"),
            json!("volume"),
            json!("foreground"),
            json!("inputs"),
            json!("settings")
        ]
    );

    ws.send(r#"{"id":41,"verb":"volume.set","args":{"level":20}}"#);
    assert_eq!(ws.reply(41), json!({ "id": 41, "ok": true, "result": {} }));
    // The change comes as an event, before or after the reply.
    let want = json!({ "event": "volume", "data": { "level": 20, "muted": false } });
    if !ws.seen.contains(&want) {
        assert_eq!(ws.next(), want);
    }
    ws.send(r#"{"id":42,"verb":"settings.set","args":{"key":"picture.mode","value":"game"}}"#);
    assert_eq!(ws.reply(42)["ok"], json!(true));
    for (id, line) in [
        (43, r#"{"id":43,"verb":"privacy.state"}"#),
        (
            44,
            r#"{"id":44,"verb":"settings.set","args":{"key":"picture.oledLight","value":5}}"#,
        ),
        (45, r#"{"id":45,"verb":"toast","args":{"text":"hi"}}"#),
    ] {
        ws.send(line);
        let r = ws.reply(id);
        assert_eq!(r["error"]["code"], json!("forbidden"), "{line}: {r}");
    }
    ws.send(r#"{"id":46,"verb":"pair.complete","args":{"code":"000000","name":"x"}}"#);
    assert_eq!(ws.reply(46)["error"]["code"], json!("unknown-verb"));
    // A WebSocket from another origin is refused, cookie or not.
    let err = ws_open(p, &token, "https://evil.example")
        .err()
        .expect("refused");
    assert_eq!(err.status, 403);
    // The log names the device and never holds the token or the code.
    let log = s.log();
    assert!(log.contains("paired from 127.0.0.1, remote-only"), "{log}");
    assert!(!log.contains(&token) && !log.contains(&code), "{log}");
}

#[test]
fn five_wrong_codes_close_pairing() {
    let s = start("wrong", "server", "127.0.0.0/8");
    let mut pc = Pc::start(&s);
    let code = pc.open_pairing();
    let wrong = if code == "999999" { "999998" } else { "999999" };
    for left in [4, 3, 2, 1] {
        let r = post_code(s.port, wrong);
        assert!(
            r.body.contains(&format!("{left} of 5 tries left")),
            "{}",
            r.body
        );
    }
    let r = post_code(s.port, wrong);
    assert!(r.body.contains("pairing is closed"), "{}", r.body);
    // The right code is too late now.
    let r = post_code(s.port, &code);
    assert_eq!(r.status, 200);
    assert!(r.body.contains("Pairing is closed"), "{}", r.body);
    assert!(r.header("set-cookie").is_none());
    let devices = pc.ask("devices.list", json!({}));
    assert_eq!(devices["devices"], json!([]));
    assert_eq!(devices["pairing"]["open"], json!(false));
}

#[test]
fn revoking_or_promoting_a_device_closes_its_websocket() {
    let s = start("revoke", "server", "127.0.0.0/8");
    let mut pc = Pc::start(&s);
    let p = s.port;
    let token = pair(&s, &mut pc);
    let id = pc.ask("devices.list", json!({}))["devices"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Owner rights come from the PC: the open socket closes with 4001,
    // and the page's next one has the owner's verbs.
    let mut ws = ws_open(p, &token, &origin(p)).ok().unwrap();
    assert_eq!(ws.next()["data"]["role"], json!("remote"));
    pc.ask("devices.role", json!({ "id": id, "role": "owner" }));
    assert_eq!(ws.closed(), 4001);
    let mut ws = ws_open(p, &token, &origin(p)).ok().unwrap();
    let hello = ws.next();
    assert_eq!(hello["data"]["role"], json!("owner"));
    assert!(
        hello["data"]["verbs"]
            .as_array()
            .unwrap()
            .contains(&json!("privacy.state"))
    );
    ws.send(r#"{"id":1,"verb":"privacy.state"}"#);
    assert_eq!(ws.reply(1)["ok"], json!(true));

    // Revoked: the socket closes at once (4401), and the cookie is worth
    // nothing from then on.
    let started = Instant::now();
    pc.ask("devices.revoke", json!({ "id": id }));
    assert_eq!(ws.closed(), 4401);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    assert!(get(p, "/", Some(&token)).body.contains("Pair this phone"));
    assert_eq!(get(p, "/whoami", Some(&token)).status, 401);
    assert_eq!(ws_open(p, &token, &origin(p)).err().unwrap().status, 401);
    assert!(s.log().contains(&format!("device {id} was revoked")));
}

#[test]
fn refuses_other_names_origins_and_networks() {
    let s = start("refuse", "server", "127.0.0.0/8");
    let p = s.port;
    let r = request(p, "GET", "/", &[("Host", "evil.example")], "");
    assert_eq!(r.status, 421);
    // localhost is one of the TV's names here.
    let r = request(p, "GET", "/", &[("Host", &format!("localhost:{p}"))], "");
    assert_eq!(r.status, 200);
    let r = request(
        p,
        "POST",
        "/pair",
        &[("Origin", "https://evil.example")],
        "code=123456&name=x",
    );
    assert_eq!(r.status, 403);
    let r = request(p, "POST", "/pair", &[], "code=123456&name=x");
    assert_eq!(r.status, 403);
    drop(s);
    // A server for another network closes the connection unread.
    let s = start("network", "server", "10.0.0.0/8");
    assert!(
        connect_as(
            s.port,
            ServerName::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST).into())
        )
        .is_err()
    );
    thread::sleep(Duration::from_millis(100));
    assert!(s.log().contains("from outside --allow"), "{}", s.log());
}

#[test]
fn the_authority_vouches_for_the_tvs_names_only() {
    let s = start("names", "server", "127.0.0.0/8");
    assert!(connect_as(s.port, ServerName::try_from("localhost").unwrap()).is_ok());
    assert!(connect_as(s.port, ServerName::try_from("lumen.example").unwrap()).is_err());
    drop(s);
    // A certificate the same authority signed for another name is refused
    // by its name constraints, whatever name the client asks for.
    let s = start("rogue", "rogue", "127.0.0.0/8");
    let e = connect_as(s.port, ServerName::try_from("rogue.example").unwrap())
        .err()
        .unwrap();
    assert!(e.to_string().to_lowercase().contains("name"), "{e}");
}

#[test]
fn drops_root_before_it_serves() {
    if !is_root() {
        eprintln!("not root: lumen-remote ran with --no-drop, nothing to check");
        return;
    }
    let s = start("drop", "server", "127.0.0.0/8");
    let pid = s.child.id();
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    let field = |name: &str| {
        status
            .lines()
            .find(|l| l.starts_with(name))
            .unwrap()
            .split_whitespace()
            .skip(1)
            .collect::<Vec<_>>()
    };
    assert_eq!(field("Uid:"), ["61101"; 4]);
    assert_eq!(field("Gid:"), ["61101"; 4]);
    assert_eq!(field("Groups:"), Vec::<&str>::new());
    assert_eq!(field("NoNewPrivs:"), ["1"]);
    // Not dumpable: even root may not look at its root folder without
    // CAP_SYS_PTRACE, which the container lacks; the log says where it is.
    match std::fs::read_link(format!("/proc/{pid}/root")) {
        Ok(root) => assert_eq!(root, s.dir.0.join("empty")),
        Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied),
    }
    let jail = s.dir.0.join("empty");
    assert!(
        s.log()
            .contains(&format!("as 61101:61101 in {}", jail.display())),
        "{}",
        s.log()
    );
    // It still serves.
    assert_eq!(get(s.port, "/", None).status, 200);
    // And lumen-ctl, its child, stayed root.
    let ctl = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")).unwrap();
    let ctl_pid = ctl.split_whitespace().next().unwrap();
    let ctl_status = std::fs::read_to_string(format!("/proc/{ctl_pid}/status")).unwrap();
    assert!(
        ctl_status
            .lines()
            .any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(1) == Some("0"))
    );
}
