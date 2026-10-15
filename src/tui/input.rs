//! Key decoding and literal editor input; no persistence or scope lifecycle.

use super::model::{Action, Model, Overlay};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

pub fn action_for_key(key: KeyEvent) -> Option<Action> {
    if key.modifiers == KeyModifiers::NONE {
        match key.code {
            KeyCode::Left => return Some(Action::Collapse),
            KeyCode::Down => return Some(Action::MoveDown),
            KeyCode::Up => return Some(Action::MoveUp),
            KeyCode::Right => return Some(Action::Expand),
            KeyCode::PageDown => return Some(Action::PageDown),
            KeyCode::PageUp => return Some(Action::PageUp),
            _ => {}
        }
    } else if key.modifiers == KeyModifiers::SHIFT {
        match key.code {
            KeyCode::Left => return Some(Action::Collapse),
            KeyCode::Down => return Some(Action::NextGroup),
            KeyCode::Up => return Some(Action::PreviousGroup),
            KeyCode::Right => return Some(Action::Expand),
            _ => {}
        }
    }
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match (key.code, control) {
        (KeyCode::Char('s'), true) => Some(Action::Save { fast: true }),
        (KeyCode::Char('h'), true) => Some(Action::PreviousScope),
        (KeyCode::Char('l'), true) => Some(Action::NextScope),
        (KeyCode::Char('t'), true) => Some(Action::NewTargetTab),
        (KeyCode::Char('j'), false) => Some(Action::MoveDown),
        (KeyCode::Char('k'), false) => Some(Action::MoveUp),
        (KeyCode::Char('J'), false) => Some(Action::NextGroup),
        (KeyCode::Char('K'), false) => Some(Action::PreviousGroup),
        (KeyCode::Char('h'), false) => Some(Action::Collapse),
        (KeyCode::Char('l'), false) => Some(Action::Expand),
        (KeyCode::Char(' '), false) => Some(Action::Toggle),
        (KeyCode::Char('m'), false) => Some(Action::SwitchMode),
        (KeyCode::Tab, false) => Some(Action::NextDirectory),
        (KeyCode::BackTab, false) => Some(Action::PreviousDirectory),
        (KeyCode::Char('/'), false) => Some(Action::StartFilter),
        (KeyCode::Esc, false) => Some(Action::Escape),
        (KeyCode::Char('s'), false) => Some(Action::Save { fast: false }),
        (KeyCode::Char('u'), false) => Some(Action::Undo),
        (KeyCode::Char('q'), false) => Some(Action::Quit),
        (KeyCode::Char('t'), false) => Some(Action::ChangeTarget),
        (KeyCode::Char('a'), false) => Some(Action::AddDirectory),
        (KeyCode::Char('e'), false) => Some(Action::EditDirectory),
        (KeyCode::Char('d'), false) => Some(Action::DeleteDirectory),
        (KeyCode::Char('r'), false) => Some(Action::RefreshLibrary),
        (KeyCode::Char('?'), false) => Some(Action::Help),
        (KeyCode::Enter, false) | (KeyCode::Char('y'), false) => Some(Action::Confirm),
        (KeyCode::Char('n'), false) => Some(Action::ReturnToEditing),
        (KeyCode::Backspace, false) => Some(Action::Backspace),
        (KeyCode::Char(character), false) => Some(Action::Input(character)),
        _ => None,
    }
}

pub(super) fn complete_path(input: &str) -> Option<String> {
    let (typed_directory, typed_prefix) = match input.rsplit_once('/') {
        Some((directory, prefix)) => (format!("{directory}/"), prefix),
        None => (String::new(), input),
    };
    let directory = expand_completion_directory(&typed_directory)?;
    let mut matches = std::fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            name.starts_with(typed_prefix)
                .then_some((name, entry.path().is_dir()))
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.0.cmp(&right.0));
    let first = matches.first()?;

    let completed_name = if matches.len() == 1 {
        first.0.clone()
    } else {
        common_prefix(matches.iter().map(|(name, _)| name.as_str()))
    };
    let trailing_separator = matches.len() == 1 && first.1;
    if completed_name == typed_prefix && !(trailing_separator && !input.ends_with('/')) {
        return None;
    }
    Some(format!(
        "{typed_directory}{completed_name}{}",
        if trailing_separator { "/" } else { "" }
    ))
}

fn expand_completion_directory(typed_directory: &str) -> Option<PathBuf> {
    if typed_directory == "~/" {
        return std::env::var_os("HOME").map(PathBuf::from);
    }
    if let Some(relative) = typed_directory.strip_prefix("~/") {
        return std::env::var_os("HOME").map(|home| PathBuf::from(home).join(relative));
    }
    if typed_directory.is_empty() {
        Some(PathBuf::from("."))
    } else {
        Some(PathBuf::from(typed_directory))
    }
}

fn common_prefix<'a>(mut values: impl Iterator<Item = &'a str>) -> String {
    let Some(first) = values.next() else {
        return String::new();
    };
    values.fold(first.to_owned(), |prefix, value| {
        prefix
            .chars()
            .zip(value.chars())
            .take_while(|(left, right)| left == right)
            .map(|(character, _)| character)
            .collect()
    })
}

pub(super) fn action_for_model_key(model: &Model, key: KeyEvent) -> Option<Action> {
    if matches!(
        model.overlay,
        Overlay::Filter
            | Overlay::DirectoryEditor { .. }
            | Overlay::DirectoryChooser { .. }
            | Overlay::FollowerEditor(_)
            | Overlay::LocationEditor { .. }
            | Overlay::SourceKeyEditor(_)
            | Overlay::TargetPicker(_)
    ) && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
    {
        if matches!(model.overlay, Overlay::DirectoryChooser { .. }) {
            match key.code {
                KeyCode::Down => return Some(Action::MoveDown),
                KeyCode::Up => return Some(Action::MoveUp),
                _ => {}
            }
        }
        return match key.code {
            KeyCode::Char(character) => Some(Action::Input(character)),
            KeyCode::Backspace => Some(Action::Backspace),
            KeyCode::Tab
                if matches!(
                    model.overlay,
                    Overlay::LocationEditor { .. } | Overlay::TargetPicker(_)
                ) =>
            {
                Some(Action::CompletePath)
            }
            KeyCode::Enter => Some(Action::Confirm),
            KeyCode::Esc => Some(Action::Escape),
            _ => None,
        };
    }
    action_for_key(key)
}

pub(super) fn chooser_matches(input: &str) -> Vec<(&'static str, &'static str)> {
    let needle = input.trim().to_ascii_lowercase();
    [
        ("Generic / Codex", ".agents/skills"),
        ("Claude", ".claude/skills"),
    ]
    .into_iter()
    .filter(|(agent, path)| agent.to_ascii_lowercase().contains(&needle) || path.contains(&needle))
    .collect()
}
