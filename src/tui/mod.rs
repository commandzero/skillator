//! Terminal session, scope navigation and shared event dispatch.

pub mod input;
pub mod library;
pub mod model;
pub mod reducer;
pub mod render;
mod repo;
mod target;
mod user;

#[cfg(test)]
mod tests;

use crate::app::{AppPaths, WorkflowError};
use crate::config::Fingerprint;
use crate::library::LibrarySnapshot;
use model::{BrowseState, Effect, Model, Scope};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::collections::BTreeMap;
use std::io::Stdout;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub fn run_target(paths: &AppPaths, directory: &Path) -> Result<u8, WorkflowError> {
    navigate(paths, Navigation::Target(directory.to_owned(), Scope::Repo))
}

enum Navigation {
    Exit(u8),
    Target(std::path::PathBuf, Scope),
    Library {
        return_target: Option<std::path::PathBuf>,
    },
}
#[derive(Default)]
struct SessionNavigation {
    target: Option<target::TargetView>,
    library_host: Option<String>,
    library_browse: BTreeMap<String, BrowseState>,
    library_snapshot: Option<(Fingerprint, Arc<LibrarySnapshot>)>,
    library: Option<library::LibraryView>,
}

enum InteractionExit {
    Exit(u8),
    Target(PathBuf),
    Scope(Scope),
    Reload,
}

type AppTerminal = Terminal<CrosstermBackend<Stdout>>;

struct TerminalSession {
    terminal: AppTerminal,
}

impl TerminalSession {
    fn new() -> Result<Self, WorkflowError> {
        use crossterm::execute;
        use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
        use std::io::stdout;

        enable_raw_mode().map_err(fatal)?;
        if let Err(error) = execute!(stdout(), EnterAlternateScreen) {
            let _ = crossterm::terminal::disable_raw_mode();
            return Err(fatal(error));
        }
        let terminal = match Terminal::new(CrosstermBackend::new(stdout())) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = crossterm::terminal::disable_raw_mode();
                let _ = execute!(stdout(), crossterm::terminal::LeaveAlternateScreen);
                return Err(fatal(error));
            }
        };
        Ok(Self { terminal })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        use crossterm::execute;
        use crossterm::terminal::{LeaveAlternateScreen, disable_raw_mode};
        use std::io::stdout;

        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
    }
}

fn navigate(paths: &AppPaths, mut navigation: Navigation) -> Result<u8, WorkflowError> {
    let mut session = TerminalSession::new()?;
    let mut initial_navigation = true;
    let mut browsing = SessionNavigation::default();
    loop {
        let onboard = initial_navigation;
        initial_navigation = false;
        navigation = match navigation {
            Navigation::Exit(status) => return Ok(status),
            Navigation::Target(directory, scope) => target::run_target_once(
                paths,
                &directory,
                scope,
                onboard,
                &mut browsing,
                &mut session.terminal,
            )?,
            Navigation::Library { return_target } => library::run_library_once(
                paths,
                return_target.as_deref(),
                &mut browsing,
                &mut session.terminal,
            )?,
        };
    }
}

fn invalid(error: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::InvalidInput {
        message: error.to_string(),
    }
}

fn config_issues(issues: Vec<crate::config::ConfigIssue>) -> WorkflowError {
    WorkflowError::InvalidInput {
        message: issues
            .into_iter()
            .map(|issue| format!("{}: {}", issue.path, issue.message))
            .collect::<Vec<_>>()
            .join("; "),
    }
}

fn run_interactive_with_tick(
    terminal: &mut AppTerminal,
    mut model: Model,
    mut handle_effect: impl FnMut(&mut Model, Effect) -> Result<Option<InteractionExit>, WorkflowError>,
    mut on_tick: impl FnMut(&mut Model) -> Result<bool, WorkflowError>,
) -> Result<(InteractionExit, Model), WorkflowError> {
    use crossterm::event::{self, Event};
    let mut redraw = true;
    loop {
        redraw |= on_tick(&mut model)?;
        if redraw {
            terminal
                .draw(|frame| render::render(frame, &model))
                .map_err(fatal)?;
            redraw = false;
        }
        if !event::poll(Duration::from_millis(80)).map_err(fatal)? {
            continue;
        }
        let key = match event::read().map_err(fatal)? {
            Event::Resize(_, _) => {
                redraw = true;
                continue;
            }
            Event::Key(key) => key,
            _ => continue,
        };
        let Some(action) = input::action_for_model_key(&model, key) else {
            continue;
        };
        redraw = true;
        for effect in reducer::reduce(&mut model, action) {
            if let Some(status) = handle_effect(&mut model, effect)? {
                return Ok((status, model));
            }
        }
    }
}

fn fatal(error: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::Fatal {
        message: error.to_string(),
    }
}
