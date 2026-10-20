//! Lumen's shell logic, with no SDL or GL: what the keys mean, where focus
//! goes, where everything sits in the 1920x1080 space, how it moves, and which
//! apps there are. Time is always passed in, never read from a clock, so every
//! behaviour here is deterministic and runs the same on a PC and on the TV.
//!
//! The behaviour is specified in `docs/specs/shell-mvp.md`.

#![forbid(unsafe_code)]

pub mod anim;
pub mod apps;
pub mod focus;
pub mod home;
pub mod input;
pub mod layout;
pub mod script;
