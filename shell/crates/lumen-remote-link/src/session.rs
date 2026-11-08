//! One ssh process to `lumen-ctl serve`: lines in on its stdin, lines out of
//! its stdout passed on as they come, and a reason when it ends.
//!
//! Nothing is queued while the link is down; the web front-end fails such a
//! request at once (`client.ts`), so a key press never reaches the TV late.

use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crate::ssh;

/// The longest line either side sends (`MAX_LINE_BYTES` in protocol.ts).
pub const MAX_LINE: usize = 64 << 10;
/// How long ssh gets to end after its stdin closes, before it is killed.
const GRACE: Duration = Duration::from_secs(3);
/// How much of ssh's stderr is kept to explain a close.
const STDERR_KEEP: usize = 8 << 10;
/// How long ssh's stderr may stay open after ssh has ended.
const STDERR_WAIT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A line from lumen-ctl, without its line ending.
    Line(String),
    /// The session ended; the reason in a few words. Always the last event.
    Closed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// More than one line, or longer than `MAX_LINE`.
    BadLine(&'static str),
    /// The session has ended or is ending.
    Closed,
}

/// What `explain` needs to word a close.
#[derive(Debug, Clone)]
pub struct Context {
    pub key: PathBuf,
    pub host: String,
}

pub struct Session {
    child: Arc<Mutex<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Session {
    /// Starts `argv` (ssh and its arguments) and passes each line of its
    /// stdout to `on_event`, on a thread of the session's own, then
    /// `Event::Closed` once.
    pub fn spawn(
        argv: &[OsString],
        context: Context,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> io::Result<Self> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no program"))?;
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console flashes up behind the popup.
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let child = Arc::new(Mutex::new(child));

        let (err_tx, err_rx) = mpsc::channel();
        thread::Builder::new()
            .name("link-stderr".into())
            .spawn(move || {
                let _ = err_tx.send(keep_tail(stderr));
            })?;
        let reader_child = Arc::clone(&child);
        thread::Builder::new()
            .name("link-stdout".into())
            .spawn(move || read_lines(stdout, err_rx, reader_child, context, on_event))?;
        Ok(Self {
            child,
            stdin: Mutex::new(stdin),
        })
    }

    /// Sends one line; the line ending is added here.
    pub fn send(&self, line: &str) -> Result<(), SendError> {
        if line.contains(['\n', '\r']) {
            return Err(SendError::BadLine("a request is one line"));
        }
        if line.len() + 1 > MAX_LINE {
            return Err(SendError::BadLine("a request is at most 64 KiB"));
        }
        let mut guard = lock(&self.stdin);
        let stdin = guard.as_mut().ok_or(SendError::Closed)?;
        let mut buf = Vec::with_capacity(line.len() + 1);
        buf.extend_from_slice(line.as_bytes());
        buf.push(b'\n');
        if stdin.write_all(&buf).and_then(|()| stdin.flush()).is_err() {
            *guard = None;
            return Err(SendError::Closed);
        }
        Ok(())
    }

    /// Closes ssh's stdin, so lumen-ctl ends as it does at EOF, and kills
    /// ssh if it is still there after a grace period. `Closed` follows.
    pub fn close(&self) {
        if lock(&self.stdin).take().is_none() {
            return;
        }
        let child = Arc::clone(&self.child);
        let _ = thread::Builder::new()
            .name("link-close".into())
            .spawn(move || {
                let end = Instant::now() + GRACE;
                while Instant::now() < end {
                    if !matches!(lock(&child).try_wait(), Ok(None)) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                let _ = lock(&child).kill();
            });
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

fn keep_tail(mut stderr: impl Read) -> String {
    let mut tail = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = stderr.read(&mut buf) {
        if n == 0 {
            break;
        }
        tail.extend_from_slice(&buf[..n]);
        if tail.len() > STDERR_KEEP {
            tail.drain(..tail.len() - STDERR_KEEP);
        }
    }
    String::from_utf8_lossy(&tail).into_owned()
}

fn read_lines(
    stdout: impl Read,
    stderr: mpsc::Receiver<String>,
    child: Arc<Mutex<Child>>,
    context: Context,
    mut on_event: impl FnMut(Event),
) {
    let mut reader = BufReader::new(stdout);
    let mut raw = Vec::new();
    let mut too_long = false;
    loop {
        raw.clear();
        let limit = (MAX_LINE + 1) as u64;
        match (&mut reader).take(limit).read_until(b'\n', &mut raw) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if raw.last() != Some(&b'\n') && raw.len() > MAX_LINE {
            too_long = true;
            let _ = lock(&child).kill();
            break;
        }
        while matches!(raw.last(), Some(b'\n' | b'\r')) {
            raw.pop();
        }
        if !raw.is_empty() {
            on_event(Event::Line(String::from_utf8_lossy(&raw).into_owned()));
        }
    }
    // stdout has ended, so ssh is on its way out; give it the grace period.
    let end = Instant::now() + GRACE;
    let code = loop {
        match lock(&child).try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if Instant::now() < end => {}
            _ => {
                let mut c = lock(&child);
                let _ = c.kill();
                break c.wait().ok().and_then(|s| s.code());
            }
        }
        thread::sleep(Duration::from_millis(20));
    };
    // Something ssh started could hold stderr open after ssh has gone.
    let stderr = stderr.recv_timeout(STDERR_WAIT).unwrap_or_default();
    let reason = if too_long {
        "A line from the TV was longer than 64 KiB".to_owned()
    } else {
        ssh::explain(&stderr, code, &context.key, &context.host)
    };
    on_event(Event::Closed(reason));
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            key: PathBuf::from("/k/id"),
            host: "tv".into(),
        }
    }

    fn sh(script: &str) -> (Session, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let argv: Vec<OsString> = vec!["sh".into(), "-c".into(), script.into()];
        let s = Session::spawn(&argv, context(), move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        (s, rx)
    }

    fn next(rx: &mpsc::Receiver<Event>) -> Event {
        rx.recv_timeout(Duration::from_secs(10)).expect("an event")
    }

    #[test]
    fn lines_out_and_back_then_closed_when_stdin_ends() {
        let (s, rx) = sh("echo hello; while read l; do echo \"got $l\"; done");
        assert_eq!(next(&rx), Event::Line("hello".into()));
        s.send("{\"id\":1}").unwrap();
        assert_eq!(next(&rx), Event::Line("got {\"id\":1}".into()));
        s.close();
        assert_eq!(next(&rx), Event::Closed("The TV closed the link".into()));
        assert_eq!(s.send("x"), Err(SendError::Closed));
    }

    #[test]
    fn refuses_more_than_one_line_and_long_ones() {
        let (s, _rx) = sh("cat >/dev/null");
        assert!(matches!(s.send("a\nb"), Err(SendError::BadLine(_))));
        assert!(matches!(s.send("a\r"), Err(SendError::BadLine(_))));
        assert!(matches!(
            s.send(&"x".repeat(MAX_LINE)),
            Err(SendError::BadLine(_))
        ));
        s.send(&"x".repeat(MAX_LINE - 1)).unwrap();
    }

    #[test]
    fn says_why_ssh_gave_up() {
        let (_s, rx) = sh("echo 'root@tv: Permission denied (publickey).' >&2; exit 255");
        match next(&rx) {
            Event::Closed(r) => assert!(r.contains("refused the key"), "{r}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_line_over_64_kib_ends_the_session() {
        let (_s, rx) = sh("head -c 70000 /dev/zero | tr '\\0' x; echo; sleep 30");
        let started = Instant::now();
        assert_eq!(
            next(&rx),
            Event::Closed("A line from the TV was longer than 64 KiB".into())
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn close_kills_what_does_not_end_at_eof() {
        let (s, rx) = sh("trap '' HUP; exec sleep 60");
        s.close();
        let started = Instant::now();
        assert!(matches!(next(&rx), Event::Closed(_)));
        assert!(started.elapsed() < GRACE + Duration::from_secs(3));
    }
}
