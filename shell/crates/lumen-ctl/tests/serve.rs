//! `lumen-ctl serve --bus fake` as a process: requests on stdin, every line
//! out checked, and each way a session ends. The fake TV is in the process;
//! nothing here touches a network, luna or a TV.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(5);

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Option<Value>>,
    dir: PathBuf,
}

impl Session {
    fn start(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("lumen-ctl-it-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_lumen-ctl"))
            .args(["serve", "--bus", "fake", "--alive-ms", "300"])
            .arg("--log")
            .arg(dir.join("lumen-ctl.log"))
            .arg("--config")
            .arg(dir.join("no-ctl.conf"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let value: Value = serde_json::from_str(&line)
                    .unwrap_or_else(|e| panic!("not JSON from lumen-ctl: {line:?}: {e}"));
                if tx.send(Some(value)).is_err() {
                    return;
                }
            }
            let _ = tx.send(None);
        });
        let stdin = child.stdin.take();
        Self {
            child,
            stdin,
            lines,
            dir,
        }
    }

    fn send(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(line.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    /// The next line; panics after `WAIT`, or at the end of output.
    fn next(&self) -> Value {
        match self.lines.recv_timeout(WAIT) {
            Ok(Some(v)) => v,
            Ok(None) => panic!("lumen-ctl's output ended"),
            Err(e) => panic!("no line from lumen-ctl: {e}"),
        }
    }

    /// Lines until one matches `want`, returning those before it too.
    fn until(&self, want: impl Fn(&Value) -> bool) -> Vec<Value> {
        let mut seen = Vec::new();
        loop {
            let v = self.next();
            let done = want(&v);
            seen.push(v);
            if done {
                return seen;
            }
        }
    }

    /// Waits for the end of output and the exit, and how long they took.
    fn end(&mut self) -> (Option<i32>, Duration) {
        let started = Instant::now();
        loop {
            match self.lines.recv_timeout(WAIT) {
                Ok(Some(_)) => continue,
                Ok(None) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => panic!("lumen-ctl did not end"),
            }
        }
        while started.elapsed() < WAIT {
            if let Some(status) = self.child.try_wait().unwrap() {
                return (status.code(), started.elapsed());
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("lumen-ctl did not exit");
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("lumen-ctl.log")).unwrap_or_default()
    }

    fn signal(&self, name: &str) {
        let pid = self.child.id().to_string();
        assert!(
            Command::new("kill")
                .args([name, &pid])
                .status()
                .unwrap()
                .success()
        );
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn is_event(name: &'static str) -> impl Fn(&Value) -> bool {
    move |v| v["event"] == json!(name)
}

fn is_reply(id: u64) -> impl Fn(&Value) -> bool {
    move |v| v["id"] == json!(id)
}

#[test]
fn hello_the_state_a_reply_an_event_alive_and_the_end() {
    let mut s = Session::start("session");
    let hello = s.next();
    assert_eq!(hello["event"], json!("hello"));
    assert_eq!(hello["data"]["protocol"], json!(1));
    assert_eq!(hello["data"]["bus"], json!("fake"));
    assert_eq!(hello["data"]["keys"], json!("none"));
    assert_eq!(hello["data"]["polling"], json!(["power"]));
    let verbs = hello["data"]["verbs"].as_array().unwrap();
    assert_eq!(verbs.len(), 17);
    assert!(!verbs.contains(&json!("key")) && !verbs.contains(&json!("ambient.start")));

    let first: Vec<Value> = (0..5).map(|_| s.next()).collect();
    let names: Vec<&str> = first.iter().map(|v| v["event"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        ["power", "volume", "foreground", "inputs", "settings"]
    );
    assert_eq!(
        first[0]["data"],
        json!({ "state": "active", "lg": "Active" })
    );
    assert_eq!(first[1]["data"], json!({ "level": 14, "muted": false }));
    assert_eq!(first[2]["data"], json!({ "id": "netflix" }));
    assert_eq!(first[3]["data"]["inputs"].as_array().unwrap().len(), 5);

    // The scripted press of the Magic Remote's volume key.
    let seen = s.until(is_event("volume"));
    assert_eq!(
        seen.last().unwrap()["data"],
        json!({ "level": 15, "muted": false })
    );

    // A request: its reply, and the change as an event.
    s.send(r#"{"id":1,"verb":"volume.set","args":{"level":20}}"#);
    let seen = s.until(is_reply(1));
    let seen = if seen.iter().any(is_event("volume")) {
        seen
    } else {
        let mut more = seen;
        more.extend(s.until(is_event("volume")));
        more
    };
    assert!(
        seen.contains(&json!({ "id": 1, "ok": true, "result": {} })),
        "{seen:?}"
    );
    assert!(
        seen.contains(&json!({ "event": "volume", "data": { "level": 20, "muted": false } })),
        "{seen:?}"
    );

    // Refused: an unknown verb, an out-of-range value, a line that isn't JSON.
    s.send(r#"{"id":2,"verb":"sh"}"#);
    s.send(r#"{"id":3,"verb":"volume.set","args":{"level":101}}"#);
    s.send("ls -la /");
    let r2 = s.until(is_reply(2)).pop().unwrap();
    assert_eq!(
        r2["error"],
        json!({ "code": "unknown-verb", "message": "unknown verb \"sh\"" })
    );
    let r3 = s.until(is_reply(3)).pop().unwrap();
    assert_eq!(r3["error"]["code"], json!("bad-args"));
    let r4 = s
        .until(|v| v["id"].is_null() && v["ok"] == json!(false))
        .pop()
        .unwrap();
    assert_eq!(
        r4["error"],
        json!({ "code": "bad-line", "message": "not JSON" })
    );

    // A blank line is skipped, not answered.
    s.send("   ");
    s.send(r#"{"id":4,"verb":"apps.list"}"#);
    let apps = s.until(is_reply(4)).pop().unwrap();
    assert_eq!(apps["result"]["apps"][0]["id"], json!("netflix"));

    // Quiet: alive.
    s.until(is_event("alive"));

    // stdin ends: so does the session, at once.
    drop(s.stdin.take());
    let (code, took) = s.end();
    assert_eq!(code, Some(0));
    assert!(took < Duration::from_secs(1), "{took:?}");
    let log = s.log();
    assert!(
        log.contains("start: fake bus, keys none, polling [\"power\"]"),
        "{log}"
    );
    assert!(log.contains("> 1 volume.set {\"level\":20}"), "{log}");
    assert!(log.contains("< 1 ok ("), "{log}");
    assert!(
        log.contains("> 2 refused, unknown-verb: unknown verb \"sh\""),
        "{log}"
    );
    assert!(log.contains("> - refused, bad-line: not JSON"), "{log}");
    assert!(log.trim_end().ends_with("end: stdin closed"), "{log}");
}

#[test]
fn a_line_over_64_kib_is_refused_and_ends_the_session() {
    let mut s = Session::start("long");
    s.until(is_event("settings"));
    let long = format!(
        r#"{{"id":1,"verb":"toast","args":{{"text":"{}"}}}}"#,
        "x".repeat(70_000)
    );
    s.send(&long);
    let refused = s.until(|v| v.get("ok").is_some()).pop().unwrap();
    assert_eq!(
        refused,
        json!({ "id": null, "ok": false, "error": { "code": "bad-line", "message": "line is longer than 65536 bytes" } })
    );
    let (code, _) = s.end();
    assert_eq!(code, Some(0));
    assert!(s.log().contains("end: a line over 64 KiB"));
}

#[test]
fn sigterm_and_sighup_end_it_within_a_second() {
    for (signal, name) in [("-TERM", "SIGTERM"), ("-HUP", "SIGHUP")] {
        let mut s = Session::start(&format!("signal{signal}"));
        s.until(is_event("settings"));
        s.signal(signal);
        let (code, took) = s.end();
        assert_eq!(code, Some(0), "{signal}");
        assert!(took < Duration::from_secs(1), "{signal}: {took:?}");
        assert!(
            s.log().contains(&format!("end: stopped by {name}")),
            "{}",
            s.log()
        );
    }
}

#[test]
fn asking_for_a_shell_gets_lumen_ctl() {
    // What a forced command does with `ssh … sh`: dropbear runs lumen-ctl
    // serve, and the client's own words arrive as a request line at most.
    let mut s = Session::start("shell");
    assert_eq!(s.next()["event"], json!("hello"));
    s.send("sh");
    s.send("id; cat /etc/shadow");
    for _ in 0..2 {
        let refused = s.until(|v| v.get("ok").is_some()).pop().unwrap();
        assert_eq!(
            refused,
            json!({ "id": null, "ok": false, "error": { "code": "bad-line", "message": "not JSON" } })
        );
    }
    drop(s.stdin.take());
    assert_eq!(s.end().0, Some(0));
}
