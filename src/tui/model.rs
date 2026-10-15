//! Shared TUI state and row identities. Scope workflows own loading and persistence.

use crate::acquisition::LibraryAcquisitionMode;
use crate::domain::MaterializationKind;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Target,
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TargetTabScope {
    User,
    Repository,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Library,
    User,
    Repo,
}

impl Scope {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Library => "Library",
            Self::User => "User",
            Self::Repo => "Repo",
        }
    }

    pub(super) fn cycle(self, direction: isize) -> Self {
        match (self, direction > 0) {
            (Self::Library, true) | (Self::Repo, false) => Self::User,
            (Self::User, true) => Self::Repo,
            (Self::User, false) => Self::Library,
            (Self::Repo, true) => Self::Library,
            (Self::Library, false) => Self::Repo,
        }
    }
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
pub(super) enum RowKind {
    Diagnostic,
    Location,
    Source,
    Skill,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub(super) kind: RowKind,
    pub(super) group: Option<String>,
    pub(super) inventory_id: Option<String>,
    pub(super) name: String,
    pub(super) description: String,
    pub(super) check: Option<CheckState>,
    pub(super) mode: Option<MaterializationKind>,
    pub(super) state: String,
    pub(super) action: String,
    pub(super) details: String,
    pub(super) frontmatter: String,
    pub(super) warnings: Vec<String>,
    pub(super) metadata_errors: Vec<String>,
    pub(super) initial_action: String,
    pub(super) initial_check: Option<CheckState>,
    pub(super) initial_mode: Option<MaterializationKind>,
    pub(super) available: bool,
    pub(super) valid: bool,
    pub(super) location_index: Option<usize>,
    pub(super) source_path: Option<String>,
    pub(super) skill_path: Option<String>,
    pub(super) key_collision: bool,
    pub(super) acquisition_mode: Option<LibraryAcquisitionMode>,
    pub(super) initial_acquisition_mode: Option<LibraryAcquisitionMode>,
    pub(super) acquisition_pending: bool,
    pub(super) acquisition_source: Option<PathBuf>,
    pub(super) acquisition_source_root_git: bool,
    pub(super) inherited_user: bool,
    pub(super) repository_candidate: bool,
    pub(super) repository_name: Option<String>,
}

pub(super) struct SkillInventoryRow {
    pub(super) group: String,
    pub(super) inventory_id: Option<String>,
    pub(super) path: String,
    pub(super) name: String,
    pub(super) description: String,
    pub(super) check: CheckState,
    pub(super) available: bool,
    pub(super) valid: bool,
    pub(super) mode: Option<MaterializationKind>,
    pub(super) state: String,
    pub(super) details: String,
    pub(super) location_index: Option<usize>,
}

impl Row {
    fn new(kind: RowKind, name: String) -> Self {
        Self {
            kind,
            name,
            group: None,
            inventory_id: None,
            description: String::new(),
            check: None,
            mode: None,
            state: String::new(),
            action: String::new(),
            details: String::new(),
            frontmatter: String::new(),
            warnings: Vec::new(),
            metadata_errors: Vec::new(),
            initial_action: String::new(),
            initial_check: None,
            initial_mode: None,
            available: true,
            valid: true,
            location_index: None,
            source_path: None,
            skill_path: None,
            key_collision: false,
            acquisition_mode: None,
            initial_acquisition_mode: None,
            acquisition_pending: false,
            acquisition_source: None,
            acquisition_source_root_git: false,
            inherited_user: false,
            repository_candidate: false,
            repository_name: None,
        }
    }

    pub fn source(name: impl Into<String>, check: CheckState) -> Self {
        let mut row = Self::new(RowKind::Source, name.into());
        row.check = Some(check);
        row.initial_check = Some(check);
        row
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
        let mut row = Self::new(RowKind::Skill, name.into());
        row.group = Some(group.into());
        row.description = description.into();
        let check = if enabled {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        };
        row.check = Some(check);
        row.initial_check = Some(check);
        row.mode = Some(mode);
        row.initial_mode = Some(mode);
        row.state = state.into();
        row.available = available;
        row
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
        row.inherited_user = true;
        row.check = Some(CheckState::User);
        row.initial_check = Some(CheckState::User);
        row.mode = None;
        row.initial_mode = None;
        row
    }

    pub fn location(path: impl Into<String>) -> Self {
        Self::new(RowKind::Location, path.into())
    }

    pub fn diagnostic(message: impl Into<String>) -> Self {
        let mut row = Self::new(RowKind::Diagnostic, "Diagnostic".to_owned());
        row.description = message.into();
        row.state = "Warning".to_owned();
        row.available = false;
        row
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

    pub(super) fn source_inventory(
        name: String,
        check: CheckState,
        location_index: usize,
        source_path: String,
        available: bool,
        key_collision: bool,
    ) -> Self {
        let mut row = Self::source(name, check);
        row.location_index = Some(location_index);
        row.source_path = Some(source_path);
        row.inventory_id = Some(source_inventory_id(
            location_index,
            row.source_path.as_deref().unwrap_or("."),
        ));
        row.available = available;
        row.key_collision = key_collision;
        row
    }

    pub(super) fn skill_inventory(inventory: SkillInventoryRow) -> Self {
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
        row.initial_check = Some(inventory.check);
        row.mode = inventory.mode;
        row.initial_mode = inventory.mode;
        row.skill_path = Some(inventory.path);
        row.location_index = inventory.location_index;
        row.inventory_id = inventory.inventory_id;
        row.valid = inventory.valid;
        row.details = inventory.details;
        row
    }

    pub(super) fn repository_skill(
        repository_name: String,
        name: String,
        description: String,
        document: String,
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
        row.frontmatter = document;
        row
    }
}

pub(super) fn source_inventory_id(location_index: usize, source_path: &str) -> String {
    format!("{location_index}:{source_path}")
}

pub(super) fn row_identity(row: &Row) -> Option<&str> {
    row.inventory_id.as_deref().or(match row.kind {
        RowKind::Source => Some(row.name.as_str()),
        RowKind::Skill => row.group.as_deref(),
        _ => None,
    })
}

pub(super) fn same_row_identity(row: &Row, selected: &Row) -> bool {
    row.kind == selected.kind
        && if selected.inventory_id.is_some() {
            row.inventory_id == selected.inventory_id && row.skill_path == selected.skill_path
        } else if selected.skill_path.is_some() {
            row.group == selected.group && row.skill_path == selected.skill_path
        } else {
            row.group == selected.group && row.name == selected.name
        }
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
    DiscardTarget,
    DirectoryEditor {
        edit: bool,
        input: String,
    },
    DirectoryChooser {
        input: String,
        selected: usize,
    },
    FollowerEditor(String),
    FollowerProbe {
        name: String,
    },
    ConfirmFollowerSync {
        alias: String,
        initial: bool,
    },
    FollowerSyncResult {
        title: String,
        body: String,
    },
    ScopeSwitch {
        destination: Scope,
        host_to: Option<usize>,
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
        warnings: Vec<String>,
        errors: Vec<String>,
    },
    Diagnostic {
        title: String,
        body: String,
    },
    Notice(String),
    Result(String),
}

#[derive(Debug, Clone)]
pub(super) struct BrowseState {
    pub(super) selected: usize,
    pub(super) collapsed: BTreeSet<String>,
    pub(super) filter: String,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub(super) workspace: Workspace,
    pub(super) rows: Vec<Row>,
    pub(super) selected: usize,
    pub(super) collapsed: BTreeSet<String>,
    pub(super) filter: String,
    pub(super) overlay: Overlay,
    pub(super) detail_scroll: u16,
    pub(super) exit_after_save: bool,
    pub(super) dirty: bool,
    pub(super) directory_index: usize,
    pub(super) directory_count: usize,
    pub(super) directory_labels: Vec<String>,
    pub(super) directory_values: Vec<String>,
    pub(super) directory_paths: Vec<String>,
    pub(super) directory_scopes: Vec<TargetTabScope>,
    pub(super) browse: BTreeMap<(TargetTabScope, String), BrowseState>,
    pub(super) last_directory_keys: BTreeMap<TargetTabScope, String>,
    pub(super) scope: Scope,
    pub(super) host_labels: Vec<String>,
    pub(super) host_index: usize,
    pub(super) host_hostname: Option<String>,
    pub(super) host_dirty: bool,
    pub(super) host_syncing: bool,
    pub(super) unavailable: Option<String>,
    pub(super) scope_error: Option<String>,
    pub(super) target_path: Option<String>,
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
            browse: BTreeMap::new(),
            last_directory_keys: BTreeMap::new(),
            scope: if workspace == Workspace::Library {
                Scope::Library
            } else {
                Scope::Repo
            },
            host_labels: vec!["Local".to_owned()],
            host_index: 0,
            host_hostname: None,
            host_dirty: false,
            host_syncing: false,
            unavailable: None,
            scope_error: None,
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

    pub(super) fn visible_indices(&self) -> Vec<usize> {
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

    pub(super) fn select_visible_offset(&mut self, delta: isize) {
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

    pub(super) fn select_group(&mut self, direction: isize) {
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

    pub(super) fn recompute_group(&mut self, group: &str) {
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
    NextScope,
    PreviousScope,
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
    Undo,
    Help,
    RefreshLibrary,
    Confirm,
    ReturnToEditing,
    Acknowledge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    PrepareSave {
        fast: bool,
    },
    Quit {
        status: u8,
    },
    ChangeTargetTo(String),
    Undo,
    ApplyDirectoryEdit {
        edit: bool,
        value: String,
    },
    ApplyLocationEdit {
        edit: bool,
        value: String,
    },
    RefreshLibrary,
    ApplySourceKey(String),
    DeleteDirectory,
    RetrySave,
    DirectoryChanged {
        from: usize,
        to: usize,
    },
    SwitchToScope {
        destination: Scope,
        host_to: Option<usize>,
        save: bool,
        discard: bool,
    },
    StartFollowerProbe(String),
    CancelFollowerProbe,
    StartFollowerSync(String),
    CancelFollowerSync,
    FinishFollowerInit,
    CommitSave,
    CancelSave,
}
