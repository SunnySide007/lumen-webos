//! lumend: Lumen's one root daemon on the TV (decision 14 in
//! docs/decisions.md; depth.md item 18).
//!
//! Each part is its own switch in `lumend.conf` and its own `lumend try`
//! and `lumend undo`, so each is tried by hand before it runs at boot
//! (docs/safety.md). The parts:
//!
//! * `s1`: the remote-diagnostics jobs held down, and tcp 11111 closed
//!   (privacy stage S1);
//! * `senders`: LG's on-demand senders (acr2) held down by a stand-in;
//! * `lg-home`: LG Home closed after boot, on SAM's events;
//! * `icons`: the copy of LG's app icons into Lumen's folder;
//! * `modes`: Lumen's folder made root's alone after LG's devmode job;
//! * `shell`: the shell started and kept (decision 15);
//! * `screen-off`: LG's Screen Off at the shell's request (decision 16);
//! * `link`: the shell's socket, in its jail.
//!
//! No network socket: clippy refuses std's network types and libc's raw
//! sockets in this crate (clippy.toml). All `unsafe` is in `sys`.

#![deny(unsafe_code)]

pub mod bus;
pub mod conf;
pub mod daemon;
pub mod fsat;
pub mod groups;
pub mod icons;
pub mod keeper;
pub mod lg_home;
pub mod link;
pub mod log;
pub mod modes;
pub mod paths;
pub mod run;
pub mod s1;
pub mod senders;
#[allow(unsafe_code)]
pub mod sys;
