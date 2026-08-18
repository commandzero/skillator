//! Terminal reducer, rendering, and lifecycle.

use crate::app::{
    AppPaths, LibraryWorkflow, PreparedTargetSave, ReportStatus, TargetSession, TargetWorkflow,
    WorkflowError,
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
use crate::reconcile::Authorization;
use crate::target::{MaterializationState, ObservedState, observe};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row as TableRow, Table};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Target,
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Checked,
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
    details: String,
    initial_state: String,
    initial_check: Option<CheckState>,
    initial_mode: Option<MaterializationKind>,
    available: bool,
    valid: bool,
    registered: Option<bool>,
    location_index: Option<usize>,
    source_path: Option<String>,
    skill_path: Option<String>,
    key_collision: bool,
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
            details: String::new(),
            initial_state: String::new(),
            initial_check: Some(check),
            initial_mode: None,
            available: true,
            valid: true,
            registered: Some(true),
            location_index: None,
            source_path: None,
            skill_path: None,
            key_collision: false,
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
            details: String::new(),
            initial_state: String::new(),
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
        }
        .with_initial_state()
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
            details: String::new(),
            initial_state: String::new(),
            initial_check: None,
            initial_mode: None,
            available: true,
            valid: true,
            registered: None,
            location_index: None,
            source_path: None,
            skill_path: None,
            key_collision: false,
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
            details: String::new(),
            initial_state: "Warning".to_owned(),
            initial_check: None,
            initial_mode: None,
            available: false,
            valid: true,
            registered: None,
            location_index: None,
            source_path: None,
            skill_path: None,
            key_collision: false,
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
        let matching_groups: BTreeSet<_> = self
            .rows
            .iter()
            .filter(|row| {
                row.kind == RowKind::Skill
                    && (row.name.to_ascii_lowercase().contains(&needle)
                        || row.description.to_ascii_lowercase().contains(&needle))
            })
            .filter_map(|row| row_identity(row).map(str::to_owned))
            .collect();
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| match row.kind {
                RowKind::Source if filtering => row_identity(row)
                    .is_some_and(|identity| matching_groups.contains(identity))
                    .then_some(index),
                RowKind::Skill if filtering => (row.name.to_ascii_lowercase().contains(&needle)
                    || row.description.to_ascii_lowercase().contains(&needle))
                .then_some(index),
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
                row.mode = Some(match row.mode {
                    Some(MaterializationKind::Linked) => MaterializationKind::Copied,
                    _ => MaterializationKind::Linked,
                });
                refresh_staged_state(row);
                model.dirty = true;
            }
        }
        Action::NextDirectory => {
            let previous = model.directory_index;
            model.directory_index = (model.directory_index + 1) % model.directory_count.max(1);
            return vec![Effect::DirectoryChanged {
                from: previous,
                to: model.directory_index,
            }];
        }
        Action::PreviousDirectory => {
            let previous = model.directory_index;
            model.directory_index = model
                .directory_index
                .checked_sub(1)
                .unwrap_or(model.directory_count.saturating_sub(1));
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
            model.overlay = if model.workspace == Workspace::Library {
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
            if model.workspace == Workspace::Library {
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
            if model.workspace == Workspace::Library
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
            if let Some(row) = model.rows.get_mut(model.selected)
                && row.kind == RowKind::Source
            {
                if row.key_collision {
                    model.overlay = Overlay::SourceKeyEditor(row.name.clone());
                    return Vec::new();
                }
                row.registered = Some(!row.registered.unwrap_or(false));
                row.state = if row.registered == Some(true) {
                    "Registered"
                } else {
                    "Unregistered"
                }
                .to_owned();
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
                    if model.workspace == Workspace::Target {
                        refresh_staged_state(candidate);
                    }
                }
            }
            model.recompute_group(&identity);
            model.dirty = true;
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
                if model.workspace == Workspace::Target {
                    refresh_staged_state(candidate);
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
    row.state = if row.check != row.initial_check {
        if row.check == Some(CheckState::Checked) {
            "Staged Enable".to_owned()
        } else {
            "Staged Disable".to_owned()
        }
    } else if row.check == Some(CheckState::Checked) && row.mode != row.initial_mode {
        "Staged Convert".to_owned()
    } else {
        row.initial_state.clone()
    };
}

pub fn action_for_key(key: KeyEvent) -> Option<Action> {
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
            Span::styled(title, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(if model.dirty { "  [staged]" } else { "" }),
            Span::raw(directory_strip),
        ])),
        areas[0],
    );
    let header = match model.workspace {
        Workspace::Target => TableRow::new(["Enabled", "Mode", "Skill", "Description", "State"]),
        Workspace::Library => TableRow::new(["Status", "", "Name", "Description", ""]),
    }
    .style(Style::default().add_modifier(Modifier::BOLD));
    let visible = model.visible_indices();
    let rows = visible.iter().map(|index| {
        let row = &model.rows[*index];
        let selected = *index == model.selected;
        let check = match row.check {
            Some(CheckState::Checked) => "[x]",
            Some(CheckState::Unchecked) => "[ ]",
            Some(CheckState::Mixed) => "[-]",
            Some(CheckState::Invalid) => "[!]",
            None => "",
        };
        let mode = match row.mode {
            Some(MaterializationKind::Linked) => "link",
            Some(MaterializationKind::Copied) => "copy",
            None => "",
        };
        let name = match row.kind {
            RowKind::Location => format!("── {} ", row.name),
            RowKind::Source => {
                let glyph = if row_identity(row)
                    .is_some_and(|identity| model.collapsed.contains(identity))
                    && model.filter.is_empty()
                {
                    "▸"
                } else {
                    "▾"
                };
                format!("{glyph} {}", row.name)
            }
            RowKind::Skill => format!("  └─ {}", row.name),
            RowKind::Diagnostic => format!("! {}", row.name),
        };
        let cells = if row.kind == RowKind::Location {
            [
                "─────────".to_owned(),
                "──────".to_owned(),
                name,
                "────────────────────────────────────────────────────────────────".to_owned(),
                "────────────────────────────────".to_owned(),
            ]
        } else {
            [
                check.to_owned(),
                mode.to_owned(),
                name,
                row.description.clone(),
                row.state.clone(),
            ]
        };
        TableRow::new(cells).style(if selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else if !row.available {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default()
        })
    });
    let widths = [
        Constraint::Length(9),
        Constraint::Length(6),
        Constraint::Percentage(26),
        Constraint::Percentage(43),
        Constraint::Percentage(20),
    ];
    frame.render_widget(
        Table::new(rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL)),
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
            format!("{} — {}", row.name, details)
        })
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(inspector).block(
            Block::default()
                .borders(Borders::TOP)
                .title("j/k rows · J/K sources · Space toggle · m mode · ? help"),
        ),
        areas[2],
    );
    render_overlay(frame, model);
}

fn render_overlay(frame: &mut Frame<'_>, model: &Model) {
    let text = match &model.overlay {
        Overlay::None => return,
        Overlay::Help => {
            "j/k rows   J/K sources   h/l collapse/expand\nSpace toggle   m link/copy   Tab directories\n/ filter   s confirm save   Ctrl+S fast save\nq quit   t target   a/e/d directory   Ctrl+L library"
        }
        Overlay::Filter => return render_filter(frame, &model.filter),
        Overlay::ConfirmSave => "Save desired state and exit?  y/Enter proceed · n/Esc return",
        Overlay::ConfirmSaveWarning(message) => message,
        Overlay::GuardedConfirmation(message) => message,
        Overlay::DiscardWorkspace => {
            "Discard staged edits and switch workspace?\ny/Enter discard · n/Esc return"
        }
        Overlay::DiscardTarget => {
            "Discard staged edits and change Target?\ny/Enter discard · n/Esc return"
        }
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
            return render_input(frame, &format!("{mode} Library Location"), "path", input);
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
        Overlay::ConfirmDelete => "Delete the selected directory or location? y/Enter · n/Esc",
        Overlay::Busy => "Target Busy. Enter retry · Esc return to editing",
        Overlay::Notice(message) => message,
        Overlay::Result(message) => message,
    };
    let area = centered(frame.area(), 70, 30);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("skillator")),
        area,
    );
}

fn render_input(frame: &mut Frame<'_>, title: &str, hint: &str, input: &str) {
    let area = centered(frame.area(), 70, 30);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(format!("{hint}\n> {input}"))
            .block(Block::default().borders(Borders::ALL).title(title)),
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
            .block(Block::default().borders(Borders::ALL).title("Filter")),
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
}

fn navigate(paths: &AppPaths, mut navigation: Navigation) -> Result<u8, WorkflowError> {
    loop {
        navigation = match navigation {
            Navigation::Exit(status) => return Ok(status),
            Navigation::Target(directory) => run_target_once(paths, &directory)?,
            Navigation::Library { return_target } => {
                run_library_once(paths, return_target.as_deref())?
            }
        };
    }
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
    let snapshot = LibraryWorkflow::snapshot(paths, &session.config);
    let mut working_config = session.config.clone();
    let mut model = Model::new(Workspace::Library, library_rows(&working_config, &snapshot));
    model.dirty = session.first_run;
    let mut staged: Option<LibraryConfig> = None;
    let mut target_to_open = None;
    let status = run_interactive(model, |model, effect| match effect {
        Effect::Quit { status } => Ok(Some(status)),
        Effect::PrepareSave { fast } => {
            let config = library_config_from_rows(&working_config, &model.rows)?;
            if fast && library_fast_save_is_safe(&session.config, &config) {
                LibraryWorkflow::save(paths, &session, &config, true)?;
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
                staged = Some(config);
                model.overlay = if affected.is_empty() {
                    Overlay::ConfirmSave
                } else {
                    Overlay::ConfirmSaveWarning(format!(
                        "Save Library changes? {} Enablement reference(s) in the current Target will become Unresolved.\ny/Enter proceed · n/Esc return",
                        affected.len()
                    ))
                };
                Ok(None)
            }
        }
        Effect::CommitSave => {
            let config = staged.take().unwrap_or_else(|| session.config.clone());
            LibraryWorkflow::save(paths, &session, &config, true)?;
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
            let Some(source) = model.rows.get_mut(model.selected) else {
                return Ok(None);
            };
            source.name = key.as_str().to_owned();
            source.registered = Some(true);
            source.key_collision = false;
            source.state = "Registered".to_owned();
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
    let session = match TargetWorkflow::load(directory) {
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
    let library = LibraryWorkflow::snapshot(paths, &library_session.config);
    let observed = observe(&session.target, &session.config, &library);
    let mut directories = session.config.skill_directories().to_vec();
    let mut rows_by_directory = target_rows(&session.config, &library, &observed);
    if rows_by_directory.is_empty() {
        rows_by_directory.push(Vec::new());
    }
    if session.first_run {
        for recommendation in session.recommendations.iter().rev() {
            let mut row = Row::diagnostic(format!(
                "[ ] {} exists; press `a` to add this Skill Directory",
                recommendation.path()
            ));
            row.name = "Recommendation".to_owned();
            row.state = "Unchecked".to_owned();
            rows_by_directory[0].insert(0, row);
        }
    }
    let mut model = Model::new(Workspace::Target, rows_by_directory[0].clone());
    model.directory_count = rows_by_directory.len();
    model.directory_labels = directories
        .iter()
        .map(|directory| {
            directory
                .label()
                .unwrap_or(directory.key().as_str())
                .to_owned()
        })
        .collect();
    model.directory_values = directories.iter().map(directory_editor_value).collect();
    model.dirty = session.first_run;
    let mut pending: Option<PreparedTargetSave> = None;
    let mut retry_fast = false;
    let mut target_to_open = None;
    let status = run_interactive(model, |model, effect| match effect {
        Effect::Quit { status } => Ok(Some(status)),
        Effect::DirectoryChanged { from, to } => {
            if from < rows_by_directory.len() {
                rows_by_directory[from] = model.rows.clone();
            }
            if to < rows_by_directory.len() {
                model.rows = rows_by_directory[to].clone();
                model.selected = model.selected.min(model.rows.len().saturating_sub(1));
            }
            Ok(None)
        }
        Effect::PrepareSave { fast } => {
            retry_fast = fast;
            prepare_target_save(
                paths,
                &session,
                model,
                &mut rows_by_directory,
                &directories,
                &mut pending,
                fast,
            )
        }
        Effect::RetrySave => prepare_target_save(
            paths,
            &session,
            model,
            &mut rows_by_directory,
            &directories,
            &mut pending,
            retry_fast,
        ),
        Effect::CommitSave => commit_target_save(model, &mut pending),
        Effect::CancelSave => {
            pending.take();
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
            if edit && directories.is_empty() {
                model.overlay = Overlay::Notice(
                    "There is no Skill Directory to edit; add one instead.".to_owned(),
                );
                return Ok(None);
            }
            if edit {
                let index = model.directory_index.min(directories.len() - 1);
                if candidate.path() != directories[index].path()
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
            let mut proposed = directories.clone();
            if edit {
                let index = model.directory_index.min(directories.len() - 1);
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
                let index = model.directory_index.min(directories.len() - 1);
                directories[index] = candidate;
            } else {
                rows_by_directory[model.directory_index] = model.rows.clone();
                rows_by_directory.push(rows_for_directory(
                    &candidate,
                    &session.config,
                    &library,
                    &observed,
                ));
                directories.push(candidate);
                model.directory_index = directories.len() - 1;
                model.rows = rows_by_directory[model.directory_index].clone();
            }
            model.directory_count = directories.len();
            model.directory_labels = directories
                .iter()
                .map(|directory| {
                    directory
                        .label()
                        .unwrap_or(directory.key().as_str())
                        .to_owned()
                })
                .collect();
            model.directory_values = directories.iter().map(directory_editor_value).collect();
            model.dirty = true;
            Ok(None)
        }
        Effect::DeleteDirectory => {
            if directories.len() > 1 {
                let index = model.directory_index.min(directories.len() - 1);
                let rows = if index == model.directory_index {
                    &model.rows
                } else {
                    &rows_by_directory[index]
                };
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
                directories.remove(index);
                rows_by_directory.remove(index);
                model.directory_index = index.min(directories.len() - 1);
                model.directory_count = directories.len();
                model.directory_labels = directories
                    .iter()
                    .map(|directory| {
                        directory
                            .label()
                            .unwrap_or(directory.key().as_str())
                            .to_owned()
                    })
                    .collect();
                model.directory_values = directories.iter().map(directory_editor_value).collect();
                model.rows = rows_by_directory[model.directory_index].clone();
                model.dirty = true;
            } else {
                model.overlay =
                    Overlay::Notice("A Target must keep at least one Skill Directory.".to_owned());
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
            return_target: Some(session.target.root().to_owned()),
        }),
        status => Ok(Navigation::Exit(status)),
    }
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

fn prepare_target_save(
    paths: &AppPaths,
    session: &TargetSession,
    model: &mut Model,
    rows_by_directory: &mut [Vec<Row>],
    directories: &[SkillDirectoryConfig],
    pending: &mut Option<PreparedTargetSave>,
    fast: bool,
) -> Result<Option<u8>, WorkflowError> {
    rows_by_directory[model.directory_index] = model.rows.clone();
    let staged = repository_config_from_rows(directories, rows_by_directory)?;
    match TargetWorkflow::prepare_save(paths, session, staged) {
        Ok(prepared)
            if fast
                && prepared
                    .plan()
                    .items()
                    .iter()
                    .all(|item| item.safety() == crate::reconcile::Safety::Safe) =>
        {
            *pending = Some(prepared);
            commit_target_save(model, pending)
        }
        Ok(prepared) => {
            model.overlay = if prepared
                .plan()
                .items()
                .iter()
                .any(|item| item.safety() != crate::reconcile::Safety::Safe)
            {
                let mut message = String::from("Review Guarded and Blocked Changes:\n");
                for item in prepared
                    .plan()
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
                message.push_str(
                    "\ny/Enter save and authorize every listed Guarded Change · n/Esc return",
                );
                Overlay::GuardedConfirmation(message)
            } else {
                Overlay::ConfirmSave
            };
            *pending = Some(prepared);
            Ok(None)
        }
        Err(WorkflowError::Busy) => {
            model.overlay = Overlay::Busy;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn commit_target_save(
    model: &mut Model,
    pending: &mut Option<PreparedTargetSave>,
) -> Result<Option<u8>, WorkflowError> {
    let Some(prepared) = pending.take() else {
        return Ok(None);
    };
    let authorization = if prepared.plan().has_guarded() {
        Authorization::AllGuarded
    } else {
        Authorization::SafeOnly
    };
    let report = TargetWorkflow::commit_save(prepared, authorization)?;
    if report.status == ReportStatus::InSync {
        Ok(Some(0))
    } else {
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
        Ok(None)
    }
}

fn target_rows(
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
    observed: &ObservedState,
) -> Vec<Vec<Row>> {
    config
        .skill_directories()
        .iter()
        .map(|directory| rows_for_directory(directory, config, library, observed))
        .collect()
}

fn rows_for_directory(
    directory: &SkillDirectoryConfig,
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
    observed: &ObservedState,
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
        rows.push(Row::skill_inventory(SkillInventoryRow {
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
        }));
    }
    rows
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
                source_row.check = Some(CheckState::Invalid);
            }
            rows.push(source_row);
            for skill in source.skills() {
                let inventory_id = source_inventory_id(index, source.relative_path());
                rows.push(Row::skill_inventory(SkillInventoryRow {
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
                }));
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
    LibraryConfig::new(locations).map_err(|issues| WorkflowError::InvalidInput {
        message: issues
            .into_iter()
            .map(|issue| format!("{}: {}", issue.path, issue.message))
            .collect::<Vec<_>>()
            .join("; "),
    })
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
}
