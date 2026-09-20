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
