//! Lumen Remote's PC end (docs/specs/remote.md, R3), without the window: the
//! app's settings in `~/.lumen/remote/`, the SSH session to `lumen-ctl serve`
//! with the app's own forced-command key, and Wake-on-LAN.
//!
//! The session is Windows' own OpenSSH client run with the options
//! `tools/remote/ctl.py` uses on the B9 (only the app's key, no password, no
//! terminal, no forwarding, no configuration file), one JSON line each way.
//! It passes lines through: the web front-end's `RemoteClient` checks the
//! hello and every reply, as it does over a WebSocket.

pub mod session;
pub mod settings;
pub mod ssh;
pub mod wol;

pub use session::{Event, Session};
pub use settings::{Mac, Paths, Settings};
