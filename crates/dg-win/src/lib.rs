//! Windows implementation of the `dg-engine` backends plus the small amount of shell
//! and UI plumbing the CLI needs.
//!
//! CCD and GDI display structs are declared by hand in [`ffi`] (their unions are much
//! simpler to handle as fixed byte layouts, and every size is checked at compile time).
//! COM audio goes through `windows-rs`.

#![cfg(windows)]

pub mod agent;
mod audio;
pub mod confirm;
mod display;
mod ffi;
mod ffi_ui;
pub mod icons;
pub mod integration;
pub mod osd;
pub mod shell;
pub mod system;
pub mod tray;
pub mod ui;
mod util;
pub mod window;

pub use audio::WinAudio;
pub use display::WinBackend;
