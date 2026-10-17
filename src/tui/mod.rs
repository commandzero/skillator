//! Terminal session, scope navigation and shared event dispatch.

pub mod input;
pub mod interaction;
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
        use crossterm::terminal::enable_raw_mode;
        use std::io::stdout;

        enable_raw_mode().map_err(fatal)?;
        let mut stdout = stdout();
        if let Err(error) = enter_terminal_with_cleanup(&mut stdout) {
            let _ = crossterm::terminal::disable_raw_mode();
            return Err(fatal(error));
        }
        let terminal = match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => terminal,
            Err(error) => {
                let mut stdout = std::io::stdout();
                let _ = restore_terminal(&mut stdout);
                let _ = crossterm::terminal::disable_raw_mode();
                return Err(fatal(error));
            }
        };
        Ok(Self { terminal })
    }
}

fn enter_terminal(writer: &mut impl std::io::Write) -> std::io::Result<()> {
    use crossterm::ExecutableCommand;
    writer
        .execute(crossterm::terminal::EnterAlternateScreen)?
        .execute(crossterm::event::EnableMouseCapture)
        .map(|_| ())
}

fn enter_terminal_with_cleanup(writer: &mut impl std::io::Write) -> std::io::Result<()> {
    match enter_terminal(writer) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = restore_terminal(writer);
            Err(error)
        }
    }
}

fn restore_terminal(writer: &mut impl std::io::Write) -> std::io::Result<()> {
    use crossterm::ExecutableCommand;
    let disable_mouse = writer
        .execute(crossterm::event::DisableMouseCapture)
        .map(|_| ());
    let leave_screen = writer
        .execute(crossterm::terminal::LeaveAlternateScreen)
        .map(|_| ());
    match (disable_mouse, leave_screen) {
        (Err(error), _) => Err(error),
        (Ok(()), result) => result,
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        use std::io::stdout;

        let _ = crossterm::terminal::disable_raw_mode();
        let _ = restore_terminal(&mut stdout());
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
    let mut interactions = interaction::InteractionMap::default();
    loop {
        redraw |= on_tick(&mut model)?;
        if redraw {
            terminal
                .draw(|frame| render::render(frame, &model, &mut interactions))
                .map_err(fatal)?;
            if let Some(max) = interactions.detail_scroll_max {
                model.detail_scroll = model.detail_scroll.min(max);
            }
            redraw = false;
        }
        if !event::poll(Duration::from_millis(80)).map_err(fatal)? {
            continue;
        }
        let event = event::read().map_err(fatal)?;
        let action = match event {
            Event::Resize(_, _) => {
                interactions.clear_targets();
                redraw = true;
                continue;
            }
            Event::Mouse(mouse) => {
                let size = terminal.size().map_err(fatal)?;
                if interactions.area.width != size.width || interactions.area.height != size.height
                {
                    interactions.clear_targets();
                    redraw = true;
                    continue;
                }
                input::action_for_mouse(mouse, &interactions)
            }
            Event::Key(key) => input::action_for_model_key(&model, key),
            _ => None,
        };
        let Some(action) = action else {
            continue;
        };
        let keeps_detail_scroll_view = interactions.detail_scroll_max.is_some()
            && matches!(
                &action,
                model::Action::MoveDown
                    | model::Action::MoveUp
                    | model::Action::PageDown
                    | model::Action::PageUp
            )
            && matches!(&model.overlay, model::Overlay::Details { .. });
        let overlay_before_action =
            keeps_detail_scroll_view.then(|| std::mem::discriminant(&model.overlay));
        for effect in reducer::reduce(&mut model, action) {
            if let Some(status) = handle_effect(&mut model, effect)? {
                return Ok((status, model));
            }
        }
        if let Some(overlay_before_action) = overlay_before_action
            && std::mem::discriminant(&model.overlay) == overlay_before_action
            && let Some(max) = interactions.detail_scroll_max
        {
            model.detail_scroll = model.detail_scroll.min(max);
        }
    }
}

fn fatal(error: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::Fatal {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod terminal_lifecycle_tests {
    use super::{enter_terminal_with_cleanup, restore_terminal};
    use std::io::{self, Write};

    #[derive(Default)]
    struct FailWrite {
        fail_at: [usize; 2],
        writes: usize,
        bytes: Vec<u8>,
    }

    impl Write for FailWrite {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            if self.fail_at.contains(&self.writes) {
                return Err(io::Error::other(format!(
                    "injected write failure {}",
                    self.writes
                )));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn terminal_cleanup_commands_disable_capture_before_leaving_screen() {
        let mut output = Vec::new();
        restore_terminal(&mut output).unwrap();
        let commands = String::from_utf8(output).unwrap();
        assert!(commands.find("?1006l").is_some());
        assert!(commands.find("?1049l").is_some());
        assert!(commands.find("?1006l") < commands.find("?1049l"));
    }

    #[test]
    fn failed_setup_after_entering_screen_preserves_error_and_attempts_cleanup() {
        let mut writer = FailWrite {
            fail_at: [2, 3],
            ..FailWrite::default()
        };
        let error = enter_terminal_with_cleanup(&mut writer).unwrap_err();
        assert_eq!(error.to_string(), "injected write failure 2");
        assert_eq!(writer.writes, 4);
        let commands = String::from_utf8(writer.bytes).unwrap();
        assert!(commands.contains("?1049h"));
        assert!(commands.contains("?1049l"));
    }

    #[test]
    fn cleanup_attempts_leave_screen_after_disabling_capture_fails() {
        let mut writer = FailWrite {
            fail_at: [1, 0],
            ..FailWrite::default()
        };
        let error = restore_terminal(&mut writer).unwrap_err();
        assert_eq!(error.to_string(), "injected write failure 1");
        assert!(String::from_utf8(writer.bytes).unwrap().contains("?1049l"));
    }
}
