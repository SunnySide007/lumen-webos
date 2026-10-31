//! The bus behind the verbs: luna, reached one of two ways (`--bus`).
//!
//! * `library` (`sys::luna`) loads `libluna-service2.so.3` as the shell does
//!   and registers as an anonymous client: calls answer in a few
//!   milliseconds, and LG's state arrives through subscriptions. Whether the
//!   hub lets an unknown root client call LG's private methods is unknown
//!   until `lumen-ctl probe` has run on the TV.
//! * `command` runs `/usr/bin/luna-send`, which works as root over SSH for
//!   every method the verbs use (the surveys call it), one process per call.
//!   It cannot hold a subscription, so the state is polled.
//!
//! The only programs lumen-ctl ever starts are fixed absolute paths, run here
//! with fixed arguments, without a shell, with stdin from /dev/null, an
//! environment of only `PATH`, and a deadline.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::lg::{self, Call};

/// How long one call may take before it counts as failed.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// The largest reply kept: `listLaunchPoints` is the largest the verbs read.
pub const MAX_REPLY: usize = 4 << 20;
pub const LUNA_SEND: &str = "/usr/bin/luna-send";
/// What a started program finds on `PATH`; it needs none of it to start.
const CHILD_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";
/// stderr is only read for a short reason.
const MAX_STDERR: usize = 4 << 10;
const WAIT_TICK: Duration = Duration::from_millis(5);

/// Where a subscription's messages go: its index in the list `watch` was
/// given, and the message or why it failed.
pub type Sink = Arc<dyn Fn(usize, Result<Value, String>) + Send + Sync>;

/// One way to reach luna.
pub trait Bus: Send + Sync {
    fn name(&self) -> &'static str;

    /// One call and its reply, within `CALL_TIMEOUT`. `Err` is the bus
    /// failing; LG's own refusals are replies with `returnValue` false.
    fn call(&self, uri: &str, payload: &Value) -> Result<Value, String>;

    /// Holds a subscription open on each of `subs` for the life of the
    /// process and sends every message to `sink`. `None` for a bus that
    /// cannot subscribe, which is then polled.
    fn watch(&self, subs: Vec<(&'static str, Value)>, sink: Sink) -> Option<Result<(), String>> {
        let _ = (subs, sink);
        None
    }
}

/// A call, and LG's refusal as an error.
pub fn checked(bus: &dyn Bus, c: &Call) -> Result<Value, String> {
    let reply = bus.call(c.uri, &c.payload)?;
    lg::check(&reply)?;
    Ok(reply)
}

/// What a finished program left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Starts programs; the tests give a fake.
pub trait Runner: Send + Sync {
    fn run(&self, program: &'static str, args: &[&str]) -> Result<Output, String>;
}

/// The real thing: `spawn` with `CALL_TIMEOUT`.
pub struct Spawn;

impl Runner for Spawn {
    fn run(&self, program: &'static str, args: &[&str]) -> Result<Output, String> {
        spawn(program, args, CALL_TIMEOUT)
    }
}

/// Reads a pipe to its end, keeping the first `max` bytes.
fn read_capped(mut pipe: impl Read, max: usize) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut buf = [0u8; 8192];
    let mut over = false;
    loop {
        match pipe.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = max.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
                over |= n > room;
            }
        }
    }
    (kept, over)
}

/// Runs `program` with `args`, as the module's header says, and kills it at
/// `timeout`. The pipes are read on threads of their own so a chatty child
/// never blocks on a full pipe.
pub fn spawn(program: &'static str, args: &[&str], timeout: Duration) -> Result<Output, String> {
    debug_assert!(
        program.starts_with('/'),
        "{program} is not an absolute path"
    );
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .env("PATH", CHILD_PATH)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let reader = |pipe: Option<Box<dyn Read + Send>>, max: usize| {
        thread::Builder::new()
            .name("pipe".to_owned())
            .stack_size(64 << 10)
            .spawn(move || pipe.map_or((Vec::new(), false), |p| read_capped(p, max)))
    };
    let out = reader(child.stdout.take().map(|p| Box::new(p) as _), MAX_REPLY);
    let err = reader(child.stderr.take().map(|p| Box::new(p) as _), MAX_STDERR);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!(
                    "{program} gave no answer in {} ms",
                    timeout.as_millis()
                ));
            }
            Ok(None) => thread::sleep(WAIT_TICK),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("{program}: {e}"));
            }
        }
    };
    let join = |handle: std::io::Result<thread::JoinHandle<(Vec<u8>, bool)>>| {
        handle.ok().and_then(|h| h.join().ok()).unwrap_or_default()
    };
    let (stdout, over) = join(out);
    let (stderr, _) = join(err);
    let status = status?;
    if over {
        return Err(format!("{program} wrote more than {MAX_REPLY} bytes"));
    }
    Ok(Output {
        success: status.success(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// The first JSON object in `text`: `luna-send -f` prints one, indented.
fn first_object(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    serde_json::Deserializer::from_str(&text[start..])
        .into_iter::<Value>()
        .next()?
        .ok()
        .filter(Value::is_object)
}

/// `/usr/bin/luna-send -n 1 -f <uri> <payload>` for each call.
pub struct CommandBus {
    runner: Box<dyn Runner>,
}

impl CommandBus {
    pub fn new(runner: Box<dyn Runner>) -> Self {
        Self { runner }
    }
}

impl Bus for CommandBus {
    fn name(&self) -> &'static str {
        "command"
    }

    fn call(&self, uri: &str, payload: &Value) -> Result<Value, String> {
        let payload = payload.to_string();
        let out = self
            .runner
            .run(LUNA_SEND, &["-n", "1", "-f", uri, &payload])?;
        first_object(&out.stdout).ok_or_else(|| {
            let why = if out.stderr.trim().is_empty() {
                &out.stdout
            } else {
                &out.stderr
            };
            let why = lg::clean(why, lg::MAX_TEXT);
            if why.is_empty() {
                "luna-send printed no reply".to_owned()
            } else {
                format!("luna-send: {why}")
            }
        })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::Mutex;

    type Script = Box<dyn Fn(&str, &[&str]) -> Result<Output, String> + Send + Sync>;

    /// Records each command line and answers from a script.
    pub struct FakeRunner {
        pub calls: Mutex<Vec<(String, Vec<String>)>>,
        answer: Script,
    }

    impl FakeRunner {
        pub fn new(
            answer: impl Fn(&str, &[&str]) -> Result<Output, String> + Send + Sync + 'static,
        ) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                answer: Box::new(answer),
            }
        }
    }

    impl Runner for FakeRunner {
        fn run(&self, program: &'static str, args: &[&str]) -> Result<Output, String> {
            self.calls.lock().unwrap().push((
                program.to_owned(),
                args.iter().map(|a| (*a).to_owned()).collect(),
            ));
            (self.answer)(program, args)
        }
    }

    pub fn printed(stdout: &str) -> Result<Output, String> {
        Ok(Output {
            success: true,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        })
    }

    impl Runner for Arc<FakeRunner> {
        fn run(&self, program: &'static str, args: &[&str]) -> Result<Output, String> {
            FakeRunner::run(self, program, args)
        }
    }

    #[test]
    fn luna_send_gets_the_uri_and_the_payload_as_two_arguments() {
        let runner = Arc::new(FakeRunner::new(|_, _| {
            printed("{\n    \"returnValue\": true,\n    \"volume\": 6\n}\n")
        }));
        let bus = CommandBus::new(Box::new(Arc::clone(&runner)));
        let reply = bus
            .call(lg::SET_VOLUME, &serde_json::json!({ "volume": 20 }))
            .unwrap();
        assert_eq!(reply["volume"], 6);
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].0, "/usr/bin/luna-send");
        assert_eq!(
            calls[0].1,
            [
                "-n",
                "1",
                "-f",
                "luna://com.webos.audio/setVolume",
                r#"{"volume":20}"#
            ]
        );
    }

    #[test]
    fn luna_sends_failures_carry_its_words() {
        let bus = CommandBus::new(Box::new(FakeRunner::new(|_, _| {
            Ok(Output {
                success: false,
                stdout: String::new(),
                stderr: "LSCall failed\n".to_owned(),
            })
        })));
        assert_eq!(
            bus.call(lg::GET_VOLUME, &serde_json::json!({})),
            Err("luna-send: LSCall failed".to_owned())
        );
        let bus = CommandBus::new(Box::new(FakeRunner::new(|_, _| printed(""))));
        assert_eq!(
            bus.call(lg::GET_VOLUME, &serde_json::json!({})),
            Err("luna-send printed no reply".to_owned())
        );
        let bus = CommandBus::new(Box::new(FakeRunner::new(|_, _| {
            Err("/usr/bin/luna-send gave no answer in 5000 ms".to_owned())
        })));
        assert!(
            bus.call(lg::GET_VOLUME, &serde_json::json!({}))
                .unwrap_err()
                .contains("no answer")
        );
    }

    #[test]
    fn reads_the_first_object_luna_send_prints() {
        assert_eq!(
            first_object("note\n{\"a\": 1}\n{\"b\": 2}"),
            Some(serde_json::json!({ "a": 1 }))
        );
        assert_eq!(first_object("[1]"), None);
        assert_eq!(first_object("{broken"), None);
    }

    #[cfg(unix)]
    #[test]
    fn spawns_without_a_shell_and_with_an_empty_environment() {
        // `env` prints the environment: PATH alone.
        let out = spawn("/usr/bin/env", &[], CALL_TIMEOUT).unwrap();
        assert!(out.success);
        assert_eq!(out.stdout, format!("PATH={CHILD_PATH}\n"));
        // An argument is one argument, whatever it holds.
        let out = spawn("/bin/echo", &["a; rm -rf /", "$(id)"], CALL_TIMEOUT).unwrap();
        assert_eq!(out.stdout, "a; rm -rf / $(id)\n");
        // stdin is /dev/null: cat ends at once.
        let out = spawn("/bin/cat", &[], CALL_TIMEOUT).unwrap();
        assert_eq!(out.stdout, "");
        let out = spawn("/bin/sh", &["-c", "echo why >&2; exit 3"], CALL_TIMEOUT).unwrap();
        assert!(!out.success);
        assert_eq!(out.stderr, "why\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_hangs_is_killed_at_the_deadline() {
        let start = Instant::now();
        let r = spawn("/bin/sleep", &["10"], Duration::from_millis(200));
        assert!(r.unwrap_err().contains("no answer in 200 ms"));
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(spawn("/nonexistent/program", &[], CALL_TIMEOUT).is_err());
    }
}
