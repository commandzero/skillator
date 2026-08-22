pub mod acquisition;
pub mod app;
pub mod cli;
pub mod config;
pub mod domain;
mod fs_safety;
pub mod git;
pub mod library;
mod materialization;
pub mod reconcile;
pub mod target;
pub mod tui;

pub use cli::run;
