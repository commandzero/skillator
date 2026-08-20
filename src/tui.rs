//! Terminal reducer, rendering, and lifecycle.

use crate::acquisition::{LibraryAcquisition, LibraryAcquisitionMode};
use crate::app::{
    AppPaths, LibraryWorkflow, PreparedTargetSave, PreparedUserScopeSave, ReportStatus,
    TargetSession, TargetWorkflow, UserScopeSession, UserScopeWorkflow, WorkflowError,
};
use crate::config::{
    LibraryConfig, LibraryLocationConfig, RegisteredSkillConfig, RegisteredSourceConfig,
    RepositoryConfig, SkillDirectoryConfig,
};
use crate::domain::{
    Enablement, MaterializationKind, RepositoryRelativePath, SkillDirectoryKey, SkillKey,
    SkillPath, SourceKey,
};
use crate::library::{LibrarySnapshot, Registration, SkillValidity};
use crate::onboarding::{OnboardingEntryKind, OnboardingWorkflow, PreparedOnboarding};
use crate::reconcile::Authorization;
use crate::target::{MaterializationState, ObservedState, observe};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row as TableRow, Table};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const PURPLE: Color = Color::Indexed(99);
const BLUE: Color = Color::Indexed(33);
const BONE: Color = Color::Indexed(230);
const WARNING: Color = Color::Indexed(220);
const ERROR: Color = Color::Indexed(196);
const DIM_FOREGROUND: Color = Color::Indexed(244);
const SELECTED_BACKGROUND: Color = Color::Indexed(24);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Target,
    Library,
    Onboarding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TargetTabScope {
    User,
    Repository,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Checked,
    User,
    Unchecked,
    Mixed,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Diagnostic,
    Location,
    Source,
    Skill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    kind: RowKind,
    group: Option<String>,
    inventory_id: Option<String>,
    name: String,
    description: String,
    check: Option<CheckState>,
    mode: Option<MaterializationKind>,
    state: String,
    action: String,
    details: String,
    initial_state: String,
    initial_action: String,
    initial_check: Option<CheckState>,
    initial_mode: Option<MaterializationKind>,
    available: bool,
    valid: bool,
    registered: Option<bool>,
    location_index: Option<usize>,
    source_path: Option<String>,
    skill_path: Option<String>,
    key_collision: bool,
    acquisition_mode: Option<LibraryAcquisitionMode>,
    initial_acquisition_mode: Option<LibraryAcquisitionMode>,
    acquisition_pending: bool,
    acquisition_source: Option<std::path::PathBuf>,
    acquisition_source_root_git: bool,
}

struct SkillInventoryRow {
    group: String,
    inventory_id: Option<String>,
    path: String,
    name: String,
    description: String,
    check: CheckState,
    available: bool,
    valid: bool,
    mode: Option<MaterializationKind>,
    state: String,
    details: String,
    location_index: Option<usize>,
}

impl Row {
    pub fn source(name: impl Into<String>, check: CheckState) -> Self {
        Self {
            kind: RowKind::Source,
            group: None,
            inventory_id: None,
            name: name.into(),
            description: String::new(),
            check: Some(check),
            mode: None,
            state: String::new(),
            action: String::new(),
            details: String::new(),
            initial_state: String::new(),
            initial_action: String::new(),
            initial_check: Some(check),
            initial_mode: None,
            available: true,
            valid: true,
            registered: Some(true),
            location_index: None,
            source_path: None,
            skill_path: None,
            key_collision: false,
            acquisition_mode: None,
            initial_acquisition_mode: None,
            acquisition_pending: false,
            acquisition_source: None,
            acquisition_source_root_git: false,
        }
    }

    pub fn skill(
        group: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        enabled: bool,
        available: bool,
        mode: MaterializationKind,
        state: impl Into<String>,
    ) -> Self {
        Self {
            kind: RowKind::Skill,
            group: Some(group.into()),
            inventory_id: None,
            name: name.into(),
            description: description.into(),
            check: Some(if enabled {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            }),
            mode: Some(mode),
            state: state.into(),
            action: String::new(),
            details: String::new(),
            initial_state: String::new(),
            initial_action: String::new(),
            initial_check: Some(if enabled {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            }),
            initial_mode: Some(mode),
            available,
            valid: true,
            registered: None,
            location_index: None,
            source_path: None,
            skill_path: None,
            key_collision: false,
            acquisition_mode: None,
            initial_acquisition_mode: None,
            acquisition_pending: false,
            acquisition_source: None,
            acquisition_source_root_git: false,
        }
        .with_initial_state()
    }

    pub fn inherited_user(
        group: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        available: bool,
        state: impl Into<String>,
    ) -> Self {
        let mut row = Self::skill(
            group,
            name,
            description,
            false,
            available,
            MaterializationKind::Linked,
            state,
        );
        row.check = Some(CheckState::User);
        row.initial_check = Some(CheckState::User);
        row.mode = None;
        row.initial_mode = None;
        row
    }

    pub fn location(path: impl Into<String>) -> Self {
        Self {
            kind: RowKind::Location,
            group: None,
            inventory_id: None,
            name: path.into(),
            description: String::new(),
            check: None,
            mode: None,
            state: String::new(),
            action: String::new(),
            details: String::new(),
            initial_state: String::new(),
            initial_action: String::new(),
            initial_check: None,
            initial_mode: None,
            available: true,
            valid: true,
            registered: None,
            location_index: None,
            source_path: None,
            skill_path: None,
            key_collision: false,
            acquisition_mode: None,
            initial_acquisition_mode: None,
            acquisition_pending: false,
            acquisition_source: None,
            acquisition_source_root_git: false,
        }
    }

    pub fn diagnostic(message: impl Into<String>) -> Self {
        Self {
            kind: RowKind::Diagnostic,
            group: None,
            inventory_id: None,
            name: "Diagnostic".to_owned(),
            description: message.into(),
            check: None,
            mode: None,
            state: "Warning".to_owned(),
            action: String::new(),
            details: String::new(),
            initial_state: "Warning".to_owned(),
            initial_action: String::new(),
            initial_check: None,
            initial_mode: None,
            available: false,
            valid: true,
            registered: None,
            location_index: None,
            source_path: None,
            skill_path: None,
            key_collision: false,
            acquisition_mode: None,
            initial_acquisition_mode: None,
            acquisition_pending: false,
            acquisition_source: None,
            acquisition_source_root_git: false,
        }
    }

    pub fn is_skill(&self) -> bool {
        self.kind == RowKind::Skill
    }

    pub fn check(&self) -> Option<CheckState> {
        self.check
    }

    pub fn mode(&self) -> Option<MaterializationKind> {
        self.mode
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn state(&self) -> &str {
        &self.state
    }

    pub fn action(&self) -> &str {
        &self.action
    }

    fn source_inventory(
        name: String,
        check: CheckState,
        registered: bool,
        location_index: usize,
        source_path: String,
        available: bool,
        key_collision: bool,
    ) -> Self {
        let mut row = Self::source(name, check);
        row.registered = Some(registered);
        row.state = if registered {
            "Registered"
        } else {
            "Unregistered"
        }
        .to_owned();
        row.location_index = Some(location_index);
        row.source_path = Some(source_path);
        row.inventory_id = Some(source_inventory_id(
            location_index,
            row.source_path.as_deref().unwrap_or("."),
        ));
        row.available = available;
        row.key_collision = key_collision;
        row.initial_state = row.state.clone();
        row
    }

    fn skill_inventory(inventory: SkillInventoryRow) -> Self {
        let mut row = Self::skill(
            inventory.group,
            inventory.name,
            inventory.description,
            inventory.check == CheckState::Checked,
            inventory.available,
            inventory.mode.unwrap_or(MaterializationKind::Linked),
            inventory.state,
        );
        row.check = Some(inventory.check);
        row.mode = inventory.mode;
        row.skill_path = Some(inventory.path);
        row.location_index = inventory.location_index;
        row.inventory_id = inventory.inventory_id;
        row.valid = inventory.valid;
        row.details = inventory.details;
        row
    }

    fn with_initial_state(mut self) -> Self {
        self.initial_state = self.state.clone();
        self.initial_action = self.action.clone();
        self
    }
}

fn source_inventory_id(location_index: usize, source_path: &str) -> String {
    format!("{location_index}:{source_path}")
}

fn row_identity(row: &Row) -> Option<&str> {
    row.inventory_id.as_deref().or(match row.kind {
        RowKind::Source => Some(row.name.as_str()),
        RowKind::Skill => row.group.as_deref(),
        _ => None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
    Filter,
    ConfirmSave,
    ConfirmSaveWarning(String),
    GuardedConfirmation(String),
    DiscardWorkspace,
    DiscardTarget,
    SwitchScope { from: usize, to: usize },
    DirectoryEditor { edit: bool, input: String },
    LocationEditor { edit: bool, input: String },
    SourceKeyEditor(String),
    TargetPicker(String),
    ConfirmDelete,
    Busy,
    Notice(String),
    Result(String),
}

#[derive(Debug, Clone)]
pub struct Model {
    workspace: Workspace,
    rows: Vec<Row>,
    selected: usize,
    collapsed: BTreeSet<String>,
    filter: String,
    overlay: Overlay,
    dirty: bool,
    directory_index: usize,
    directory_count: usize,
    directory_labels: Vec<String>,
    directory_values: Vec<String>,
    directory_scopes: Vec<TargetTabScope>,
}

impl Model {
    pub fn new(workspace: Workspace, rows: Vec<Row>) -> Self {
        Self {
            workspace,
            rows,
            selected: 0,
            collapsed: BTreeSet::new(),
            filter: String::new(),
            overlay: Overlay::None,
            dirty: false,
            directory_index: 0,
            directory_count: 1,
            directory_labels: Vec::new(),
            directory_values: Vec::new(),
            directory_scopes: Vec::new(),
        }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    pub fn is_collapsed(&self, source: &str) -> bool {
        self.collapsed.contains(source)
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    fn visible_indices(&self) -> Vec<usize> {
        let needle = self.filter.to_ascii_lowercase();
        let filtering = !needle.is_empty();
        let pending_only = matches!(needle.as_str(), "pending" | "pending actions");
        let matching_groups: BTreeSet<_> = self
            .rows
            .iter()
            .filter(|row| {
                row.kind == RowKind::Skill && row_matches_filter(row, &needle, pending_only)
            })
            .filter_map(|row| row_identity(row).map(str::to_owned))
            .collect();
        let matching_locations: BTreeSet<_> = self
            .rows
            .iter()
            .filter(|row| match row.kind {
                RowKind::Source => {
                    row_matches_filter(row, &needle, pending_only)
                        || row_identity(row)
                            .is_some_and(|identity| matching_groups.contains(identity))
                }
                RowKind::Skill => row_matches_filter(row, &needle, pending_only),
                _ => false,
            })
            .filter_map(|row| row.location_index)
            .collect();
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| match row.kind {
                RowKind::Location if filtering => (row_matches_filter(row, &needle, pending_only)
                    || row
                        .location_index
                        .is_some_and(|location| matching_locations.contains(&location)))
                .then_some(index),
                RowKind::Source if filtering => row_identity(row)
                    .is_some_and(|identity| matching_groups.contains(identity))
                    .then_some(index)
                    .or_else(|| row_matches_filter(row, &needle, pending_only).then_some(index)),
                RowKind::Skill if filtering => {
                    row_matches_filter(row, &needle, pending_only).then_some(index)
                }
                RowKind::Diagnostic if filtering => {
                    row_matches_filter(row, &needle, pending_only).then_some(index)
                }
                RowKind::Skill => (!row_identity(row)
                    .is_some_and(|identity| self.collapsed.contains(identity)))
                .then_some(index),
                _ => Some(index),
            })
            .collect()
    }

    fn select_visible_offset(&mut self, delta: isize) {
        let visible = self.visible_indices();
        if visible.is_empty() {
            return;
        }
        let position = visible
            .iter()
            .position(|index| *index == self.selected)
            .unwrap_or(0);
        let next = position.saturating_add_signed(delta).min(visible.len() - 1);
        self.selected = visible[next];
    }

    fn select_group(&mut self, direction: isize) {
        let groups: Vec<_> = self
            .visible_indices()
            .into_iter()
            .filter(|index| self.rows[*index].kind == RowKind::Source)
            .collect();
        if groups.is_empty() {
            return;
        }
        self.selected = if direction > 0 {
            groups
                .iter()
                .copied()
                .find(|index| *index > self.selected)
                .unwrap_or(*groups.last().expect("groups is nonempty"))
        } else {
            groups
                .iter()
                .rev()
                .copied()
                .find(|index| *index < self.selected)
                .unwrap_or(groups[0])
        };
    }

    fn recompute_group(&mut self, group: &str) {
        let checks: Vec<_> = self
            .rows
            .iter()
            .filter(|row| row.kind == RowKind::Skill && row_identity(row) == Some(group))
            .filter(|row| row.valid)
            .filter_map(|row| row.check)
            .collect();
        let state = if checks.is_empty() {
            CheckState::Unchecked
        } else if checks.iter().all(|state| *state == CheckState::Checked) {
            CheckState::Checked
        } else if checks.iter().all(|state| *state == CheckState::User) {
            CheckState::User
        } else if checks.iter().all(|state| *state == CheckState::Unchecked) {
            CheckState::Unchecked
        } else {
            CheckState::Mixed
        };
        if let Some(source) = self
            .rows
            .iter_mut()
            .find(|row| row.kind == RowKind::Source && row_identity(row) == Some(group))
        {
            source.check = Some(state);
        }
    }
}

fn row_matches_filter(row: &Row, needle: &str, pending_only: bool) -> bool {
    if pending_only {
        !row.action.is_empty()
    } else {
        row.name.to_ascii_lowercase().contains(needle)
            || row.description.to_ascii_lowercase().contains(needle)
            || row.action.to_ascii_lowercase().contains(needle)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    MoveDown,
    MoveUp,
    NextGroup,
    PreviousGroup,
    Collapse,
    Expand,
    Toggle,
    SwitchMode,
    NextDirectory,
    PreviousDirectory,
    StartFilter,
    Input(char),
    Backspace,
    Escape,
    Save { fast: bool },
    Quit,
    ChangeTarget,
    AddDirectory,
    EditDirectory,
    DeleteDirectory,
    ToggleWorkspace,
    Help,
    RegisterSource,
    Confirm,
    ReturnToEditing,
    Acknowledge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    PrepareSave { fast: bool },
    Quit { status: u8 },
    ChangeTargetTo(String),
    ToggleWorkspace,
    ApplyDirectoryEdit { edit: bool, value: String },
    ApplyLocationEdit { edit: bool, value: String },
    ApplySourceKey(String),
    DeleteDirectory,
    RetrySave,
    DirectoryChanged { from: usize, to: usize },
    SaveScopeAndSwitch { from: usize, to: usize },
    DiscardScopeAndSwitch { from: usize, to: usize },
    CommitSave,
    CancelSave,
}

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
    if model.overlay != Overlay::None {
        match (&mut model.overlay, &action) {
            (Overlay::DirectoryEditor { input, .. }, Action::Input(character))
            | (Overlay::LocationEditor { input, .. }, Action::Input(character))
            | (Overlay::SourceKeyEditor(input), Action::Input(character))
            | (Overlay::TargetPicker(input), Action::Input(character)) => {
                input.push(*character);
                return Vec::new();
            }
            (Overlay::DirectoryEditor { input, .. }, Action::Backspace)
            | (Overlay::LocationEditor { input, .. }, Action::Backspace)
            | (Overlay::SourceKeyEditor(input), Action::Backspace)
            | (Overlay::TargetPicker(input), Action::Backspace) => {
                input.pop();
                return Vec::new();
            }
            _ => {}
        }
        match action {
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
            Action::Confirm if model.overlay == Overlay::DiscardWorkspace => {
                model.dirty = false;
                model.overlay = Overlay::None;
                return vec![Effect::ToggleWorkspace];
            }
            Action::Confirm if model.overlay == Overlay::DiscardTarget => {
                model.dirty = false;
                model.overlay = Overlay::TargetPicker(String::new());
            }
            Action::Confirm if matches!(model.overlay, Overlay::SwitchScope { .. }) => {
                let Overlay::SwitchScope { from, to } = model.overlay else {
                    unreachable!()
                };
                model.overlay = Overlay::None;
                return vec![Effect::SaveScopeAndSwitch { from, to }];
            }
            Action::DeleteDirectory if matches!(model.overlay, Overlay::SwitchScope { .. }) => {
                let Overlay::SwitchScope { from, to } = model.overlay else {
                    unreachable!()
                };
                model.overlay = Overlay::None;
                return vec![Effect::DiscardScopeAndSwitch { from, to }];
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
            Action::Confirm => match std::mem::replace(&mut model.overlay, Overlay::None) {
                Overlay::DirectoryEditor { edit, input } => {
                    return vec![Effect::ApplyDirectoryEdit { edit, value: input }];
                }
                Overlay::LocationEditor { edit, input } => {
                    return vec![Effect::ApplyLocationEdit { edit, value: input }];
                }
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
        Action::MoveDown => model.select_visible_offset(1),
        Action::MoveUp => model.select_visible_offset(-1),
        Action::NextGroup => model.select_group(1),
        Action::PreviousGroup => model.select_group(-1),
        Action::Collapse => {
            if let Some(row) = model.rows.get(model.selected)
                && row.kind == RowKind::Source
                && let Some(identity) = row_identity(row)
            {
                model.collapsed.insert(identity.to_owned());
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
            if let Some(row) = model.rows.get_mut(model.selected)
                && row.kind == RowKind::Skill
                && row.check == Some(CheckState::Checked)
                && row.available
            {
                if model.workspace == Workspace::Target {
                    row.mode = Some(match row.mode {
                        Some(MaterializationKind::Linked) => MaterializationKind::Copied,
                        _ => MaterializationKind::Linked,
                    });
                    refresh_staged_state(row);
                } else if row.acquisition_source.is_some() {
                    row.acquisition_mode = match (model.workspace, row.acquisition_mode) {
                        (Workspace::Onboarding, Some(LibraryAcquisitionMode::Move)) => {
                            Some(LibraryAcquisitionMode::Copy)
                        }
                        (Workspace::Onboarding, Some(LibraryAcquisitionMode::Copy)) => {
                            Some(LibraryAcquisitionMode::Link)
                        }
                        (Workspace::Onboarding, _) => Some(LibraryAcquisitionMode::Move),
                        (_, Some(LibraryAcquisitionMode::Move)) => {
                            Some(LibraryAcquisitionMode::Copy)
                        }
                        (_, Some(LibraryAcquisitionMode::Copy)) => {
                            Some(LibraryAcquisitionMode::Link)
                        }
                        (_, Some(LibraryAcquisitionMode::Link)) => None,
                        (_, None) => Some(LibraryAcquisitionMode::Move),
                    };
                    row.acquisition_pending = row.acquisition_mode != row.initial_acquisition_mode;
                    match model.workspace {
                        Workspace::Library => refresh_library_action(row),
                        Workspace::Onboarding => refresh_onboarding_action(row),
                        Workspace::Target => unreachable!(),
                    }
                }
                model.dirty = true;
            }
        }
        Action::NextDirectory => {
            let previous = model.directory_index;
            let next = (model.directory_index + 1) % model.directory_count.max(1);
            if model.dirty
                && model.directory_scopes.get(previous) != model.directory_scopes.get(next)
            {
                model.overlay = Overlay::SwitchScope {
                    from: previous,
                    to: next,
                };
                return Vec::new();
            }
            model.directory_index = next;
            return vec![Effect::DirectoryChanged {
                from: previous,
                to: model.directory_index,
            }];
        }
        Action::PreviousDirectory => {
            let previous = model.directory_index;
            let next = model
                .directory_index
                .checked_sub(1)
                .unwrap_or(model.directory_count.saturating_sub(1));
            if model.dirty
                && model.directory_scopes.get(previous) != model.directory_scopes.get(next)
            {
                model.overlay = Overlay::SwitchScope {
                    from: previous,
                    to: next,
                };
                return Vec::new();
            }
            model.directory_index = next;
            return vec![Effect::DirectoryChanged {
                from: previous,
                to: model.directory_index,
            }];
        }
        Action::StartFilter => model.overlay = Overlay::Filter,
        Action::Save { fast } => return vec![Effect::PrepareSave { fast }],
        Action::Quit => return vec![Effect::Quit { status: 0 }],
        Action::ChangeTarget => {
            model.overlay = if model.dirty {
                Overlay::DiscardTarget
            } else {
                Overlay::TargetPicker(String::new())
            };
        }
        Action::AddDirectory => {
            model.overlay = if matches!(model.workspace, Workspace::Library | Workspace::Onboarding)
            {
                Overlay::LocationEditor {
                    edit: false,
                    input: String::new(),
                }
            } else {
                Overlay::DirectoryEditor {
                    edit: false,
                    input: String::new(),
                }
            };
        }
        Action::EditDirectory => {
            if matches!(model.workspace, Workspace::Library | Workspace::Onboarding) {
                let input = model
                    .selected_row()
                    .filter(|row| row.kind == RowKind::Location)
                    .map(|row| row.name.clone());
                if let Some(input) = input {
                    model.overlay = Overlay::LocationEditor { edit: true, input };
                }
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
            if matches!(model.workspace, Workspace::Library | Workspace::Onboarding)
                && !model
                    .selected_row()
                    .is_some_and(|row| row.kind == RowKind::Location)
            {
                model.overlay =
                    Overlay::Notice("Select a Library Location divider to delete it.".to_owned());
            } else {
                model.overlay = Overlay::ConfirmDelete;
            }
        }
        Action::ToggleWorkspace => {
            if model.dirty {
                model.overlay = Overlay::DiscardWorkspace;
            } else {
                return vec![Effect::ToggleWorkspace];
            }
        }
        Action::Help => model.overlay = Overlay::Help,
        Action::RegisterSource => {
            if let Some(row) = model.rows.get(model.selected).cloned()
                && row.kind == RowKind::Source
                && model.workspace == Workspace::Library
            {
                if row.key_collision {
                    model.overlay = Overlay::SourceKeyEditor(row.name.clone());
                    return Vec::new();
                }
                let identity = row_identity(&row).map(str::to_owned);
                let registered = !row.registered.unwrap_or(false);
                let row = &mut model.rows[model.selected];
                row.registered = Some(registered);
                row.state = if row.registered == Some(true) {
                    "Registered"
                } else {
                    "Unregistered"
                }
                .to_owned();
                refresh_source_action(row);
                if let Some(identity) = identity {
                    for child in model.rows.iter_mut().filter(|candidate| {
                        candidate.kind == RowKind::Skill
                            && row_identity(candidate) == Some(identity.as_str())
                    }) {
                        child.registered = Some(registered);
                        refresh_library_action(child);
                    }
                }
                model.dirty = true;
            }
        }
        Action::Escape if !model.filter.is_empty() => model.filter.clear(),
        Action::Input(_)
        | Action::Backspace
        | Action::Escape
        | Action::Confirm
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
                    && (all_enabled || candidate.available)
                {
                    candidate.check = Some(if all_enabled {
                        CheckState::Unchecked
                    } else {
                        CheckState::Checked
                    });
                    if !all_enabled && candidate.mode.is_none() {
                        candidate.mode = Some(MaterializationKind::Linked);
                    }
                    match model.workspace {
                        Workspace::Target => refresh_staged_state(candidate),
                        Workspace::Library => refresh_library_action(candidate),
                        Workspace::Onboarding => refresh_onboarding_action(candidate),
                    }
                }
            }
            model.recompute_group(&identity);
            model.dirty = true;
        }
        RowKind::Skill if row.check == Some(CheckState::User) => {
            model.overlay = Overlay::Notice(
                "This Skill is enabled in User Scope. Manage it from the User tab.".to_owned(),
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
                if candidate.mode.is_none() {
                    candidate.mode = Some(MaterializationKind::Linked);
                }
                match model.workspace {
                    Workspace::Target => refresh_staged_state(candidate),
                    Workspace::Library => refresh_library_action(candidate),
                    Workspace::Onboarding => refresh_onboarding_action(candidate),
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

fn refresh_library_action(row: &mut Row) {
    row.action = if row.check == Some(CheckState::Checked)
        && let Some(mode) = row.acquisition_mode
        && (row.initial_check != Some(CheckState::Checked) || row.acquisition_pending)
    {
        match mode {
            LibraryAcquisitionMode::Move => "Move to Library".to_owned(),
            LibraryAcquisitionMode::Copy => "Copy to Library".to_owned(),
            LibraryAcquisitionMode::Link => "Link to Library".to_owned(),
        }
    } else if row.check != row.initial_check {
        if row.check == Some(CheckState::Checked) {
            if row.registered == Some(true) {
                "Register".to_owned()
            } else {
                String::new()
            }
        } else {
            "Unregister".to_owned()
        }
    } else {
        row.initial_action.clone()
    };
}

fn refresh_source_action(row: &mut Row) {
    row.action = match (row.initial_state.as_str(), row.registered) {
        ("Registered", Some(false)) => "Unregister Source".to_owned(),
        ("Unregistered" | "Key Collision", Some(true)) => "Register Source".to_owned(),
        _ => String::new(),
    };
}

fn refresh_onboarding_action(row: &mut Row) {
    row.action = if row.check != Some(CheckState::Checked) {
        String::new()
    } else if let Some(mode) = row.acquisition_mode {
        match mode {
            LibraryAcquisitionMode::Move => "Move to Library".to_owned(),
            LibraryAcquisitionMode::Copy => "Copy to Library".to_owned(),
            LibraryAcquisitionMode::Link => "Link to Library".to_owned(),
        }
    } else if row.state == "Register; preserve link" {
        "Register Source".to_owned()
    } else {
        String::new()
    };
}

pub fn action_for_key(key: KeyEvent) -> Option<Action> {
    if key.modifiers == KeyModifiers::NONE {
        match key.code {
            KeyCode::Left => return Some(Action::Collapse),
            KeyCode::Down => return Some(Action::MoveDown),
            KeyCode::Up => return Some(Action::MoveUp),
            KeyCode::Right => return Some(Action::Expand),
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
        (KeyCode::Char('l'), true) => Some(Action::ToggleWorkspace),
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
        (KeyCode::Char('q'), false) => Some(Action::Quit),
        (KeyCode::Char('t'), false) => Some(Action::ChangeTarget),
        (KeyCode::Char('a'), false) => Some(Action::AddDirectory),
        (KeyCode::Char('e'), false) => Some(Action::EditDirectory),
        (KeyCode::Char('d'), false) => Some(Action::DeleteDirectory),
        (KeyCode::Char('?'), false) => Some(Action::Help),
        (KeyCode::Char('r'), false) => Some(Action::RegisterSource),
        (KeyCode::Enter, false) | (KeyCode::Char('y'), false) => Some(Action::Confirm),
        (KeyCode::Char('n'), false) => Some(Action::ReturnToEditing),
        (KeyCode::Backspace, false) => Some(Action::Backspace),
        (KeyCode::Char(character), false) => Some(Action::Input(character)),
        _ => None,
    }
}

fn dim_style() -> Style {
    Style::default().fg(DIM_FOREGROUND)
}

fn dim_span(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), dim_style())
}

fn row_cells(
    row: &Row,
    check: &str,
    mode: &str,
    collapsed: &BTreeSet<String>,
    filter: &str,
) -> Vec<Cell<'static>> {
    if row.kind == RowKind::Location {
        return vec![
            Cell::from(Span::styled("─────────", dim_style())),
            Cell::from(Span::styled("──────", dim_style())),
            Cell::from(Line::from(vec![
                dim_span("── "),
                Span::raw(row.name.clone()),
                dim_span(" "),
            ])),
            Cell::from(Span::styled(
                "────────────────────────────────────────────────────────────────",
                dim_style(),
            )),
            Cell::from(Span::styled(
                "────────────────────────────────",
                dim_style(),
            )),
        ];
    }

    let check = if row.check == Some(CheckState::Unchecked) {
        Cell::from(Span::styled(check.to_owned(), dim_style()))
    } else {
        Cell::from(check.to_owned())
    };
    let name = match row.kind {
        RowKind::Source => {
            let glyph = if row_identity(row).is_some_and(|identity| collapsed.contains(identity))
                && filter.is_empty()
            {
                "▸"
            } else {
                "▾"
            };
            Cell::from(Line::from(vec![
                dim_span(format!("{glyph} ")),
                Span::raw(row.name.clone()),
            ]))
        }
        RowKind::Skill => Cell::from(Line::from(vec![
            dim_span("  └─ "),
            Span::raw(row.name.clone()),
        ])),
        RowKind::Diagnostic => Cell::from(format!("! {}", row.name)),
        RowKind::Location => unreachable!(),
    };
    vec![
        check,
        Cell::from(mode.to_owned()),
        name,
        Cell::from(row.description.clone()),
        Cell::from(row.action.clone()),
    ]
}

fn row_style(row: &Row, selected: bool) -> Style {
    let mut style = if row_is_error(row) {
        Style::default().fg(ERROR)
    } else if row_is_warning(row) {
        Style::default().fg(WARNING)
    } else if !row.available {
        dim_style()
    } else {
        Style::default()
    };
    if selected {
        style = style.bg(SELECTED_BACKGROUND);
    }
    style
}

fn row_is_error(row: &Row) -> bool {
    if !row.valid || row.check == Some(CheckState::Invalid) || row.key_collision {
        return true;
    }
    matches!(
        row.state.as_str(),
        "Error" | "Invalid" | "Blocked" | "Failed" | "Recovery Required"
    )
}

fn row_is_warning(row: &Row) -> bool {
    row.kind == RowKind::Diagnostic || !row.action.is_empty()
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    let value = value.to_ascii_lowercase();
    needles.iter().any(|needle| value.contains(needle))
}

fn footer_help(workspace: Workspace) -> Line<'static> {
    let entries: &[(&str, &str)] = match workspace {
        Workspace::Target => &[
            ("s", "save & exit"),
            ("Ctrl+S", "quick save"),
            ("Space", "toggle"),
            ("m", "link/copy"),
            ("?", "help"),
        ],
        Workspace::Library => &[
            ("s", "save & exit"),
            ("Ctrl+S", "quick save"),
            ("Space", "toggle"),
            ("m", "move/copy/link"),
            ("?", "help"),
        ],
        Workspace::Onboarding => &[
            ("s", "initialize"),
            ("e", "edit location"),
            ("Space", "toggle"),
            ("m", "move/copy/link"),
            ("?", "help"),
        ],
    };
    let mut spans = Vec::new();
    for (index, (key, description)) in entries.iter().enumerate() {
        if index > 0 {
            spans.push(dim_span(" · "));
        }
        spans.push(Span::styled(
            (*key).to_owned(),
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(format!(" {description}")));
    }
    Line::from(spans)
}

pub fn render(frame: &mut Frame<'_>, model: &Model) {
    let areas = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(5),
        Constraint::Length(3),
    ])
    .split(frame.area());
    let title = match model.workspace {
        Workspace::Target => "skillator — Target",
        Workspace::Library => "skillator — Library",
        Workspace::Onboarding => "skillator — First-time setup",
    };
    let directory_strip = if model.workspace == Workspace::Target {
        model
            .directory_labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                if index == model.directory_index {
                    format!(" [{label}] ")
                } else {
                    format!(" {label} ")
                }
            })
            .collect::<String>()
    } else {
        String::new()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                title,
                Style::default().fg(BONE).add_modifier(Modifier::BOLD),
            ),
            Span::raw(if model.dirty { "  [staged]" } else { "" }),
            Span::styled(directory_strip, Style::default().fg(BONE)),
        ])),
        areas[0],
    );
    let header = match model.workspace {
        Workspace::Target => TableRow::new(["", "Mode", "Skill", "Description", "Action"]),
        Workspace::Library => TableRow::new(["", "Mode", "Location", "Description", "Action"]),
        Workspace::Onboarding => TableRow::new(["", "Mode", "Name", "Description", "Action"]),
    }
    .style(Style::default().fg(BONE).add_modifier(Modifier::BOLD));
    let visible = model.visible_indices();
    let rows = visible.iter().map(|index| {
        let row = &model.rows[*index];
        let selected = *index == model.selected;
        let check = match row.check {
            Some(CheckState::Checked) => "[x]",
            Some(CheckState::User) => "[u]",
            Some(CheckState::Unchecked) => "[ ]",
            Some(CheckState::Mixed) => "[-]",
            Some(CheckState::Invalid) => "[!]",
            None => "",
        };
        let mode = if row.check == Some(CheckState::User) {
            "user"
        } else if let Some(mode) = row.acquisition_mode {
            mode.label()
        } else {
            match row.mode {
                Some(MaterializationKind::Linked) => "link",
                Some(MaterializationKind::Copied) => "copy",
                None => "",
            }
        };
        TableRow::new(row_cells(row, check, mode, &model.collapsed, &model.filter))
            .style(row_style(row, selected))
    });
    let widths = [
        Constraint::Length(9),
        Constraint::Length(6),
        Constraint::Percentage(26),
        Constraint::Percentage(43),
        Constraint::Percentage(20),
    ];
    let key_help = footer_help(model.workspace).right_aligned();
    frame.render_widget(
        Table::new(rows, widths).header(header).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(PURPLE))
                .title_bottom(key_help),
        ),
        areas[1],
    );
    let inspector = model
        .selected_row()
        .map(|row| {
            let details = if row.details.is_empty() {
                &row.description
            } else {
                &row.details
            };
            Line::from(vec![
                Span::raw(row.name.clone()),
                dim_span(" — "),
                Span::raw(details.clone()),
            ])
        })
        .unwrap_or_default();
    frame.render_widget(Paragraph::new(inspector), areas[2]);
    render_overlay(frame, model);
}

fn render_overlay(frame: &mut Frame<'_>, model: &Model) {
    let (title, body, footer, confirmation) = match &model.overlay {
        Overlay::None => return,
        Overlay::Help => (
            "Help".to_owned(),
            match model.workspace {
                Workspace::Target => {
                    "j/k or Up/Down rows   J/K or Shift+Up/Down sources\nh/l or Left/Right collapse/expand   Space toggle   m link/copy\n/ filter (including /pending)   s save & exit   Ctrl+S quick save\nq quit   t target   a/e/d directory   Ctrl+L library"
                }
                Workspace::Library | Workspace::Onboarding => {
                    "j/k or Up/Down rows   J/K or Shift+Up/Down sources\nh/l or Left/Right collapse/expand   Space toggle   m move/copy/link\nr register Source   / filter (including /pending)\ns save & exit   Ctrl+S quick save   q quit   Ctrl+L target"
                }
            }
            .to_owned(),
            None,
            false,
        ),
        Overlay::Filter => return render_filter(frame, &model.filter),
        Overlay::ConfirmSave => (
            "Save and Exit".to_owned(),
            "Save the desired state?".to_owned(),
            Some("y/Enter save · n/Esc return".to_owned()),
            true,
        ),
        Overlay::ConfirmSaveWarning(message) => save_warning_modal(message),
        Overlay::GuardedConfirmation(message) => {
            let (body, footer) = split_confirmation_message(message);
            (
                "Review Guarded Changes".to_owned(),
                remove_first_line(&body),
                Some(footer),
                true,
            )
        }
        Overlay::DiscardWorkspace => (
            "Discard Workspace Changes".to_owned(),
            "Discard staged edits before switching workspaces?".to_owned(),
            Some("y/Enter discard · n/Esc return".to_owned()),
            true,
        ),
        Overlay::DiscardTarget => (
            "Discard Target Changes".to_owned(),
            "Discard staged edits before changing Target?".to_owned(),
            Some("y/Enter discard · n/Esc return".to_owned()),
            true,
        ),
        Overlay::SwitchScope { .. } => (
            "Switch Scope".to_owned(),
            "This scope has staged edits.".to_owned(),
            Some(
                "y/Enter save and switch · d discard and switch · n/Esc return".to_owned(),
            ),
            true,
        ),
        Overlay::DirectoryEditor { edit, input } => {
            let mode = if *edit { "Edit" } else { "Add" };
            return render_input(
                frame,
                &format!("{mode} Skill Directory"),
                "agents | claude | key,path,label",
                input,
            );
        }
        Overlay::LocationEditor { edit, input } => {
            let mode = if *edit { "Edit" } else { "Add" };
            return render_input(
                frame,
                &format!("{mode} Library Location"),
                "path",
                input,
            );
        }
        Overlay::SourceKeyEditor(input) => {
            return render_input(
                frame,
                "Resolve Source Key Collision",
                "owner/repository",
                input,
            );
        }
        Overlay::TargetPicker(input) => {
            return render_input(frame, "Change Target", "directory", input);
        }
        Overlay::ConfirmDelete => (
            "Delete Selection".to_owned(),
            "Delete the selected directory or location?".to_owned(),
            Some("y/Enter delete · n/Esc return".to_owned()),
            true,
        ),
        Overlay::Busy => (
            "Target Busy".to_owned(),
            "Another process is changing this Target.".to_owned(),
            Some("Enter retry · Esc return to editing".to_owned()),
            true,
        ),
        Overlay::Notice(message) => (
            "Notice".to_owned(),
            message.to_owned(),
            Some("Enter close · Esc close".to_owned()),
            false,
        ),
        Overlay::Result(message) => (
            "Save Result".to_owned(),
            message.to_owned(),
            Some("Enter exit".to_owned()),
            false,
        ),
    };
    let text = if confirmation {
        confirmation_text(&body)
    } else {
        Text::styled(body, overlay_text_style(&model.overlay))
    };
    let area = centered(frame.area(), 70, 30);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(modal_block(&title, footer.as_deref())),
        area,
    );
}

fn save_warning_modal(message: &str) -> (String, String, Option<String>, bool) {
    let (body, footer) = split_confirmation_message(message);
    if message.starts_with("Initialize Skillator") {
        (
            "Initialize Skillator".to_owned(),
            remove_first_line(&body),
            Some(footer),
            true,
        )
    } else if message.starts_with("Save Library changes?") {
        (
            "Save Library Changes".to_owned(),
            remove_question_from_first_line(&body),
            Some(footer),
            true,
        )
    } else {
        ("Confirm Save".to_owned(), body, Some(footer), true)
    }
}

fn split_confirmation_message(message: &str) -> (String, String) {
    let mut lines = message.lines().collect::<Vec<_>>();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let footer = if lines
        .last()
        .is_some_and(|line| line.contains("Enter") || line.contains("Esc"))
    {
        lines.pop().unwrap_or_default().to_owned()
    } else {
        String::new()
    };
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    (lines.join("\n"), footer)
}

fn remove_first_line(text: &str) -> String {
    text.split_once('\n')
        .map_or_else(String::new, |(_, remainder)| remainder.to_owned())
}

fn remove_question_from_first_line(text: &str) -> String {
    let (first, remainder) = text.split_once('\n').unwrap_or((text, ""));
    let detail = first
        .split_once('?')
        .map_or("", |(_, detail)| detail.trim());
    [detail, remainder]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn confirmation_text(text: &str) -> Text<'static> {
    let lines = text
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Line::default();
            }
            if line.starts_with("• Blocked")
                || contains_any(
                    line,
                    &[
                        "error",
                        "invalid",
                        "failed",
                        "recovery required",
                        "collision",
                    ],
                )
            {
                return Line::styled(line.to_owned(), Style::default().fg(ERROR));
            }
            if line.starts_with("• Guarded") || contains_any(line, &["warning", "conflict"]) {
                return Line::styled(line.to_owned(), Style::default().fg(WARNING));
            }
            Line::raw(line.to_owned())
        })
        .collect::<Vec<_>>();
    Text::from(lines)
}

fn confirmation_prompt_line(line: &str) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, segment) in line.split(" · ").enumerate() {
        if index > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(DIM_FOREGROUND)));
        }
        let (key, description) = segment.split_once(' ').unwrap_or((segment, ""));
        spans.push(Span::styled(
            key.to_owned(),
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        ));
        if !description.is_empty() {
            spans.push(Span::styled(
                format!(" {description}"),
                Style::default().fg(DIM_FOREGROUND),
            ));
        }
    }
    Line::from(spans)
}

fn overlay_text_style(overlay: &Overlay) -> Style {
    match overlay {
        Overlay::Help => Style::default().fg(BONE),
        Overlay::Notice(message) | Overlay::Result(message)
            if contains_any(
                message,
                &[
                    "error",
                    "invalid",
                    "blocked",
                    "failed",
                    "recovery required",
                    "collision",
                ],
            ) =>
        {
            Style::default().fg(ERROR)
        }
        Overlay::Notice(_) => Style::default().fg(WARNING),
        _ => Style::default(),
    }
}

fn modal_block(title: &str, footer: Option<&str>) -> Block<'static> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BLUE))
        .title_style(Style::default().fg(BONE))
        .title(Line::styled(
            format!(" {title} "),
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        ));
    match footer {
        Some(footer) => block.title_bottom(confirmation_prompt_line(footer).right_aligned()),
        None => block,
    }
}

fn render_input(frame: &mut Frame<'_>, title: &str, hint: &str, input: &str) {
    let area = centered(frame.area(), 70, 30);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(format!("{hint}\n> {input}"))
            .style(Style::default().fg(BONE))
            .block(modal_block(title, Some("Enter apply · Esc cancel"))),
        area,
    );
}

fn render_filter(frame: &mut Frame<'_>, filter: &str) {
    let area = Rect::new(
        2,
        frame.area().height.saturating_sub(3),
        frame.area().width.saturating_sub(4),
        3,
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(format!("/{filter}"))
            .style(Style::default().fg(BONE))
            .block(modal_block("Filter", Some("Enter apply · Esc clear"))),
        area,
    );
}

fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

pub fn run_library(paths: &AppPaths) -> Result<u8, WorkflowError> {
    navigate(
        paths,
        Navigation::Library {
            return_target: None,
        },
    )
}

pub fn run_target(paths: &AppPaths, directory: &Path) -> Result<u8, WorkflowError> {
    navigate(paths, Navigation::Target(directory.to_owned()))
}

enum Navigation {
    Exit(u8),
    Target(std::path::PathBuf),
    Library {
        return_target: Option<std::path::PathBuf>,
    },
    Onboarding {
        return_target: Option<std::path::PathBuf>,
    },
}

#[derive(Clone)]
struct TargetTab {
    scope: TargetTabScope,
    directory: SkillDirectoryConfig,
    rows: Vec<Row>,
}

struct LoadedTargetState {
    user: UserScopeSession,
    repository: TargetSession,
    tabs: Vec<TargetTab>,
}

enum PreparedScopeSave {
    User(PreparedUserScopeSave),
    Repository(PreparedTargetSave),
}

impl PreparedScopeSave {
    fn plan(&self) -> &crate::reconcile::Plan {
        match self {
            Self::User(prepared) => prepared.plan(),
            Self::Repository(prepared) => prepared.plan(),
        }
    }

    fn commit(
        self,
        paths: &AppPaths,
        authorization: Authorization,
    ) -> Result<crate::app::CommandReport, WorkflowError> {
        match self {
            Self::User(prepared) => UserScopeWorkflow::commit_save(paths, prepared, authorization),
            Self::Repository(prepared) => TargetWorkflow::commit_save(prepared, authorization),
        }
    }
}

fn navigate(paths: &AppPaths, mut navigation: Navigation) -> Result<u8, WorkflowError> {
    loop {
        navigation = match navigation {
            Navigation::Exit(status) => return Ok(status),
            Navigation::Target(directory) => run_target_once(paths, &directory)?,
            Navigation::Library { return_target } => {
                run_library_once(paths, return_target.as_deref())?
            }
            Navigation::Onboarding { return_target } => {
                run_onboarding_once(paths, return_target.as_deref())?
            }
        };
    }
}

fn run_onboarding_once(
    paths: &AppPaths,
    return_target: Option<&Path>,
) -> Result<Navigation, WorkflowError> {
    let session = OnboardingWorkflow::load(paths).map_err(WorkflowError::from)?;
    let mut location_expression = session.default_location_expression().to_owned();
    let model = initial_onboarding_model(paths, &session);
    let mut pending: Option<PreparedOnboarding> = None;
    let status = run_interactive(model, |model, effect| match effect {
        Effect::Quit { status } => Ok(Some(status)),
        Effect::ApplyLocationEdit { value, .. } => {
            let value = value.trim();
            if value.is_empty() {
                model.overlay = Overlay::Notice("Library Location cannot be empty.".to_owned());
                return Ok(None);
            }
            location_expression = value.to_owned();
            if let Some(location) = model
                .rows
                .iter_mut()
                .find(|row| row.kind == RowKind::Location)
            {
                location.name = location_expression.clone();
                location.description = format!(
                    "First Library Location (default resolves to {})",
                    paths.home().join(".skillator/library").display()
                );
            }
            Ok(None)
        }
        Effect::PrepareSave { .. } => {
            let selected = model
                .rows
                .iter()
                .filter(|row| row.kind == RowKind::Skill && row.check == Some(CheckState::Checked))
                .filter_map(|row| {
                    Some((
                        row.skill_path.clone()?,
                        row.acquisition_mode.unwrap_or(LibraryAcquisitionMode::Link),
                    ))
                })
                .collect::<BTreeMap<_, _>>();
            match OnboardingWorkflow::prepare_with_modes(
                paths,
                &session,
                &location_expression,
                &selected,
            ) {
                Ok(prepared) => {
                    let mut message = String::from("Initialize Skillator with these changes?\n");
                    for item in prepared.review() {
                        if let Some(source) = &item.source {
                            message.push_str(&format!(
                                "• {}: {} → {}\n",
                                item.action,
                                source.display(),
                                item.destination.display()
                            ));
                        } else {
                            message.push_str(&format!(
                                "• {}: {}\n",
                                item.action,
                                item.destination.display()
                            ));
                        }
                    }
                    message.push_str("\ny/Enter initialize · n/Esc return");
                    pending = Some(prepared);
                    model.overlay = Overlay::ConfirmSaveWarning(message);
                }
                Err(error) => model.overlay = Overlay::Notice(error.to_string()),
            }
            Ok(None)
        }
        Effect::CommitSave => {
            let Some(prepared) = pending.take() else {
                return Ok(None);
            };
            OnboardingWorkflow::commit(prepared).map_err(WorkflowError::from)?;
            Ok(Some(252))
        }
        Effect::CancelSave => {
            pending.take();
            Ok(None)
        }
        Effect::ToggleWorkspace => {
            model.overlay = Overlay::Notice(
                "Complete or cancel first-time setup before changing workspaces.".to_owned(),
            );
            Ok(None)
        }
        Effect::DeleteDirectory => {
            model.overlay = Overlay::Notice(
                "Edit the first Library Location with `e`; it is required for setup.".to_owned(),
            );
            Ok(None)
        }
        _ => Ok(None),
    })?;
    if status == 252 {
        Ok(return_target.map_or(Navigation::Exit(0), |target| {
            Navigation::Target(target.to_owned())
        }))
    } else {
        Ok(Navigation::Exit(status))
    }
}

fn initial_onboarding_model(
    paths: &AppPaths,
    session: &crate::onboarding::OnboardingSession,
) -> Model {
    let mut model = Model::new(Workspace::Onboarding, onboarding_rows(paths, session));
    model.dirty = true;
    model
}

fn onboarding_rows(paths: &AppPaths, session: &crate::onboarding::OnboardingSession) -> Vec<Row> {
    let mut location = Row::location(session.default_location_expression());
    location.location_index = Some(0);
    location.description = format!(
        "First Library Location (default resolves to {})",
        paths.home().join(".skillator/library").display()
    );
    let mut rows = vec![location];
    let checks = session
        .entries()
        .iter()
        .filter(|entry| entry.selectable())
        .map(|entry| entry.selected_by_default())
        .collect::<Vec<_>>();
    let rollup = if checks.is_empty() || checks.iter().all(|checked| !checked) {
        CheckState::Unchecked
    } else if checks.iter().all(|checked| *checked) {
        CheckState::Checked
    } else {
        CheckState::Mixed
    };
    let mut source = Row::source("Existing user-scoped Skills", rollup);
    source.location_index = Some(0);
    source.description = "Skills currently exposed from ~/.agents/skills".to_owned();
    rows.push(source);
    for entry in session.entries() {
        let state = match entry.kind() {
            OnboardingEntryKind::Physical => "Import and link",
            OnboardingEntryKind::Symlink => "Register; preserve link",
            OnboardingEntryKind::Invalid => "Leave untouched",
        };
        let mut row = Row::skill(
            "Existing user-scoped Skills",
            entry.name(),
            entry.detail(),
            entry.selected_by_default(),
            entry.selectable(),
            MaterializationKind::Linked,
            state,
        );
        match entry.kind() {
            OnboardingEntryKind::Physical => {
                row.mode = None;
                row.acquisition_mode = Some(LibraryAcquisitionMode::Move);
                row.initial_acquisition_mode = row.acquisition_mode;
                row.acquisition_source = Some(entry.path().to_owned());
            }
            OnboardingEntryKind::Symlink => {
                row.mode = Some(MaterializationKind::Linked);
            }
            OnboardingEntryKind::Invalid => row.mode = None,
        }
        row.skill_path = Some(entry.name().to_owned());
        row.location_index = Some(0);
        if !entry.selectable() {
            row.check = Some(CheckState::Invalid);
            row.initial_check = Some(CheckState::Invalid);
            row.valid = false;
        }
        refresh_onboarding_action(&mut row);
        row.initial_action = row.action.clone();
        rows.push(row);
    }
    rows
}

fn run_library_once(
    paths: &AppPaths,
    return_target: Option<&Path>,
) -> Result<Navigation, WorkflowError> {
    let session = match LibraryWorkflow::load(paths) {
        Ok(session) => session,
        Err(error @ WorkflowError::InvalidInput { .. }) => {
            return run_static(
                Model::new(Workspace::Library, vec![Row::diagnostic(error.to_string())]),
                3,
            )
            .map(Navigation::Exit);
        }
        Err(error) => return Err(error),
    };
    if session.first_run {
        return Ok(Navigation::Onboarding {
            return_target: return_target.map(Path::to_owned),
        });
    }
    let snapshot = LibraryWorkflow::snapshot(paths, &session.config);
    let mut working_config = session.config.clone();
    let mut model = Model::new(Workspace::Library, library_rows(&working_config, &snapshot));
    model.dirty = session.first_run;
    let mut staged: Option<(LibraryConfig, Vec<LibraryAcquisition>)> = None;
    let mut target_to_open = None;
    let status = run_interactive(model, |model, effect| match effect {
        Effect::Quit { status } => Ok(Some(status)),
        Effect::PrepareSave { fast } => {
            let config = library_config_from_rows(&working_config, &model.rows)?;
            let acquisitions = library_acquisitions_from_rows(&model.rows);
            if fast
                && acquisitions.is_empty()
                && library_fast_save_is_safe(&session.config, &config)
            {
                LibraryWorkflow::save_with_acquisitions(
                    paths,
                    &session,
                    &config,
                    &acquisitions,
                    true,
                )?;
                Ok(Some(0))
            } else {
                let affected = std::env::current_dir()
                    .ok()
                    .and_then(|directory| TargetWorkflow::load(directory).ok())
                    .filter(|target| !target.first_run)
                    .map(|target| {
                        LibraryWorkflow::affected_references(
                            &session.config,
                            &config,
                            &target.config,
                        )
                    })
                    .unwrap_or_default();
                staged = Some((config, acquisitions));
                model.overlay = if affected.is_empty()
                    && staged
                        .as_ref()
                        .is_none_or(|(_, acquisitions)| acquisitions.is_empty())
                {
                    Overlay::ConfirmSave
                } else {
                    let acquisition_count = staged
                        .as_ref()
                        .map_or(0, |(_, acquisitions)| acquisitions.len());
                    Overlay::ConfirmSaveWarning(format!(
                        "Save Library changes? {acquisition_count} acquisition(s); {} Enablement reference(s) in the current Target will become Unresolved.\ny/Enter proceed · n/Esc return",
                        affected.len()
                    ))
                };
                Ok(None)
            }
        }
        Effect::CommitSave => {
            let (config, acquisitions) = staged
                .take()
                .unwrap_or_else(|| (session.config.clone(), Vec::new()));
            LibraryWorkflow::save_with_acquisitions(paths, &session, &config, &acquisitions, true)?;
            Ok(Some(0))
        }
        Effect::CancelSave => Ok(None),
        Effect::ToggleWorkspace => {
            if return_target.is_some() {
                Ok(Some(251))
            } else {
                model.overlay = Overlay::TargetPicker(String::new());
                Ok(None)
            }
        }
        Effect::ChangeTargetTo(value) => {
            target_to_open = Some(std::path::PathBuf::from(value));
            Ok(Some(250))
        }
        Effect::ApplyLocationEdit { edit, value } => {
            let value = value.trim();
            if value.is_empty() {
                model.overlay = Overlay::Notice("Location path cannot be empty.".to_owned());
                return Ok(None);
            }
            working_config = library_config_from_rows(&working_config, &model.rows)?;
            let mut locations = working_config.locations().to_vec();
            if edit {
                let Some(index) = model.selected_row().and_then(|row| row.location_index) else {
                    model.overlay =
                        Overlay::Notice("Select a Location divider to edit.".to_owned());
                    return Ok(None);
                };
                let old = &locations[index];
                locations[index] = LibraryLocationConfig::new(
                    value.to_owned(),
                    old.exclusions().to_vec(),
                    old.allow_overlap(),
                    old.sources().to_vec(),
                );
            } else {
                locations.push(LibraryLocationConfig::new(
                    value.to_owned(),
                    Vec::new(),
                    false,
                    Vec::new(),
                ));
            }
            working_config = LibraryConfig::new(locations).map_err(config_issues)?;
            let snapshot = LibraryWorkflow::snapshot(paths, &working_config);
            model.rows = library_rows(&working_config, &snapshot);
            model.selected = model.selected.min(model.rows.len().saturating_sub(1));
            model.dirty = true;
            Ok(None)
        }
        Effect::ApplySourceKey(value) => {
            let key = match SourceKey::parse(value.trim()) {
                Ok(key) => key,
                Err(error) => {
                    model.overlay = Overlay::Notice(error.to_string());
                    return Ok(None);
                }
            };
            if model.rows.iter().enumerate().any(|(index, row)| {
                index != model.selected
                    && row.kind == RowKind::Source
                    && row.name.eq_ignore_ascii_case(key.as_str())
            }) {
                model.overlay = Overlay::Notice(format!(
                    "Source Key `{key}` is already present; choose a distinct key."
                ));
                return Ok(None);
            }
            let Some(identity) = model
                .rows
                .get(model.selected)
                .and_then(row_identity)
                .map(str::to_owned)
            else {
                return Ok(None);
            };
            {
                let source = &mut model.rows[model.selected];
                source.name = key.as_str().to_owned();
                source.registered = Some(true);
                source.key_collision = false;
                source.state = "Registered".to_owned();
                refresh_source_action(source);
            }
            for child in model.rows.iter_mut().filter(|candidate| {
                candidate.kind == RowKind::Skill
                    && row_identity(candidate) == Some(identity.as_str())
            }) {
                child.registered = Some(true);
                refresh_library_action(child);
            }
            model.dirty = true;
            Ok(None)
        }
        Effect::DeleteDirectory => {
            let Some(index) = model
                .selected_row()
                .filter(|row| row.kind == RowKind::Location)
                .and_then(|row| row.location_index)
            else {
                model.overlay = Overlay::Notice("Select a Location divider to delete.".to_owned());
                return Ok(None);
            };
            working_config = library_config_from_rows(&working_config, &model.rows)?;
            let mut locations = working_config.locations().to_vec();
            locations.remove(index);
            working_config = LibraryConfig::new(locations).map_err(config_issues)?;
            let snapshot = LibraryWorkflow::snapshot(paths, &working_config);
            model.rows = library_rows(&working_config, &snapshot);
            model.selected = model.selected.min(model.rows.len().saturating_sub(1));
            model.dirty = true;
            Ok(None)
        }
        _ => Ok(None),
    })?;
    match status {
        250 => Ok(Navigation::Target(
            target_to_open.unwrap_or_else(|| Path::new(".").to_owned()),
        )),
        251 => Ok(Navigation::Target(
            return_target.unwrap_or_else(|| Path::new(".")).to_owned(),
        )),
        status => Ok(Navigation::Exit(status)),
    }
}

fn run_target_once(paths: &AppPaths, directory: &Path) -> Result<Navigation, WorkflowError> {
    let library_session = match LibraryWorkflow::load(paths) {
        Ok(session) => session,
        Err(error @ WorkflowError::InvalidInput { .. }) => {
            return run_static(
                Model::new(Workspace::Target, vec![Row::diagnostic(error.to_string())]),
                3,
            )
            .map(Navigation::Exit);
        }
        Err(error) => return Err(error),
    };
    if library_session.first_run {
        return Ok(Navigation::Onboarding {
            return_target: Some(directory.to_owned()),
        });
    }
    let repository_session = match TargetWorkflow::load(directory) {
        Ok(session) => session,
        Err(error @ WorkflowError::InvalidInput { .. }) => {
            return run_static(
                Model::new(Workspace::Target, vec![Row::diagnostic(error.to_string())]),
                3,
            )
            .map(Navigation::Exit);
        }
        Err(error) => return Err(error),
    };
    let library = LibraryWorkflow::snapshot(paths, &library_session.config);
    let user_session = match UserScopeWorkflow::load(paths) {
        Ok(session) => session,
        Err(error @ WorkflowError::InvalidInput { .. }) => {
            return run_static(
                Model::new(Workspace::Target, vec![Row::diagnostic(error.to_string())]),
                3,
            )
            .map(Navigation::Exit);
        }
        Err(error) => return Err(error),
    };
    let mut state = build_target_state(user_session, repository_session, &library);
    let mut dirty_scopes = BTreeSet::new();
    let model = initial_target_model(&state.tabs);
    let mut pending: Option<PreparedScopeSave> = None;
    let mut switch_after_save: Option<(TargetTabScope, String)> = None;
    let mut target_to_open = None;
    let status = run_interactive(model, |model, effect| match effect {
        Effect::Quit { status } => Ok(Some(status)),
        Effect::DirectoryChanged { from, to } => {
            if let Some(tab) = state.tabs.get_mut(from) {
                tab.rows = model.rows.clone();
                if model.dirty {
                    dirty_scopes.insert(tab.scope);
                }
            }
            activate_target_tab(model, &state.tabs, &dirty_scopes, to);
            Ok(None)
        }
        Effect::PrepareSave { fast } => {
            store_active_target_tab(model, &mut state.tabs);
            let scope = active_target_scope(model, &state.tabs);
            let prepared = match prepare_scope_save(paths, &state, &state.tabs, scope) {
                Ok(prepared) => prepared,
                Err(WorkflowError::Busy) => {
                    model.overlay = Overlay::Busy;
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            if fast && plan_is_safe(prepared.plan()) {
                let report = commit_scope_save(paths, prepared)?;
                if report.status == ReportStatus::InSync {
                    Ok(Some(0))
                } else {
                    show_save_result(model, &report);
                    Ok(None)
                }
            } else {
                model.overlay = save_review_overlay(prepared.plan());
                pending = Some(prepared);
                Ok(None)
            }
        }
        Effect::RetrySave => {
            store_active_target_tab(model, &mut state.tabs);
            let scope = active_target_scope(model, &state.tabs);
            let prepared = match prepare_scope_save(paths, &state, &state.tabs, scope) {
                Ok(prepared) => prepared,
                Err(WorkflowError::Busy) => {
                    model.overlay = Overlay::Busy;
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            model.overlay = save_review_overlay(prepared.plan());
            pending = Some(prepared);
            Ok(None)
        }
        Effect::CommitSave => {
            let Some(prepared) = pending.take() else {
                return Ok(None);
            };
            let report = commit_scope_save(paths, prepared)?;
            if report.status != ReportStatus::InSync {
                show_save_result(model, &report);
                return Ok(None);
            }
            if let Some((scope, key)) = switch_after_save.take() {
                let user = UserScopeWorkflow::load(paths)?;
                let repository = TargetWorkflow::load(directory)?;
                state = build_target_state(user, repository, &library);
                dirty_scopes.clear();
                let target = state
                    .tabs
                    .iter()
                    .position(|tab| tab.scope == scope && tab.directory.key().as_str() == key)
                    .or_else(|| state.tabs.iter().position(|tab| tab.scope == scope))
                    .unwrap_or(0);
                sync_target_tab_model(model, &state.tabs, &dirty_scopes);
                activate_target_tab(model, &state.tabs, &dirty_scopes, target);
                Ok(None)
            } else {
                Ok(Some(0))
            }
        }
        Effect::CancelSave => {
            pending.take();
            switch_after_save.take();
            Ok(None)
        }
        Effect::SaveScopeAndSwitch { from, to } => {
            if let Some(tab) = state.tabs.get_mut(from) {
                tab.rows = model.rows.clone();
            }
            let source_scope = state.tabs[from].scope;
            let destination = &state.tabs[to];
            switch_after_save = Some((
                destination.scope,
                destination.directory.key().as_str().to_owned(),
            ));
            let prepared = match prepare_scope_save(paths, &state, &state.tabs, source_scope) {
                Ok(prepared) => prepared,
                Err(WorkflowError::Busy) => {
                    model.overlay = Overlay::Busy;
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            if plan_is_safe(prepared.plan()) {
                let report = commit_scope_save(paths, prepared)?;
                if report.status != ReportStatus::InSync {
                    show_save_result(model, &report);
                    switch_after_save.take();
                    return Ok(None);
                }
                let user = UserScopeWorkflow::load(paths)?;
                let repository = TargetWorkflow::load(directory)?;
                state = build_target_state(user, repository, &library);
                dirty_scopes.clear();
                let (scope, key) = switch_after_save.take().expect("switch destination exists");
                let target = state
                    .tabs
                    .iter()
                    .position(|tab| tab.scope == scope && tab.directory.key().as_str() == key)
                    .or_else(|| state.tabs.iter().position(|tab| tab.scope == scope))
                    .unwrap_or(0);
                sync_target_tab_model(model, &state.tabs, &dirty_scopes);
                activate_target_tab(model, &state.tabs, &dirty_scopes, target);
            } else {
                model.overlay = save_review_overlay(prepared.plan());
                pending = Some(prepared);
            }
            Ok(None)
        }
        Effect::DiscardScopeAndSwitch { from, to } => {
            let discarded_scope = state.tabs[from].scope;
            let user = UserScopeWorkflow::load(paths)?;
            let repository = TargetWorkflow::load(directory)?;
            let destination_scope = state.tabs[to].scope;
            let destination_key = state.tabs[to].directory.key().as_str().to_owned();
            state = build_target_state(user, repository, &library);
            dirty_scopes.remove(&discarded_scope);
            let target = state
                .tabs
                .iter()
                .position(|tab| {
                    tab.scope == destination_scope
                        && tab.directory.key().as_str() == destination_key
                })
                .or_else(|| {
                    state
                        .tabs
                        .iter()
                        .position(|tab| tab.scope == destination_scope)
                })
                .unwrap_or(0);
            sync_target_tab_model(model, &state.tabs, &dirty_scopes);
            activate_target_tab(model, &state.tabs, &dirty_scopes, target);
            Ok(None)
        }
        Effect::ApplyDirectoryEdit { edit, value } => {
            let candidate = match parse_directory_editor(&value) {
                Ok(candidate) => candidate,
                Err(message) => {
                    model.overlay = Overlay::Notice(message);
                    return Ok(None);
                }
            };
            if state.tabs.is_empty() {
                model.overlay = Overlay::Notice(
                    "There is no Skill Directory to edit; add one instead.".to_owned(),
                );
                return Ok(None);
            }
            if edit {
                let index = model.directory_index.min(state.tabs.len() - 1);
                if candidate.path() != state.tabs[index].directory.path()
                    && model.rows.iter().any(|row| {
                        row.kind == RowKind::Skill
                            && (row.check == Some(CheckState::Checked)
                                || row.initial_check == Some(CheckState::Checked))
                    })
                {
                    model.overlay = Overlay::Notice(
                        "Disable and save every Skill before changing this directory path."
                            .to_owned(),
                    );
                    return Ok(None);
                }
            }
            store_active_target_tab(model, &mut state.tabs);
            let scope = active_target_scope(model, &state.tabs);
            let mut proposed = state
                .tabs
                .iter()
                .filter(|tab| tab.scope == scope)
                .map(|tab| tab.directory.clone())
                .collect::<Vec<_>>();
            if edit {
                let current_key = state.tabs[model.directory_index].directory.key();
                let index = proposed
                    .iter()
                    .position(|directory| directory.key() == current_key)
                    .expect("active directory belongs to its scope");
                proposed[index] = candidate.clone();
            } else {
                proposed.push(candidate.clone());
            }
            if let Err(issues) = RepositoryConfig::new(proposed, Vec::new()) {
                model.overlay = Overlay::Notice(
                    issues
                        .into_iter()
                        .map(|issue| format!("{}: {}", issue.path, issue.message))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
                return Ok(None);
            }
            if edit {
                state.tabs[model.directory_index].directory = candidate;
            } else {
                let (config, observed, inherited) = match scope {
                    TargetTabScope::User => {
                        let observed = observe(&state.user.target, &state.user.config, &library);
                        (&state.user.config, observed, BTreeSet::new())
                    }
                    TargetTabScope::Repository => {
                        let observed =
                            observe(&state.repository.target, &state.repository.config, &library);
                        let inherited = user_enabled_skills(&state.user.config);
                        (&state.repository.config, observed, inherited)
                    }
                };
                state.tabs.push(TargetTab {
                    scope,
                    rows: rows_for_directory(&candidate, config, &library, &observed, &inherited),
                    directory: candidate,
                });
                model.directory_index = state.tabs.len() - 1;
            }
            dirty_scopes.insert(scope);
            sync_target_tab_model(model, &state.tabs, &dirty_scopes);
            activate_target_tab(model, &state.tabs, &dirty_scopes, model.directory_index);
            Ok(None)
        }
        Effect::DeleteDirectory => {
            store_active_target_tab(model, &mut state.tabs);
            let scope = active_target_scope(model, &state.tabs);
            if state.tabs.iter().filter(|tab| tab.scope == scope).count() > 1 {
                let index = model.directory_index.min(state.tabs.len() - 1);
                let rows = &state.tabs[index].rows;
                if rows.iter().any(|row| {
                    row.kind == RowKind::Skill
                        && (row.check == Some(CheckState::Checked)
                            || row.initial_check == Some(CheckState::Checked))
                }) {
                    model.overlay = Overlay::Notice(
                        "Disable and save every Skill in this directory before deleting it."
                            .to_owned(),
                    );
                    return Ok(None);
                }
                state.tabs.remove(index);
                dirty_scopes.insert(scope);
                let target = index.min(state.tabs.len() - 1);
                sync_target_tab_model(model, &state.tabs, &dirty_scopes);
                activate_target_tab(model, &state.tabs, &dirty_scopes, target);
            } else {
                model.overlay = Overlay::Notice(
                    "Each scope must keep at least one Skill Directory.".to_owned(),
                );
            }
            Ok(None)
        }
        Effect::ChangeTargetTo(value) => {
            target_to_open = Some(std::path::PathBuf::from(value));
            Ok(Some(250))
        }
        Effect::ToggleWorkspace => Ok(Some(251)),
        Effect::ApplyLocationEdit { .. } => Ok(None),
        Effect::ApplySourceKey(_) => Ok(None),
    })?;
    match status {
        250 => Ok(Navigation::Target(
            target_to_open.unwrap_or_else(|| Path::new(".").to_owned()),
        )),
        251 => Ok(Navigation::Library {
            return_target: Some(state.repository.target.root().to_owned()),
        }),
        status => Ok(Navigation::Exit(status)),
    }
}

fn build_target_state(
    user: UserScopeSession,
    repository: TargetSession,
    library: &LibrarySnapshot,
) -> LoadedTargetState {
    let user_observed = observe(&user.target, &user.config, library);
    let repository_observed = observe(&repository.target, &repository.config, library);
    let inherited = user_enabled_skills(&user.config);
    let mut tabs = user
        .config
        .skill_directories()
        .iter()
        .cloned()
        .zip(target_rows(
            &user.config,
            library,
            &user_observed,
            &BTreeSet::new(),
        ))
        .map(|(directory, rows)| TargetTab {
            scope: TargetTabScope::User,
            directory,
            rows,
        })
        .collect::<Vec<_>>();
    let mut repository_tabs = repository
        .config
        .skill_directories()
        .iter()
        .cloned()
        .zip(target_rows(
            &repository.config,
            library,
            &repository_observed,
            &inherited,
        ))
        .map(|(directory, rows)| TargetTab {
            scope: TargetTabScope::Repository,
            directory,
            rows,
        })
        .collect::<Vec<_>>();
    if repository.first_run
        && let Some(first) = repository_tabs.first_mut()
    {
        for recommendation in repository.recommendations.iter().rev() {
            let mut row = Row::diagnostic(format!(
                "[ ] {} exists; press `a` to add this Skill Directory",
                recommendation.path()
            ));
            row.name = "Recommendation".to_owned();
            row.state = "Unchecked".to_owned();
            first.rows.insert(0, row);
        }
    }
    tabs.extend(repository_tabs);
    LoadedTargetState {
        user,
        repository,
        tabs,
    }
}

fn user_enabled_skills(config: &RepositoryConfig) -> BTreeSet<SkillKey> {
    config
        .enablements()
        .iter()
        .map(|enablement| enablement.skill().clone())
        .collect()
}

fn target_tab_label(tab: &TargetTab, user_index: usize) -> String {
    let label = tab
        .directory
        .label()
        .unwrap_or(tab.directory.key().as_str());
    match tab.scope {
        TargetTabScope::User if user_index == 0 => "User".to_owned(),
        TargetTabScope::User => format!("User · {label}"),
        TargetTabScope::Repository => label.to_owned(),
    }
}

fn initial_target_tab_index(tabs: &[TargetTab]) -> usize {
    tabs.iter()
        .position(|tab| tab.scope == TargetTabScope::Repository)
        .unwrap_or(0)
}

fn initial_target_model(tabs: &[TargetTab]) -> Model {
    let index = initial_target_tab_index(tabs);
    let mut model = Model::new(
        Workspace::Target,
        tabs.get(index)
            .map(|tab| tab.rows.clone())
            .unwrap_or_default(),
    );
    model.directory_index = index;
    sync_target_tab_model(&mut model, tabs, &BTreeSet::new());
    model
}

fn sync_target_tab_model(
    model: &mut Model,
    tabs: &[TargetTab],
    dirty_scopes: &BTreeSet<TargetTabScope>,
) {
    model.directory_count = tabs.len().max(1);
    let mut user_index = 0;
    model.directory_labels = tabs
        .iter()
        .map(|tab| {
            let label = target_tab_label(tab, user_index);
            if tab.scope == TargetTabScope::User {
                user_index += 1;
            }
            label
        })
        .collect();
    model.directory_values = tabs
        .iter()
        .map(|tab| directory_editor_value(&tab.directory))
        .collect();
    model.directory_scopes = tabs.iter().map(|tab| tab.scope).collect();
    model.dirty = tabs
        .get(model.directory_index)
        .is_some_and(|tab| dirty_scopes.contains(&tab.scope));
}

fn activate_target_tab(
    model: &mut Model,
    tabs: &[TargetTab],
    dirty_scopes: &BTreeSet<TargetTabScope>,
    index: usize,
) {
    if tabs.is_empty() {
        model.directory_index = 0;
        model.rows.clear();
        model.dirty = false;
        return;
    }
    model.directory_index = index.min(tabs.len() - 1);
    model.rows = tabs[model.directory_index].rows.clone();
    model.selected = model.selected.min(model.rows.len().saturating_sub(1));
    model.dirty = dirty_scopes.contains(&tabs[model.directory_index].scope);
}

fn store_active_target_tab(model: &Model, tabs: &mut [TargetTab]) {
    if let Some(tab) = tabs.get_mut(model.directory_index) {
        tab.rows = model.rows.clone();
    }
}

fn active_target_scope(model: &Model, tabs: &[TargetTab]) -> TargetTabScope {
    tabs.get(model.directory_index)
        .map_or(TargetTabScope::User, |tab| tab.scope)
}

fn scope_config(
    tabs: &[TargetTab],
    scope: TargetTabScope,
) -> Result<RepositoryConfig, WorkflowError> {
    let scoped = tabs
        .iter()
        .filter(|tab| tab.scope == scope)
        .collect::<Vec<_>>();
    let directories = scoped
        .iter()
        .map(|tab| tab.directory.clone())
        .collect::<Vec<_>>();
    let rows = scoped
        .iter()
        .map(|tab| tab.rows.clone())
        .collect::<Vec<_>>();
    repository_config_from_rows(&directories, &rows)
}

fn prepare_scope_save(
    paths: &AppPaths,
    state: &LoadedTargetState,
    tabs: &[TargetTab],
    scope: TargetTabScope,
) -> Result<PreparedScopeSave, WorkflowError> {
    let staged = scope_config(tabs, scope)?;
    match scope {
        TargetTabScope::User => {
            UserScopeWorkflow::prepare_save(paths, &state.user, staged).map(PreparedScopeSave::User)
        }
        TargetTabScope::Repository => {
            TargetWorkflow::prepare_save(paths, &state.repository, staged)
                .map(PreparedScopeSave::Repository)
        }
    }
}

fn plan_is_safe(plan: &crate::reconcile::Plan) -> bool {
    plan.items()
        .iter()
        .all(|item| item.safety() == crate::reconcile::Safety::Safe)
}

fn save_review_overlay(plan: &crate::reconcile::Plan) -> Overlay {
    if plan_is_safe(plan) {
        Overlay::ConfirmSave
    } else {
        let mut message = String::from("Review Guarded and Blocked Changes:\n");
        for item in plan
            .items()
            .iter()
            .filter(|item| item.safety() != crate::reconcile::Safety::Safe)
        {
            message.push_str(&format!(
                "• {:?} {} — {}\n",
                item.safety(),
                item.path().display(),
                item.reason()
            ));
        }
        message.push_str("\ny/Enter save and authorize every listed Guarded Change · n/Esc return");
        Overlay::GuardedConfirmation(message)
    }
}

fn commit_scope_save(
    paths: &AppPaths,
    prepared: PreparedScopeSave,
) -> Result<crate::app::CommandReport, WorkflowError> {
    let authorization = if prepared.plan().has_guarded() {
        Authorization::AllGuarded
    } else {
        Authorization::SafeOnly
    };
    prepared.commit(paths, authorization)
}

fn show_save_result(model: &mut Model, report: &crate::app::CommandReport) {
    let mut summary = String::from("Save completed with unresolved work:\n");
    for change in &report.changes {
        summary.push_str(&format!(
            "• {:?}: {} ({})\n",
            change.outcome, change.path, change.action
        ));
    }
    for diagnostic in &report.diagnostics {
        summary.push_str("• ");
        summary.push_str(&diagnostic.message);
        summary.push('\n');
    }
    summary.push_str("\nPress Enter to exit.");
    model.overlay = Overlay::Result(summary);
}

fn parse_directory_editor(value: &str) -> Result<SkillDirectoryConfig, String> {
    match value.trim() {
        "agents" | "1" => Ok(SkillDirectoryConfig::agents_preset()),
        "claude" | "2" => Ok(SkillDirectoryConfig::claude_preset()),
        custom => {
            let mut fields = custom.splitn(3, ',').map(str::trim);
            let key = fields.next().unwrap_or_default();
            let path = fields.next().unwrap_or_default();
            let label = fields.next().filter(|label| !label.is_empty());
            if key.is_empty() || path.is_empty() {
                return Err("Enter `agents`, `claude`, or a custom `key,path,label`.".to_owned());
            }
            let key = SkillDirectoryKey::parse(key).map_err(|error| error.to_string())?;
            let path = RepositoryRelativePath::parse(path).map_err(|error| error.to_string())?;
            Ok(SkillDirectoryConfig::new(
                key,
                path,
                label.map(str::to_owned),
            ))
        }
    }
}

fn directory_editor_value(directory: &SkillDirectoryConfig) -> String {
    format!(
        "{},{},{}",
        directory.key().as_str(),
        directory.path().as_str(),
        directory.label().unwrap_or("")
    )
}

fn target_rows(
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
    observed: &ObservedState,
    inherited_user: &BTreeSet<SkillKey>,
) -> Vec<Vec<Row>> {
    config
        .skill_directories()
        .iter()
        .map(|directory| rows_for_directory(directory, config, library, observed, inherited_user))
        .collect()
}

fn rows_for_directory(
    directory: &SkillDirectoryConfig,
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
    observed: &ObservedState,
    inherited_user: &BTreeSet<SkillKey>,
) -> Vec<Row> {
    let mut skills: BTreeMap<(String, String), (String, String, bool)> = BTreeMap::new();
    for source in library
        .sources()
        .filter(|source| source.registration() == Registration::Registered)
    {
        for skill in source.skills().filter(|skill| {
            skill.registration() == Registration::Registered
                && skill.validity() == SkillValidity::Valid
        }) {
            skills.insert(
                (source.key().as_str().to_owned(), skill.path().to_owned()),
                (
                    skill.name().unwrap_or(skill.path()).to_owned(),
                    skill.description().unwrap_or("").to_owned(),
                    skill.available(),
                ),
            );
        }
    }
    for enablement in config
        .enablements()
        .iter()
        .filter(|enablement| enablement.directory() == directory.key())
    {
        skills
            .entry((
                enablement.skill().source().as_str().to_owned(),
                enablement.skill().path().as_str().to_owned(),
            ))
            .or_insert_with(|| {
                (
                    enablement
                        .skill()
                        .path()
                        .as_str()
                        .rsplit('/')
                        .next()
                        .unwrap_or("unresolved")
                        .to_owned(),
                    "Unavailable or unresolved Skill".to_owned(),
                    false,
                )
            });
    }
    for skill in inherited_user {
        skills
            .entry((
                skill.source().as_str().to_owned(),
                skill.path().as_str().to_owned(),
            ))
            .or_insert_with(|| {
                (
                    skill
                        .path()
                        .as_str()
                        .rsplit('/')
                        .next()
                        .unwrap_or("unresolved")
                        .to_owned(),
                    "Enabled in User Scope".to_owned(),
                    false,
                )
            });
    }
    let mut rows = Vec::new();
    if let Some(directory_observation) = observed
        .directories()
        .iter()
        .find(|candidate| candidate.key() == directory.key().as_str())
    {
        rows.extend(
            directory_observation
                .diagnostics()
                .iter()
                .cloned()
                .map(Row::diagnostic),
        );
    }
    let mut current_source = String::new();
    for ((source, path), (name, description, available)) in skills {
        if source != current_source {
            current_source = source.clone();
            let child_enablements: Vec<_> = config
                .enablements()
                .iter()
                .filter(|enablement| {
                    enablement.directory() == directory.key()
                        && enablement.skill().source().as_str() == source
                })
                .collect();
            let child_count = library
                .source(&source)
                .map(|source| {
                    source
                        .skills()
                        .filter(|skill| skill.registration() == Registration::Registered)
                        .count()
                })
                .unwrap_or(child_enablements.len());
            let check = if child_enablements.is_empty() {
                CheckState::Unchecked
            } else if child_enablements.len() == child_count {
                CheckState::Checked
            } else {
                CheckState::Mixed
            };
            let mut source_row = Row::source(source.clone(), check);
            source_row.description = format!(
                "{child_count} skill{}",
                if child_count == 1 { "" } else { "s" }
            );
            rows.push(source_row);
        }
        let desired = config.enablements().iter().find(|enablement| {
            enablement.directory() == directory.key()
                && enablement.skill().source().as_str() == source
                && enablement.skill().path().as_str() == path
        });
        let skill_key = SkillKey::new(
            SourceKey::parse(&source).expect("Library Source Keys are valid"),
            SkillPath::parse(&path).expect("Library Skill paths are valid"),
        );
        let inherited = inherited_user.contains(&skill_key);
        let observation = observed.enablements().find(|observation| {
            observation.enablement().directory() == directory.key()
                && observation.enablement().skill().source().as_str() == source
                && observation.enablement().skill().path().as_str() == path
        });
        let state = observation
            .map(|observation| materialization_state(observation.state()))
            .unwrap_or(if desired.is_some() {
                "Missing"
            } else {
                "Disabled"
            });
        let overlap = if observation.is_some_and(|observation| observation.overlap_advisory()) {
            " · Library Location overlap may affect this Enablement"
        } else {
            ""
        };
        let mut row = if desired.is_none() && inherited {
            Row::inherited_user(source.clone(), name, description, available, "User Scope")
        } else {
            Row::skill_inventory(SkillInventoryRow {
                group: source.clone(),
                inventory_id: None,
                path: path.clone(),
                name,
                description,
                check: if desired.is_some() {
                    CheckState::Checked
                } else {
                    CheckState::Unchecked
                },
                available,
                valid: true,
                mode: desired.map(Enablement::materialization),
                state: state.to_owned(),
                details: format!("{source}/{path} · {state}{overlap}"),
                location_index: None,
            })
        };
        row.skill_path = Some(path.clone());
        if let Some(desired) = desired {
            row.action = initial_target_action(
                desired.materialization(),
                observation.map(|observation| observation.state()),
            );
            row.initial_action = row.action.clone();
        }
        if inherited && desired.is_some() {
            row.details.push_str(" · also active in User Scope");
        }
        rows.push(row);
    }
    let groups = rows
        .iter()
        .filter(|row| row.kind == RowKind::Source)
        .filter_map(row_identity)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut model = Model::new(Workspace::Target, rows);
    for group in groups {
        model.recompute_group(&group);
    }
    model.rows
}

fn materialization_state(state: &MaterializationState) -> &'static str {
    match state {
        MaterializationState::Missing => "Missing",
        MaterializationState::CanonicalLink | MaterializationState::EquivalentCopy => "In Sync",
        MaterializationState::DivergedCopy => "Diverged Copy",
        MaterializationState::UnknownExpectedEntry | MaterializationState::Uninspectable => {
            "Unresolved"
        }
        MaterializationState::NoncanonicalLink => "Noncanonical Link",
        MaterializationState::BrokenLink => "Broken Link",
        MaterializationState::MisdirectedLink => "Misdirected Link",
        MaterializationState::CopyIneligible => "Copy-Ineligible",
        MaterializationState::WrongKind => "Wrong Kind",
        MaterializationState::ExpectedEntryCollision => "Collision",
    }
}

fn initial_target_action(
    mode: MaterializationKind,
    state: Option<&MaterializationState>,
) -> String {
    match state.unwrap_or(&MaterializationState::Missing) {
        MaterializationState::CanonicalLink | MaterializationState::EquivalentCopy => String::new(),
        MaterializationState::Missing => match mode {
            MaterializationKind::Linked => "Create link".to_owned(),
            MaterializationKind::Copied => "Create copy".to_owned(),
        },
        MaterializationState::NoncanonicalLink => "Repair link".to_owned(),
        MaterializationState::BrokenLink
        | MaterializationState::MisdirectedLink
        | MaterializationState::WrongKind => match mode {
            MaterializationKind::Linked => "Replace with link".to_owned(),
            MaterializationKind::Copied => "Replace with copy".to_owned(),
        },
        MaterializationState::DivergedCopy => "Replace copy".to_owned(),
        MaterializationState::UnknownExpectedEntry
        | MaterializationState::Uninspectable
        | MaterializationState::CopyIneligible
        | MaterializationState::ExpectedEntryCollision => String::new(),
    }
}

fn repository_config_from_rows(
    directories: &[SkillDirectoryConfig],
    rows_by_directory: &[Vec<Row>],
) -> Result<RepositoryConfig, WorkflowError> {
    let mut enablements = Vec::new();
    for (directory, rows) in directories.iter().zip(rows_by_directory) {
        for row in rows
            .iter()
            .filter(|row| row.kind == RowKind::Skill && row.check == Some(CheckState::Checked))
        {
            let source =
                SourceKey::parse(row.group.as_deref().unwrap_or_default()).map_err(invalid)?;
            let path =
                SkillPath::parse(row.skill_path.as_deref().unwrap_or_default()).map_err(invalid)?;
            enablements.push(Enablement::new(
                directory.key().clone(),
                SkillKey::new(source, path),
                row.mode.unwrap_or(MaterializationKind::Linked),
            ));
        }
    }
    RepositoryConfig::new(directories.to_vec(), enablements).map_err(|issues| {
        WorkflowError::InvalidInput {
            message: issues
                .into_iter()
                .map(|issue| format!("{}: {}", issue.path, issue.message))
                .collect::<Vec<_>>()
                .join("; "),
        }
    })
}

fn library_rows(config: &LibraryConfig, snapshot: &LibrarySnapshot) -> Vec<Row> {
    let mut rows = snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| Row::diagnostic(diagnostic.message.clone()))
        .collect::<Vec<_>>();
    for (index, location) in config.locations().iter().enumerate() {
        let mut location_row = Row::location(location.path());
        location_row.location_index = Some(index);
        location_row.description = if index == 0 {
            "Writable local Library Location".to_owned()
        } else {
            "Read-only Library Location".to_owned()
        };
        if let Some(observed) = snapshot.locations().get(index) {
            location_row.details = format!(
                "expression {} · resolved {} · {}",
                observed.expression(),
                observed
                    .resolved()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "unresolved".to_owned()),
                if observed.available() {
                    "available"
                } else {
                    "unavailable"
                }
            );
        }
        if snapshot.location_has_overlap_advisory(index) {
            location_row
                .details
                .push_str(" · WARNING: overlaps another explicitly authorized Library Location");
        }
        rows.push(location_row);
        for source in snapshot
            .sources()
            .filter(|source| source.location_index() == index)
        {
            let checks: Vec<_> = source
                .skills()
                .filter(|skill| skill.validity() == SkillValidity::Valid)
                .map(|skill| skill.registration() == Registration::Registered)
                .collect();
            let check = if checks.iter().all(|registered| *registered) && !checks.is_empty() {
                CheckState::Checked
            } else if checks.iter().all(|registered| !*registered) {
                CheckState::Unchecked
            } else {
                CheckState::Mixed
            };
            let mut source_row = Row::source_inventory(
                source.key().as_str().to_owned(),
                check,
                source.registration() == Registration::Registered,
                index,
                source.relative_path().to_owned(),
                source.available(),
                source.key_collision(),
            );
            source_row.description = format!(
                "{:?} Source · {} skill{}",
                source.kind(),
                checks.len(),
                if checks.len() == 1 { "" } else { "s" }
            );
            source_row.details = format!(
                "{:?} Source · root {} · origin {} · {:?}{}",
                source.kind(),
                source
                    .root()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "unavailable".to_owned()),
                source.origin().unwrap_or("none"),
                source.registration(),
                if snapshot.location_has_overlap_advisory(index) {
                    " · WARNING: Library Location overlap"
                } else {
                    ""
                }
            );
            if source.key_collision() {
                source_row.state = "Key Collision".to_owned();
                source_row.initial_state = source_row.state.clone();
                source_row.check = Some(CheckState::Invalid);
            }
            rows.push(source_row);
            for skill in source.skills() {
                let inventory_id = source_inventory_id(index, source.relative_path());
                let mut row = Row::skill_inventory(SkillInventoryRow {
                    group: source.key().as_str().to_owned(),
                    inventory_id: Some(inventory_id),
                    path: skill.path().to_owned(),
                    name: skill.name().unwrap_or(skill.path()).to_owned(),
                    description: skill.description().unwrap_or("").to_owned(),
                    check: if skill.registration() == Registration::Registered {
                        CheckState::Checked
                    } else if skill.validity() == SkillValidity::Invalid {
                        CheckState::Invalid
                    } else {
                        CheckState::Unchecked
                    },
                    available: skill.available() && skill.validity() == SkillValidity::Valid,
                    valid: skill.validity() == SkillValidity::Valid,
                    mode: None,
                    state: if skill.available() { "" } else { "Unavailable" }.to_owned(),
                    details: format!(
                        "path {} · {:?} · {:?}{}{}",
                        skill
                            .absolute_path()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "unavailable".to_owned()),
                        skill.registration(),
                        skill.validity(),
                        if skill.diagnostics().is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", skill.diagnostics().join("; "))
                        },
                        if snapshot.location_has_overlap_advisory(index) {
                            " · WARNING: Library Location overlap"
                        } else {
                            ""
                        }
                    ),
                    location_index: Some(index),
                });
                row.registered = Some(source.registration() == Registration::Registered);
                let local_destination = index == 0
                    && source.key().as_str() == "local/library"
                    && source.relative_path() == ".";
                if !local_destination
                    && skill.validity() == SkillValidity::Valid
                    && let Some(path) = skill.absolute_path()
                {
                    row.acquisition_source = Some(path.to_owned());
                    row.acquisition_source_root_git = source.kind()
                        == crate::library::SourceKind::Git
                        && source.root() == Some(path);
                    if skill.registration() == Registration::Unregistered {
                        row.acquisition_mode = Some(LibraryAcquisitionMode::Move);
                        row.initial_acquisition_mode = row.acquisition_mode;
                    }
                }
                rows.push(row);
            }
        }
    }
    rows
}

fn library_config_from_rows(
    original: &LibraryConfig,
    rows: &[Row],
) -> Result<LibraryConfig, WorkflowError> {
    let mut locations = Vec::new();
    for (index, location) in original.locations().iter().enumerate() {
        let mut sources = Vec::new();
        for source in rows.iter().filter(|row| {
            row.kind == RowKind::Source
                && row.location_index == Some(index)
                && row.registered == Some(true)
        }) {
            let key = SourceKey::parse(&source.name).map_err(invalid)?;
            let path =
                SkillPath::parse(source.source_path.as_deref().unwrap_or(".")).map_err(invalid)?;
            let skills = rows
                .iter()
                .filter(|row| {
                    row.kind == RowKind::Skill
                        && row_identity(row) == row_identity(source)
                        && row.check == Some(CheckState::Checked)
                        && (!row_has_acquisition(row)
                            || (row.initial_check == Some(CheckState::Checked)
                                && row.acquisition_mode != Some(LibraryAcquisitionMode::Move)))
                })
                .map(|row| {
                    SkillPath::parse(row.skill_path.as_deref().unwrap_or_default())
                        .map(RegisteredSkillConfig::new)
                        .map_err(invalid)
                })
                .collect::<Result<Vec<_>, _>>()?;
            sources.push(RegisteredSourceConfig::new(key, path, skills));
        }
        locations.push(LibraryLocationConfig::new(
            location.path().to_owned(),
            location.exclusions().to_vec(),
            location.allow_overlap(),
            sources,
        ));
    }
    let acquired = rows
        .iter()
        .filter(|row| row_has_acquisition(row))
        .map(|row| SkillPath::parse(&row.name).map_err(invalid))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if !acquired.is_empty() {
        let Some(first) = locations.first().cloned() else {
            return Err(WorkflowError::InvalidInput {
                message: "Library acquisition requires a first Library Location".to_owned(),
            });
        };
        let mut sources = first.sources().to_vec();
        if let Some(index) = sources
            .iter()
            .position(|source| source.key().as_str() == "local/library")
        {
            if sources[index].path().as_str() != "." {
                return Err(WorkflowError::InvalidInput {
                    message: "`local/library` must use path `.` in the first Library Location"
                        .to_owned(),
                });
            }
            let mut skills = sources[index].skills().to_vec();
            for path in acquired {
                if !skills.iter().any(|skill| skill.path() == &path) {
                    skills.push(RegisteredSkillConfig::new(path));
                }
            }
            sources[index] = RegisteredSourceConfig::new(
                sources[index].key().clone(),
                sources[index].path().clone(),
                skills,
            );
        } else {
            sources.push(RegisteredSourceConfig::new(
                SourceKey::parse("local/library").expect("built-in key"),
                SkillPath::parse(".").expect("root path"),
                acquired
                    .into_iter()
                    .map(RegisteredSkillConfig::new)
                    .collect(),
            ));
        }
        locations[0] = LibraryLocationConfig::new(
            first.path().to_owned(),
            first.exclusions().to_vec(),
            first.allow_overlap(),
            sources,
        );
    }
    LibraryConfig::new(locations).map_err(|issues| WorkflowError::InvalidInput {
        message: issues
            .into_iter()
            .map(|issue| format!("{}: {}", issue.path, issue.message))
            .collect::<Vec<_>>()
            .join("; "),
    })
}

fn row_has_acquisition(row: &Row) -> bool {
    row.kind == RowKind::Skill
        && row.check == Some(CheckState::Checked)
        && row.acquisition_source.is_some()
        && row.acquisition_mode.is_some()
        && (row.initial_check != Some(CheckState::Checked) || row.acquisition_pending)
}

fn library_acquisitions_from_rows(rows: &[Row]) -> Vec<LibraryAcquisition> {
    rows.iter()
        .filter(|row| row_has_acquisition(row))
        .filter_map(|row| {
            Some(LibraryAcquisition::new(
                row.acquisition_source.clone()?,
                row.name.clone(),
                row.acquisition_mode?,
                row.acquisition_source_root_git,
            ))
        })
        .collect()
}

fn library_fast_save_is_safe(original: &LibraryConfig, staged: &LibraryConfig) -> bool {
    let original_sources: BTreeSet<_> = original
        .locations()
        .iter()
        .flat_map(|location| location.sources())
        .map(|source| source.key().as_str())
        .collect();
    let staged_sources: BTreeSet<_> = staged
        .locations()
        .iter()
        .flat_map(|location| location.sources())
        .map(|source| source.key().as_str())
        .collect();
    let registered_skills = |config: &LibraryConfig| {
        config
            .locations()
            .iter()
            .flat_map(|location| location.sources())
            .flat_map(|source| {
                source.skills().iter().map(|skill| {
                    (
                        source.key().as_str().to_owned(),
                        skill.path().as_str().to_owned(),
                    )
                })
            })
            .collect::<BTreeSet<_>>()
    };
    original_sources.is_subset(&staged_sources)
        && registered_skills(original).is_subset(&registered_skills(staged))
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

fn run_static(model: Model, failure_status: u8) -> Result<u8, WorkflowError> {
    run_interactive(model, |_model, effect| match effect {
        Effect::Quit { .. } => Ok(Some(failure_status)),
        Effect::PrepareSave { .. } => Ok(None),
        Effect::CancelSave => Ok(None),
        _ => Ok(None),
    })
}

fn run_interactive(
    mut model: Model,
    mut handle_effect: impl FnMut(&mut Model, Effect) -> Result<Option<u8>, WorkflowError>,
) -> Result<u8, WorkflowError> {
    use crossterm::event::{self, Event};
    use crossterm::execute;
    use crossterm::terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    };
    use ratatui::Terminal;
    use ratatui::backend::CrosstermBackend;
    use std::io::stdout;

    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
            let _ = execute!(stdout(), LeaveAlternateScreen);
        }
    }

    enable_raw_mode().map_err(fatal)?;
    execute!(stdout(), EnterAlternateScreen).map_err(fatal)?;
    let _restore = Restore;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout())).map_err(fatal)?;
    loop {
        terminal
            .draw(|frame| render(frame, &model))
            .map_err(fatal)?;
        let Event::Key(key) = event::read().map_err(fatal)? else {
            continue;
        };
        let Some(action) = action_for_key(key) else {
            continue;
        };
        for effect in reduce(&mut model, action) {
            if let Some(status) = handle_effect(&mut model, effect)? {
                return Ok(status);
            }
        }
    }
}

fn fatal(error: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::Fatal {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod internal_tests {
    use super::*;

    #[test]
    fn initial_target_tab_prefers_the_first_repository_tab() {
        let tabs = vec![
            TargetTab {
                scope: TargetTabScope::User,
                directory: SkillDirectoryConfig::agents_preset(),
                rows: Vec::new(),
            },
            TargetTab {
                scope: TargetTabScope::Repository,
                directory: SkillDirectoryConfig::claude_preset(),
                rows: Vec::new(),
            },
        ];

        assert_eq!(initial_target_tab_index(&tabs), 1);
        assert_eq!(initial_target_tab_index(&tabs[..1]), 0);
    }

    #[test]
    fn untouched_first_run_defaults_do_not_block_scope_switching() {
        let tabs = vec![
            TargetTab {
                scope: TargetTabScope::User,
                directory: SkillDirectoryConfig::agents_preset(),
                rows: Vec::new(),
            },
            TargetTab {
                scope: TargetTabScope::Repository,
                directory: SkillDirectoryConfig::claude_preset(),
                rows: Vec::new(),
            },
        ];
        let mut model = initial_target_model(&tabs);

        assert_eq!(model.directory_index, 1);
        assert!(!model.dirty);
        assert_eq!(
            reduce(&mut model, Action::NextDirectory),
            [Effect::DirectoryChanged { from: 1, to: 0 }]
        );
        assert_eq!(model.directory_index, 0);
        assert_eq!(model.overlay, Overlay::None);
    }

    #[test]
    fn row_selection_adds_background_without_replacing_semantic_foreground() {
        let warning = Row::diagnostic("Needs attention");
        let warning_style = row_style(&warning, true);
        assert_eq!(warning_style.fg, Some(WARNING));
        assert_eq!(warning_style.bg, Some(SELECTED_BACKGROUND));

        let mut error = Row::diagnostic("Cannot continue");
        error.state = "Invalid".to_owned();
        let error_style = row_style(&error, true);
        assert_eq!(error_style.fg, Some(ERROR));
        assert_eq!(error_style.bg, Some(SELECTED_BACKGROUND));

        let normal_style = row_style(&Row::location("./library"), true);
        assert_eq!(normal_style.fg, None);
        assert_eq!(normal_style.bg, Some(SELECTED_BACKGROUND));
    }

    #[test]
    fn confirmation_distinguishes_desired_actions_from_warnings() {
        let text = confirmation_text(
            "• Move to Library: source → destination\n\
             • Guarded existing path — conflict\n\
             • Blocked tracked path — cannot replace",
        );

        assert_eq!(text.lines[0].style.fg, None);
        assert_eq!(text.lines[1].style.fg, Some(WARNING));
        assert_eq!(text.lines[2].style.fg, Some(ERROR));

        let footer = confirmation_prompt_line("y/Enter initialize · n/Esc return");
        assert!(footer.spans.iter().any(|span| span.style.fg == Some(BONE)));
        assert!(
            footer
                .spans
                .iter()
                .any(|span| span.style.fg == Some(DIM_FOREGROUND))
        );
    }

    #[test]
    fn first_run_starts_in_the_onboarding_table() {
        let home = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(home.path().to_owned());
        let session = OnboardingWorkflow::load(&paths).unwrap();

        let model = initial_onboarding_model(&paths, &session);

        assert_eq!(model.overlay, Overlay::None);
        assert_eq!(model.rows[model.selected].kind, RowKind::Location);
        assert_eq!(model.rows[model.selected].name, "./library");
    }

    #[test]
    fn colliding_source_rows_round_trip_by_inventory_identity() {
        let old_key = SourceKey::parse("acme/skills").unwrap();
        let original = LibraryConfig::new(vec![LibraryLocationConfig::new(
            "./library".to_owned(),
            Vec::new(),
            false,
            vec![RegisteredSourceConfig::new(
                old_key,
                SkillPath::parse("old").unwrap(),
                vec![RegisteredSkillConfig::new(
                    SkillPath::parse("old-skill").unwrap(),
                )],
            )],
        )])
        .unwrap();
        let first_id = source_inventory_id(0, "old");
        let second_id = source_inventory_id(0, "new");
        let first = Row::source_inventory(
            "acme/skills".to_owned(),
            CheckState::Checked,
            true,
            0,
            "old".to_owned(),
            true,
            false,
        );
        let first_skill = Row::skill_inventory(SkillInventoryRow {
            group: "acme/skills".to_owned(),
            inventory_id: Some(first_id),
            path: "old-skill".to_owned(),
            name: "old-skill".to_owned(),
            description: String::new(),
            check: CheckState::Checked,
            available: true,
            valid: true,
            mode: None,
            state: String::new(),
            details: String::new(),
            location_index: Some(0),
        });
        let mut second = Row::source_inventory(
            "acme/skills".to_owned(),
            CheckState::Unchecked,
            false,
            0,
            "new".to_owned(),
            true,
            true,
        );
        second.name = "other/skills".to_owned();
        second.registered = Some(true);
        second.key_collision = false;
        let second_skill = Row::skill_inventory(SkillInventoryRow {
            group: "acme/skills".to_owned(),
            inventory_id: Some(second_id),
            path: "new-skill".to_owned(),
            name: "new-skill".to_owned(),
            description: String::new(),
            check: CheckState::Checked,
            available: true,
            valid: true,
            mode: None,
            state: String::new(),
            details: String::new(),
            location_index: Some(0),
        });

        let config =
            library_config_from_rows(&original, &[first, first_skill, second, second_skill])
                .unwrap();
        let sources = config.locations()[0].sources();

        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].key().as_str(), "acme/skills");
        assert_eq!(sources[0].skills()[0].path().as_str(), "old-skill");
        assert_eq!(sources[1].key().as_str(), "other/skills");
        assert_eq!(sources[1].skills()[0].path().as_str(), "new-skill");
    }

    #[test]
    fn escape_clears_an_active_filter_without_changing_collapse_state() {
        let mut model = Model::new(Workspace::Library, Vec::new());
        model.filter = "release".to_owned();
        model.collapsed.insert("source".to_owned());
        model.overlay = Overlay::Filter;

        reduce(&mut model, Action::Escape);

        assert!(model.filter.is_empty());
        assert_eq!(model.overlay, Overlay::None);
        assert!(model.collapsed.contains("source"));
    }

    #[test]
    fn invalid_children_are_excluded_from_source_rollups_and_bulk_toggles() {
        let source = Row::source("local/library", CheckState::Mixed);
        let valid = Row::skill(
            "local/library",
            "valid",
            "Valid",
            true,
            true,
            MaterializationKind::Linked,
            "Registered",
        );
        let mut invalid = Row::skill(
            "local/library",
            "invalid",
            "Invalid",
            false,
            false,
            MaterializationKind::Linked,
            "Invalid",
        );
        invalid.check = Some(CheckState::Invalid);
        invalid.valid = false;
        let mut model = Model::new(Workspace::Library, vec![source, valid, invalid]);

        reduce(&mut model, Action::Toggle);

        assert_eq!(model.rows[0].check, Some(CheckState::Unchecked));
        assert_eq!(model.rows[1].check, Some(CheckState::Unchecked));
        assert_eq!(model.rows[2].check, Some(CheckState::Invalid));
    }

    #[test]
    fn returning_from_save_review_emits_cancellation_for_the_prepared_plan() {
        let mut model = Model::new(Workspace::Target, Vec::new());
        model.overlay = Overlay::ConfirmSave;

        let effects = reduce(&mut model, Action::ReturnToEditing);

        assert_eq!(effects, [Effect::CancelSave]);
        assert_eq!(model.overlay, Overlay::None);
    }

    #[test]
    fn dirty_scope_switch_offers_save_discard_or_return() {
        let mut model = Model::new(Workspace::Target, Vec::new());
        model.directory_count = 2;
        model.directory_scopes = vec![TargetTabScope::User, TargetTabScope::Repository];
        model.dirty = true;

        assert!(reduce(&mut model, Action::NextDirectory).is_empty());
        assert_eq!(model.overlay, Overlay::SwitchScope { from: 0, to: 1 });
        assert_eq!(model.directory_index, 0);
        assert_eq!(
            reduce(&mut model, Action::Confirm),
            [Effect::SaveScopeAndSwitch { from: 0, to: 1 }]
        );

        model.overlay = Overlay::SwitchScope { from: 0, to: 1 };
        assert_eq!(
            reduce(&mut model, Action::DeleteDirectory),
            [Effect::DiscardScopeAndSwitch { from: 0, to: 1 }]
        );
    }

    #[test]
    fn library_acquisition_mode_cycles_move_copy_link_and_registration_only() {
        let mut row = Row::skill(
            "external/skills",
            "demo",
            "Demo Skill",
            false,
            true,
            MaterializationKind::Linked,
            "",
        );
        row.acquisition_source = Some(std::path::PathBuf::from("/external/demo"));
        row.registered = Some(true);
        row.acquisition_mode = Some(LibraryAcquisitionMode::Move);
        row.initial_acquisition_mode = row.acquisition_mode;
        let mut model = Model::new(Workspace::Library, vec![row]);

        reduce(&mut model, Action::Toggle);
        assert_eq!(
            model.rows[0].acquisition_mode,
            Some(LibraryAcquisitionMode::Move)
        );
        assert_eq!(model.rows[0].action, "Move to Library");

        reduce(&mut model, Action::SwitchMode);
        assert_eq!(
            model.rows[0].acquisition_mode,
            Some(LibraryAcquisitionMode::Copy)
        );
        assert_eq!(model.rows[0].action, "Copy to Library");

        reduce(&mut model, Action::SwitchMode);
        assert_eq!(
            model.rows[0].acquisition_mode,
            Some(LibraryAcquisitionMode::Link)
        );
        assert_eq!(model.rows[0].action, "Link to Library");

        reduce(&mut model, Action::SwitchMode);
        assert_eq!(model.rows[0].acquisition_mode, None);
        assert_eq!(model.rows[0].action, "Register");
    }

    #[test]
    fn reverting_source_registration_clears_its_pending_action() {
        let source = Row::source_inventory(
            "external/skills".to_owned(),
            CheckState::Checked,
            true,
            1,
            ".".to_owned(),
            true,
            false,
        );
        let mut model = Model::new(Workspace::Library, vec![source]);

        reduce(&mut model, Action::RegisterSource);
        assert_eq!(model.rows[0].action, "Unregister Source");
        model.filter = "pending".to_owned();
        assert_eq!(model.visible_indices(), vec![0]);
        model.filter.clear();

        reduce(&mut model, Action::RegisterSource);
        assert_eq!(model.rows[0].action, "");
    }

    #[test]
    fn onboarding_acquisition_actions_stay_concise() {
        let mut row = Row::skill(
            "Existing user-scoped Skills",
            "demo",
            "Demo Skill",
            true,
            true,
            MaterializationKind::Linked,
            "",
        );
        row.acquisition_source = Some(std::path::PathBuf::from("/user/demo"));
        row.acquisition_mode = Some(LibraryAcquisitionMode::Move);
        row.initial_acquisition_mode = row.acquisition_mode;
        refresh_onboarding_action(&mut row);
        let mut model = Model::new(Workspace::Onboarding, vec![row]);

        assert_eq!(model.rows[0].action, "Move to Library");
        reduce(&mut model, Action::SwitchMode);
        assert_eq!(model.rows[0].action, "Copy to Library");
        reduce(&mut model, Action::SwitchMode);
        assert_eq!(model.rows[0].action, "Link to Library");
    }
}
