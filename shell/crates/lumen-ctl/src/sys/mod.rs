//! The only module allowed `unsafe` (the crate root denies it everywhere
//! else): the C library (`os`), libluna-service2 (`luna`) and the kernel's
//! input devices (`uinput`). Each exposes a
//! safe interface; every `unsafe` block says why it holds.

pub mod luna;
pub mod os;
pub mod uinput;
