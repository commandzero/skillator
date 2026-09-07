pub mod acquisition;
pub mod app;
pub mod cli;
pub mod config;
pub mod domain;
// Atomic no-replace/exchange renames need platform APIs absent from std.
// Keep the only unsafe exception private and confined to this module.
#[allow(unsafe_code)]
mod fs_safety;
pub mod git;
pub mod library;
mod materialization;
pub mod reconcile;
pub mod target;
pub mod tui;

pub use cli::run;
