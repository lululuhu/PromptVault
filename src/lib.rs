//! PromptVault library surface.
//!
//! The binary (`pv`) lives in `src/main.rs`; this lib exposes all modules so
//! they can be unit/integration tested.

pub mod cli;
pub mod commands;
pub mod core;
pub mod diff;
pub mod render;
pub mod tui;
pub mod ui;
