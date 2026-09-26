//! Explicit SSH library synchronization. Git revisions and file edits are separate state.

mod config;
mod coordinator;
mod planner;
mod process;
mod session;
mod snapshot;
mod state;
mod transport;

pub(crate) use coordinator::{Options, run};
pub(crate) use planner::{ConflictPolicy, MissingPolicy};
pub(crate) use transport::serve;
pub(crate) use transport::serve_transfer;

pub(crate) fn validate_location_path(home: &std::path::Path, path: &std::path::Path) -> Result<()> {
    let canonical_home = home.canonicalize().map_err(Error::input_display)?;
    let relative = path
        .strip_prefix(home)
        .or_else(|_| path.strip_prefix(&canonical_home))
        .map_err(Error::input_display)?;
    let relative = relative
        .to_str()
        .ok_or_else(|| Error::input("non-UTF-8 library location"))?;
    let contained = state::contained(home, relative)?;
    if std::fs::symlink_metadata(&contained).is_ok_and(|metadata| metadata.file_type().is_symlink())
        && contained.canonicalize().is_err()
    {
        return Err(Error::input("unresolvable library location link"));
    }
    Ok(())
}

use std::fmt;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub(crate) struct Error {
    pub code: u8,
    message: String,
}

impl Error {
    fn input(message: impl Into<String>) -> Self {
        Self {
            code: 3,
            message: message.into(),
        }
    }
    fn argument(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }
    fn input_display(error: impl fmt::Display) -> Self {
        Self::input(error.to_string())
    }
    fn busy() -> Self {
        Self {
            code: 4,
            message: "another Skillator process is saving changes".into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for Error {}
