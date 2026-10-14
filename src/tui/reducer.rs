//! Pure keyboard-driven state transitions; scope workflows execute returned effects.

use super::input::{chooser_matches, complete_path};
use super::library::{refresh_library_action, refresh_library_visibility};
use super::model::{
    Action, CheckState, Effect, Model, Overlay, Row, RowKind, Scope, TargetTabScope, Workspace,
    row_identity,
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
                let old = model.host_index;
                let count = model.host_labels.len().max(1);
                let next = if action == Action::NextDirectory {
                    (old + 1) % count
                } else {
                    (old + count - 1) % count
                };
                if old != next && (model.dirty || model.host_dirty) {
                    model.overlay = Overlay::ScopeSwitch {
                        destination: Scope::Library,
                        host_to: Some(next),
                    };
                    return Vec::new();
                }
                model.host_index = next;
                return vec![Effect::DirectoryChanged {
                    from: old,
                    to: next,
                }];
            }
            let old = model.directory_index;
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
            let current = scoped.iter().position(|index| *index == old).unwrap_or(0);
            let next = scoped[if action == Action::NextDirectory {
                (current + 1) % scoped.len()
            } else {
                (current + scoped.len() - 1) % scoped.len()
            }];
            model.directory_index = next;
            return vec![Effect::DirectoryChanged {
                from: old,
                to: next,
            }];
        }
        Action::NextScope | Action::PreviousScope => {
            let destination = model
                .scope
                .cycle(if action == Action::NextScope { 1 } else { -1 });
            if model.dirty || model.host_dirty {
                model.overlay = Overlay::ScopeSwitch {
                    destination,
                    host_to: None,
                };
            } else {
                return vec![Effect::SwitchToScope {
                    destination,
                    host_to: None,
                    save: false,
                    discard: false,
                }];
            }
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
        Action::Input(_)
        | Action::Backspace
        | Action::CompletePath
        | Action::Escape
        | Action::Undo
        | Action::ReturnToEditing
        | Action::Acknowledge => {}
    }
    Vec::new()
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
        RowKind::Skill if row.available || row.check == Some(CheckState::Checked) => {
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
