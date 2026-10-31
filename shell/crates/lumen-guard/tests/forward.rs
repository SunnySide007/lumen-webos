//! lumen-guard end to end on loopback: the binary on a high port, a fake
//! upstream in this test that answers with canned bytes, real UDP and TCP
//! queries, then the answers, the lookup log and the status file. No network
//! beyond 127.0.0.1.

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const GUARD: &str = env!("CARGO_BIN_EXE_lumen-guard");
/// The name the fake upstream never answers.
const SILENT: &str = "silent.example";

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
    answer.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 192, 0, 2, 33]);
    answer.extend(std::iter::repeat_n(0xEE, pad));
    answer
}

fn mentions(msg: &[u8], name: &str) -> bool {
    let first = name.split('.').next().unwrap_or("");
    msg.windows(first.len()).any(|w| w == first.as_bytes())
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

struct Guard {
    child: Child,
    listen: SocketAddr,
    dir: PathBuf,
}

impl Guard {
    fn start(name: &str, upstream: SocketAddr, extra: &[&str]) -> Self {
        let dir =
            std::env::temp_dir().join(format!("lumen-guard-it-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("rules")).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tools/tests/fixtures/store-safe/profile");
        for file in [
            "allowlist.txt",
            "required-blocks.txt",
            "b9-aggressive.hosts",
        ] {
            fs::copy(fixture.join(file), dir.join("rules").join(file)).unwrap();
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

    fn udp(&self, msg: &[u8], wait: Duration) -> Option<Vec<u8>> {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.set_read_timeout(Some(wait)).unwrap();
        sock.send_to(msg, self.listen).unwrap();
        let mut buf = [0; 4096];
        let (n, _) = sock.recv_from(&mut buf).ok()?;
        Some(buf[..n].to_vec())
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
fn forwards_and_logs_udp_and_tcp_lookups() {
    let upstream = Upstream::start(5000);
    let guard = Guard::start("both", upstream.addr, &["--dscp", "7"]);

    // UDP: the bytes go up and come back unchanged.
    let q1 = query(0x1234, "WWW.Netflix.com", 1);
    let answer = guard
        .udp(&q1, Duration::from_secs(5))
        .expect("no UDP answer");
    assert_eq!(answer, canned_answer(&q1, 0));

    // On a block list: G1 still forwards it, and logs what it would do.
    let q2 = query(0x4321, "snu.lge.com", 28);
    let answer = guard
        .udp(&q2, Duration::from_secs(5))
        .expect("no UDP answer");
    assert_eq!(answer, canned_answer(&q2, 0));

    // TCP, with an answer longer than Guard's 4,096-byte buffer.
    let q3 = query(0x0BAD, "ads.example.com", 65);
    let answer = guard.tcp(&q3);
    assert_eq!(answer, canned_answer(&q3, 5000));

    // The upstream saw each query exactly as it was sent.
    let received = upstream.received();
    for q in [&q1, &q2, &q3] {
        assert!(received.iter().any(|r| r == q), "upstream never got {q:?}");
    }

    let (log, state, stderr) = guard.stop();
    assert!(has_line(&log, "A www.netflix.com allow"), "{log}");
    assert!(has_line(&log, "AAAA snu.lge.com would-block"), "{log}");
    assert!(has_line(&log, "HTTPS ads.example.com would-block"), "{log}");
    // Names only: no addresses or ports in the log.
    assert!(!log.contains("127.0.0.1"), "{log}");
    assert!(stderr.contains("listening on"), "{stderr}");
    assert!(stderr.contains("stopped"), "{stderr}");

    assert_eq!(status_value(&state, "mode"), "log");
    assert_eq!(status_value(&state, "upstream"), upstream.addr.to_string());
    assert_eq!(status_value(&state, "dscp"), "7");
    // Three lookups plus the start-up probes, which are lookups too.
    let lookups: u64 = status_value(&state, "lookups").parse().unwrap();
    assert!(lookups >= 4, "{state}");
    assert_eq!(status_value(&state, "would_block"), "2");
    assert_eq!(status_value(&state, "would_block_today"), "2");
    assert_eq!(status_value(&state, "upstream_timeouts"), "0");
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
    assert!(has_line(&log, "A silent.example allow"), "{log}");
    assert_eq!(status_value(&state, "upstream_timeouts"), "1");
}

#[test]
fn drops_answers_and_junk_sent_to_it() {
    let upstream = Upstream::start(0);
    let guard = Guard::start("junk", upstream.addr, &[]);
    let mut answer = canned_answer(&query(1, "x.example", 1), 0);
    assert_eq!(guard.udp(&answer, Duration::from_millis(300)), None);
    answer.truncate(5);
    assert_eq!(guard.udp(&answer, Duration::from_millis(300)), None);
    // A query whose question can't be read still goes up, logged as "- -".
    let mut cut = query(9, "cut.example", 1);
    cut.truncate(16);
    assert!(guard.udp(&cut, Duration::from_secs(5)).is_some());
    let (log, _, _) = guard.stop();
    assert!(has_line(&log, "- - allow"), "{log}");
    assert!(!log.contains("x.example"), "{log}");
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
    assert!(has_line(&log, "A lumen-check-1.invalid allow"), "{log}");
}

#[test]
fn bad_arguments_exit_2_and_say_why() {
    let out = Command::new(GUARD)
        .args(["--mode", "block"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("G2"));
    let out = Command::new(GUARD).arg("--listen").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
