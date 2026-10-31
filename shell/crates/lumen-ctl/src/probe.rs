//! `lumen-ctl probe` and `probe-keys`: what this TV's bus lets lumen-ctl do,
//! for the PM to read before `serve` runs (remote.md, R2).
//!
//! `probe` makes only read-only calls: the four state methods, the settings,
//! the launch points and LG's descriptions of the four settings, first over
//! the library bus, then over luna-send. Over the library it also opens each
//! state's subscription, waits for the first reply and cancels it, to see
//! which methods honour `subscribe`. Last, it shows what `serve` would make
//! of the replies. It writes nothing anywhere.
//!
//! `probe-keys` calls each `createKeyEvent` with `{}`, so that LG's error
//! names the parameters it wants; no key is sent.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::args::{BusKind, CONFIG_FILE};
use crate::bus::{Bus, CommandBus, LUNA_SEND, Spawn};
use crate::fake::FakeBus;
use crate::keys::KEYS_FILE;
use crate::lg::{self, Call, LG_SETTINGS};
use crate::privacy::{INITCTL, IPTABLES};
use crate::sys::luna::LibraryBus;
use crate::watch::{Source, States};

/// How long `probe` waits for a subscription's first reply.
const SUBSCRIBE_WAIT: Duration = Duration::from_secs(3);
/// How much of each reply is shown.
const SHOWN: usize = 300;

/// The read-only calls, with a short name each.
fn reads() -> Vec<(String, Call)> {
    let mut list: Vec<(String, Call)> = Source::ALL
        .iter()
        .map(|s| {
            let c = s.call(false);
            let name = match s {
                Source::Picture => "getSystemSettings picture".to_owned(),
                Source::Sound => "getSystemSettings sound".to_owned(),
                _ => method(c.uri).to_owned(),
            };
            (name, c)
        })
        .collect();
    list.push(("listLaunchPoints".to_owned(), lg::list_launch_points()));
    for s in &LG_SETTINGS {
        list.push((format!("getSystemSettingDesc {}", s.key), lg::read_desc(s)));
    }
    list
}

fn method(uri: &str) -> &str {
    uri.rsplit('/').next().unwrap_or(uri)
}

/// One line for a reply: ok, refused (the hub's or LG's no), or failed.
fn verdict(reply: &Result<Value, String>) -> (&'static str, String) {
    match reply {
        Err(e) => ("error", lg::clean(e, lg::MAX_TEXT)),
        Ok(v) => match lg::check(v) {
            Ok(()) => ("ok", shown(v)),
            Err(why) => {
                let lower = why.to_ascii_lowercase();
                let hub = [
                    "denied",
                    "permission",
                    "not allowed",
                    "privilege",
                    "not authorized",
                ]
                .iter()
                .any(|w| lower.contains(w));
                (if hub { "refused" } else { "failed" }, why)
            }
        },
    }
}

/// A reply on one line, cut to `SHOWN` characters; the launch points as a
/// count, since they are long and say nothing about the bus.
fn shown(v: &Value) -> String {
    if let Some(points) = v.get("launchPoints").and_then(Value::as_array) {
        return format!("{} launch points", points.len());
    }
    let text = v.to_string();
    let cut: String = text.chars().take(SHOWN).collect();
    if cut.len() < text.len() {
        format!("{cut}…")
    } else {
        cut
    }
}

/// Runs the read-only calls on `bus`, prints a line each, and returns the
/// replies that succeeded, by name.
fn calls(bus: &dyn Bus, out: &mut dyn Write) -> Vec<(String, Value)> {
    let mut good = Vec::new();
    for (name, c) in reads() {
        let started = Instant::now();
        let reply = bus.call(c.uri, &c.payload);
        let ms = started.elapsed().as_millis();
        let (word, detail) = verdict(&reply);
        let _ = writeln!(out, "  call {name:<36} {word:<8} {ms:>5} ms  {detail}");
        if word == "ok"
            && let Ok(v) = reply
        {
            good.push((name, v));
        }
    }
    good
}

/// Opens each state's subscription on the library, waits for its first
/// reply, and cancels it.
fn subscriptions(bus: &LibraryBus, out: &mut dyn Write) {
    for s in Source::ALL {
        let c = s.call(true);
        let started = Instant::now();
        let first = bus.call_for(c.uri, &c.payload, SUBSCRIBE_WAIT, true);
        let ms = started.elapsed().as_millis();
        let word = match &first {
            Ok(v) if crate::watch::subscribed(v) => "subscribed".to_owned(),
            Ok(v) => match lg::check(v) {
                Ok(()) => "answered once, not subscribed: serve polls it".to_owned(),
                Err(why) => format!("refused: {why}"),
            },
            Err(e) => format!("error: {}", lg::clean(e, lg::MAX_TEXT)),
        };
        let name = match s {
            Source::Picture => "getSystemSettings picture",
            Source::Sound => "getSystemSettings sound",
            _ => method(c.uri),
        };
        let _ = writeln!(out, "  subscribe {name:<31} {word} ({ms} ms)");
    }
}

/// What `serve` would send for the replies `good`: the snapshot after
/// `hello`, and the values LG's descriptions list.
fn reading(good: &[(String, Value)], out: &mut dyn Write) {
    let mut states = States::default();
    for s in Source::ALL {
        let c = s.call(false);
        let name = match s {
            Source::Picture => "getSystemSettings picture".to_owned(),
            Source::Sound => "getSystemSettings sound".to_owned(),
            _ => method(c.uri).to_owned(),
        };
        if let Some((_, v)) = good.iter().find(|(n, _)| *n == name) {
            states.update(s, v);
        }
    }
    for line in states.snapshot() {
        let _ = writeln!(out, "  {line}");
    }
    if let Some((_, v)) = good.iter().find(|(n, _)| n == "getSystemSettings picture") {
        let range = lg::range(v).map_or_else(|e| e, |r| r.name().to_owned());
        let _ = writeln!(out, "  dynamic range: {range}");
    }
    for s in &LG_SETTINGS {
        let name = format!("getSystemSettingDesc {}", s.key);
        let listed = good
            .iter()
            .find(|(n, _)| *n == name)
            .and_then(|(_, v)| lg::desc_values(v, s.key));
        let shown = listed.map_or_else(
            || "not readable: serve writes only the values seen on 2026-10-31".to_owned(),
            |l| l.join(", "),
        );
        let _ = writeln!(out, "  LG lists for {}: {shown}", s.key);
    }
}

fn exists(path: &str) -> &'static str {
    if Path::new(path).exists() {
        "yes"
    } else {
        "no"
    }
}

/// `probe` over `which`, or library then command; for the fake, its own TV.
pub fn probe(which: Option<BusKind>, out: &mut dyn Write) {
    let _ = writeln!(
        out,
        "lumen-ctl {} probe: read-only calls only; nothing is written",
        env!("CARGO_PKG_VERSION")
    );
    let _ = writeln!(
        out,
        "files: {LUNA_SEND} {}, {IPTABLES} {}, {INITCTL} {}, {CONFIG_FILE} {}, {KEYS_FILE} {}",
        exists(LUNA_SEND),
        exists(IPTABLES),
        exists(INITCTL),
        exists(CONFIG_FILE),
        exists(KEYS_FILE)
    );
    let mut last = Vec::new();
    if matches!(which, None | Some(BusKind::Library)) {
        let _ = writeln!(out, "bus library (libluna-service2.so.3):");
        match LibraryBus::open() {
            Ok(bus) => {
                let _ = writeln!(out, "  registered as an anonymous client");
                last = calls(&bus, out);
                subscriptions(&bus, out);
            }
            Err(e) => {
                let _ = writeln!(out, "  error: {}", lg::clean(&e, lg::MAX_TEXT));
            }
        }
    }
    if matches!(which, None | Some(BusKind::Command)) {
        let _ = writeln!(out, "bus command ({LUNA_SEND}, polled by serve):");
        let good = calls(&CommandBus::new(Box::new(Spawn)), out);
        if good.len() >= last.len() {
            last = good;
        }
    }
    if which == Some(BusKind::Fake) {
        let _ = writeln!(out, "bus fake (no TV):");
        last = calls(&FakeBus::new(false), out);
    }
    let _ = writeln!(
        out,
        "what serve would send after hello, from the replies above:"
    );
    reading(&last, out);
}

/// `probe-keys` over `which`.
pub fn probe_keys(which: BusKind, out: &mut dyn Write) {
    let _ = writeln!(
        out,
        "lumen-ctl {} probe-keys: createKeyEvent with no parameters; no key is sent",
        env!("CARGO_PKG_VERSION")
    );
    let bus: Box<dyn Bus> = match which {
        BusKind::Command => Box::new(CommandBus::new(Box::new(Spawn))),
        BusKind::Fake => Box::new(FakeBus::new(false)),
        BusKind::Library => match LibraryBus::open() {
            Ok(bus) => Box::new(bus),
            Err(e) => {
                let _ = writeln!(out, "bus library: error: {}", lg::clean(&e, lg::MAX_TEXT));
                return;
            }
        },
    };
    let _ = writeln!(out, "bus {}:", bus.name());
    for uri in [lg::KEYMANAGER_KEY, lg::LEGACYINPUT_KEY] {
        let reply = bus.call(uri, &json!({}));
        let text = match &reply {
            Ok(v) => v.to_string(),
            Err(e) => format!("error: {}", lg::clean(e, lg::MAX_TEXT)),
        };
        let service = uri
            .strip_prefix("luna://")
            .and_then(|u| u.split('/').next())
            .unwrap_or(uri);
        let _ = writeln!(out, "  {service}/createKeyEvent {{}}: {text}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_reads_and_never_writes() {
        // Every call the probe makes is a get, a list or a description.
        for (name, c) in reads() {
            let m = method(c.uri);
            assert!(
                m.starts_with("get") || m.starts_with("list"),
                "{name} calls {m}"
            );
            assert_eq!(c.payload.get("subscribe"), None, "{name}");
        }
        assert_eq!(reads().len(), Source::ALL.len() + 1 + LG_SETTINGS.len());
    }

    #[test]
    fn reports_the_fake_tv() {
        let mut out = Vec::new();
        probe(Some(BusKind::Fake), &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("call getVolume"), "{text}");
        assert!(text.contains("8 launch points"), "{text}");
        assert!(
            text.contains(r#"{"data":{"level":14,"muted":false},"event":"volume"}"#),
            "{text}"
        );
        assert!(
            text.contains("LG lists for pictureMode: vivid, normal"),
            "{text}"
        );
        assert!(
            text.contains("LG lists for energySaving: not readable"),
            "{text}"
        );
        assert!(text.contains("dynamic range: SDR"), "{text}");
        assert!(!text.contains("bus library"), "{text}");
    }

    #[test]
    fn probe_keys_shows_lgs_error() {
        let mut out = Vec::new();
        probe_keys(BusKind::Fake, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("com.webos.service.tv.keymanager/createKeyEvent {}"),
            "{text}"
        );
        assert!(text.contains("keyCode is a required parameter"), "{text}");
        assert!(text.contains("no key is sent"));
    }

    #[test]
    fn tells_the_hubs_refusal_from_lgs() {
        let denied = Ok(json!({
            "returnValue": false, "errorCode": -1,
            "errorText": "Denied method call \"powerOff\" for category \"/power\"",
        }));
        assert_eq!(verdict(&denied).0, "refused");
        let lg_no = Ok(json!({ "returnValue": false, "errorText": "Unknown method" }));
        assert_eq!(verdict(&lg_no).0, "failed");
        assert_eq!(verdict(&Err("no reply in 5000 ms".into())).0, "error");
        assert_eq!(verdict(&Ok(json!({ "returnValue": true }))).0, "ok");
    }
}
