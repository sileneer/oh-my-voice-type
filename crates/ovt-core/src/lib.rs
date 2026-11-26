//! ovt-core — framework-independent voice dictation engine.
//!
//! Shared by every host (desktop today: macOS/Windows via Tauri; Linux and
//! mobile hosts later). Knows nothing about Windows, Tauri or UI; hosts own
//! audio capture, hotkeys, text insertion and persistence wiring.

pub mod config;
pub mod dictation;
pub mod dictionary;
pub mod error;
pub mod history;
pub mod hotkey;
pub mod pending;
pub mod prompt;
pub mod providers;
pub mod wav;

pub use error::{Error, Result};
