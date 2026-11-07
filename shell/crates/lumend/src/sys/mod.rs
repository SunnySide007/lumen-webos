//! The only module allowed `unsafe` (the crate root denies it everywhere
//! else): the C library (`os`, `inotify`) and libluna-service2 (`luna`).
//! Each exposes a safe interface, and every `unsafe` block says why it
//! holds. File work relative to a pinned folder is in `crate::fsat`, safe
//! code over `/proc/self/fd`.

pub mod inotify;
pub mod luna;
pub mod os;
