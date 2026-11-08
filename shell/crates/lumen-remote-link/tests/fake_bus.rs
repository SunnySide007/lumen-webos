//! The app's session against `lumen-ctl serve --bus fake`, the TV in a
//! process, through a fake ssh that takes the app's ssh arguments and runs
//! lumen-ctl as the TV's forced command would. Nothing reaches a network or a
//! TV. lumen-ctl builds for Linux only, so this runs in the build container
//! (`make check`, where `cargo test --workspace` has built it first).
#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use lumen_remote_link::session::Context;
use lumen_remote_link::{Event, Session, ssh};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(10);

/// lumen-ctl beside this test's own binary: target/<profile>/lumen-ctl.
fn lumen_ctl() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().and_then(Path::parent).unwrap();
    let ctl = profile.join("lumen-ctl");
    assert!(
        ctl.is_file(),
        "no {}: cargo test --workspace builds it",
        ctl.display()
    );
    ctl
}

struct Tv {
    session: Session,
    events: Receiver<Event>,
    dir: PathBuf,
}

impl Tv {
    fn start(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("lumen-link-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake_ssh = dir.join("ssh");
        std::fs::write(
            &fake_ssh,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{d}/argv'\nexec '{ctl}' serve --bus fake \
                 --alive-ms 300 --log '{d}/ctl.log' --config '{d}/no-ctl.conf'\n",
                d = dir.display(),
                ctl = lumen_ctl().display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
        let key = dir.join("id_ed25519");
        let argv: Vec<OsString> = ssh::argv(&fake_ssh, &key, "tv.example", 2222);
        let (tx, events) = mpsc::channel();
        let session = Session::spawn(
            &argv,
            Context {
                key,
                host: "tv.example".into(),
            },
            move |e| {
                let _ = tx.send(e);
            },
        )
        .unwrap();
        Self {
            session,
            events,
            dir,
        }
    }

    fn next(&self) -> Value {
        match self.events.recv_timeout(WAIT) {
            Ok(Event::Line(l)) => serde_json::from_str(&l).unwrap(),
            other => panic!("wanted a line, got {other:?}"),
        }
    }

    /// The first line `want` takes; panics after `WAIT`, since `alive`
    /// keeps lines coming whatever else does.
    fn until(&self, want: impl Fn(&Value) -> bool) -> Value {
        let end = Instant::now() + WAIT;
        let mut seen = Vec::new();
        while Instant::now() < end {
            let v = self.next();
            if want(&v) {
                return v;
            }
            if v["event"] != "alive" {
                seen.push(v);
            }
        }
        panic!("not seen in {WAIT:?}; saw {seen:?}");
    }

    fn reply(&self, id: i64) -> Value {
        self.until(|v| v["id"] == id)
    }

    /// Sends a request and waits for both its reply and the first `event`
    /// `want` takes, in whichever order they come: the fake bus may push
    /// the change before lumen-ctl writes the reply.
    fn act(&self, request: Value, want: impl Fn(&Value) -> bool) -> (Value, Value) {
        let id = request["id"].clone();
        self.session.send(&request.to_string()).unwrap();
        let (mut reply, mut event) = (None, None);
        let end = Instant::now() + WAIT;
        while reply.is_none() || event.is_none() {
            assert!(
                Instant::now() < end,
                "{request}: reply {reply:?}, event {event:?}"
            );
            let v = self.next();
            if v["id"] == id {
                reply = Some(v);
            } else if event.is_none() && v.get("event").is_some() && want(&v) {
                event = Some(v);
            }
        }
        (reply.unwrap(), event.unwrap())
    }
}

impl Drop for Tv {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn hello_without_key_then_the_state_as_events() {
    let tv = Tv::start("hello");
    let hello = tv.next();
    assert_eq!(hello["event"], "hello");
    assert_eq!(hello["data"]["protocol"], 1);
    assert!(
        hello["data"]["server"]
            .as_str()
            .unwrap()
            .starts_with("lumen-ctl")
    );
    let verbs: Vec<&str> = hello["data"]["verbs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    // What the app's Remote screen reads: no `key` verb, so no key pad.
    assert!(!verbs.contains(&"key"), "{verbs:?}");
    for v in ["power.off", "volume.set", "mute.set", "apps.list", "launch"] {
        assert!(verbs.contains(&v), "{v} in {verbs:?}");
    }
    let mut seen = Vec::new();
    while seen.len() < 5 {
        seen.push(tv.next()["event"].as_str().unwrap().to_owned());
    }
    seen.sort();
    assert_eq!(
        seen,
        ["foreground", "inputs", "power", "settings", "volume"]
    );

    // The app's ssh arguments reached the fake ssh as given.
    let argv = std::fs::read_to_string(tv.dir.join("argv")).unwrap();
    assert!(argv.starts_with("-F\nnone\n-i\n"), "{argv}");
    assert!(argv.ends_with("root@tv.example\n"), "{argv}");
}

#[test]
fn power_volume_inputs_and_apps_round_trip() {
    let tv = Tv::start("verbs");
    // The fake TV's one script: 300 ms in, someone presses volume up on the
    // Magic Remote (14 to 15), and it arrives as an event like any change.
    tv.until(|v| v["event"] == "volume" && v["data"]["level"] == 15);
    let send = |v: Value| tv.session.send(&v.to_string()).unwrap();

    let (reply, ev) = tv.act(
        json!({"id": 1, "verb": "volume.set", "args": {"level": 20}}),
        |v| v["event"] == "volume" && v["data"]["level"] == 20,
    );
    assert_eq!(reply, json!({"id": 1, "ok": true, "result": {}}));
    assert_eq!(ev["data"]["muted"], false);

    let (reply, _) = tv.act(
        json!({"id": 2, "verb": "mute.set", "args": {"muted": true}}),
        |v| v["event"] == "volume" && v["data"]["muted"] == true,
    );
    assert_eq!(reply["ok"], true);

    send(json!({"id": 3, "verb": "apps.list"}));
    let apps = tv.reply(3);
    let list = apps["result"]["apps"].as_array().unwrap();
    assert!(list.iter().any(|a| a["kind"] == "input"), "{apps}");
    // The fake TV starts with Netflix in front (lumen-ctl's fake.rs), and a
    // launch of what is already in front changes nothing to push.
    let app = list
        .iter()
        .find(|a| a["kind"] == "app" && a["id"] != "netflix")
        .unwrap();

    let (reply, _) = tv.act(
        json!({"id": 4, "verb": "launch", "args": {"id": app["id"]}}),
        |v| v["event"] == "foreground" && v["data"]["id"] == app["id"],
    );
    assert_eq!(reply["ok"], true);

    let (reply, _) = tv.act(json!({"id": 5, "verb": "screen.off"}), |v| {
        v["event"] == "power" && v["data"]["state"] == "screen-off"
    });
    assert_eq!(reply["ok"], true);

    // Refusals come back as replies, and the session goes on.
    send(json!({"id": 6, "verb": "key", "args": {"name": "ok"}}));
    assert_eq!(tv.reply(6)["ok"], false);
    send(json!({"id": 7, "verb": "volume.set", "args": {"level": 101}}));
    assert_eq!(tv.reply(7)["error"]["code"], "bad-args");
    send(json!({"id": 8, "verb": "volume.up"}));
    assert_eq!(tv.reply(8)["ok"], true);
}

#[test]
fn alive_comes_and_closing_ends_lumen_ctl() {
    let tv = Tv::start("close");
    tv.until(|v| v["event"] == "alive");
    tv.session.close();
    loop {
        match tv.events.recv_timeout(WAIT) {
            Ok(Event::Line(_)) => {}
            Ok(Event::Closed(reason)) => {
                assert_eq!(reason, "The TV closed the link");
                break;
            }
            Err(e) => panic!("no close: {e}"),
        }
    }
}
