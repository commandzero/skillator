//! Pure semantic action transitions; scope workflows execute returned effects.

use super::input::{chooser_matches, complete_path};
use super::library::{refresh_library_action, refresh_library_visibility};
use super::model::{
    Action, CheckState, Effect, Model, Overlay, Row, RowAction, RowKind, Scope, TargetTabScope,
    Workspace, row_identity,
};
use crate::acquisition::LibraryAcquisitionMode;
use crate::domain::MaterializationKind;

pub fn reduce(model: &mut Model, action: Action) -> Vec<Effect> {
    if model.overlay == Overlay::Filter {
        match action {
            Action::Input(character) => model.filter.push(character),
            Action::Backspace => {
                model.filter.pop();
            }
            Action::Escape => {
                model.filter.clear();
                model.overlay = Overlay::None;
            }
            Action::Confirm => model.overlay = Overlay::None,
            Action::MoveDown => model.select_visible_offset(1),
            Action::MoveUp => model.select_visible_offset(-1),
            _ => {}
        }
        return Vec::new();
    }
    if let Overlay::DirectoryChooser { input, selected } = &mut model.overlay {
        match action {
            Action::Input(character) => {
                input.push(character);
                *selected = 0;
            }
            Action::Backspace => {
                input.pop();
                *selected = 0;
            }
            Action::MoveDown => {
                *selected = (*selected + 1).min(chooser_matches(input).len().saturating_sub(1))
            }
            Action::MoveUp => *selected = selected.saturating_sub(1),
            Action::SelectSuggestion(index) => {
                if index < chooser_matches(input).len() {
                    *selected = index;
                }
            }
            Action::Confirm => {
                let value = chooser_matches(input)
                    .get(*selected)
                    .map_or_else(|| input.clone(), |(_, path)| (*path).to_owned());
                model.overlay = Overlay::None;
                return vec![Effect::ApplyDirectoryEdit { edit: false, value }];
            }
            Action::Escape | Action::Quit => model.overlay = Overlay::None,
            _ => {}
        }
        return Vec::new();
    }
    if matches!(model.overlay, Overlay::FollowerProbe { .. }) {
        if matches!(action, Action::Escape | Action::Quit) {
            model.overlay = Overlay::None;
            return vec![Effect::CancelFollowerProbe];
        }
        return Vec::new();
    }
    if matches!(model.overlay, Overlay::Notice(_)) {
        model.overlay = Overlay::None;
    }
    if model.overlay != Overlay::None {
        match (&mut model.overlay, &action) {
            (Overlay::DirectoryEditor { input, .. }, Action::Input(character))
            | (Overlay::LocationEditor { input, .. }, Action::Input(character))
            | (Overlay::SourceKeyEditor(input), Action::Input(character))
            | (Overlay::TargetPicker(input), Action::Input(character))
            | (Overlay::FollowerEditor(input), Action::Input(character)) => {
                input.push(*character);
                return Vec::new();
            }
            (Overlay::DirectoryEditor { input, .. }, Action::Backspace)
            | (Overlay::LocationEditor { input, .. }, Action::Backspace)
            | (Overlay::SourceKeyEditor(input), Action::Backspace)
            | (Overlay::TargetPicker(input), Action::Backspace)
            | (Overlay::FollowerEditor(input), Action::Backspace) => {
                input.pop();
                return Vec::new();
            }
            (Overlay::LocationEditor { input, .. }, Action::CompletePath)
            | (Overlay::TargetPicker(input), Action::CompletePath) => {
                if let Some(completed) = complete_path(input) {
                    *input = completed;
                }
                return Vec::new();
            }
            (
                Overlay::Details { .. }
                | Overlay::Diagnostic { .. }
                | Overlay::FollowerSyncResult { .. }
                | Overlay::Help,
                Action::MoveDown,
            ) => {
                model.detail_scroll = model.detail_scroll.saturating_add(1);
                return Vec::new();
            }
            (
                Overlay::Details { .. }
                | Overlay::Diagnostic { .. }
                | Overlay::FollowerSyncResult { .. }
                | Overlay::Help,
                Action::MoveUp,
            ) => {
                model.detail_scroll = model.detail_scroll.saturating_sub(1);
                return Vec::new();
            }
            (
                Overlay::Details { .. }
                | Overlay::Diagnostic { .. }
                | Overlay::FollowerSyncResult { .. }
                | Overlay::Help,
                Action::PageDown,
            ) => {
                model.detail_scroll = model.detail_scroll.saturating_add(10);
                return Vec::new();
            }
            (
                Overlay::Details { .. }
                | Overlay::Diagnostic { .. }
                | Overlay::FollowerSyncResult { .. }
                | Overlay::Help,
                Action::PageUp,
            ) => {
                model.detail_scroll = model.detail_scroll.saturating_sub(10);
                return Vec::new();
            }
            _ => {}
        }
        let action = if action == Action::Quit {
            Action::Escape
        } else {
            action
        };
        match action {
            Action::Confirm if model.overlay == Overlay::Welcome => {
                model.overlay = Overlay::None;
            }
            Action::Quit | Action::Escape | Action::ReturnToEditing
                if model.overlay == Overlay::Welcome =>
            {
                return vec![Effect::Quit { status: 0 }];
            }
            Action::Escape | Action::ReturnToEditing
                if matches!(
                    model.overlay,
                    Overlay::ConfirmFollowerSync { .. } | Overlay::FollowerSyncResult { .. }
                ) =>
            {
                model.overlay = Overlay::None;
                return vec![Effect::FinishFollowerInit];
            }
            Action::Escape | Action::ReturnToEditing
                if matches!(
                    model.overlay,
                    Overlay::ConfirmSave
                        | Overlay::ConfirmSaveWarning(_)
                        | Overlay::GuardedConfirmation(_)
                ) =>
            {
                model.overlay = Overlay::None;
                return vec![Effect::CancelSave];
            }
            Action::Escape | Action::ReturnToEditing => model.overlay = Overlay::None,
            Action::Acknowledge => return vec![Effect::Quit { status: 1 }],
            Action::Confirm if matches!(model.overlay, Overlay::Result(_)) => {
                return vec![Effect::Quit { status: 1 }];
            }
            Action::Confirm if model.overlay == Overlay::DiscardTarget => {
                model.overlay = Overlay::TargetPicker(String::new());
            }
            Action::Confirm if matches!(model.overlay, Overlay::ScopeSwitch { .. }) => {
                let Overlay::ScopeSwitch {
                    destination,
                    host_to,
                } = model.overlay
                else {
                    unreachable!()
                };
                model.overlay = Overlay::None;
                return vec![Effect::SwitchToScope {
                    destination,
                    host_to,
                    save: true,
                    discard: false,
                }];
            }
            Action::DeleteDirectory if matches!(model.overlay, Overlay::ScopeSwitch { .. }) => {
                let Overlay::ScopeSwitch {
                    destination,
                    host_to,
                } = model.overlay
                else {
                    unreachable!()
                };
                model.overlay = Overlay::None;
                return vec![Effect::SwitchToScope {
                    destination,
                    host_to,
                    save: false,
                    discard: true,
                }];
            }
            Action::Confirm
                if matches!(
                    model.overlay,
                    Overlay::ConfirmSave
                        | Overlay::ConfirmSaveWarning(_)
                        | Overlay::GuardedConfirmation(_)
                ) =>
            {
                model.overlay = Overlay::None;
                return vec![Effect::CommitSave];
            }
            Action::Confirm if model.overlay == Overlay::Busy => {
                model.overlay = Overlay::None;
                return vec![Effect::RetrySave];
            }
            Action::Confirm if matches!(model.overlay, Overlay::Notice(_)) => {
                model.overlay = Overlay::None;
            }
            Action::Confirm
                if matches!(
                    model.overlay,
                    Overlay::Details { .. } | Overlay::Diagnostic { .. }
                ) =>
            {
                model.overlay = Overlay::None;
            }
            Action::Confirm => match std::mem::replace(&mut model.overlay, Overlay::None) {
                Overlay::DirectoryEditor { edit, input } => {
                    return vec![Effect::ApplyDirectoryEdit { edit, value: input }];
                }
                Overlay::LocationEditor { edit, input } => {
                    return vec![Effect::ApplyLocationEdit { edit, value: input }];
                }
                Overlay::FollowerEditor(name) => return vec![Effect::StartFollowerProbe(name)],
                Overlay::ConfirmFollowerSync { alias, .. } => {
                    return vec![Effect::StartFollowerSync(alias)];
                }
                Overlay::FollowerSyncResult { .. } => return vec![Effect::FinishFollowerInit],
                Overlay::TargetPicker(input) => return vec![Effect::ChangeTargetTo(input)],
                Overlay::SourceKeyEditor(input) => return vec![Effect::ApplySourceKey(input)],
                Overlay::ConfirmDelete => return vec![Effect::DeleteDirectory],
                overlay => model.overlay = overlay,
            },
            _ => {}
        }
        return Vec::new();
    }
    match action {
        Action::ActivateRow { index, intent } => {
            if !model.is_visible_index(index) {
                return Vec::new();
            }
            let was_selected = model.selected == index;
            model.selected = index;
            match intent {
                RowAction::Select
                    if was_selected
                        && model
                            .rows
                            .get(index)
                            .is_some_and(|row| row.kind == RowKind::Skill) =>
                {
                    return reduce(model, Action::Confirm);
                }
                RowAction::Select => return Vec::new(),
                RowAction::Toggle => return reduce(model, Action::Toggle),
                RowAction::SwitchMode => return reduce(model, Action::SwitchMode),
                RowAction::ToggleDisclosure => {
                    if let Some(row) = model.rows.get(index)
                        && row.kind == RowKind::Source
                        && let Some(identity) = row_identity(row)
                        && !model.collapsed.remove(identity)
                    {
                        model.collapsed.insert(identity.to_owned());
                    }
                    return Vec::new();
                }
            }
        }
        Action::SelectScope(destination) => return select_scope(model, destination, None),
        Action::SelectHost(index) => {
            if model.workspace != Workspace::Library || index >= model.host_labels.len() {
                return Vec::new();
            }
            return select_directory_or_host(model, index, true);
        }
        Action::SelectDirectory(index) => {
            if index >= model.directory_scopes.len() {
                return Vec::new();
            }
            let valid_scope = match model.scope {
                Scope::User => model.directory_scopes[index] == TargetTabScope::User,
                Scope::Repo => model.directory_scopes[index] == TargetTabScope::Repository,
                Scope::Library => false,
            };
            if !valid_scope {
                return Vec::new();
            }
            return select_directory_or_host(model, index, false);
        }
        _ => {}
    }
    if model.host_syncing {
        match action {
            Action::Escape => return vec![Effect::CancelFollowerSync],
            Action::Save { .. } | Action::NewTargetTab => {
                model.overlay = Overlay::Notice(
                    "Sync is running. Esc cancels; leaving the host or scope also cancels."
                        .to_owned(),
                );
                return Vec::new();
            }
            _ => {}
        }
    }
    if model.scope_error.is_some()
        && (matches!(
            action,
            Action::Toggle
                | Action::SwitchMode
                | Action::AddDirectory
                | Action::EditDirectory
                | Action::DeleteDirectory
        ) || model.workspace == Workspace::Target && action == Action::NewTargetTab)
    {
        model.overlay = Overlay::Notice(model.scope_error.clone().unwrap_or_default());
        return Vec::new();
    }
    if model.workspace == Workspace::Library
        && model.host_index != 0
        && matches!(
            action,
            Action::Toggle
                | Action::SwitchMode
                | Action::AddDirectory
                | Action::EditDirectory
                | Action::DeleteDirectory
                | Action::RefreshLibrary
        )
    {
        model.overlay = Overlay::Notice(
            "Follower replicas are read-only; select Local to edit the Library.".to_owned(),
        );
        return Vec::new();
    }
    match action {
        Action::MoveDown => model.select_visible_offset(1),
        Action::MoveUp => model.select_visible_offset(-1),
        Action::PageDown => model.select_visible_offset(10),
        Action::PageUp => model.select_visible_offset(-10),
        Action::NextGroup => model.select_group(1),
        Action::PreviousGroup => model.select_group(-1),
        Action::Collapse => {
            if let Some(row) = model.rows.get(model.selected)
                && let Some(identity) = row_identity(row)
            {
                model.collapsed.insert(identity.to_owned());
                if row.kind == RowKind::Skill
                    && let Some(source_index) =
                        model.rows[..model.selected].iter().rposition(|candidate| {
                            candidate.kind == RowKind::Source
                                && row_identity(candidate) == Some(identity)
                        })
                {
                    model.selected = source_index;
                }
            }
        }
        Action::Expand => {
            if let Some(row) = model.rows.get(model.selected)
                && row.kind == RowKind::Source
                && let Some(identity) = row_identity(row)
            {
                model.collapsed.remove(identity);
            }
        }
        Action::Toggle => toggle_selected(model),
        Action::SwitchMode => {
            let mut changed_group = None;
            if let Some(row) = model.rows.get_mut(model.selected)
                && row.kind == RowKind::Skill
            {
                if model.workspace == Workspace::Target && row.check == Some(CheckState::User) {
                    if row.available && row.valid {
                        row.check = Some(CheckState::Checked);
                        row.mode = Some(MaterializationKind::Linked);
                        refresh_staged_state(row);
                        changed_group = row_identity(row).map(str::to_owned);
                        model.dirty = true;
                    } else {
                        model.overlay = Overlay::Notice(
                            "Cannot link this skill: it is not available as a valid, registered Library skill."
                                .to_owned(),
                        );
                    }
                } else if !row.available {
                    return Vec::new();
                } else if model.workspace == Workspace::Target && row.repository_candidate {
                    if row.check == Some(CheckState::Repository) {
                        model.overlay = Overlay::Notice(
                            "This Skill is repository-owned. Manage its tracking exception in the parent .gitignore."
                                .to_owned(),
                        );
                    } else {
                        row.check = Some(CheckState::Repository);
                        row.mode = None;
                        row.action = "Track in repository".to_owned();
                        changed_group = row_identity(row).map(str::to_owned);
                        model.dirty = true;
                    }
                } else if model.workspace == Workspace::Target
                    && row.check == Some(CheckState::Checked)
                {
                    row.mode = Some(match row.mode {
                        Some(MaterializationKind::Linked) => MaterializationKind::Copied,
                        _ => MaterializationKind::Linked,
                    });
                    refresh_staged_state(row);
                    model.dirty = true;
                } else if model.workspace == Workspace::Library && row.acquisition_source.is_some()
                {
                    row.acquisition_mode = match row.acquisition_mode {
                        Some(LibraryAcquisitionMode::Move) => Some(LibraryAcquisitionMode::Copy),
                        Some(LibraryAcquisitionMode::Copy) => Some(LibraryAcquisitionMode::Link),
                        Some(LibraryAcquisitionMode::Link) => None,
                        None => Some(LibraryAcquisitionMode::Move),
                    };
                    row.acquisition_pending = row.acquisition_mode != row.initial_acquisition_mode;
                    refresh_library_action(row);
                    model.dirty = true;
                }
            }
            if let Some(group) = changed_group {
                model.recompute_group(&group);
            }
        }
        Action::NextDirectory | Action::PreviousDirectory => {
            if model.workspace == Workspace::Library {
                let count = model.host_labels.len();
                if count == 0 {
                    return Vec::new();
                }
                let next = if action == Action::NextDirectory {
                    (model.host_index + 1) % count
                } else {
                    (model.host_index + count - 1) % count
                };
                return select_directory_or_host(model, next, true);
            }
            let scoped: Vec<_> = model
                .directory_scopes
                .iter()
                .enumerate()
                .filter(|(_, scope)| match model.scope {
                    Scope::User => **scope == TargetTabScope::User,
                    Scope::Repo => **scope == TargetTabScope::Repository,
                    Scope::Library => false,
                })
                .map(|(index, _)| index)
                .collect();
            if scoped.is_empty() {
                return Vec::new();
            }
            let current = scoped
                .iter()
                .position(|index| *index == model.directory_index)
                .unwrap_or(0);
            let next = scoped[if action == Action::NextDirectory {
                (current + 1) % scoped.len()
            } else {
                (current + scoped.len() - 1) % scoped.len()
            }];
            return select_directory_or_host(model, next, false);
        }
        Action::NextScope | Action::PreviousScope => {
            let destination = model
                .scope
                .cycle(if action == Action::NextScope { 1 } else { -1 });
            return select_scope(model, destination, None);
        }
        Action::StartFilter => model.overlay = Overlay::Filter,
        Action::Save { fast } => {
            if model.workspace == Workspace::Library && model.host_index != 0 {
                model.exit_after_save = false;
                model.overlay = if model.dirty || model.host_dirty {
                    Overlay::ScopeSwitch {
                        destination: Scope::Library,
                        host_to: Some(0),
                    }
                } else if let Some(alias) = model.host_labels.get(model.host_index) {
                    Overlay::ConfirmFollowerSync {
                        alias: alias.clone(),
                        initial: false,
                    }
                } else {
                    Overlay::Notice("Follower is not configured.".to_owned())
                };
                return Vec::new();
            }
            model.exit_after_save = fast;
            return vec![Effect::PrepareSave { fast }];
        }
        Action::Undo if model.dirty || model.host_dirty => return vec![Effect::Undo],
        Action::Quit => return vec![Effect::Quit { status: 0 }],
        Action::ChangeTarget => {
            model.overlay = if model.dirty || model.host_dirty {
                Overlay::DiscardTarget
            } else {
                Overlay::TargetPicker(String::new())
            };
        }
        Action::NewTargetTab if model.workspace == Workspace::Library => {
            model.overlay = Overlay::FollowerEditor(String::new());
        }
        Action::AddDirectory | Action::NewTargetTab => {
            model.overlay = if model.workspace == Workspace::Library {
                Overlay::LocationEditor {
                    edit: false,
                    input: String::new(),
                }
            } else {
                Overlay::DirectoryChooser {
                    input: String::new(),
                    selected: 0,
                }
            };
        }
        Action::EditDirectory => {
            if model.workspace == Workspace::Library {
                let input = model
                    .selected_row()
                    .filter(|row| row.kind == RowKind::Location)
                    .map(|row| row.name.clone());
                if let Some(input) = input {
                    model.overlay = Overlay::LocationEditor { edit: true, input };
                }
            } else if !model
                .directory_scopes
                .get(model.directory_index)
                .is_some_and(|scope| {
                    matches!(
                        (model.scope, scope),
                        (Scope::User, TargetTabScope::User)
                            | (Scope::Repo, TargetTabScope::Repository)
                    )
                })
            {
                model.overlay = Overlay::Notice(
                    model
                        .unavailable
                        .clone()
                        .unwrap_or_else(|| "No skill folder is selected.".to_owned()),
                );
            } else {
                model.overlay = Overlay::DirectoryEditor {
                    edit: true,
                    input: model
                        .directory_values
                        .get(model.directory_index)
                        .cloned()
                        .unwrap_or_default(),
                };
            }
        }
        Action::DeleteDirectory => {
            if model.workspace == Workspace::Library
                && !model
                    .selected_row()
                    .is_some_and(|row| row.kind == RowKind::Location)
            {
                model.overlay =
                    Overlay::Notice("Select a library folder row to remove it.".to_owned());
            } else {
                model.overlay = Overlay::ConfirmDelete;
            }
        }
        Action::Help => {
            model.overlay = Overlay::Help;
            model.detail_scroll = 0;
        }
        Action::RefreshLibrary => {
            if model.workspace == Workspace::Library {
                if model.dirty {
                    model.overlay = Overlay::Notice(
                        "Save or discard unsaved library changes before refreshing.".to_owned(),
                    );
                } else {
                    return vec![Effect::RefreshLibrary];
                }
            }
        }
        Action::Confirm => {
            if let Some(row) = model
                .selected_row()
                .filter(|row| row.kind != RowKind::Diagnostic)
            {
                model.overlay = Overlay::Details {
                    title: row.name.clone(),
                    path: if row.details.is_empty() {
                        row.name.clone()
                    } else {
                        row.details.clone()
                    },
                    document: if row.frontmatter.is_empty() {
                        row.description.clone()
                    } else {
                        row.frontmatter.clone()
                    },
                    warnings: row.warnings.clone(),
                    errors: row.metadata_errors.clone(),
                };
                model.detail_scroll = 0;
            }
        }
        Action::Escape if !model.filter.is_empty() => model.filter.clear(),
        Action::ActivateRow { .. }
        | Action::SelectScope(_)
        | Action::SelectDirectory(_)
        | Action::SelectHost(_)
        | Action::SelectSuggestion(_)
        | Action::Input(_)
        | Action::Backspace
        | Action::CompletePath
        | Action::Escape
        | Action::Undo
        | Action::ReturnToEditing
        | Action::Acknowledge => {}
    }
    Vec::new()
}

fn select_scope(model: &mut Model, destination: Scope, host_to: Option<usize>) -> Vec<Effect> {
    if model.scope == destination && host_to.is_none() {
        return Vec::new();
    }
    if model.dirty || model.host_dirty {
        model.overlay = Overlay::ScopeSwitch {
            destination,
            host_to,
        };
        Vec::new()
    } else {
        vec![Effect::SwitchToScope {
            destination,
            host_to,
            save: false,
            discard: false,
        }]
    }
}

fn select_directory_or_host(model: &mut Model, index: usize, host: bool) -> Vec<Effect> {
    let current = if host {
        model.host_index
    } else {
        model.directory_index
    };
    if index == current {
        return Vec::new();
    }
    if host && (model.dirty || model.host_dirty) {
        model.overlay = Overlay::ScopeSwitch {
            destination: Scope::Library,
            host_to: Some(index),
        };
        return Vec::new();
    }
    if host {
        model.host_index = index;
    } else {
        model.directory_index = index;
    }
    vec![Effect::DirectoryChanged {
        from: current,
        to: index,
    }]
}

fn toggle_selected(model: &mut Model) {
    let Some(row) = model.rows.get(model.selected).cloned() else {
        return;
    };
    match row.kind {
        RowKind::Source => {
            let Some(identity) = row_identity(&row).map(str::to_owned) else {
                return;
            };
            let eligible = model
                .rows
                .iter()
                .filter(|candidate| {
                    candidate.kind == RowKind::Skill
                        && row_identity(candidate) == Some(identity.as_str())
                        && candidate.valid
                        && candidate.check != Some(CheckState::User)
                        && !candidate.repository_candidate
                })
                .collect::<Vec<_>>();
            if eligible.is_empty() {
                return;
            }
            let all_enabled = eligible
                .iter()
                .all(|candidate| candidate.check == Some(CheckState::Checked));
            for candidate in &mut model.rows {
                if candidate.kind == RowKind::Skill
                    && row_identity(candidate) == Some(identity.as_str())
                    && candidate.check != Some(CheckState::User)
                    && candidate.valid
                    && !candidate.repository_candidate
                    && (all_enabled || candidate.available)
                {
                    candidate.check = Some(if all_enabled {
                        CheckState::Unchecked
                    } else {
                        CheckState::Checked
                    });
                    if model.workspace == Workspace::Target {
                        update_target_materialization_mode(candidate);
                        refresh_staged_state(candidate);
                    } else {
                        refresh_library_visibility(candidate);
                    }
                }
            }
            model.recompute_group(&identity);
            model.dirty = true;
        }
        RowKind::Skill if row.check == Some(CheckState::User) => {
            model.overlay = Overlay::Notice(
                "Edit in the User tab, or press `m` to link in this repository.".to_owned(),
            );
        }
        RowKind::Skill if row.repository_candidate => {
            model.overlay = Overlay::Notice(
                "Repository-owned Skills cannot be unchecked. Press `m` to stage repo mode for an untracked candidate."
                    .to_owned(),
            );
        }
        RowKind::Skill
            if (row.available && row.valid) || row.check == Some(CheckState::Checked) =>
        {
            let group = row_identity(&row).map(str::to_owned);
            if let Some(candidate) = model.rows.get_mut(model.selected) {
                candidate.check = Some(if candidate.check == Some(CheckState::Checked) {
                    CheckState::Unchecked
                } else {
                    CheckState::Checked
                });
                if model.workspace == Workspace::Target {
                    update_target_materialization_mode(candidate);
                    refresh_staged_state(candidate);
                } else {
                    refresh_library_visibility(candidate);
                }
            }
            if let Some(group) = group {
                model.recompute_group(&group);
            }
            model.dirty = true;
        }
        _ => {}
    }
}

fn update_target_materialization_mode(row: &mut Row) {
    if row.check == Some(CheckState::Checked) {
        row.mode = row
            .mode
            .or(row.initial_mode)
            .or(Some(MaterializationKind::Linked));
    } else {
        row.mode = None;
        if row.inherited_user {
            row.check = Some(CheckState::User);
        }
    }
}

fn refresh_staged_state(row: &mut Row) {
    row.action = if row.check != row.initial_check {
        if row.check == Some(CheckState::Checked) {
            match row.mode {
                Some(MaterializationKind::Copied) => "Enable copy".to_owned(),
                _ => "Enable link".to_owned(),
            }
        } else {
            "Disable".to_owned()
        }
    } else if row.check == Some(CheckState::Checked) && row.mode != row.initial_mode {
        match row.mode {
            Some(MaterializationKind::Copied) => "Convert to copy".to_owned(),
            _ => "Convert to link".to_owned(),
        }
    } else {
        row.initial_action.clone()
    };
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MaterializationKind;

    fn skill(name: &str, enabled: bool) -> Row {
        Row::skill(
            "source",
            name,
            "",
            enabled,
            true,
            MaterializationKind::Linked,
            "",
        )
    }

    #[test]
    fn addressed_visibility_matches_displayed_indices() {
        let mut location = Row::location("Tools");
        location.location_index = Some(0);
        let mut source = Row::source("source", CheckState::Unchecked);
        source.location_index = Some(0);
        let mut matching_skill = skill("Needle", false);
        matching_skill.location_index = Some(0);
        let mut pending_skill = skill("Pending child", false);
        pending_skill.action = "Install".to_owned();
        pending_skill.location_index = Some(1);
        let mut other_location = Row::location("Other");
        other_location.location_index = Some(1);
        let mut diagnostic = Row::diagnostic("Problem");
        diagnostic.location_index = Some(0);
        let mut model = Model::new(
            Workspace::Target,
            vec![
                location,
                source,
                matching_skill,
                other_location,
                pending_skill,
                diagnostic,
            ],
        );

        for (filter, collapsed) in [
            ("", false),
            ("needle", false),
            ("PENDING", false),
            ("pending actions", false),
            ("nothing", false),
            ("", true),
            ("needle", true),
        ] {
            model.filter = filter.to_owned();
            model.collapsed.clear();
            if collapsed {
                model.collapsed.insert("source".to_owned());
            }
            let displayed = model.visible_indices();
            for index in 0..=model.rows.len() {
                assert_eq!(
                    model.is_visible_index(index),
                    displayed.contains(&index),
                    "filter={filter:?}, collapsed={collapsed}, index={index}"
                );
            }
        }
    }

    #[test]
    fn addressed_row_selects_and_toggles_only_that_visible_skill() {
        let mut model = Model::new(
            Workspace::Target,
            vec![skill("first", false), skill("second", false)],
        );
        reduce(
            &mut model,
            Action::ActivateRow {
                index: 1,
                intent: RowAction::Toggle,
            },
        );
        assert_eq!(model.selected, 1);
        assert_eq!(model.rows[0].check, Some(CheckState::Unchecked));
        assert_eq!(model.rows[1].check, Some(CheckState::Checked));
    }

    #[test]
    fn addressed_selection_rejects_hidden_and_invalid_indices() {
        let mut model = Model::new(
            Workspace::Target,
            vec![
                Row::source("source", CheckState::Unchecked),
                skill("child", false),
            ],
        );
        model.collapsed.insert("source".to_owned());
        reduce(
            &mut model,
            Action::ActivateRow {
                index: 1,
                intent: RowAction::Toggle,
            },
        );
        assert_eq!(model.rows[1].check, Some(CheckState::Unchecked));
        reduce(
            &mut model,
            Action::ActivateRow {
                index: 500,
                intent: RowAction::Toggle,
            },
        );
        assert_eq!(model.selected, 0);
    }

    #[test]
    fn direct_scope_selection_keeps_exact_dirty_destination() {
        let mut model = Model::new(Workspace::Library, vec![]);
        model.dirty = true;
        assert!(reduce(&mut model, Action::SelectScope(Scope::Repo)).is_empty());
        assert!(matches!(
            model.overlay,
            Overlay::ScopeSwitch {
                destination: Scope::Repo,
                host_to: None
            }
        ));
    }

    #[test]
    fn direct_directory_selection_retains_staged_state_and_same_target_is_noop() {
        let mut model = Model::new(Workspace::Target, vec![]);
        model.scope = Scope::User;
        model.directory_scopes = vec![TargetTabScope::User, TargetTabScope::User];
        model.directory_index = 0;
        model.dirty = true;
        assert_eq!(
            reduce(&mut model, Action::SelectDirectory(1)),
            vec![Effect::DirectoryChanged { from: 0, to: 1 }]
        );
        assert!(model.dirty);
        assert!(reduce(&mut model, Action::SelectDirectory(1)).is_empty());
        assert!(reduce(&mut model, Action::SelectDirectory(20)).is_empty());
    }
    #[test]
    fn suggestion_selection_only_changes_a_valid_chooser_match() {
        let mut model = Model::new(Workspace::Target, vec![]);
        model.overlay = Overlay::DirectoryChooser {
            input: String::new(),
            selected: 0,
        };
        let count = chooser_matches("").len();
        assert!(count > 0);
        reduce(&mut model, Action::SelectSuggestion(count - 1));
        assert!(matches!(
            model.overlay,
            Overlay::DirectoryChooser {
                selected,
                ..
            } if selected == count - 1
        ));
        reduce(&mut model, Action::SelectSuggestion(count));
        assert!(matches!(
            model.overlay,
            Overlay::DirectoryChooser {
                selected,
                ..
            } if selected == count - 1
        ));
    }
    fn activate(model: &mut Model, index: usize, intent: RowAction) -> Vec<Effect> {
        reduce(model, Action::ActivateRow { index, intent })
    }

    #[test]
    fn addressed_mode_cycles_the_clicked_row_not_the_previous_selection() {
        let first = skill("first", true);
        let mut second = skill("second", true);
        second.mode = Some(MaterializationKind::Copied);
        let mut model = Model::new(Workspace::Target, vec![first, second]);

        activate(&mut model, 1, RowAction::SwitchMode);

        assert_eq!(model.selected, 1);
        assert_eq!(model.rows[0].mode, Some(MaterializationKind::Linked));
        assert_eq!(model.rows[1].mode, Some(MaterializationKind::Linked));
    }

    #[test]
    fn source_bulk_toggle_covers_hidden_children_and_preserves_protected_rows() {
        let mut inherited = Row::inherited_user("source", "inherited", "", true, "");
        inherited.group = Some("source".into());
        let mut invalid = skill("invalid", false);
        invalid.valid = false;
        let mut unavailable = skill("offline", false);
        unavailable.available = false;
        let mut owned = skill("owned", false);
        owned.repository_candidate = true;
        let mut other_source_skill = skill("outside", false);
        other_source_skill.group = Some("other-source".into());
        let mut model = Model::new(
            Workspace::Target,
            vec![
                Row::source("source", CheckState::Mixed),
                skill("checked", true),
                skill("disabled", false),
                unavailable,
                inherited,
                invalid,
                owned,
                Row::source("other-source", CheckState::Unchecked),
                other_source_skill,
            ],
        );
        assert_eq!(model.rows[7].check, Some(CheckState::Unchecked));
        assert_eq!(model.rows[8].check, Some(CheckState::Unchecked));
        model.collapsed.insert("source".into());
        model.filter = "checked".into();
        let preserved_mode = model.rows[1].mode;

        activate(&mut model, 0, RowAction::Toggle);
        assert_eq!(model.rows[7].check, Some(CheckState::Unchecked));
        assert_eq!(model.rows[8].check, Some(CheckState::Unchecked));

        assert_eq!(model.rows[1].check, Some(CheckState::Checked));
        assert_eq!(model.rows[1].mode, preserved_mode);
        assert_eq!(model.rows[2].check, Some(CheckState::Checked));
        assert_eq!(model.rows[2].mode, Some(MaterializationKind::Linked));
        assert_eq!(model.rows[3].check, Some(CheckState::Unchecked));
        assert_eq!(model.rows[3].mode, Some(MaterializationKind::Linked));
        assert_eq!(model.rows[4].check, Some(CheckState::User));
        assert_eq!(model.rows[4].mode, None);
        assert_eq!(model.rows[5].check, Some(CheckState::Unchecked));
        assert_eq!(model.rows[5].mode, Some(MaterializationKind::Linked));
        assert_eq!(model.rows[6].check, Some(CheckState::Unchecked));
        assert_eq!(model.rows[6].mode, Some(MaterializationKind::Linked));
    }

    #[test]
    fn addressed_protected_rows_keep_toggle_and_mode_constraints() {
        let inherited = Row::inherited_user("source", "inherited", "", true, "");
        let unavailable_inherited =
            Row::inherited_user("source", "inherited-offline", "", false, "");
        let mut owned = skill("owned", false);
        owned.check = Some(CheckState::Repository);
        owned.initial_check = Some(CheckState::Repository);
        owned.repository_candidate = true;
        let mut invalid = skill("invalid", false);
        invalid.valid = false;
        let mut unavailable = skill("unavailable", false);
        unavailable.available = false;
        let mut model = Model::new(
            Workspace::Target,
            vec![
                inherited,
                unavailable_inherited.clone(),
                owned.clone(),
                invalid.clone(),
                unavailable.clone(),
            ],
        );

        activate(&mut model, 0, RowAction::Toggle);
        assert_eq!(model.rows[0].check, Some(CheckState::User));
        assert!(matches!(model.overlay, Overlay::Notice(_)));
        model.overlay = Overlay::None;
        activate(&mut model, 0, RowAction::SwitchMode);
        assert_eq!(model.rows[0].check, Some(CheckState::Checked));
        assert_eq!(model.rows[0].mode, Some(MaterializationKind::Linked));
        activate(&mut model, 1, RowAction::SwitchMode);
        assert_eq!(model.rows[1], unavailable_inherited);
        activate(&mut model, 2, RowAction::Toggle);
        activate(&mut model, 2, RowAction::SwitchMode);
        assert_eq!(model.rows[2], owned);
        activate(&mut model, 3, RowAction::Toggle);
        activate(&mut model, 3, RowAction::SwitchMode);
        assert_eq!(model.rows[3], invalid);
        activate(&mut model, 4, RowAction::Toggle);
        activate(&mut model, 4, RowAction::SwitchMode);
        assert_eq!(model.rows[4], unavailable);
    }

    #[test]
    fn addressed_library_mode_cycles_only_the_clicked_acquisition() {
        let mut first = skill("first", false);
        first.acquisition_source = Some("first".into());
        let mut second = skill("second", false);
        second.acquisition_source = Some("second".into());
        let mut model = Model::new(Workspace::Library, vec![first, second]);

        activate(&mut model, 1, RowAction::SwitchMode);

        assert_eq!(model.rows[0].acquisition_mode, None);
        assert_eq!(
            model.rows[1].acquisition_mode,
            Some(LibraryAcquisitionMode::Move)
        );
        assert!(model.rows[1].acquisition_pending);
        assert!(model.dirty);
    }

    #[test]
    fn addressed_follower_rows_are_read_only() {
        let mut model = Model::new(Workspace::Library, vec![skill("replica", false)]);
        model.host_labels = vec!["Local".into(), "worker".into()];
        model.host_index = 1;

        activate(&mut model, 0, RowAction::Toggle);
        assert_eq!(model.rows[0].check, Some(CheckState::Unchecked));
        assert!(matches!(model.overlay, Overlay::Notice(_)));
        assert!(!model.dirty);
        model.overlay = Overlay::None;
        activate(&mut model, 0, RowAction::SwitchMode);
        assert_eq!(model.rows[0].acquisition_mode, None);
        assert!(matches!(model.overlay, Overlay::Notice(_)));
        assert!(!model.dirty);
    }

    #[test]
    fn addressed_disclosure_changes_stored_state_during_filter_expansion() {
        let mut model = Model::new(
            Workspace::Target,
            vec![
                Row::source("source", CheckState::Unchecked),
                skill("match", false),
                skill("other", false),
            ],
        );
        model.collapsed.insert("source".into());
        model.filter = "match".into();
        assert!(model.visible_indices().contains(&1));

        activate(&mut model, 0, RowAction::ToggleDisclosure);
        assert!(!model.collapsed.contains("source"));
        model.filter.clear();
        assert!(model.visible_indices().contains(&2));
        activate(&mut model, 0, RowAction::ToggleDisclosure);
        assert!(model.collapsed.contains("source"));
        model.filter = "match".into();
        assert!(model.visible_indices().contains(&1));
        model.filter.clear();
        assert!(!model.visible_indices().contains(&1));
    }

    #[test]
    fn dirty_host_switch_preserves_exact_destination_for_each_outcome() {
        for outcome in [
            Action::Confirm,
            Action::DeleteDirectory,
            Action::ReturnToEditing,
        ] {
            let mut model = Model::new(Workspace::Library, vec![]);
            model.host_labels = vec!["Local".into(), "one".into(), "two".into()];
            model.host_dirty = true;
            assert!(reduce(&mut model, Action::SelectHost(2)).is_empty());
            assert_eq!(
                model.overlay,
                Overlay::ScopeSwitch {
                    destination: Scope::Library,
                    host_to: Some(2),
                }
            );

            let effects = reduce(&mut model, outcome.clone());
            match outcome {
                Action::Confirm => assert_eq!(
                    effects,
                    [Effect::SwitchToScope {
                        destination: Scope::Library,
                        host_to: Some(2),
                        save: true,
                        discard: false,
                    }]
                ),
                Action::DeleteDirectory => assert_eq!(
                    effects,
                    [Effect::SwitchToScope {
                        destination: Scope::Library,
                        host_to: Some(2),
                        save: false,
                        discard: true,
                    }]
                ),
                Action::ReturnToEditing => {
                    assert!(effects.is_empty());
                    assert_eq!(model.overlay, Overlay::None);
                    assert_eq!(model.host_index, 0);
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn direct_selection_noops_for_active_invalid_and_selects_valid_scope() {
        let mut model = Model::new(Workspace::Target, vec![]);
        model.scope = Scope::User;
        model.directory_scopes = vec![TargetTabScope::User, TargetTabScope::Repository];
        assert!(reduce(&mut model, Action::SelectDirectory(0)).is_empty());
        assert!(reduce(&mut model, Action::SelectDirectory(1)).is_empty());
        assert!(reduce(&mut model, Action::SelectDirectory(99)).is_empty());
        assert!(reduce(&mut model, Action::SelectHost(0)).is_empty());
        assert!(reduce(&mut model, Action::SelectScope(Scope::User)).is_empty());
        assert_eq!(
            reduce(&mut model, Action::SelectScope(Scope::Repo)),
            [Effect::SwitchToScope {
                destination: Scope::Repo,
                host_to: None,
                save: false,
                discard: false,
            }]
        );
        assert_eq!(model.directory_index, 0);
        assert_eq!(model.scope, Scope::User);

        let mut library = Model::new(Workspace::Library, vec![]);
        library.host_labels = vec!["Local".into()];
        assert!(reduce(&mut library, Action::SelectHost(0)).is_empty());
        assert!(reduce(&mut library, Action::SelectHost(1)).is_empty());
        assert_eq!(library.host_index, 0);
    }

    #[test]
    fn same_scope_directory_switch_preserves_staged_edits() {
        let mut model = Model::new(Workspace::Target, vec![skill("staged", false)]);
        model.scope = Scope::User;
        model.directory_scopes = vec![
            TargetTabScope::User,
            TargetTabScope::User,
            TargetTabScope::Repository,
        ];
        model.dirty = true;
        activate(&mut model, 0, RowAction::Toggle);
        assert_eq!(model.rows[0].check, Some(CheckState::Checked));

        assert_eq!(
            reduce(&mut model, Action::SelectDirectory(1)),
            [Effect::DirectoryChanged { from: 0, to: 1 }]
        );
        assert!(model.dirty);
        assert_eq!(model.rows[0].check, Some(CheckState::Checked));
        assert!(reduce(&mut model, Action::SelectDirectory(2)).is_empty());
        assert_eq!(model.directory_index, 1);
    }
}
