//! The shell and `lumend`'s own protocol (decision 14 in docs/decisions.md).
//!
//! `lumend`, root's daemon, listens on a Unix socket it makes from the host
//! side inside the shell's jail; the shell connects to it from inside. Each
//! side checks the other's uid through the kernel (`SO_PEERCRED`, in
//! [`unix`]): root on the shell's side, the jail's uid on `lumend`'s.
//!
//! What crosses it is one line of text per message, at most [`MAX_LINE`]
//! bytes before its `\n`: words of lower-case letters, digits, `-` and `=`,
//! one space apart. Every word is one of a fixed set or a bounded decimal
//! number, so no line can carry a path, a name or free text, and anything
//! else is a protocol error that ends the connection.
//!
//! The shell asks ([`Request`]):
//!
//! | Line | What for |
//! |---|---|
//! | `hello 1` | first, always: the protocol version |
//! | `status` | each part's state, for the Privacy panel |
//! | `icons` | refresh the copy of LG's icons in Lumen's folder |
//! | `screen-off` | LG's Screen Off, after the screensaver's two hours (decision 16) |
//! | `leaving` | the shell is about to exit on purpose: don't start it again |
//!
//! `lumend` answers each in order ([`Reply`]), and may send one event
//! unasked: `relaunch home-key` or `relaunch other`, when SAM starts Lumen
//! again while it runs (a Home press with Lumen in front, if the B9 reports
//! one to a root subscriber).

#![deny(unsafe_code)]

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub mod unix;

use std::fmt;
use std::io::{self, BufRead, Read};

/// The protocol's version, the number in `hello`.
pub const VERSION: u32 = 1;
/// The longest line either side sends or accepts, without its `\n`.
pub const MAX_LINE: usize = 160;
/// The socket's path as the shell sees it, inside its jail.
pub const SOCKET_IN_JAIL: &str = "/var/lumen/lumend.sock";
/// The same folder, one name a level, from the jail's root: `lumend` walks
/// it from the host side without following a link.
pub const SOCKET_FOLDER: [&str; 2] = ["var", "lumen"];
/// The socket's name in that folder.
pub const SOCKET_NAME: &str = "lumend.sock";

/// What went wrong with a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtoError {
    /// Over [`MAX_LINE`], or cut off by the end of the stream.
    Length,
    /// A byte outside the protocol's letters, or two spaces in a row.
    Bytes,
    /// Words that make no message.
    Unknown,
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Length => "a line too long, or cut off",
            Self::Bytes => "a byte the protocol doesn't use",
            Self::Unknown => "words that make no message",
        })
    }
}

impl std::error::Error for ProtoError {}

/// `lumend`'s parts, each behind its own switch in `lumend.conf`, in the
/// order `status` lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    /// Privacy stage S1: LG's diagnostics jobs held down, tcp 11111 closed.
    S1,
    /// S2: LG's on-demand senders (acr2) held down by a stand-in.
    Senders,
    /// LG's Home closed after boot while Lumen is the TV's Home.
    LgHome,
    /// LG's icons copied into Lumen's folder.
    Icons,
    /// Lumen's file modes put back after LG's developer-mode job.
    Modes,
    /// The shell started hidden after boot, and again if it is closed.
    Shell,
    /// LG's Screen Off on the shell's request.
    ScreenOff,
    /// This socket.
    Link,
}

impl Part {
    pub const ALL: [Self; 8] = [
        Self::S1,
        Self::Senders,
        Self::LgHome,
        Self::Icons,
        Self::Modes,
        Self::Shell,
        Self::ScreenOff,
        Self::Link,
    ];

    /// The part's name in `lumend.conf`, the log and the protocol.
    pub fn name(self) -> &'static str {
        match self {
            Self::S1 => "s1",
            Self::Senders => "senders",
            Self::LgHome => "lg-home",
            Self::Icons => "icons",
            Self::Modes => "modes",
            Self::Shell => "shell",
            Self::ScreenOff => "screen-off",
            Self::Link => "link",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }
}

/// What a part is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Switched off in `lumend.conf`.
    Off,
    /// Switched on, and working.
    On,
    /// Switched on, but it failed and stopped: the log says why.
    Failed,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Failed => "failed",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        [Self::Off, Self::On, Self::Failed]
            .into_iter()
            .find(|s| s.name() == name)
    }
}

/// Every part's state, in [`Part::ALL`]'s order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parts(pub [State; 8]);

impl Parts {
    pub fn get(&self, part: Part) -> State {
        self.0[part as usize]
    }

    pub fn set(&mut self, part: Part, state: State) {
        self.0[part as usize] = state;
    }
}

impl Default for Parts {
    fn default() -> Self {
        Self([State::Off; 8])
    }
}

/// Why an icon copy failed, as a fixed word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconsFault {
    /// A folder on the way failed its check (a link, or writable by others).
    Folders,
    /// LG's app list couldn't be read.
    Luna,
    /// Reading or writing a file failed.
    Io,
}

impl IconsFault {
    fn name(self) -> &'static str {
        match self {
            Self::Folders => "folders",
            Self::Luna => "luna",
            Self::Io => "io",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        [Self::Folders, Self::Luna, Self::Io]
            .into_iter()
            .find(|f| f.name() == name)
    }
}

/// How an icon copy went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icons {
    Changed {
        copied: u32,
        refused: u32,
    },
    Unchanged,
    /// Another copy is running: this one was not made.
    Busy,
    Failed(IconsFault),
    /// The part is switched off.
    Off,
}

/// How a Screen Off request went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenOff {
    Done,
    /// LG's call failed.
    Failed,
    /// Not done: Lumen isn't in front, or one came less than a minute ago.
    Refused,
    /// The part is switched off.
    Off,
}

/// Why `lumend` refused a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A request it doesn't know, or one before `hello`.
    Unknown,
    /// `hello` with another version.
    Version,
    /// Requests faster than it takes them.
    TooFast,
}

impl Refusal {
    fn name(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Version => "version",
            Self::TooFast => "too-fast",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        [Self::Unknown, Self::Version, Self::TooFast]
            .into_iter()
            .find(|r| r.name() == name)
    }
}

/// What the shell asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Hello { version: u32 },
    Status,
    Icons,
    ScreenOff,
    Leaving,
}

/// What `lumend` sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Hello {
        version: u32,
    },
    Status(Parts),
    Icons(Icons),
    ScreenOff(ScreenOff),
    Leaving,
    /// Unasked: SAM started Lumen again while it ran; `home_key` when the
    /// reason was a Home press.
    Relaunch {
        home_key: bool,
    },
    Error(Refusal),
}

/// The words of a line, checked: only the protocol's bytes, single spaces,
/// no space at either end.
fn words(line: &str) -> Result<Vec<&str>, ProtoError> {
    if line.len() > MAX_LINE {
        return Err(ProtoError::Length);
    }
    if !line
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-= ".contains(&b))
    {
        return Err(ProtoError::Bytes);
    }
    let words: Vec<&str> = line.split(' ').collect();
    if words.iter().any(|w| w.is_empty()) {
        return Err(ProtoError::Bytes);
    }
    Ok(words)
}

/// A decimal number of at most nine digits, without leading zeros.
fn number(word: &str) -> Result<u32, ProtoError> {
    let digits = word.bytes().all(|b| b.is_ascii_digit());
    if !digits || word.is_empty() || word.len() > 9 || (word.len() > 1 && word.starts_with('0')) {
        return Err(ProtoError::Unknown);
    }
    word.parse().map_err(|_| ProtoError::Unknown)
}

impl Request {
    pub fn parse(line: &str) -> Result<Self, ProtoError> {
        Ok(match words(line)?.as_slice() {
            ["hello", v] => Self::Hello {
                version: number(v)?,
            },
            ["status"] => Self::Status,
            ["icons"] => Self::Icons,
            ["screen-off"] => Self::ScreenOff,
            ["leaving"] => Self::Leaving,
            _ => return Err(ProtoError::Unknown),
        })
    }

    /// The request as a line, with its `\n`.
    pub fn line(&self) -> String {
        let text = match self {
            Self::Hello { version } => format!("hello {version}"),
            Self::Status => "status".to_owned(),
            Self::Icons => "icons".to_owned(),
            Self::ScreenOff => "screen-off".to_owned(),
            Self::Leaving => "leaving".to_owned(),
        };
        text + "\n"
    }
}

impl Reply {
    pub fn parse(line: &str) -> Result<Self, ProtoError> {
        let words = words(line)?;
        Ok(match words.as_slice() {
            ["hello", v] => Self::Hello {
                version: number(v)?,
            },
            ["status", rest @ ..] => {
                if rest.len() != Part::ALL.len() {
                    return Err(ProtoError::Unknown);
                }
                let mut parts = Parts::default();
                for (word, part) in rest.iter().zip(Part::ALL) {
                    let (name, state) = word.split_once('=').ok_or(ProtoError::Unknown)?;
                    if name != part.name() {
                        return Err(ProtoError::Unknown);
                    }
                    parts.set(part, State::from_name(state).ok_or(ProtoError::Unknown)?);
                }
                Self::Status(parts)
            }
            ["icons", "changed", c, r] => Self::Icons(Icons::Changed {
                copied: number(c)?,
                refused: number(r)?,
            }),
            ["icons", "unchanged"] => Self::Icons(Icons::Unchanged),
            ["icons", "busy"] => Self::Icons(Icons::Busy),
            ["icons", "failed", f] => Self::Icons(Icons::Failed(
                IconsFault::from_name(f).ok_or(ProtoError::Unknown)?,
            )),
            ["icons", "off"] => Self::Icons(Icons::Off),
            ["screen-off", "done"] => Self::ScreenOff(ScreenOff::Done),
            ["screen-off", "failed"] => Self::ScreenOff(ScreenOff::Failed),
            ["screen-off", "refused"] => Self::ScreenOff(ScreenOff::Refused),
            ["screen-off", "off"] => Self::ScreenOff(ScreenOff::Off),
            ["leaving", "ok"] => Self::Leaving,
            ["relaunch", "home-key"] => Self::Relaunch { home_key: true },
            ["relaunch", "other"] => Self::Relaunch { home_key: false },
            ["error", r] => Self::Error(Refusal::from_name(r).ok_or(ProtoError::Unknown)?),
            _ => return Err(ProtoError::Unknown),
        })
    }

    /// The reply as a line, with its `\n`.
    pub fn line(&self) -> String {
        let text = match self {
            Self::Hello { version } => format!("hello {version}"),
            Self::Status(parts) => {
                let mut text = "status".to_owned();
                for part in Part::ALL {
                    text.push(' ');
                    text.push_str(part.name());
                    text.push('=');
                    text.push_str(parts.get(part).name());
                }
                text
            }
            Self::Icons(Icons::Changed { copied, refused }) => {
                format!("icons changed {copied} {refused}")
            }
            Self::Icons(Icons::Unchanged) => "icons unchanged".to_owned(),
            Self::Icons(Icons::Busy) => "icons busy".to_owned(),
            Self::Icons(Icons::Failed(f)) => format!("icons failed {}", f.name()),
            Self::Icons(Icons::Off) => "icons off".to_owned(),
            Self::ScreenOff(ScreenOff::Done) => "screen-off done".to_owned(),
            Self::ScreenOff(ScreenOff::Failed) => "screen-off failed".to_owned(),
            Self::ScreenOff(ScreenOff::Refused) => "screen-off refused".to_owned(),
            Self::ScreenOff(ScreenOff::Off) => "screen-off off".to_owned(),
            Self::Leaving => "leaving ok".to_owned(),
            Self::Relaunch { home_key: true } => "relaunch home-key".to_owned(),
            Self::Relaunch { home_key: false } => "relaunch other".to_owned(),
            Self::Error(r) => format!("error {}", r.name()),
        };
        text + "\n"
    }
}

/// Reads one line, without its `\n`: `None` at a clean end of the stream.
/// It never reads more than [`MAX_LINE`] bytes and the `\n` into memory; a
/// longer line, a line cut off by the end of the stream, or a byte outside
/// the protocol's is an error, after which the connection is closed.
pub fn read_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut buf = Vec::with_capacity(64);
    let n = reader
        .by_ref()
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.pop() != Some(b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            ProtoError::Length,
        ));
    }
    let line = String::from_utf8(buf)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, ProtoError::Bytes))?;
    words(&line).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(line))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_replies() -> Vec<Reply> {
        let mut parts = Parts::default();
        parts.set(Part::S1, State::On);
        parts.set(Part::Senders, State::Failed);
        vec![
            Reply::Hello { version: 1 },
            Reply::Status(parts),
            Reply::Status(Parts::default()),
            Reply::Icons(Icons::Changed {
                copied: 81,
                refused: 0,
            }),
            Reply::Icons(Icons::Unchanged),
            Reply::Icons(Icons::Busy),
            Reply::Icons(Icons::Failed(IconsFault::Folders)),
            Reply::Icons(Icons::Failed(IconsFault::Luna)),
            Reply::Icons(Icons::Failed(IconsFault::Io)),
            Reply::Icons(Icons::Off),
            Reply::ScreenOff(ScreenOff::Done),
            Reply::ScreenOff(ScreenOff::Failed),
            Reply::ScreenOff(ScreenOff::Refused),
            Reply::ScreenOff(ScreenOff::Off),
            Reply::Leaving,
            Reply::Relaunch { home_key: true },
            Reply::Relaunch { home_key: false },
            Reply::Error(Refusal::Unknown),
            Reply::Error(Refusal::Version),
            Reply::Error(Refusal::TooFast),
        ]
    }

    #[test]
    fn every_message_round_trips_within_a_line() {
        for reply in all_replies() {
            let line = reply.line();
            assert!(line.ends_with('\n'));
            let text = line.trim_end_matches('\n');
            assert!(text.len() <= MAX_LINE, "{text}");
            assert_eq!(Reply::parse(text), Ok(reply), "{text}");
        }
        for request in [
            Request::Hello { version: 1 },
            Request::Status,
            Request::Icons,
            Request::ScreenOff,
            Request::Leaving,
        ] {
            let line = request.line();
            assert_eq!(Request::parse(line.trim_end_matches('\n')), Ok(request));
        }
    }

    #[test]
    fn the_status_line_is_fixed() {
        let mut parts = Parts::default();
        parts.set(Part::Icons, State::On);
        assert_eq!(
            Reply::Status(parts).line(),
            "status s1=off senders=off lg-home=off icons=on modes=off shell=off screen-off=off link=off\n"
        );
    }

    #[test]
    fn refuses_anything_else() {
        for bad in [
            "",
            " status",
            "status ",
            "status  ",
            "STATUS",
            "hello",
            "hello 01",
            "hello 1234567890",
            "hello -1",
            "hello 1 2",
            "icons /etc/passwd",
            "icons\tx",
            "status\0",
            "screen-off now",
            "quit",
        ] {
            assert!(Request::parse(bad).is_err(), "{bad:?}");
        }
        for bad in [
            "status s1=on",
            "status senders=on s1=on lg-home=off icons=on modes=off shell=off screen-off=off link=off",
            "status s1=maybe senders=on lg-home=off icons=on modes=off shell=off screen-off=off link=off",
            "icons changed 1",
            "icons failed everything",
            "error",
            "relaunch",
        ] {
            assert!(Reply::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn parts_have_unique_names() {
        for part in Part::ALL {
            assert_eq!(Part::from_name(part.name()), Some(part));
        }
        assert_eq!(Part::from_name("S1"), None);
    }

    #[test]
    fn reads_lines_within_the_cap() {
        let mut input: &[u8] = b"hello 1\nstatus\n";
        assert_eq!(read_line(&mut input).unwrap().as_deref(), Some("hello 1"));
        assert_eq!(read_line(&mut input).unwrap().as_deref(), Some("status"));
        assert_eq!(read_line(&mut input).unwrap(), None);

        let long = format!("{}\n", "a".repeat(MAX_LINE + 1));
        assert!(read_line(&mut long.as_bytes()).is_err());
        let exact = format!("{}\n", "a".repeat(MAX_LINE));
        assert!(read_line(&mut exact.as_bytes()).is_ok());
        assert!(read_line(&mut &b"status"[..]).is_err(), "cut off");
        assert!(read_line(&mut &b"st\xffus\n"[..]).is_err());
        assert!(read_line(&mut &b"Status\n"[..]).is_err());
    }

    #[test]
    fn the_socket_path_is_its_folder_and_name() {
        assert_eq!(
            SOCKET_IN_JAIL,
            format!("/{}/{}", SOCKET_FOLDER.join("/"), SOCKET_NAME)
        );
    }

    #[test]
    fn the_network_lint_stays_configured() {
        let conf = include_str!("../clippy.toml");
        for name in ["TcpListener", "TcpStream", "UdpSocket", "libc::socket"] {
            assert!(conf.contains(name), "clippy.toml no longer names {name}");
        }
    }
}
