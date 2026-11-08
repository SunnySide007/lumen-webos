//! lumen-guard end to end on loopback: the binary on a high port, a fake
//! upstream in this test that answers with canned bytes, real UDP and TCP
//! queries, then the answers, the lookup log and the status file. The rules
//! are tools/tests/fixtures/guard-g2's, and the owner's control files are
//! written as the tests go. No network beyond 127.0.0.1.

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const GUARD: &str = env!("CARGO_BIN_EXE_lumen-guard");
/// The name the fake upstream never answers.
const SILENT: &str = "silent.example";
/// The address in the fake upstream's answers; Guard's own are all zeros.
const UPSTREAM_A: [u8; 4] = [192, 0, 2, 33];

/// Picking a free port and binding it are two steps, and the tests run in
/// parallel: this keeps one test from taking the port another just picked.
static PORTS: Mutex<()> = Mutex::new(());

fn ports() -> std::sync::MutexGuard<'static, ()> {
    PORTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn query(id: u16, name: &str, qtype: u16) -> Vec<u8> {
    let mut msg = id.to_be_bytes().to_vec();
    msg.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.split('.') {
        msg.push(label.len() as u8);
        msg.extend_from_slice(label.as_bytes());
    }
    msg.extend_from_slice(&[0, 0, qtype as u8, 0, 1]);
    msg
}

/// The fake upstream's answer: the query with QR set and one made-up A
/// record pointing back at the question, padded to `pad` extra bytes so a
/// long TCP answer has to stream through Guard in pieces.
fn canned_answer(query: &[u8], pad: usize) -> Vec<u8> {
    let mut answer = query.to_vec();
    answer[2] |= 0x80;
    answer[7] = 1; // ANCOUNT
    answer.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
    answer.extend_from_slice(&UPSTREAM_A);
    answer.extend(std::iter::repeat_n(0xEE, pad));
    answer
}

fn mentions(msg: &[u8], name: &str) -> bool {
    let first = name.split('.').next().unwrap_or("");
    msg.windows(first.len()).any(|w| w == first.as_bytes())
}

fn rcode(answer: &[u8]) -> u8 {
    answer[3] & 0x0F
}

fn ancount(answer: &[u8]) -> u16 {
    u16::from_be_bytes([answer[6], answer[7]])
}

/// The first answer record's data, for an answer to a plain one-question
/// query (no compression in the question).
fn first_rdata(answer: &[u8]) -> Option<Vec<u8>> {
    if ancount(answer) == 0 {
        return None;
    }
    let mut off = 12;
    while answer[off] != 0 {
        off += 1 + usize::from(answer[off]);
    }
    off += 1 + 4; // the root label, QTYPE and QCLASS
    off += 2 + 2 + 2 + 4; // a name pointer, TYPE, CLASS, TTL
    let len = usize::from(u16::from_be_bytes([answer[off], answer[off + 1]]));
    Some(answer[off + 2..off + 2 + len].to_vec())
}

/// Guard's sinkhole for `q`: its ID, NOERROR, and all-zero data for A and
/// AAAA or no record for another type.
fn is_sinkhole(q: &[u8], answer: &[u8]) -> bool {
    answer[..2] == q[..2]
        && answer[2] & 0x80 != 0
        && rcode(answer) == 0
        && match first_rdata(answer) {
            Some(data) => !data.is_empty() && data.iter().all(|b| *b == 0),
            None => true,
        }
}

/// The fake upstream: UDP and TCP on one port, as a router's resolver is.
/// It keeps every query it receives.
struct Upstream {
    addr: SocketAddr,
    got: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Upstream {
    fn start(tcp_pad: usize) -> Self {
        let (udp, tcp) = {
            let _ports = ports();
            bind_pair()
        };
        let addr = udp.local_addr().unwrap();
        let got = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&got);
        thread::spawn(move || {
            let mut buf = [0; 4096];
            while let Ok((n, from)) = udp.recv_from(&mut buf) {
                let msg = buf[..n].to_vec();
                seen.lock().unwrap().push(msg.clone());
                if !mentions(&msg, SILENT) {
                    let _ = udp.send_to(&canned_answer(&msg, 0), from);
                }
            }
        });
        let seen = Arc::clone(&got);
        thread::spawn(move || {
            for stream in tcp.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut len = [0; 2];
                if stream.read_exact(&mut len).is_err() {
                    continue;
                }
                let mut msg = vec![0; usize::from(u16::from_be_bytes(len))];
                if stream.read_exact(&mut msg).is_err() {
                    continue;
                }
                seen.lock().unwrap().push(msg.clone());
                let answer = canned_answer(&msg, tcp_pad);
                let mut framed = (answer.len() as u16).to_be_bytes().to_vec();
                framed.extend_from_slice(&answer);
                let _ = stream.write_all(&framed);
            }
        });
        Self { addr, got }
    }

    fn received(&self) -> Vec<Vec<u8>> {
        self.got.lock().unwrap().clone()
    }

    fn saw(&self, name: &str) -> bool {
        self.received().iter().any(|q| mentions(q, name))
    }
}

/// A UDP socket and a TCP listener on the same free loopback port.
fn bind_pair() -> (UdpSocket, TcpListener) {
    for _ in 0..50 {
        let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = tcp.local_addr().unwrap().port();
        if let Ok(udp) = UdpSocket::bind(("127.0.0.1", port)) {
            return (udp, tcp);
        }
    }
    panic!("no free port for both UDP and TCP");
}

fn free_port() -> u16 {
    let (udp, tcp) = bind_pair();
    let port = udp.local_addr().unwrap().port();
    drop((udp, tcp));
    port
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Waits up to 5 s for `cond`: Guard reads its control files every second.
fn eventually(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("never: {what}");
}

struct Guard {
    child: Child,
    listen: SocketAddr,
    dir: PathBuf,
}

impl Guard {
    /// Guard with the shared fixture's rules, the owner's control files in
    /// its folder (none written yet), and `extra` arguments.
    fn start(name: &str, upstream: SocketAddr, extra: &[&str]) -> Self {
        Self::start_with(name, upstream, extra, true)
    }

    fn start_with(name: &str, upstream: SocketAddr, extra: &[&str], allowlist: bool) -> Self {
        let dir =
            std::env::temp_dir().join(format!("lumen-guard-it-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("rules/imported")).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tools/tests/fixtures/guard-g2/rules");
        for file in [
            "allowlist.txt",
            "carve-outs.txt",
            "required-blocks.txt",
            "imported/list.txt",
        ] {
            if file != "allowlist.txt" || allowlist {
                fs::copy(fixture.join(file), dir.join("rules").join(file)).unwrap();
            }
        }
        // Held until Guard answers, so its port is still free when it binds.
        let ports = ports();
        let listen: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
        let child = Command::new(GUARD)
            .arg("--listen")
            .arg(listen.to_string())
            .arg("--upstream")
            .arg(upstream.to_string())
            .arg("--rules")
            .arg(dir.join("rules"))
            .arg("--log")
            .arg(&dir)
            .arg("--status")
            .arg(dir.join("status"))
            .arg("--mode-file")
            .arg(dir.join("mode"))
            .arg("--owner")
            .arg(dir.join("owner.txt"))
            .arg("--learn")
            .arg(dir.join("learn"))
            .args(extra)
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut guard = Self { child, listen, dir };
        guard.wait_until_answering();
        drop(ports);
        guard
    }

    fn wait_until_answering(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.child.try_wait() {
                let mut err = String::new();
                if let Some(mut stderr) = self.child.stderr.take() {
                    let _ = stderr.read_to_string(&mut err);
                }
                panic!(
                    "lumen-guard exited with {status} before answering on {}: {}",
                    self.listen,
                    err.trim()
                );
            }
            let probe = Command::new(GUARD)
                .args(["--probe", &self.listen.to_string()])
                .stderr(Stdio::null())
                .status()
                .unwrap();
            if probe.success() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("lumen-guard never answered on {}", self.listen);
    }

    /// Writes one of the owner's control files as tools/guard.sh does: whole,
    /// owner-only, renamed into place.
    fn control(&self, file: &str, text: &str) {
        let tmp = self.dir.join(format!("{file}.tmp"));
        fs::write(&tmp, text).unwrap();
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600)).unwrap();
        fs::rename(&tmp, self.dir.join(file)).unwrap();
    }

    fn remove(&self, file: &str) {
        fs::remove_file(self.dir.join(file)).unwrap();
    }

    fn status(&self) -> String {
        fs::read_to_string(self.dir.join("status")).unwrap_or_default()
    }

    fn udp(&self, msg: &[u8], wait: Duration) -> Option<Vec<u8>> {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.set_read_timeout(Some(wait)).unwrap();
        sock.send_to(msg, self.listen).unwrap();
        let mut buf = [0; 4096];
        let (n, _) = sock.recv_from(&mut buf).ok()?;
        Some(buf[..n].to_vec())
    }

    /// One UDP query for `name`, answered within 5 s.
    fn ask(&self, id: u16, name: &str, qtype: u16) -> (Vec<u8>, Vec<u8>) {
        let q = query(id, name, qtype);
        let answer = self
            .udp(&q, Duration::from_secs(5))
            .unwrap_or_else(|| panic!("no UDP answer for {name}"));
        (q, answer)
    }

    fn sinkholed(&self, id: u16, name: &str) -> bool {
        let (q, answer) = self.ask(id, name, 1);
        is_sinkhole(&q, &answer)
    }

    fn tcp(&self, msg: &[u8]) -> Vec<u8> {
        let mut stream = TcpStream::connect(self.listen).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut framed = (msg.len() as u16).to_be_bytes().to_vec();
        framed.extend_from_slice(msg);
        // In two writes, as a slow asker might send it.
        stream.write_all(&framed[..5]).unwrap();
        thread::sleep(Duration::from_millis(20));
        stream.write_all(&framed[5..]).unwrap();
        let mut len = [0; 2];
        stream.read_exact(&mut len).unwrap();
        let mut answer = vec![0; usize::from(u16::from_be_bytes(len))];
        stream.read_exact(&mut answer).unwrap();
        // One query per connection: Guard closes it after the answer.
        let mut rest = Vec::new();
        assert_eq!(stream.read_to_end(&mut rest).unwrap(), 0);
        answer
    }

    /// SIGTERM, as guard-undo.sh sends it; Guard must write its status and exit 0.
    fn stop(mut self) -> (String, String, String) {
        let killed = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status()
            .unwrap();
        assert!(killed.success());
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "lumen-guard ignored SIGTERM");
            thread::sleep(Duration::from_millis(20));
        };
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(status.success(), "exit {status}: {stderr}");
        let log = fs::read_to_string(self.dir.join("lookups.log")).unwrap();
        let state = fs::read_to_string(self.dir.join("status")).unwrap();
        let _ = fs::remove_dir_all(&self.dir);
        (log, state, stderr)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn has_line(text: &str, rest: &str) -> bool {
    text.lines().any(|line| {
        line.split_once(' ')
            .is_some_and(|(secs, tail)| secs.parse::<u64>().is_ok() && tail == rest)
    })
}

fn status_value<'a>(state: &'a str, key: &str) -> &'a str {
    state
        .lines()
        .find_map(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix(' ')))
        .unwrap_or_else(|| panic!("no {key} in\n{state}"))
}

#[test]
fn log_mode_forwards_everything_and_logs_the_verdict() {
    let upstream = Upstream::start(5000);
    let guard = Guard::start("log", upstream.addr, &["--dscp", "7"]);

    // UDP: the bytes go up and come back unchanged.
    let (q1, answer) = guard.ask(0x1234, "WWW.Netflix.com", 1);
    assert_eq!(answer, canned_answer(&q1, 0));

    // No rule allows it, and the firmware host: forwarded all the same.
    let (q2, answer) = guard.ask(0x4321, "nobody.example", 28);
    assert_eq!(answer, canned_answer(&q2, 0));
    let (q3, answer) = guard.ask(0x4322, "snu.lge.com", 1);
    assert_eq!(answer, canned_answer(&q3, 0));

    // TCP, with an answer longer than Guard's 4,096-byte buffer.
    let q4 = query(0x0BAD, "ads.example", 65);
    assert_eq!(guard.tcp(&q4), canned_answer(&q4, 5000));

    let received = upstream.received();
    for q in [&q1, &q2, &q3, &q4] {
        assert!(received.iter().any(|r| r == q), "upstream never got {q:?}");
    }

    let (log, state, stderr) = guard.stop();
    assert!(has_line(&log, "A www.netflix.com allow"), "{log}");
    assert!(has_line(&log, "AAAA nobody.example would-block"), "{log}");
    assert!(has_line(&log, "A snu.lge.com would-block"), "{log}");
    assert!(has_line(&log, "HTTPS ads.example would-block"), "{log}");
    // Names only: no addresses or ports in the log.
    assert!(!log.contains("127.0.0.1"), "{log}");
    assert!(stderr.contains("listening on"), "{stderr}");
    assert!(stderr.contains("mode log"), "{stderr}");
    assert!(stderr.contains("stopped"), "{stderr}");

    assert_eq!(status_value(&state, "mode"), "log");
    assert_eq!(status_value(&state, "block_refused"), "none");
    assert_eq!(status_value(&state, "upstream"), upstream.addr.to_string());
    assert_eq!(status_value(&state, "dscp"), "7");
    // Four lookups plus the start-up probes, which no rule allows either.
    let lookups: u64 = status_value(&state, "lookups").parse().unwrap();
    assert!(lookups >= 5, "{state}");
    let would: u64 = status_value(&state, "would_block").parse().unwrap();
    assert_eq!(would, lookups - 1, "{state}");
    assert_eq!(status_value(&state, "blocked"), "0");
    assert_eq!(status_value(&state, "upstream_timeouts"), "0");
}

#[test]
fn block_mode_answers_what_the_rules_refuse_itself() {
    let upstream = Upstream::start(5000);
    let guard = Guard::start("block", upstream.addr, &["--mode", "block"]);

    // Allowed: forwarded, the upstream's answer unchanged.
    let (q, answer) = guard.ask(0x0101, "us.lgtvsdp.com", 1);
    assert_eq!(answer, canned_answer(&q, 0));

    // Not allowed: A gets 0.0.0.0, AAAA ::, HTTPS no record.
    for (id, name, qtype, len) in [
        (0x0201, "nobody.example", 1, Some(4)),
        (0x0202, "nobody.example", 28, Some(16)),
        (0x0203, "nobody.example", 65, None),
        (0x0204, "logs.netflix.com", 1, Some(4)),
        (0x0205, "us.rdx2.lgtvsdp.com", 1, Some(4)),
        (0x0206, "snu.lge.com", 1, Some(4)),
    ] {
        let (q, answer) = guard.ask(id, name, qtype);
        assert!(is_sinkhole(&q, &answer), "{name} {qtype}: {answer:?}");
        assert_eq!(first_rdata(&answer).map(|d| d.len()), len, "{name}");
    }

    // TCP: the same sinkhole, and no upstream connection for it.
    let q = query(0x0301, "a.track.example", 1);
    let answer = guard.tcp(&q);
    assert!(is_sinkhole(&q, &answer), "{answer:?}");
    let q = query(0x0302, "www.netflix.com", 1);
    assert_eq!(guard.tcp(&q), canned_answer(&q, 5000));

    for name in ["nobody", "logs", "rdx2", "snu", "track"] {
        assert!(!upstream.saw(name), "upstream got a blocked query: {name}");
    }

    let (log, state, _) = guard.stop();
    assert!(has_line(&log, "A us.lgtvsdp.com allow"), "{log}");
    assert!(has_line(&log, "AAAA nobody.example block"), "{log}");
    assert!(has_line(&log, "A logs.netflix.com block"), "{log}");
    assert!(has_line(&log, "A a.track.example block"), "{log}");
    assert_eq!(status_value(&state, "mode"), "block");
    let blocked: u64 = status_value(&state, "blocked").parse().unwrap();
    assert!(blocked >= 7, "{state}");
    assert_eq!(status_value(&state, "blocked_today"), blocked.to_string());
}

#[test]
fn block_mode_refuses_what_it_cannot_read_or_answer() {
    let upstream = Upstream::start(0);
    let guard = Guard::start("junk", upstream.addr, &["--mode", "block"]);
    // A standard query with two questions: FORMERR, nothing upstream.
    let mut two = query(0x0401, "us.lgtvsdp.com", 1);
    two[5] = 2;
    let answer = guard.udp(&two, Duration::from_secs(5)).unwrap();
    assert_eq!((rcode(&answer), &answer[..2]), (1, &two[..2]));
    // Another opcode (UPDATE), for an allowed name: REFUSED.
    let mut update = query(0x0402, "us.lgtvsdp.com", 1);
    update[2] = 0x28;
    let answer = guard.udp(&update, Duration::from_secs(5)).unwrap();
    assert_eq!(rcode(&answer), 5);
    // A question that can't be read: FORMERR.
    let mut cut = query(0x0403, "cut.example", 1);
    cut.truncate(16);
    let answer = guard.udp(&cut, Duration::from_secs(5)).unwrap();
    assert_eq!(rcode(&answer), 1);
    // Answers sent to Guard are dropped.
    let reply = canned_answer(&query(1, "x.example", 1), 0);
    assert_eq!(guard.udp(&reply, Duration::from_millis(300)), None);
    assert!(
        !upstream
            .received()
            .iter()
            .any(|q| q[..2] == two[..2] || q[..2] == update[..2] || q[..2] == cut[..2]),
        "upstream got a refused query"
    );
    let (log, _, _) = guard.stop();
    assert!(has_line(&log, "- - block"), "{log}");
    assert!(has_line(&log, "A us.lgtvsdp.com block"), "{log}");
    assert!(!log.contains("x.example"), "{log}");
}

#[test]
fn the_mode_file_switches_blocking_on_and_off() {
    let upstream = Upstream::start(0);
    let guard = Guard::start("modefile", upstream.addr, &[]);
    assert!(!guard.sinkholed(0x0501, "nobody.example"));
    guard.control("mode", "block\n");
    eventually("blocking on", || guard.sinkholed(0x0502, "nobody.example"));
    // A trial for another boot is over: back to logging.
    guard.control("mode", "block 00000000-0000-0000-0000-000000000000\n");
    eventually("trial over", || !guard.sinkholed(0x0503, "nobody.example"));
    // This boot's trial blocks.
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap();
    guard.control("mode", &format!("block {}", boot.trim()));
    eventually("trial on", || guard.sinkholed(0x0504, "nobody.example"));
    // The undo: the file goes, and so does blocking.
    guard.remove("mode");
    eventually("blocking off", || {
        !guard.sinkholed(0x0505, "nobody.example")
    });
    // A mode file anyone could write is left out.
    guard.control("mode", "block\n");
    fs::set_permissions(guard.dir.join("mode"), fs::Permissions::from_mode(0o666)).unwrap();
    thread::sleep(Duration::from_millis(1500));
    assert!(!guard.sinkholed(0x0506, "nobody.example"));
    let (_, _, stderr) = guard.stop();
    assert!(stderr.contains("mode block"), "{stderr}");
    assert!(stderr.contains("writable by others"), "{stderr}");
}

#[test]
fn a_learning_window_and_the_owners_rules() {
    let upstream = Upstream::start(0);
    let guard = Guard::start("learn", upstream.addr, &["--mode", "block"]);
    assert!(guard.sinkholed(0x0601, "new-app.example"));

    // The window: names no rule covers resolve, and are logged as learned.
    let now = unix_now();
    guard.control("learn", &format!("{} {}\n", now - 1, now + 600));
    eventually("learning", || !guard.sinkholed(0x0602, "new-app.example"));
    assert!(upstream.saw("new-app"));
    // A carve-out and an imported name stay blocked while learning.
    assert!(guard.sinkholed(0x0603, "logs.netflix.com"));
    assert!(guard.sinkholed(0x0604, "a.track.example"));
    // The window closed by hand: blocked again.
    guard.remove("learn");
    eventually("window closed", || {
        guard.sinkholed(0x0605, "new-app.example")
    });

    // Kept by the owner: it resolves for good.
    guard.control("owner.txt", "=new-app.example\n!=www.amazon.com.example\n");
    eventually("owner rule", || !guard.sinkholed(0x0606, "new-app.example"));
    // An owner file that would block the store is refused whole.
    guard.control("owner.txt", "!=us.lgtvsdp.com\n");
    thread::sleep(Duration::from_millis(1500));
    assert!(!guard.sinkholed(0x0607, "new-app.example"));
    assert!(!guard.sinkholed(0x0608, "us.lgtvsdp.com"));
    // Dropped: the owner's file loses the name, and it is blocked again.
    guard.control("owner.txt", "# nothing kept\n");
    eventually("dropped", || guard.sinkholed(0x0609, "new-app.example"));

    let (log, state, stderr) = guard.stop();
    assert!(has_line(&log, "A new-app.example learn"), "{log}");
    assert!(has_line(&log, "A new-app.example block"), "{log}");
    assert!(has_line(&log, "A new-app.example allow"), "{log}");
    assert!(stderr.contains("learning window open"), "{stderr}");
    assert!(
        stderr.contains("learning window closed: 1 new names"),
        "{stderr}"
    );
    assert!(stderr.contains("owner rules refused"), "{stderr}");
    assert_eq!(status_value(&state, "learning_until"), "0");
    assert_eq!(status_value(&state, "learned_names"), "1");
    assert_eq!(status_value(&state, "owner_rules"), "0");
}

#[test]
fn rules_that_would_block_the_store_keep_guard_to_logging() {
    let upstream = Upstream::start(0);
    // No allowlist.txt: blocking would cut the store off.
    let guard = Guard::start_with("unsafe", upstream.addr, &["--mode", "block"], false);
    let (q, answer) = guard.ask(0x0701, "us.lgtvsdp.com", 1);
    assert_eq!(answer, canned_answer(&q, 0));
    let (q, answer) = guard.ask(0x0702, "nobody.example", 1);
    assert_eq!(answer, canned_answer(&q, 0));
    eventually("status written", || {
        guard.status().contains("block_refused")
    });
    let (log, state, stderr) = guard.stop();
    assert_eq!(status_value(&state, "mode"), "log");
    assert_eq!(status_value(&state, "block_refused"), "allowlist.txt");
    assert!(has_line(&log, "A nobody.example would-block"), "{log}");
    assert!(stderr.contains("blocking refused"), "{stderr}");
    assert!(stderr.contains("allowlist.txt"), "{stderr}");
}

#[test]
fn sends_nothing_back_when_the_upstream_is_silent() {
    let upstream = Upstream::start(0);
    let guard = Guard::start("silent", upstream.addr, &[]);
    let q = query(0x7777, SILENT, 1);
    assert_eq!(guard.udp(&q, Duration::from_millis(3500)), None);
    // Guard counts the timeout at its next tick.
    thread::sleep(Duration::from_millis(1200));
    let (log, state, _) = guard.stop();
    assert!(has_line(&log, "A silent.example would-block"), "{log}");
    assert_eq!(status_value(&state, "upstream_timeouts"), "1");
}

#[test]
fn probe_exits_by_whether_anything_answered() {
    let upstream = Upstream::start(0);
    let guard = Guard::start("probe", upstream.addr, &[]);
    let ok = Command::new(GUARD)
        .args([
            "--probe",
            &guard.listen.to_string(),
            "--name",
            "lumen-check-1.invalid",
        ])
        .status()
        .unwrap();
    assert!(ok.success());
    let nobody = Command::new(GUARD)
        .args(["--probe", &format!("127.0.0.1:{}", free_port())])
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(nobody.code(), Some(1));
    let (log, _, _) = guard.stop();
    // Not under lumen.invalid, so no rule allows it: logged, and forwarded.
    assert!(
        has_line(&log, "A lumen-check-1.invalid would-block"),
        "{log}"
    );
}

#[test]
fn bad_arguments_exit_2_and_say_why() {
    let out = Command::new(GUARD)
        .args(["--mode", "loud"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("log or block"));
    let out = Command::new(GUARD).arg("--listen").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

// Stage G3: DNS over TLS. A local rustls server with the test certificate in
// tests/fixtures/dot (dot.lumen.test, signed by a test CA) stands in for
// Quad9; the fake router above is the fallback in clear.

const DOT_NAME: &str = "dot.lumen.test";
/// The DoT server pads answers for names with this label past 4,096 bytes.
const BIG: &str = "big";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/dot")
        .join(name)
}

/// The TLS flags pointing Guard at `dot`, trusting the test CA.
fn tls_args(dot: SocketAddr) -> Vec<String> {
    vec![
        "--tls-server".to_owned(),
        dot.to_string(),
        "--tls-name".to_owned(),
        DOT_NAME.to_owned(),
        "--tls-ca".to_owned(),
        fixture("ca.der").display().to_string(),
    ]
}

fn strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

struct DotServer {
    addr: SocketAddr,
    got: Arc<Mutex<Vec<Vec<u8>>>>,
    connections: Arc<Mutex<usize>>,
}

impl DotServer {
    fn start() -> Self {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let cert = CertificateDer::from(fs::read(fixture("server.der")).unwrap());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            fs::read(fixture("server.key.der")).unwrap(),
        ));
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = Arc::new(
            rustls::ServerConfig::builder_with_provider(provider)
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(vec![cert], key)
                .unwrap(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let got = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(Mutex::new(0));
        let (seen, counted) = (Arc::clone(&got), Arc::clone(&connections));
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                *counted.lock().unwrap() += 1;
                let (config, seen) = (Arc::clone(&config), Arc::clone(&seen));
                thread::spawn(move || {
                    let conn = rustls::ServerConnection::new(config).unwrap();
                    let mut tls = rustls::StreamOwned::new(conn, stream);
                    loop {
                        let mut len = [0; 2];
                        if tls.read_exact(&mut len).is_err() {
                            return;
                        }
                        let mut msg = vec![0; usize::from(u16::from_be_bytes(len))];
                        if tls.read_exact(&mut msg).is_err() {
                            return;
                        }
                        seen.lock().unwrap().push(msg.clone());
                        if mentions(&msg, SILENT) {
                            continue;
                        }
                        let pad = if mentions(&msg, BIG) { 5000 } else { 0 };
                        let answer = canned_answer(&msg, pad);
                        let mut framed = (answer.len() as u16).to_be_bytes().to_vec();
                        framed.extend_from_slice(&answer);
                        if tls.write_all(&framed).and_then(|()| tls.flush()).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        Self {
            addr,
            got,
            connections,
        }
    }

    fn received(&self) -> Vec<Vec<u8>> {
        self.got.lock().unwrap().clone()
    }

    fn saw(&self, name: &str) -> bool {
        self.received().iter().any(|q| mentions(q, name))
    }

    fn connections(&self) -> usize {
        *self.connections.lock().unwrap()
    }
}

fn tc(answer: &[u8]) -> bool {
    answer[2] & 0x02 != 0
}

#[test]
fn tls_on_sends_lookups_over_tls_and_reuses_connections() {
    let router = Upstream::start(0);
    let dot = DotServer::start();
    let mut args = tls_args(dot.addr);
    args.extend(["--tls".to_owned(), "on".to_owned()]);
    let guard = Guard::start("tls", router.addr, &strs(&args));

    // One at a time, as the TV mostly asks: the bytes come back unchanged.
    for i in 0..20u16 {
        let (q, answer) = guard.ask(0x2000 + i, &format!("n{i}.netflix.com"), 1);
        assert_eq!(answer, canned_answer(&q, 0));
    }
    // A name no rule covers: log mode forwards it, over TLS too.
    let (q, answer) = guard.ask(0x2100, "nobody.example", 28);
    assert_eq!(answer, canned_answer(&q, 0));
    // Too long for a UDP asker without EDNS: truncated, so it asks over TCP,
    // where the whole answer, longer than 4,096 bytes, comes through.
    let q = query(0x2200, "big.netflix.com", 1);
    let answer = guard.udp(&q, Duration::from_secs(5)).unwrap();
    assert!(tc(&answer) && ancount(&answer) == 0 && answer[..2] == q[..2]);
    assert!(answer.len() <= 512);
    assert_eq!(guard.tcp(&q), canned_answer(&q, 5000));

    // Block mode: a blocked name gets Guard's sinkhole and never reaches TLS.
    guard.control("mode", "block\n");
    eventually("block mode", || guard.sinkholed(0x2300, "wait.example"));
    assert!(guard.sinkholed(0x2301, "ads.example"));

    assert!(dot.saw("n0") && dot.saw("n19") && dot.saw("nobody") && dot.saw(BIG));
    assert!(!dot.saw("ads"));
    assert!(router.received().is_empty(), "something went in clear");
    // Connections are reused: one per worker at most.
    assert!(
        (1..=3).contains(&dot.connections()),
        "{} connections",
        dot.connections()
    );

    let (_, state, stderr) = guard.stop();
    assert_eq!(status_value(&state, "tls"), "on");
    assert_eq!(status_value(&state, "route"), "tls");
    assert_eq!(status_value(&state, "tls_name"), DOT_NAME);
    assert_eq!(status_value(&state, "tls_server"), dot.addr.to_string());
    let answers: u64 = status_value(&state, "tls_answers").parse().unwrap();
    assert!(answers >= 24, "{state}");
    let handshakes: u64 = status_value(&state, "tls_handshakes").parse().unwrap();
    assert!((1..=3).contains(&handshakes), "{state}");
    assert_eq!(status_value(&state, "tls_errors"), "0");
    assert_eq!(status_value(&state, "fallback"), "0");
    assert_eq!(status_value(&state, "fallback_since"), "0");
    assert!(stderr.contains("upstream tls"), "{stderr}");
    assert!(!stderr.contains("FALLBACK"), "{stderr}");
}

#[test]
fn tls_falls_back_in_clear_while_the_clock_is_wrong() {
    let router = Upstream::start(0);
    let dot = DotServer::start();
    let mut args = tls_args(dot.addr);
    // Any clock before 2100 counts as wrong here, as 2019 does on the B9.
    args.extend(["--tls", "on", "--clock-floor", "4102444800"].map(str::to_owned));
    let guard = Guard::start("tls-clock", router.addr, &strs(&args));
    let (q, answer) = guard.ask(0x3000, "www.netflix.com", 1);
    assert_eq!(answer, canned_answer(&q, 0));
    let q = query(0x3001, "www.netflix.com", 28);
    assert_eq!(guard.tcp(&q), canned_answer(&q, 0));
    assert!(router.saw("www"));
    assert!(dot.received().is_empty() && dot.connections() == 0);

    let (_, state, stderr) = guard.stop();
    assert_eq!(status_value(&state, "route"), "fallback-clock");
    let fallback: u64 = status_value(&state, "fallback").parse().unwrap();
    assert!(fallback >= 2, "{state}");
    assert_ne!(status_value(&state, "fallback_since"), "0");
    assert!(
        stderr.contains("FALLBACK") && stderr.contains("clock"),
        "{stderr}"
    );
}

#[test]
fn tls_falls_back_in_clear_when_the_server_is_down() {
    let router = Upstream::start(0);
    // Nothing listens there.
    let down: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
    let mut args = tls_args(down);
    args.extend(["--tls".to_owned(), "on".to_owned()]);
    let guard = Guard::start("tls-down", router.addr, &strs(&args));
    let (q, answer) = guard.ask(0x4000, "www.netflix.com", 1);
    assert_eq!(answer, canned_answer(&q, 0));
    // Within the backoff, straight to the router.
    let start = Instant::now();
    let (q, answer) = guard.ask(0x4001, "www.youtube.com", 1);
    assert_eq!(answer, canned_answer(&q, 0));
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(router.saw("youtube"));

    let (_, state, stderr) = guard.stop();
    assert_eq!(status_value(&state, "route"), "fallback-down");
    let errors: u64 = status_value(&state, "tls_errors").parse().unwrap();
    assert!(errors >= 1, "{state}");
    let fallback: u64 = status_value(&state, "fallback").parse().unwrap();
    assert!(fallback >= 2, "{state}");
    assert!(stderr.contains(&format!("tls: connect {down}")), "{stderr}");
    assert!(
        stderr.contains("FALLBACK") && stderr.contains("for 10 s"),
        "{stderr}"
    );
}

#[test]
fn tls_never_sends_a_query_to_a_server_it_cannot_check() {
    let router = Upstream::start(0);
    let dot = DotServer::start();
    // The certificate is for dot.lumen.test, not this name.
    let mut wrong_name = tls_args(dot.addr);
    wrong_name[3] = "other.lumen.test".to_owned();
    // Mozilla's roots, which don't hold the test CA.
    let mut built_in = tls_args(dot.addr);
    built_in.truncate(4);
    for (name, mut args, why) in [
        ("tls-name", wrong_name, "certificate"),
        ("tls-roots", built_in, "UnknownIssuer"),
    ] {
        args.extend(["--tls".to_owned(), "on".to_owned()]);
        let guard = Guard::start(name, router.addr, &strs(&args));
        let (q, answer) = guard.ask(0x5000, "www.netflix.com", 1);
        assert_eq!(answer, canned_answer(&q, 0), "{name}");
        let (_, state, stderr) = guard.stop();
        assert_eq!(status_value(&state, "route"), "fallback-down", "{name}");
        assert_eq!(status_value(&state, "tls_answers"), "0", "{name}");
        assert!(
            stderr.contains("handshake with") && stderr.contains(why),
            "{name}: {stderr}"
        );
    }
    assert!(dot.connections() >= 2);
    assert!(
        dot.received().is_empty(),
        "a query reached an unchecked server"
    );
    assert!(router.saw("www"));
}

#[test]
fn the_tls_file_switches_the_route() {
    let router = Upstream::start(0);
    let dot = DotServer::start();
    let dir = std::env::temp_dir().join(format!("lumen-guard-it-tls-file-{}", std::process::id()));
    let mut args = tls_args(dot.addr);
    args.extend([
        "--tls-file".to_owned(),
        dir.join("tls").display().to_string(),
    ]);
    let guard = Guard::start("tls-file", router.addr, &strs(&args));
    assert_eq!(status_value(&guard.status(), "tls"), "off");
    // Whether a fresh name goes over TLS (else in clear).
    let n = std::cell::Cell::new(0u16);
    let over_tls = || {
        n.set(n.get() + 1);
        let label = format!("q{}x", n.get());
        guard.ask(0x6000 + n.get(), &format!("{label}.netflix.com"), 1);
        assert!(dot.saw(&label) != router.saw(&label), "{label}");
        dot.saw(&label)
    };
    assert!(!over_tls());

    guard.control("tls", "tls\n");
    eventually("route tls", over_tls);
    // A trial for another boot is over: clear again.
    guard.control("tls", "tls 00000000-0000-0000-0000-000000000000\n");
    eventually("trial over", || !over_tls());
    guard.control("tls", "tls\n");
    eventually("route tls again", over_tls);
    // The undo: the file goes, and so does TLS.
    guard.remove("tls");
    eventually("route clear", || !over_tls());
    let (_, state, stderr) = guard.stop();
    // Off is not a fallback.
    assert_eq!(status_value(&state, "fallback"), "0");
    assert!(
        stderr.contains("upstream tls, as the TLS file says"),
        "{stderr}"
    );
}

/// Guard's resident memory with TLS on, after bursts of lookups that open
/// every worker's connection. The B9's figure comes from the TV; this one
/// keeps G3 from growing unnoticed (privacy.md, "Lumen Guard").
#[test]
fn memory_with_tls_stays_small() {
    let router = Upstream::start(0);
    let dot = DotServer::start();
    let mut args = tls_args(dot.addr);
    args.extend(["--tls".to_owned(), "on".to_owned()]);
    let guard = Guard::start("tls-rss", router.addr, &strs(&args));
    // Resident in all, and the part that is Guard's own (heap and stacks)
    // rather than pages of the binary and the C library.
    let rss = || {
        let status = fs::read_to_string(format!("/proc/{}/status", guard.child.id())).unwrap();
        let kb = |key: &str| {
            status
                .lines()
                .find_map(|l| l.strip_prefix(key))
                .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
                .unwrap()
        };
        (kb("VmRSS:"), kb("RssAnon:"))
    };
    let before = rss();
    for round in 0..10u16 {
        let askers: Vec<_> = (0..20u16)
            .map(|i| {
                let listen = guard.listen;
                thread::spawn(move || {
                    let q = query(round * 100 + i, &format!("r{round}-{i}.netflix.com"), 1);
                    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
                    sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    sock.send_to(&q, listen).unwrap();
                    let mut buf = [0; 4096];
                    sock.recv_from(&mut buf).is_ok()
                })
            })
            .collect();
        for asker in askers {
            assert!(asker.join().unwrap());
        }
    }
    let after = rss();
    eprintln!(
        "lumen-guard with TLS: VmRSS {} kB (RssAnon {} kB) at start, {} kB ({} kB) after 200 lookups over {} connections",
        before.0,
        before.1,
        after.0,
        after.1,
        dot.connections()
    );
    // G1 measured 940 kB on the B9; a debug build is larger than the TV's.
    assert!(after.0 < 8 * 1024, "{after:?} kB");
    assert!(after.1 < 2 * 1024, "{after:?} kB");
    assert!(router.received().is_empty());
}
