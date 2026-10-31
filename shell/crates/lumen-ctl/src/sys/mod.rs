//! The only module allowed `unsafe` (the crate root denies it everywhere
//! else): the C library (`os`) and libluna-service2 (`luna`). Each exposes a
//! safe interface; every `unsafe` block says why it holds.

pub mod luna;
pub mod os;
