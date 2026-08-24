//! Terminal reducer, rendering, and lifecycle.

use crate::acquisition::{LibraryAcquisition, LibraryAcquisitionMode};
use crate::app::{
    AppPaths, LibraryWorkflow, PreparedTargetSave, PreparedUserScopeSave, ReportStatus,
    TargetSession, TargetWorkflow, UserScopeSession, UserScopeWorkflow, WorkflowError,
};
use crate::config::{LibraryConfig, LibraryLocationConfig, RepositoryConfig, SkillDirectoryConfig};
use crate::domain::{
    Enablement, MaterializationKind, RepositoryRelativePath, SkillDirectoryKey, SkillKey,
    SkillPath, SourceKey,
};
use crate::library::{LibrarySnapshot, SkillValidity, validated_skill_metadata_at};
use crate::reconcile::Authorization;
use crate::target::{
    CONTROL_FILE_EXCEPTIONS, MaterializationState, ObservedState, RepositorySkillExceptions,
    Target, control_file_path, observe, repository_tracking_rule,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, Paragraph, Row as TableRow, Table, TableState,
};
use ratatui::{Frame, Terminal};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Stdout;
use std::path::{Path, PathBuf};

const PURPLE: Color = Color::Indexed(99);
const BLUE: Color = Color::Indexed(33);
const BONE: Color = Color::Indexed(230);
const ADD: Color = Color::Indexed(114);
const MODIFY: Color = Color::Indexed(45);
const WARNING: Color = Color::Indexed(220);
const ERROR: Color = Color::Indexed(196);
const DIM_FOREGROUND: Color = Color::Indexed(240);
const DARK_MAGENTA: Color = Color::Indexed(90);
const SELECTED_BACKGROUND: Color = Color::Indexed(24);
const TAB_TOP_BORDER: border::Set = border::Set {
    top_left: "▛",
    top_right: "▜",
    horizontal_top: "▀",
    vertical_left: "▌",
    vertical_right: "▐",
    bottom_left: "▙",
    bottom_right: "▟",
    horizontal_bottom: "▄",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Target,
    Library,
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
    Repository,
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
    frontmatter: String,
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
    repository_candidate: bool,
    repository_name: Option<String>,
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
            frontmatter: String::new(),
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
            repository_candidate: false,
            repository_name: None,
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
            frontmatter: String::new(),
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
            repository_candidate: false,
            repository_name: None,
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
            frontmatter: String::new(),
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
            repository_candidate: false,
            repository_name: None,
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
            frontmatter: String::new(),
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
            repository_candidate: false,
            repository_name: None,
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
        _registered: bool,
        location_index: usize,
        source_path: String,
        available: bool,
        key_collision: bool,
    ) -> Self {
        let mut row = Self::source(name, check);
        row.registered = Some(true);
        row.state = String::new();
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
        row.initial_mode = inventory.mode;
        row.skill_path = Some(inventory.path);
        row.location_index = inventory.location_index;
        row.inventory_id = inventory.inventory_id;
        row.valid = inventory.valid;
        row.details = inventory.details;
        row
    }

    fn repository_skill(
        repository_name: String,
        name: String,
        description: String,
        path: &Path,
        tracked: bool,
        excepted: bool,
    ) -> Self {
        let check = if tracked || excepted {
            CheckState::Repository
        } else {
            CheckState::Unchecked
        };
        let mut row = Self::skill_inventory(SkillInventoryRow {
            group: "Repository".to_owned(),
            inventory_id: None,
            path: repository_name.clone(),
            name: name.clone(),
            description,
            check,
            available: true,
            valid: true,
            mode: None,
            state: "Repository".to_owned(),
            details: path.display().to_string(),
            location_index: None,
        });
        row.repository_candidate = true;
        row.repository_name = Some(repository_name);
        row.initial_check = Some(check);
        if tracked && !excepted {
            row.action = "Track in repository".to_owned();
            row.initial_action = row.action.clone();
        }
        row.frontmatter = std::fs::read_to_string(path.join("SKILL.md")).unwrap_or_default();
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
    Welcome,
    Help,
    Filter,
    ConfirmSave,
    ConfirmSaveWarning(String),
    GuardedConfirmation(String),
    ConfirmLibrarySwitch,
    DiscardWorkspace,
    DiscardTarget,
    SwitchScope {
        from: usize,
        to: usize,
    },
    DirectoryEditor {
        edit: bool,
        input: String,
    },
    LocationEditor {
        edit: bool,
        input: String,
    },
    SourceKeyEditor(String),
    TargetPicker(String),
    ConfirmDelete,
    Busy,
    Details {
        title: String,
        path: String,
        document: String,
    },
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
    detail_scroll: u16,
    exit_after_save: bool,
    dirty: bool,
    directory_index: usize,
    directory_count: usize,
    directory_labels: Vec<String>,
    directory_values: Vec<String>,
    directory_paths: Vec<String>,
    directory_scopes: Vec<TargetTabScope>,
    target_path: Option<String>,
}

impl Model {
    pub fn new(workspace: Workspace, rows: Vec<Row>) -> Self {
        let selected = rows
            .iter()
            .position(|row| row.kind != RowKind::Diagnostic)
            .unwrap_or(0);
        Self {
            workspace,
            rows,
            selected,
            collapsed: BTreeSet::new(),
            filter: String::new(),
            overlay: Overlay::None,
            detail_scroll: 0,
            exit_after_save: false,
            dirty: false,
            directory_index: 0,
            directory_count: 1,
            directory_labels: Vec::new(),
            directory_values: Vec::new(),
            directory_paths: Vec::new(),
            directory_scopes: Vec::new(),
            target_path: None,
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
                RowKind::Diagnostic => None,
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
        } else if checks.iter().all(|state| *state == CheckState::Repository) {
            CheckState::Repository
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
    PageDown,
    PageUp,
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
    CompletePath,
    Escape,
    Save { fast: bool },
    Quit,
    ChangeTarget,
    AddDirectory,
    NewTargetTab,
    EditDirectory,
    DeleteDirectory,
    ToggleWorkspace,
    Undo,
    Help,
    RefreshLibrary,
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
    SaveLibraryAndToggle,
    Undo,
    ApplyDirectoryEdit { edit: bool, value: String },
    ApplyLocationEdit { edit: bool, value: String },
    RefreshLibrary,
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
    if matches!(model.overlay, Overlay::Notice(_)) {
        model.overlay = Overlay::None;
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
            (Overlay::LocationEditor { input, .. }, Action::CompletePath)
            | (Overlay::TargetPicker(input), Action::CompletePath) => {
                if let Some(completed) = complete_path(input) {
                    *input = completed;
                }
                return Vec::new();
            }
            (Overlay::Details { .. } | Overlay::Help, Action::MoveDown) => {
                model.detail_scroll = model.detail_scroll.saturating_add(1);
                return Vec::new();
            }
            (Overlay::Details { .. } | Overlay::Help, Action::MoveUp) => {
                model.detail_scroll = model.detail_scroll.saturating_sub(1);
                return Vec::new();
            }
            (Overlay::Details { .. } | Overlay::Help, Action::PageDown) => {
                model.detail_scroll = model.detail_scroll.saturating_add(10);
                return Vec::new();
            }
            (Overlay::Details { .. } | Overlay::Help, Action::PageUp) => {
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
            Action::Confirm if model.overlay == Overlay::ConfirmLibrarySwitch => {
                model.overlay = Overlay::None;
                return vec![Effect::SaveLibraryAndToggle];
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
            Action::Confirm if matches!(model.overlay, Overlay::Details { .. }) => {
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
                && row.available
            {
                if model.workspace == Workspace::Target && row.repository_candidate {
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
        Action::Save { fast } => {
            model.exit_after_save = fast;
            return vec![Effect::PrepareSave { fast }];
        }
        Action::Undo if model.dirty => return vec![Effect::Undo],
        Action::Quit => return vec![Effect::Quit { status: 0 }],
        Action::ChangeTarget => {
            model.overlay = if model.dirty {
                Overlay::DiscardTarget
            } else {
                Overlay::TargetPicker(String::new())
            };
        }
        Action::NewTargetTab if model.workspace != Workspace::Target => {}
        Action::AddDirectory | Action::NewTargetTab => {
            model.overlay = if model.workspace == Workspace::Library {
                Overlay::LocationEditor {
                    edit: false,
                    input: String::new(),
                }
            } else {
                Overlay::DirectoryEditor {
                    edit: false,
                    input: if action == Action::NewTargetTab {
                        ".claude".to_owned()
                    } else {
                        String::new()
                    },
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
                model.overlay = if model.workspace == Workspace::Library {
                    Overlay::ConfirmLibrarySwitch
                } else {
                    Overlay::DiscardWorkspace
                };
            } else {
                return vec![Effect::ToggleWorkspace];
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
                        "Save or discard staged Library changes before refreshing.".to_owned(),
                    );
                } else {
                    return vec![Effect::RefreshLibrary];
                }
            }
        }
        Action::Confirm => {
            if let Some(row) = model
                .selected_row()
                .filter(|row| row.kind == RowKind::Skill)
            {
                model.overlay = Overlay::Details {
                    title: row.name.clone(),
                    path: row.details.clone(),
                    document: row.frontmatter.clone(),
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
                "This Skill is enabled in User Scope. Manage it from the User tab.".to_owned(),
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
        && row.acquisition_pending
    {
        match mode {
            LibraryAcquisitionMode::Move => "Move to Library".to_owned(),
            LibraryAcquisitionMode::Copy => "Copy to Library".to_owned(),
            LibraryAcquisitionMode::Link => "Link to Library".to_owned(),
        }
    } else {
        row.initial_action.clone()
    };
}

fn refresh_library_visibility(row: &mut Row) {
    row.action = if row.check != row.initial_check {
        if row.check == Some(CheckState::Checked) {
            "Show in Targets".to_owned()
        } else {
            "Hide from Targets".to_owned()
        }
    } else {
        row.initial_action.clone()
    };
}

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
        (KeyCode::Char('l'), true) => Some(Action::ToggleWorkspace),
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

fn dim_style() -> Style {
    Style::default().fg(DIM_FOREGROUND)
}

fn dim_span(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), dim_style())
}

fn complete_path(input: &str) -> Option<String> {
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

fn row_cells(
    row: &Row,
    check: &str,
    mode: &str,
    collapsed: &BTreeSet<String>,
    filter: &str,
    last_child: bool,
    selected: bool,
) -> Vec<Cell<'static>> {
    let subdued = Style::default().fg(if selected {
        Color::Indexed(7)
    } else {
        DIM_FOREGROUND
    });
    if row.kind == RowKind::Location {
        return vec![
            Cell::from(Span::styled("─────────", subdued)),
            Cell::from(Span::styled("──────", subdued)),
            Cell::from(Line::from(vec![
                Span::styled("── ", subdued),
                Span::raw(row.name.clone()),
                Span::styled(" ", subdued),
            ])),
            Cell::from(Span::styled(
                "────────────────────────────────────────────────────────────────",
                subdued,
            )),
            Cell::from(Span::styled("────────────────────────────────", subdued)),
        ];
    }

    let check = if row.check == Some(CheckState::Unchecked) {
        Cell::from(Span::styled(check.to_owned(), subdued))
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
                Span::styled(format!("{glyph} "), subdued),
                Span::raw(row.name.clone()),
            ]))
        }
        RowKind::Skill => Cell::from(Line::from(vec![
            Span::styled(if last_child { "  └─ " } else { "  ├─ " }, subdued),
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
    } else if row_is_conflict(row) {
        Style::default().fg(WARNING)
    } else if row.check == Some(CheckState::User) || !row.available {
        dim_style()
    } else if let Some(color) = pending_action_color(&row.action) {
        Style::default().fg(color)
    } else {
        Style::default()
    };
    if selected {
        style = Style::default()
            .fg(Color::Indexed(15))
            .bg(SELECTED_BACKGROUND);
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

fn row_is_conflict(row: &Row) -> bool {
    row.kind == RowKind::Diagnostic
        && [
            row.name.as_str(),
            row.description.as_str(),
            row.state.as_str(),
        ]
        .into_iter()
        .any(|value| contains_any(value, &["conflict", "guarded", "collision"]))
}

fn pending_action_color(action: &str) -> Option<Color> {
    if action.is_empty() {
        return None;
    }

    if action == "Disable" || action.starts_with("Unregister") {
        return Some(ERROR);
    }
    if action.starts_with("Move to")
        || action.starts_with("Convert to")
        || action.starts_with("Repair")
    {
        return Some(MODIFY);
    }
    if action.starts_with("Enable")
        || action == "Register"
        || action.starts_with("Register ")
        || action.starts_with("Track ")
        || action.starts_with("Copy to")
        || action.starts_with("Link to")
    {
        return Some(ADD);
    }

    None
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    let value = value.to_ascii_lowercase();
    needles.iter().any(|needle| value.contains(needle))
}

fn footer_help(workspace: Workspace) -> Line<'static> {
    let entries: &[(&str, &str)] = match workspace {
        Workspace::Target => &[
            ("s", "save"),
            ("Ctrl+S", "save & exit"),
            ("u", "undo"),
            ("Space", "toggle"),
            ("m", "mode"),
            ("/", "filter"),
            ("Ctrl+T", "new tab"),
            ("?", "help"),
        ],
        Workspace::Library => &[
            ("s", "save"),
            ("Ctrl+S", "save & exit"),
            ("u", "undo"),
            ("Space", "show/hide"),
            ("a/e/d", "location"),
            ("m", "mode"),
            ("/", "filter"),
            ("r", "refresh"),
            ("?", "help"),
        ],
    };
    let mut spans = vec![Span::raw(" ")];
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
    spans.push(Span::raw(" "));
    Line::from(spans)
}

pub fn render(frame: &mut Frame<'_>, model: &Model) {
    let areas = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .split(frame.area());
    let (workspace_label, workspace_color) = match model.workspace {
        Workspace::Target => ("Target", PURPLE),
        Workspace::Library => ("Library", BLUE),
    };
    let mut title = vec![
        Span::styled(
            "Skillator",
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" - "),
        Span::styled(
            workspace_label,
            Style::default()
                .fg(workspace_color)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    let target_path = if model.workspace == Workspace::Target
        && model.directory_scopes.get(model.directory_index) == Some(&TargetTabScope::User)
    {
        model
            .directory_paths
            .get(model.directory_index)
            .map(|path| format!("~/{path}"))
    } else {
        model.target_path.clone()
    };
    if let Some(path) = target_path {
        title[2] = Span::styled(
            "Target:",
            Style::default()
                .fg(workspace_color)
                .add_modifier(Modifier::BOLD),
        );
        title.push(Span::raw(" "));
        title.push(Span::styled(path, Style::default().fg(BONE)));
    }
    frame.render_widget(Paragraph::new(Line::from(title)), areas[0]);
    if model.workspace == Workspace::Target {
        frame.render_widget(Paragraph::new(target_tabs(model)), areas[1]);
    }
    let header = match model.workspace {
        Workspace::Target => TableRow::new(["", "Mode", "Skill", "Description", "Action"]),
        Workspace::Library => TableRow::new(["", "Mode", "Location", "Description", "Action"]),
    }
    .style(Style::default().fg(BONE).add_modifier(Modifier::BOLD));
    let visible = model.visible_indices();
    let rows = visible.iter().map(|index| {
        let row = &model.rows[*index];
        let selected = *index == model.selected;
        let check = match row.check {
            Some(CheckState::Checked) => "[✓]",
            Some(CheckState::User) => "[u]",
            Some(CheckState::Repository) => "[r]",
            Some(CheckState::Unchecked) => "[ ]",
            Some(CheckState::Mixed) => "[-]",
            Some(CheckState::Invalid) => "[!]",
            None => "",
        };
        let mode = if row.check == Some(CheckState::User) {
            "user"
        } else if row.check == Some(CheckState::Repository) {
            "repo"
        } else if let Some(mode) = row.acquisition_mode {
            mode.label()
        } else {
            match row.mode {
                Some(MaterializationKind::Linked) => "link",
                Some(MaterializationKind::Copied) => "copy",
                None => "",
            }
        };
        let last_child = row.kind == RowKind::Skill
            && !model.rows.iter().skip(*index + 1).any(|candidate| {
                candidate.kind == RowKind::Skill && row_identity(candidate) == row_identity(row)
            });
        TableRow::new(row_cells(
            row,
            check,
            mode,
            &model.collapsed,
            &model.filter,
            last_child,
            selected,
        ))
        .style(row_style(row, selected))
    });
    let widths = [
        Constraint::Length(4),
        Constraint::Length(6),
        Constraint::Percentage(28),
        Constraint::Min(1),
        Constraint::Length(18),
    ];
    let key_help = footer_help(model.workspace).right_aligned();
    let border_color = match model.workspace {
        Workspace::Target => PURPLE,
        Workspace::Library => BLUE,
    };
    let mut table_state = TableState::default();
    table_state.select(visible.iter().position(|index| *index == model.selected));
    frame.render_stateful_widget(
        Table::new(rows, widths).header(header).block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(TAB_TOP_BORDER)
                .border_style(Style::default().fg(border_color))
                .title_bottom(key_help),
        ),
        areas[2],
        &mut table_state,
    );
    frame.render_widget(Paragraph::new(status_line(model)), areas[3]);
    render_overlay(frame, model);
}

fn target_tabs(model: &Model) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (index, label) in model.directory_labels.iter().enumerate() {
        if index > 0 {
            let separates_scopes = model.directory_scopes.get(index - 1)
                == Some(&TargetTabScope::User)
                && model.directory_scopes.get(index) == Some(&TargetTabScope::Repository);
            spans.push(if separates_scopes {
                Span::styled(" | ", dim_style())
            } else {
                Span::raw(" ")
            });
        }
        let style = if index == model.directory_index {
            Style::default()
                .fg(BONE)
                .bg(PURPLE)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM_FOREGROUND)
        };
        spans.push(Span::styled(format!(" {label} "), style));
    }
    Line::from(spans)
}

fn status_line(model: &Model) -> Line<'static> {
    if let Overlay::Notice(message) = &model.overlay {
        let style = if contains_any(
            message,
            &[
                "error",
                "invalid",
                "blocked",
                "failed",
                "recovery required",
                "collision",
            ],
        ) {
            Style::default().fg(ERROR)
        } else {
            Style::default().fg(WARNING)
        };
        return Line::from(Span::styled(format!("! {message}"), style));
    }

    let diagnostics = model
        .rows
        .iter()
        .filter(|row| row.kind == RowKind::Diagnostic)
        .map(|row| row.description.as_str())
        .collect::<Vec<_>>();
    if !diagnostics.is_empty() {
        let style = if model
            .rows
            .iter()
            .filter(|row| row.kind == RowKind::Diagnostic)
            .any(row_is_error)
        {
            Style::default().fg(ERROR)
        } else {
            Style::default().fg(WARNING)
        };
        return Line::from(Span::styled(
            format!("! {}", diagnostics.join(" · ")),
            style,
        ));
    }

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
    if model.dirty {
        let mut spans = vec![Span::styled(
            "Staged changes",
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        )];
        if !inspector.spans.is_empty() {
            spans.push(dim_span(" · "));
            spans.extend(inspector.spans);
        }
        Line::from(spans)
    } else {
        inspector
    }
}

fn render_overlay(frame: &mut Frame<'_>, model: &Model) {
    let (title, body, footer, confirmation) = match &model.overlay {
        Overlay::None => return,
        Overlay::Welcome => (
            "I AM SKILLATOR!".to_owned(),
            "Welcome to skillator, before we can manage target skills, please configure your skills library. When complete use Ctrl+L to switch to target view to assign skills to this repo.".to_owned(),
            Some("Enter OK · Esc Exit".to_owned()),
            false,
        ),
        Overlay::Help => return render_help(frame, model.workspace, model.detail_scroll),
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
        Overlay::ConfirmLibrarySwitch => (
            "Save Library Changes".to_owned(),
            "Save staged Library changes before switching to Target?".to_owned(),
            Some("Enter save and switch · Esc cancel".to_owned()),
            true,
        ),
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
            let mode = if *edit { "Edit" } else { "New" };
            return render_input(
                frame,
                &format!("{mode} Target Tab"),
                "agents | .claude | key,path,label",
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
        Overlay::Details {
            title,
            path,
            document,
        } => {
            return render_skill_details(frame, title, path, document, model.detail_scroll);
        }
        Overlay::Notice(_) => return,
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

fn render_help(frame: &mut Frame<'_>, workspace: Workspace, scroll: u16) {
    let mut entries = vec![
        ("Navigation", None),
        ("j/k · ↑/↓", Some("Move by row")),
        ("J/K · ⇧↑/⇧↓", Some("Move by source")),
        ("h/l · ←/→", Some("Collapse / expand a source")),
        ("PgUp/PgDn", Some("Page through the list")),
        ("/", Some("Filter (use /pending for staged actions)")),
    ];
    match workspace {
        Workspace::Target => entries.extend([
            ("Skills", None),
            ("Space", Some("Enable / disable")),
            ("m", Some("Cycle modes: link / copy / repo")),
            ("Target tabs", None),
            ("Ctrl+T", Some("Add target tab")),
            ("t", Some("Change repository")),
            ("a / e / d", Some("Add / edit / delete target tab")),
            ("Commands", None),
            ("Ctrl+L", Some("Switch to Library")),
            ("s", Some("Save")),
            ("Ctrl+S", Some("Save and exit")),
            ("u", Some("Undo staged edits")),
            ("q", Some("Quit; close overlays like Esc")),
        ]),
        Workspace::Library => entries.extend([
            ("Library", None),
            ("Space", Some("Show / hide in Target")),
            ("m", Some("Cycle modes: move / copy / link / none")),
            ("a / e / d", Some("Add / edit / delete location")),
            ("r", Some("Refresh locations")),
            ("Commands", None),
            ("Ctrl+L", Some("Switch to Target")),
            ("s", Some("Save")),
            ("Ctrl+S", Some("Save and exit")),
            ("u", Some("Undo staged edits")),
            ("q", Some("Quit; close overlays like Esc")),
        ]),
    }
    let rows = entries.into_iter().map(|(key, action)| match action {
        Some(action) => TableRow::new(vec![
            Cell::from(Span::styled(
                key.to_owned(),
                Style::default().fg(BONE).add_modifier(Modifier::BOLD),
            )),
            Cell::from(action),
        ]),
        None => TableRow::new(vec![
            Cell::from(Span::styled(
                key.to_owned(),
                Style::default().fg(MODIFY).add_modifier(Modifier::BOLD),
            )),
            Cell::default(),
        ]),
    });
    let area = centered(frame.area(), 70, 72);
    frame.render_widget(Clear, area);
    let mut state = TableState::default().with_offset(usize::from(scroll));
    frame.render_stateful_widget(
        Table::new(rows, [Constraint::Length(18), Constraint::Min(1)])
            .header(
                TableRow::new(["Key", "Action"])
                    .style(Style::default().fg(BONE).add_modifier(Modifier::BOLD)),
            )
            .column_spacing(1)
            .block(modal_block(
                "Help",
                Some("j/k scroll · PgUp/PgDn page · q/Esc close"),
            )),
        area,
        &mut state,
    );
}

fn render_skill_details(
    frame: &mut Frame<'_>,
    title: &str,
    path: &str,
    document: &str,
    scroll: u16,
) {
    let mut lines = vec![Line::from(Span::styled(
        path.to_owned(),
        Style::default().fg(DIM_FOREGROUND),
    ))];
    if document.is_empty() {
        lines.push(Line::from(
            "No readable SKILL.md is available for this Skill.",
        ));
    } else {
        let mut frontmatter = false;
        let mut fenced_code = false;
        for (index, line) in document.lines().enumerate() {
            if index == 0 && line == "---" {
                frontmatter = true;
                lines.push(Line::raw(line.to_owned()));
            } else if frontmatter && line == "---" {
                frontmatter = false;
                lines.push(Line::raw(line.to_owned()));
            } else if frontmatter {
                if let Some((key, value)) = line.split_once(':') {
                    lines.push(Line::from(vec![
                        Span::styled(key.to_owned(), Style::default().fg(MODIFY)),
                        Span::raw(":"),
                        Span::raw(value.to_owned()),
                    ]));
                } else {
                    lines.push(Line::raw(line.to_owned()));
                }
            } else {
                lines.push(markdown_detail_line(line, &mut fenced_code));
            }
        }
    }
    let area = centered(frame.area(), 76, 70);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(ratatui::widgets::Wrap { trim: true })
            .scroll((scroll, 0))
            .block(modal_block(
                &format!("Skill Details: {title}"),
                Some("j/k or ↑/↓ scroll · PgUp/PgDn page · Enter/Esc close"),
            )),
        area,
    );
}

fn markdown_detail_line(line: &str, fenced_code: &mut bool) -> Line<'static> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        *fenced_code = !*fenced_code;
        return Line::from(Span::styled(line.to_owned(), Style::default().fg(ADD)));
    }
    if *fenced_code {
        return Line::from(Span::styled(line.to_owned(), Style::default().fg(ADD)));
    }
    if let Some((prefix, content)) = markdown_heading(line) {
        let mut spans = vec![Span::styled(prefix, Style::default().fg(MODIFY))];
        spans.extend(markdown_inline_spans(
            content,
            Style::default().fg(BONE).add_modifier(Modifier::BOLD),
        ));
        return Line::from(spans);
    }
    if let Some((prefix, content)) = markdown_quote(line) {
        let mut spans = vec![Span::styled(prefix, dim_style())];
        spans.extend(markdown_inline_spans(content, Style::default()));
        return Line::from(spans);
    }
    if let Some((prefix, content, numbered)) = markdown_bullet(line) {
        let marker_style = if numbered {
            Style::default().fg(DARK_MAGENTA)
        } else {
            dim_style()
        };
        let mut spans = vec![Span::styled(prefix, marker_style)];
        spans.extend(markdown_inline_spans(content, Style::default()));
        return Line::from(spans);
    }
    Line::from(markdown_inline_spans(line, Style::default()))
}

fn markdown_heading(line: &str) -> Option<(String, &str)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let hashes = rest.bytes().take_while(|byte| *byte == b'#').count();
    (hashes > 0 && rest.as_bytes().get(hashes) == Some(&b' ')).then(|| {
        (
            format!("{}{} ", &line[..indent], "#".repeat(hashes)),
            &rest[hashes + 1..],
        )
    })
}

fn markdown_quote(line: &str) -> Option<(String, &str)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    rest.strip_prefix("> ")
        .map(|content| (format!("{}> ", &line[..indent]), content))
}

fn markdown_bullet(line: &str) -> Option<(String, &str, bool)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    if let Some((marker, content)) = ["- ", "* ", "+ "]
        .iter()
        .find_map(|marker| rest.strip_prefix(marker).map(|content| (*marker, content)))
    {
        return Some((format!("{}{marker}", &line[..indent]), content, false));
    }
    let digits = rest
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    let marker_end = digits + 1;
    (digits > 0
        && matches!(rest.as_bytes().get(digits), Some(b'.' | b')'))
        && rest.as_bytes().get(marker_end) == Some(&b' '))
    .then(|| {
        (
            format!("{}{}", &line[..indent], &rest[..=marker_end]),
            &rest[marker_end + 1..],
            true,
        )
    })
}

fn markdown_inline_spans(text: &str, normal: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remainder = text;
    while !remainder.is_empty() {
        if let Some(after_tick) = remainder.strip_prefix('`')
            && let Some(end) = after_tick.find('`')
        {
            let code = &after_tick[..end];
            spans.push(Span::styled(
                format!("`{code}`"),
                Style::default().fg(PURPLE),
            ));
            remainder = &after_tick[end + 1..];
            continue;
        }
        if let Some(after_open) = remainder.strip_prefix('[')
            && let Some(label_end) = after_open.find("](")
        {
            let label = &after_open[..label_end];
            let after_label = &after_open[label_end + 2..];
            if let Some(url_end) = after_label.find(')') {
                spans.push(Span::styled("[", dim_style()));
                spans.push(Span::styled(label.to_owned(), Style::default().fg(BLUE)));
                spans.push(Span::styled("](", dim_style()));
                spans.push(Span::styled(after_label[..url_end].to_owned(), dim_style()));
                spans.push(Span::styled(")", dim_style()));
                remainder = &after_label[url_end + 1..];
                continue;
            }
        }
        if let Some(after_bold) = remainder
            .strip_prefix("**")
            .or_else(|| remainder.strip_prefix("__"))
        {
            let delimiter = if remainder.starts_with("**") {
                "**"
            } else {
                "__"
            };
            if let Some(end) = after_bold.find(delimiter) {
                spans.push(Span::styled(delimiter, dim_style()));
                spans.push(Span::styled(
                    after_bold[..end].to_owned(),
                    Style::default()
                        .fg(Color::Indexed(15))
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(delimiter, dim_style()));
                remainder = &after_bold[end + delimiter.len()..];
                continue;
            }
        }
        if !remainder.starts_with("**")
            && !remainder.starts_with("__")
            && let Some(after_italic) = remainder
                .strip_prefix('*')
                .or_else(|| remainder.strip_prefix('_'))
        {
            let delimiter = if remainder.starts_with('*') { "*" } else { "_" };
            if let Some(end) = after_italic.find(delimiter) {
                spans.push(Span::styled(delimiter, dim_style()));
                spans.push(Span::styled(
                    after_italic[..end].to_owned(),
                    Style::default()
                        .fg(DIM_FOREGROUND)
                        .add_modifier(Modifier::ITALIC),
                ));
                spans.push(Span::styled(delimiter, dim_style()));
                remainder = &after_italic[end + delimiter.len()..];
                continue;
            }
        }
        let next = remainder
            .char_indices()
            .find_map(|(index, character)| {
                matches!(character, '`' | '[' | '*' | '_').then_some(index)
            })
            .filter(|index| *index > 0)
            .unwrap_or(remainder.len());
        spans.push(Span::styled(remainder[..next].to_owned(), normal));
        remainder = &remainder[next..];
    }
    spans
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
            if line.starts_with("• Guarded") || contains_any(line, &["warning", "conflict"]) {
                return Line::styled(line.to_owned(), Style::default().fg(WARNING));
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
        Overlay::Result(message)
            if !contains_any(
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
            Style::default().fg(ADD)
        }
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
        Paragraph::new(Text::from(vec![
            Line::raw(hint),
            Line::from(vec![
                Span::raw("> "),
                Span::raw(input),
                Span::styled("▌", Style::default().fg(BONE)),
            ]),
        ]))
        .style(Style::default().fg(BONE))
        .block(modal_block(
            title,
            Some("Tab complete · Enter apply · Esc cancel"),
        )),
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
            Self::Repository(prepared) => {
                TargetWorkflow::commit_save_registered(paths, prepared, authorization)
            }
        }
    }
}

fn navigate(paths: &AppPaths, mut navigation: Navigation) -> Result<u8, WorkflowError> {
    let mut session = TerminalSession::new()?;
    loop {
        navigation = match navigation {
            Navigation::Exit(status) => return Ok(status),
            Navigation::Target(directory) => {
                run_target_once(paths, &directory, &mut session.terminal)?
            }
            Navigation::Library { return_target } => {
                run_library_once(paths, return_target.as_deref(), &mut session.terminal)?
            }
        };
    }
}

fn initial_library_model(paths: &AppPaths, session: &crate::app::LibrarySession) -> Model {
    let snapshot = LibraryWorkflow::snapshot(paths, &session.config);
    let mut model = Model::new(Workspace::Library, library_rows(&session.config, &snapshot));
    if session.first_run {
        model.overlay = Overlay::Welcome;
    }
    model
}

fn run_library_once(
    paths: &AppPaths,
    return_target: Option<&Path>,
    terminal: &mut AppTerminal,
) -> Result<Navigation, WorkflowError> {
    let session = match LibraryWorkflow::load(paths) {
        Ok(session) => session,
        Err(error @ WorkflowError::InvalidInput { .. }) => {
            return run_static(
                terminal,
                Model::new(Workspace::Library, vec![Row::diagnostic(error.to_string())]),
                3,
            )
            .map(Navigation::Exit);
        }
        Err(error) => return Err(error),
    };
    let mut working_config = session.config.clone();
    let model = initial_library_model(paths, &session);
    let mut staged: Option<(LibraryConfig, Vec<LibraryAcquisition>)> = None;
    let mut target_to_open = None;
    let status = run_interactive(terminal, model, |model, effect| match effect {
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
            Ok(Some(if model.exit_after_save { 0 } else { 253 }))
        }
        Effect::CancelSave => Ok(None),
        Effect::Undo => Ok(Some(253)),
        Effect::SaveLibraryAndToggle => {
            let config = library_config_from_rows(&working_config, &model.rows)?;
            let acquisitions = library_acquisitions_from_rows(&model.rows);
            LibraryWorkflow::save_with_acquisitions(paths, &session, &config, &acquisitions, true)?;
            if return_target.is_some() {
                Ok(Some(251))
            } else {
                model.overlay = Overlay::TargetPicker(String::new());
                Ok(None)
            }
        }
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
                );
            } else {
                locations.push(LibraryLocationConfig::new(
                    value.to_owned(),
                    Vec::new(),
                    false,
                ));
            }
            working_config = LibraryConfig::new(locations).map_err(config_issues)?;
            let snapshot = LibraryWorkflow::snapshot(paths, &working_config);
            model.rows = library_rows(&working_config, &snapshot);
            model.selected = model.selected.min(model.rows.len().saturating_sub(1));
            model.dirty = true;
            Ok(None)
        }
        Effect::RefreshLibrary => {
            let snapshot = LibraryWorkflow::snapshot(paths, &working_config);
            model.rows = library_rows(&working_config, &snapshot);
            model.selected = model.selected.min(model.rows.len().saturating_sub(1));
            Ok(None)
        }
        Effect::ApplySourceKey(_value) => {
            model.overlay = Overlay::Notice(
                "Source keys are derived from the discovered Location and cannot be edited."
                    .to_owned(),
            );
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
        253 => Ok(Navigation::Library {
            return_target: return_target.map(Path::to_owned),
        }),
        status => Ok(Navigation::Exit(status)),
    }
}

fn run_target_once(
    paths: &AppPaths,
    directory: &Path,
    terminal: &mut AppTerminal,
) -> Result<Navigation, WorkflowError> {
    let library_session = match LibraryWorkflow::load(paths) {
        Ok(session) => session,
        Err(error @ WorkflowError::InvalidInput { .. }) => {
            return run_static(
                terminal,
                Model::new(Workspace::Target, vec![Row::diagnostic(error.to_string())]),
                3,
            )
            .map(Navigation::Exit);
        }
        Err(error) => return Err(error),
    };
    if library_session.first_run {
        return Ok(Navigation::Library {
            return_target: Some(directory.to_owned()),
        });
    }
    let repository_session = match TargetWorkflow::load(directory) {
        Ok(session) => session,
        Err(error @ WorkflowError::InvalidInput { .. }) => {
            return run_static(
                terminal,
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
                terminal,
                Model::new(Workspace::Target, vec![Row::diagnostic(error.to_string())]),
                3,
            )
            .map(Navigation::Exit);
        }
        Err(error) => return Err(error),
    };
    let mut state = build_target_state(
        user_session,
        repository_session,
        &library,
        &library_session.config,
    );
    let mut dirty_scopes = BTreeSet::new();
    let mut model = initial_target_model(&state.tabs);
    model.target_path = Some(user_relative_path(
        state.repository.target.root(),
        paths.home(),
    ));
    let mut pending: Option<PreparedScopeSave> = None;
    let mut switch_after_save: Option<(TargetTabScope, String)> = None;
    let mut target_to_open = None;
    let status = run_interactive(terminal, model, |model, effect| match effect {
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
                state = build_target_state(user, repository, &library, &library_session.config);
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
                Ok(Some(if model.exit_after_save { 0 } else { 253 }))
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
                state = build_target_state(user, repository, &library, &library_session.config);
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
            state = build_target_state(user, repository, &library, &library_session.config);
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
                let (config, observed, inherited, repository_target) = match scope {
                    TargetTabScope::User => {
                        let observed = observe(&state.user.target, &state.user.config, &library);
                        (&state.user.config, observed, BTreeSet::new(), None)
                    }
                    TargetTabScope::Repository => {
                        let observed =
                            observe(&state.repository.target, &state.repository.config, &library);
                        let inherited = user_enabled_skills(&state.user.config);
                        (
                            &state.repository.config,
                            observed,
                            inherited,
                            Some(&state.repository.target),
                        )
                    }
                };
                state.tabs.push(TargetTab {
                    scope,
                    rows: rows_for_directory(
                        &candidate,
                        config,
                        &library,
                        &library_session.config,
                        &observed,
                        &inherited,
                        repository_target,
                    ),
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
        Effect::SaveLibraryAndToggle => Ok(None),
        Effect::Undo => Ok(Some(253)),
        Effect::ApplyLocationEdit { .. } => Ok(None),
        Effect::RefreshLibrary => Ok(None),
        Effect::ApplySourceKey(_) => Ok(None),
    })?;
    match status {
        250 => Ok(Navigation::Target(
            target_to_open.unwrap_or_else(|| Path::new(".").to_owned()),
        )),
        251 => Ok(Navigation::Library {
            return_target: Some(state.repository.target.root().to_owned()),
        }),
        253 => Ok(Navigation::Target(
            state.repository.target.root().to_owned(),
        )),
        status => Ok(Navigation::Exit(status)),
    }
}

fn build_target_state(
    user: UserScopeSession,
    repository: TargetSession,
    library: &LibrarySnapshot,
    library_config: &LibraryConfig,
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
            library_config,
            &user_observed,
            &BTreeSet::new(),
            None,
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
            library_config,
            &repository_observed,
            &inherited,
            Some(&repository.target),
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
        .path()
        .as_str()
        .split('/')
        .next()
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

fn user_relative_path(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(relative) if relative.as_os_str().is_empty() => "~".to_owned(),
        Ok(relative) => format!("~/{}", relative.display()),
        Err(_) => path.display().to_string(),
    }
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
    model.directory_paths = tabs
        .iter()
        .map(|tab| tab.directory.path().as_str().to_owned())
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

fn repository_skill_exceptions(
    tabs: &[TargetTab],
    scope: TargetTabScope,
) -> RepositorySkillExceptions {
    let mut exceptions = RepositorySkillExceptions::new();
    if scope != TargetTabScope::Repository {
        return exceptions;
    }
    for tab in tabs.iter().filter(|tab| tab.scope == scope) {
        let names = tab
            .rows
            .iter()
            .filter(|row| row.repository_candidate && row.check == Some(CheckState::Repository))
            .filter_map(|row| row.repository_name.clone())
            .collect::<BTreeSet<_>>();
        if !names.is_empty() {
            exceptions.insert(tab.directory.key().as_str().to_owned(), names);
        }
    }
    exceptions
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
        TargetTabScope::Repository => TargetWorkflow::prepare_save_with_repository_skills(
            paths,
            &state.repository,
            staged,
            repository_skill_exceptions(tabs, scope),
        )
        .map(PreparedScopeSave::Repository),
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
        "agents" | ".agents" | "1" => Ok(SkillDirectoryConfig::agents_preset()),
        "claude" | ".claude" | "2" => Ok(SkillDirectoryConfig::claude_preset()),
        custom => {
            let mut fields = custom.splitn(3, ',').map(str::trim);
            let key = fields.next().unwrap_or_default();
            let path = fields.next().unwrap_or_default();
            let label = fields.next().filter(|label| !label.is_empty());
            if key.is_empty() || path.is_empty() {
                return Err("Enter `agents`, `.claude`, or a custom `key,path,label`.".to_owned());
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
    library_config: &LibraryConfig,
    observed: &ObservedState,
    inherited_user: &BTreeSet<SkillKey>,
    repository_target: Option<&Target>,
) -> Vec<Vec<Row>> {
    config
        .skill_directories()
        .iter()
        .map(|directory| {
            rows_for_directory(
                directory,
                config,
                library,
                library_config,
                observed,
                inherited_user,
                repository_target,
            )
        })
        .collect()
}

fn rows_for_directory(
    directory: &SkillDirectoryConfig,
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
    library_config: &LibraryConfig,
    observed: &ObservedState,
    inherited_user: &BTreeSet<SkillKey>,
    repository_target: Option<&Target>,
) -> Vec<Row> {
    let mut skills: BTreeMap<(String, String), (String, String, bool)> = BTreeMap::new();
    for source in library.sources() {
        for skill in source
            .skills()
            .filter(|skill| skill.validity() == SkillValidity::Valid)
            .filter(|skill| {
                library_config.is_visible(&SkillKey::new(
                    source.key().clone(),
                    SkillPath::parse(skill.path()).expect("discovered Skill path is valid"),
                ))
            })
        {
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
    let directory_observation = observed
        .directories()
        .iter()
        .find(|candidate| candidate.key() == directory.key().as_str());
    if let Some(directory_observation) = directory_observation {
        rows.extend(
            directory_observation
                .diagnostics()
                .iter()
                .cloned()
                .map(Row::diagnostic),
        );
    }
    if let (Some(target), Some(observation)) = (repository_target, directory_observation) {
        rows.extend(repository_skill_rows(directory, target, observation));
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
                .map(|source| source.skills().count())
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
        row.frontmatter = library
            .resolve(&skill_key)
            .and_then(|skill| skill.absolute_path())
            .and_then(|path| std::fs::read_to_string(path.join("SKILL.md")).ok())
            .unwrap_or_default();
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

fn repository_skill_rows(
    directory: &SkillDirectoryConfig,
    target: &Target,
    observation: &crate::target::DirectoryObservation,
) -> Vec<Row> {
    let control = target.root().join(control_file_path(directory));
    let exceptions = std::fs::read_to_string(control)
        .ok()
        .and_then(|content| {
            content
                .split_once(CONTROL_FILE_EXCEPTIONS)
                .map(|(_, suffix)| suffix.lines().map(str::to_owned).collect::<BTreeSet<_>>())
        })
        .unwrap_or_default();
    let mut skills = Vec::new();
    for path in observation.unmanaged_entries() {
        let Ok(metadata) = std::fs::symlink_metadata(path) else {
            continue;
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        let Some((name, description)) = validated_skill_metadata_at(path) else {
            continue;
        };
        let Some(repository_name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        let excepted = repository_tracking_rule(directory, &repository_name)
            .is_some_and(|rule| exceptions.contains(&rule));
        let relative = path.strip_prefix(target.root()).unwrap_or(path);
        let tracked = target
            .repository()
            .facts_for(relative)
            .is_ok_and(|facts| facts.tracked);
        skills.push(Row::repository_skill(
            repository_name,
            name,
            description,
            path,
            tracked,
            excepted,
        ));
    }
    if skills.is_empty() {
        return Vec::new();
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    let mut source = Row::source("Repository", CheckState::Unchecked);
    source.description = format!(
        "{} repository-owned skill{}",
        skills.len(),
        if skills.len() == 1 { "" } else { "s" }
    );
    let mut rows = vec![source];
    rows.extend(skills);
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
            let skill_count = source
                .skills()
                .filter(|skill| skill.validity() == SkillValidity::Valid)
                .count();
            let mut source_row = Row::source_inventory(
                source.key().as_str().to_owned(),
                CheckState::Checked,
                true,
                index,
                source.relative_path().to_owned(),
                source.available(),
                source.key_collision(),
            );
            source_row.description = format!(
                "{:?} Source · {} skill{}",
                source.kind(),
                skill_count,
                if skill_count == 1 { "" } else { "s" }
            );
            source_row.details = format!(
                "{:?} Source · root {} · origin {}{}",
                source.kind(),
                source
                    .root()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "unavailable".to_owned()),
                source.origin().unwrap_or("none"),
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
                let skill_file_path = skill.absolute_path().and_then(|path| {
                    snapshot
                        .locations()
                        .get(index)
                        .and_then(|location| location.resolved())
                        .and_then(|root| path.strip_prefix(root).ok())
                        .map(|relative| relative.join("SKILL.md").display().to_string())
                });
                let skill_document = skill
                    .absolute_path()
                    .and_then(|path| std::fs::read_to_string(path.join("SKILL.md")).ok())
                    .unwrap_or_default();
                let inventory_id = source_inventory_id(index, source.relative_path());
                let mut row = Row::skill_inventory(SkillInventoryRow {
                    group: source.key().as_str().to_owned(),
                    inventory_id: Some(inventory_id),
                    path: skill.path().to_owned(),
                    name: skill.name().unwrap_or(skill.path()).to_owned(),
                    description: skill.description().unwrap_or("").to_owned(),
                    check: if skill.validity() == SkillValidity::Invalid {
                        CheckState::Invalid
                    } else if config.is_visible(&SkillKey::new(
                        source.key().clone(),
                        SkillPath::parse(skill.path()).expect("discovered Skill path is valid"),
                    )) {
                        CheckState::Checked
                    } else {
                        CheckState::Unchecked
                    },
                    available: skill.available() && skill.validity() == SkillValidity::Valid,
                    valid: skill.validity() == SkillValidity::Valid,
                    mode: None,
                    state: if skill.available() { "" } else { "Unavailable" }.to_owned(),
                    details: skill_file_path.unwrap_or_else(|| "SKILL.md unavailable".to_owned()),
                    location_index: Some(index),
                });
                row.registered = Some(true);
                let local_destination = index == 0 && source.relative_path() == ".";
                if !local_destination
                    && skill.validity() == SkillValidity::Valid
                    && let Some(path) = skill.absolute_path()
                {
                    row.acquisition_source = Some(path.to_owned());
                    row.acquisition_source_root_git = source.kind()
                        == crate::library::SourceKind::Git
                        && source.root() == Some(path);
                }
                row.frontmatter = skill_document;
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
    let mut hidden = original.hidden_skills().clone();
    for row in rows.iter().filter(|row| row.kind == RowKind::Skill) {
        let (Some(source), Some(path)) = (row.group.as_deref(), row.skill_path.as_deref()) else {
            continue;
        };
        let (Ok(source), Ok(path)) = (SourceKey::parse(source), SkillPath::parse(path)) else {
            continue;
        };
        let skill = SkillKey::new(source, path);
        if row.check == Some(CheckState::Unchecked) {
            hidden.insert(skill);
        } else {
            hidden.remove(&skill);
        }
    }
    original.with_hidden_skills(hidden).map_err(config_issues)
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
    original == staged
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

fn run_static(
    terminal: &mut AppTerminal,
    model: Model,
    failure_status: u8,
) -> Result<u8, WorkflowError> {
    run_interactive(terminal, model, |_model, effect| match effect {
        Effect::Quit { .. } => Ok(Some(failure_status)),
        Effect::PrepareSave { .. } => Ok(None),
        Effect::CancelSave => Ok(None),
        _ => Ok(None),
    })
}

fn run_interactive(
    terminal: &mut AppTerminal,
    mut model: Model,
    mut handle_effect: impl FnMut(&mut Model, Effect) -> Result<Option<u8>, WorkflowError>,
) -> Result<u8, WorkflowError> {
    use crossterm::event::{self, Event};
    loop {
        terminal
            .draw(|frame| render(frame, &model))
            .map_err(fatal)?;
        let Event::Key(key) = event::read().map_err(fatal)? else {
            continue;
        };
        let Some(action) = action_for_model_key(&model, key) else {
            continue;
        };
        for effect in reduce(&mut model, action) {
            if let Some(status) = handle_effect(&mut model, effect)? {
                return Ok(status);
            }
        }
    }
}

fn action_for_model_key(model: &Model, key: KeyEvent) -> Option<Action> {
    if matches!(
        model.overlay,
        Overlay::Filter
            | Overlay::DirectoryEditor { .. }
            | Overlay::LocationEditor { .. }
            | Overlay::SourceKeyEditor(_)
            | Overlay::TargetPicker(_)
    ) && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
    {
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
    fn target_title_uses_a_home_relative_repository_path() {
        assert_eq!(
            user_relative_path(
                Path::new("/Users/ada/Development/skillator"),
                Path::new("/Users/ada")
            ),
            "~/Development/skillator"
        );
        assert_eq!(
            user_relative_path(Path::new("/opt/work/skillator"), Path::new("/Users/ada")),
            "/opt/work/skillator"
        );
    }

    #[test]
    fn skill_detail_markdown_uses_lightweight_structural_highlighting() {
        let inline = markdown_inline_spans(
            "Run `skillator sync` with [the guide](https://example.test).",
            Style::default(),
        );
        assert!(
            inline.iter().any(|span| {
                span.content == "`skillator sync`" && span.style.fg == Some(PURPLE)
            })
        );
        assert!(
            inline
                .iter()
                .any(|span| span.content == "the guide" && span.style.fg == Some(BLUE))
        );
        assert!(inline.iter().any(|span| {
            span.content == "https://example.test" && span.style.fg == Some(DIM_FOREGROUND)
        }));
        let emphasis = markdown_inline_spans("**important** and *optional*", Style::default());
        assert!(emphasis.iter().any(|span| {
            span.content == "important"
                && span.style.fg == Some(Color::Indexed(15))
                && span.style.add_modifier.contains(Modifier::BOLD)
        }));
        assert!(emphasis.iter().any(|span| {
            span.content == "optional"
                && span.style.fg == Some(DIM_FOREGROUND)
                && span.style.add_modifier.contains(Modifier::ITALIC)
        }));
        let mut un_fenced = false;
        let numbered = markdown_detail_line("12. Prepare release", &mut un_fenced);
        assert_eq!(numbered.spans[0].content, "12. ");
        assert_eq!(numbered.spans[0].style.fg, Some(DARK_MAGENTA));

        let mut fenced = false;
        let fence = markdown_detail_line("```sh", &mut fenced);
        assert!(fenced);
        assert_eq!(fence.spans[0].style.fg, Some(ADD));
        let code = markdown_detail_line("skillator sync", &mut fenced);
        assert_eq!(code.spans[0].style.fg, Some(ADD));
        let heading = markdown_detail_line("## Setup", &mut fenced);
        assert_eq!(heading.spans[0].style.fg, Some(ADD));
        markdown_detail_line("```", &mut fenced);
        let heading = markdown_detail_line("## Setup", &mut fenced);
        assert_eq!(heading.spans[0].style.fg, Some(MODIFY));
        assert!(heading.spans[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn target_tabs_use_top_level_paths_and_separate_user_scope() {
        let tabs = vec![
            TargetTab {
                scope: TargetTabScope::User,
                directory: SkillDirectoryConfig::user_preset(),
                rows: Vec::new(),
            },
            TargetTab {
                scope: TargetTabScope::Repository,
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
        model.directory_index = 1;

        assert_eq!(model.directory_labels, ["User", ".agents", ".claude"]);
        let line = target_tabs(&model);
        assert!(line.spans.iter().any(|span| span.content == " | "));
        assert!(
            line.spans
                .iter()
                .any(|span| span.content == " .agents " && span.style.bg == Some(PURPLE))
        );
        assert_eq!(line.spans.first().unwrap().content, " ");
        assert!(
            line.spans
                .iter()
                .any(|span| span.content == " User " && span.style.bg.is_none())
        );
    }

    #[test]
    fn target_header_follows_the_active_scope() {
        let tabs = vec![
            TargetTab {
                scope: TargetTabScope::User,
                directory: SkillDirectoryConfig::user_preset(),
                rows: Vec::new(),
            },
            TargetTab {
                scope: TargetTabScope::Repository,
                directory: SkillDirectoryConfig::agents_preset(),
                rows: Vec::new(),
            },
        ];
        let mut model = initial_target_model(&tabs);
        model.target_path = Some("~/Development/project".to_owned());
        let backend = ratatui::backend::TestBackend::new(100, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal.draw(|frame| render(frame, &model)).unwrap();
        let repository_screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(repository_screen.contains("Target: ~/Development/project"));

        activate_target_tab(&mut model, &tabs, &BTreeSet::new(), 0);
        terminal.draw(|frame| render(frame, &model)).unwrap();
        let user_screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(user_screen.contains("Target: ~/.agents/skills"));
        assert!(!user_screen.contains("~/Development/project"));
    }

    #[test]
    fn editor_keys_accept_literal_paths_and_complete_directories() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("library")).unwrap();
        let input = format!("{}/lib", directory.path().display());
        let expected = format!("{}/library/", directory.path().display());
        let mut model = Model::new(Workspace::Library, Vec::new());
        model.overlay = Overlay::LocationEditor { edit: false, input };

        assert_eq!(
            action_for_model_key(
                &model,
                KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)
            ),
            Some(Action::Input('/'))
        );
        assert_eq!(
            action_for_model_key(
                &model,
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)
            ),
            Some(Action::Input('q'))
        );
        assert_eq!(
            action_for_model_key(&model, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Action::CompletePath)
        );

        reduce(&mut model, Action::CompletePath);
        assert_eq!(
            model.overlay,
            Overlay::LocationEditor {
                edit: false,
                input: expected,
            }
        );
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
    fn row_selection_uses_a_light_foreground_on_the_selection_background() {
        let warning = Row::diagnostic("Conflict needs attention");
        let warning_style = row_style(&warning, true);
        assert_eq!(warning_style.fg, Some(Color::Indexed(15)));
        assert_eq!(warning_style.bg, Some(SELECTED_BACKGROUND));

        let mut error = Row::diagnostic("Cannot continue");
        error.state = "Invalid".to_owned();
        let error_style = row_style(&error, true);
        assert_eq!(error_style.fg, Some(Color::Indexed(15)));
        assert_eq!(error_style.bg, Some(SELECTED_BACKGROUND));

        let normal_style = row_style(&Row::location("./library"), true);
        assert_eq!(normal_style.fg, Some(Color::Indexed(15)));
        assert_eq!(normal_style.bg, Some(SELECTED_BACKGROUND));
    }

    #[test]
    fn pending_actions_use_git_style_semantic_accents() {
        let mut added = Row::location("./library");
        added.action = "Enable link".to_owned();
        assert_eq!(row_style(&added, false).fg, Some(ADD));

        let mut removed = Row::location("./library");
        removed.action = "Unregister Source".to_owned();
        assert_eq!(row_style(&removed, false).fg, Some(ERROR));

        let mut modified = Row::location("./library");
        modified.action = "Move to Library".to_owned();
        assert_eq!(row_style(&modified, false).fg, Some(MODIFY));
    }

    #[test]
    fn disabling_a_newly_enabled_skill_clears_its_temporary_mode() {
        let mut skill = Row::skill(
            "local/library",
            "release-checklist",
            "Prepare a release",
            false,
            true,
            MaterializationKind::Linked,
            "Missing",
        );
        skill.mode = None;
        skill.initial_mode = None;
        let mut model = Model::new(Workspace::Target, vec![skill]);

        reduce(&mut model, Action::Toggle);
        assert_eq!(model.rows[0].check, Some(CheckState::Checked));
        assert_eq!(model.rows[0].mode, Some(MaterializationKind::Linked));
        assert_eq!(model.rows[0].action, "Enable link");

        reduce(&mut model, Action::Toggle);
        assert_eq!(model.rows[0].check, Some(CheckState::Unchecked));
        assert_eq!(model.rows[0].mode, None);
        assert!(model.rows[0].action.is_empty());
    }

    #[test]
    fn re_enabling_an_existing_skill_restores_its_initial_mode() {
        let skill = Row::skill(
            "local/library",
            "release-checklist",
            "Prepare a release",
            true,
            true,
            MaterializationKind::Copied,
            "In Sync",
        );
        let mut model = Model::new(Workspace::Target, vec![skill]);

        reduce(&mut model, Action::Toggle);
        assert_eq!(model.rows[0].check, Some(CheckState::Unchecked));
        assert_eq!(model.rows[0].mode, None);
        assert_eq!(model.rows[0].action, "Disable");

        reduce(&mut model, Action::Toggle);
        assert_eq!(model.rows[0].check, Some(CheckState::Checked));
        assert_eq!(model.rows[0].mode, Some(MaterializationKind::Copied));
        assert!(model.rows[0].action.is_empty());
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
    fn first_run_opens_the_normal_library_with_a_welcome() {
        let home = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(home.path().to_owned());
        let session = LibraryWorkflow::load(&paths).unwrap();

        let model = initial_library_model(&paths, &session);

        assert_eq!(model.workspace, Workspace::Library);
        assert_eq!(model.overlay, Overlay::Welcome);
        assert_eq!(model.rows[model.selected].kind, RowKind::Location);
        assert_eq!(model.rows[model.selected].name, "./library");
        assert!(!model.dirty);
    }

    #[test]
    fn first_run_shows_the_existing_default_library_inventory() {
        let home = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(home.path().to_owned());
        let skill = home.path().join(".skillator/library/unslop");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: unslop\ndescription: Cut AI tells\n---\n",
        )
        .unwrap();
        let session = LibraryWorkflow::load(&paths).unwrap();

        let model = initial_library_model(&paths, &session);

        assert!(model.rows.iter().any(|row| {
            row.kind == RowKind::Skill && row.name == "unslop" && row.description == "Cut AI tells"
        }));
    }

    #[test]
    fn unenabled_target_skills_include_their_skill_document_for_details() {
        let home = tempfile::tempdir().unwrap();
        let location = home.path().join("library");
        let skill = location.join("release-checklist");
        std::fs::create_dir_all(&skill).unwrap();
        let document = "---\nname: release-checklist\ndescription: Prepare releases\n---\n\n# Release checklist\n";
        std::fs::write(skill.join("SKILL.md"), document).unwrap();
        let library_config = LibraryConfig::new(vec![LibraryLocationConfig::new(
            location.display().to_string(),
            Vec::new(),
            false,
        )])
        .unwrap();
        let library = crate::library::scan_library(
            &library_config,
            &home.path().join("library.yaml"),
            home.path(),
            &BTreeMap::new(),
        );
        let config =
            RepositoryConfig::new(vec![SkillDirectoryConfig::agents_preset()], Vec::new()).unwrap();
        let target = crate::target::Target::user(home.path()).unwrap();
        let observed = observe(&target, &config, &library);
        let rows = rows_for_directory(
            config.skill_directories().first().unwrap(),
            &config,
            &library,
            &library_config,
            &observed,
            &BTreeSet::new(),
            None,
        );

        let selected = rows
            .iter()
            .position(|row| row.kind == RowKind::Skill && row.name == "release-checklist")
            .unwrap();
        assert_eq!(rows[selected].frontmatter, document);

        let mut model = Model::new(Workspace::Target, rows);
        model.selected = selected;
        reduce(&mut model, Action::Confirm);
        assert!(matches!(
            model.overlay,
            Overlay::Details { document: ref detail, .. } if detail == document
        ));
    }

    #[test]
    fn first_run_keeps_user_scope_out_of_the_library_view() {
        let home = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(home.path().to_owned());
        let skill = home.path().join(".agents/skills/user-skill");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: user-skill\ndescription: User skill\n---\n",
        )
        .unwrap();
        let session = LibraryWorkflow::load(&paths).unwrap();

        let library_model = initial_library_model(&paths, &session);
        assert!(
            !library_model
                .rows
                .iter()
                .any(|row| row.name == "Existing user-scoped Skills")
        );
    }

    #[test]
    fn welcome_dialog_continues_or_exits() {
        let mut model = Model::new(Workspace::Library, Vec::new());
        model.overlay = Overlay::Welcome;

        assert!(reduce(&mut model, Action::Confirm).is_empty());
        assert_eq!(model.overlay, Overlay::None);

        model.overlay = Overlay::Welcome;
        assert_eq!(
            reduce(&mut model, Action::Escape),
            [Effect::Quit { status: 0 }]
        );
    }

    #[test]
    fn library_rows_do_not_persist_discovered_inventory() {
        let original = LibraryConfig::new(vec![LibraryLocationConfig::new(
            "./library".to_owned(),
            Vec::new(),
            false,
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

        assert_eq!(config, original);
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
    fn collapse_from_a_child_selects_and_collapses_its_source() {
        let source = Row::source("acme/skills", CheckState::Checked);
        let child = Row::skill(
            "acme/skills",
            "demo",
            "Demo Skill",
            false,
            true,
            MaterializationKind::Linked,
            "",
        );
        let mut model = Model::new(Workspace::Library, vec![source, child]);
        model.selected = 1;

        reduce(&mut model, Action::Collapse);

        assert!(model.is_collapsed("acme/skills"));
        assert_eq!(model.selected, 0);
    }

    #[test]
    fn page_navigation_moves_through_visible_rows() {
        let rows = (0..20)
            .map(|index| Row::location(format!("location-{index}")))
            .collect();
        let mut model = Model::new(Workspace::Library, rows);

        reduce(&mut model, Action::PageDown);
        assert_eq!(model.selected, 10);
        reduce(&mut model, Action::PageUp);
        assert_eq!(model.selected, 0);
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
        let mut model = Model::new(Workspace::Target, vec![source, valid, invalid]);

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
    fn library_acquisition_mode_cycles_move_copy_link_and_clear() {
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
        row.check = Some(CheckState::Checked);
        row.initial_check = row.check;
        let mut model = Model::new(Workspace::Library, vec![row]);

        reduce(&mut model, Action::SwitchMode);
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
        assert_eq!(model.rows[0].action, "");
    }

    #[test]
    fn repository_candidate_cycles_to_read_only_repo_without_an_enablement() {
        let directory = tempfile::tempdir().unwrap();
        let skill = directory.path().join("project-skill");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: project-skill\ndescription: Project-owned instructions\n---\n",
        )
        .unwrap();
        let row = Row::repository_skill(
            "project-skill".to_owned(),
            "project-skill".to_owned(),
            "Project-owned instructions".to_owned(),
            &skill,
            false,
            false,
        );
        let mut model = Model::new(Workspace::Target, vec![row]);

        reduce(&mut model, Action::SwitchMode);

        assert_eq!(model.rows[0].check, Some(CheckState::Repository));
        assert_eq!(model.rows[0].action, "Track in repository");
        let config = repository_config_from_rows(
            &[SkillDirectoryConfig::agents_preset()],
            &[model.rows.clone()],
        )
        .unwrap();
        assert!(config.enablements().is_empty());

        reduce(&mut model, Action::Toggle);
        assert_eq!(model.rows[0].check, Some(CheckState::Repository));
        assert!(matches!(model.overlay, Overlay::Notice(_)));
        model.overlay = Overlay::None;
        reduce(&mut model, Action::SwitchMode);
        assert_eq!(model.rows[0].check, Some(CheckState::Repository));
        assert!(matches!(model.overlay, Overlay::Notice(_)));
    }

    #[test]
    fn repository_skill_renders_with_r_marker_and_repo_mode() {
        let directory = tempfile::tempdir().unwrap();
        let skill = directory.path().join("project-skill");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "repository skill").unwrap();
        let row = Row::repository_skill(
            "project-skill".to_owned(),
            "project-skill".to_owned(),
            "Project-owned instructions".to_owned(),
            &skill,
            false,
            true,
        );
        let model = Model::new(Workspace::Target, vec![row]);
        let backend = ratatui::backend::TestBackend::new(90, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal.draw(|frame| render(frame, &model)).unwrap();

        let screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(screen.contains("[r]"));
        assert!(screen.contains("repo"));
    }

    #[test]
    fn target_rows_discover_excepted_repository_skills() {
        let home = tempfile::tempdir().unwrap();
        let output = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(home.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        let skill = home.path().join(".agents/skills/project-skill");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: project-skill\ndescription: Project-owned instructions\n---\n",
        )
        .unwrap();
        let library_location = home.path().join("library");
        let library_skill = library_location.join("library-skill");
        std::fs::create_dir_all(&library_skill).unwrap();
        std::fs::write(
            library_skill.join("SKILL.md"),
            "---\nname: library-skill\ndescription: Library instructions\n---\n",
        )
        .unwrap();
        std::fs::write(
            home.path().join(".agents/.gitignore"),
            "# Generated by Skillator\n.gitignore\nskillator.yaml\nskills/*\n\n# Exception list for repository tracking\n!skills/project-skill/\n",
        )
        .unwrap();
        let config =
            RepositoryConfig::new(vec![SkillDirectoryConfig::agents_preset()], Vec::new()).unwrap();
        let library_config = LibraryConfig::new(vec![LibraryLocationConfig::new(
            library_location.display().to_string(),
            Vec::new(),
            false,
        )])
        .unwrap();
        let library = crate::library::scan_library(
            &library_config,
            &home.path().join("library.yaml"),
            home.path(),
            &BTreeMap::new(),
        );
        let target = Target::select(home.path()).unwrap();
        let observed = observe(&target, &config, &library);

        let rows = rows_for_directory(
            config.skill_directories().first().unwrap(),
            &config,
            &library,
            &library_config,
            &observed,
            &BTreeSet::new(),
            Some(&target),
        );

        let row = rows
            .iter()
            .find(|row| row.repository_name.as_deref() == Some("project-skill"))
            .expect("repository Skill row");
        assert_eq!(row.check, Some(CheckState::Repository));
        assert!(row.mode.is_none());
        let repository_group = rows
            .iter()
            .position(|row| row.kind == RowKind::Source && row.name == "Repository")
            .expect("Repository group");
        let library_group = rows
            .iter()
            .position(|row| row.kind == RowKind::Source && row.name != "Repository")
            .expect("Library group");
        assert!(repository_group < library_group);
    }

    #[test]
    fn shifted_slash_is_literal_text_in_an_editor() {
        let mut model = Model::new(Workspace::Library, Vec::new());
        model.overlay = Overlay::LocationEditor {
            edit: false,
            input: String::new(),
        };

        assert_eq!(
            action_for_model_key(
                &model,
                KeyEvent::new(KeyCode::Char('/'), KeyModifiers::SHIFT)
            ),
            Some(Action::Input('/'))
        );
    }
}
