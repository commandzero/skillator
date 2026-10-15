//! Library discovery, acquisition, host browsing and follower delivery.
use super::model::{
    BrowseState, CheckState, Effect, Model, Overlay, Row, RowKind, Scope, SkillInventoryRow,
    Workspace, row_identity, same_row_identity, source_inventory_id,
};
use super::{
    AppTerminal, InteractionExit, Navigation, SessionNavigation, config_issues,
    run_interactive_with_tick,
};
use crate::acquisition::{LibraryAcquisition, LibraryAcquisitionMode};
use crate::app::{AppPaths, LibraryWorkflow, TargetWorkflow, WorkflowError};
use crate::config::{Fingerprint, LibraryConfig, LibraryLocationConfig};
use crate::domain::{MaterializationKind, SkillKey, SkillPath, SourceKey};
use crate::library::{LibrarySnapshot, SkillValidity};
use crate::remote::hosts::{self, HostRegistry, ReplicaInventory};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

pub fn run(paths: &AppPaths) -> Result<u8, WorkflowError> {
    super::navigate(
        paths,
        Navigation::Library {
            return_target: None,
        },
    )
}

pub(super) fn refresh_library_action(row: &mut Row) {
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

pub(super) fn refresh_library_visibility(row: &mut Row) {
    row.action = if row.check != row.initial_check {
        if row.check == Some(CheckState::Checked) {
            "Show in skill selection".to_owned()
        } else {
            "Hide from skill selection".to_owned()
        }
    } else {
        row.initial_action.clone()
    };
}

pub(super) struct LibraryView {
    pub(super) session: crate::app::LibrarySession,
    pub(super) working_config: LibraryConfig,
    pub(super) error: Option<String>,
    pub(super) model: Model,
    pub(super) hosts: Rc<RefCell<HostUi>>,
    pub(super) refresh: Rc<RefCell<LibraryRefresh>>,
}

pub(super) struct LibraryDiscovery {
    pub(super) generation: u64,
    pub(super) snapshot: Arc<LibrarySnapshot>,
    pub(super) rows: Vec<Row>,
}

pub(super) struct LibraryScan {
    pub(super) generation: u64,
    pub(super) config: LibraryConfig,
    pub(super) receiver: mpsc::Receiver<LibraryDiscovery>,
}

#[derive(Default)]
pub(super) struct LibraryRefresh {
    pub(super) generation: u64,
    pub(super) snapshot: Option<Arc<LibrarySnapshot>>,
    pub(super) flight: Option<LibraryScan>,
    requested: Option<LibraryConfig>,
    ready: Option<LibraryDiscovery>,
}

impl LibraryRefresh {
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.requested = None;
        self.ready = None;
    }

    pub(super) fn request(&mut self, paths: &AppPaths, config: &LibraryConfig) {
        if self
            .flight
            .as_ref()
            .is_some_and(|flight| flight.generation == self.generation && flight.config == *config)
        {
            return;
        }
        self.invalidate();
        self.requested = Some(config.clone());
        self.start(paths);
    }

    fn start(&mut self, paths: &AppPaths) {
        if self.flight.is_some() {
            return;
        }
        let Some(config) = self.requested.take() else {
            return;
        };
        let paths = paths.clone();
        let generation = self.generation;
        let worker_config = config.clone();
        let (sender, receiver) = mpsc::channel();
        self.flight = Some(LibraryScan {
            generation,
            config,
            receiver,
        });
        std::thread::spawn(move || {
            let snapshot = Arc::new(LibraryWorkflow::snapshot(&paths, &worker_config));
            let rows = library_rows(&worker_config, &snapshot);
            let _ = sender.send(LibraryDiscovery {
                generation,
                snapshot,
                rows,
            });
        });
    }

    pub(super) fn complete(&mut self, paths: &AppPaths, discovery: LibraryDiscovery) {
        self.flight = None;
        if discovery.generation == self.generation {
            self.ready = Some(discovery);
        }
        self.start(paths);
    }

    fn collect(&mut self, paths: &AppPaths) -> Result<(), String> {
        let Some(flight) = &self.flight else {
            return Ok(());
        };
        match flight.receiver.try_recv() {
            Ok(discovery) => self.complete(paths, discovery),
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => {
                let current = flight.generation == self.generation;
                self.flight = None;
                self.start(paths);
                if current {
                    return Err(
                        "Library inventory refresh failed; retained inventory is unchanged."
                            .to_owned(),
                    );
                }
            }
        }
        Ok(())
    }

    pub(super) fn apply(&mut self, model: &mut Model, hosts: &mut HostUi) -> bool {
        if model.dirty || !matches!(model.overlay, Overlay::None) {
            return false;
        }
        let Some(discovery) = self.ready.take() else {
            return false;
        };
        self.snapshot = Some(discovery.snapshot);
        replace_local_rows(model, hosts, discovery.rows);
        true
    }
}

fn replace_local_rows(model: &mut Model, hosts: &mut HostUi, rows: Vec<Row>) {
    if model.host_index == 0 {
        let selected = model
            .selected_row()
            .and_then(|selected| rows.iter().position(|row| same_row_identity(row, selected)));
        model.selected = selected
            .unwrap_or(model.selected)
            .min(rows.len().saturating_sub(1));
        model.rows = rows.clone();
        model.unavailable = hosts.registry_error.clone();
    } else if let Some(browse) = model
        .host_labels
        .first()
        .and_then(|alias| hosts.browse.get_mut(alias))
    {
        browse.selected = hosts
            .local_rows
            .get(browse.selected)
            .and_then(|selected| rows.iter().position(|row| same_row_identity(row, selected)))
            .unwrap_or(browse.selected)
            .min(rows.len().saturating_sub(1));
    }
    hosts.local_rows = rows;
}

pub(super) fn initial_library_model(
    session: &crate::app::LibrarySession,
    snapshot: Option<&LibrarySnapshot>,
) -> Model {
    let rows = snapshot.map_or_else(
        || {
            session
                .config
                .locations()
                .iter()
                .enumerate()
                .map(|(index, location)| {
                    let mut row = Row::location(location.path());
                    row.location_index = Some(index);
                    row
                })
                .collect()
        },
        |snapshot| library_rows(&session.config, snapshot),
    );
    let mut model = Model::new(Workspace::Library, rows);
    if session.first_run {
        model.overlay = Overlay::Welcome;
    }
    model
}

pub(super) enum HostReply {
    Probe {
        request: u64,
        name: String,
        result: Result<hosts::ProbeResult, String>,
    },
    Inspect {
        request: u64,
        alias: String,
        result: Result<ReplicaInventory, String>,
    },
    Sync {
        request: u64,
        alias: String,
        result: Result<crate::remote::Report, String>,
    },
}

pub(super) struct HostUi {
    pub(super) registry: Option<HostRegistry>,
    registry_error: Option<String>,
    pub(super) tx: mpsc::Sender<HostReply>,
    rx: mpsc::Receiver<HostReply>,
    cancel: Option<Arc<AtomicBool>>,
    pub(super) request: u64,
    local_rows: Vec<Row>,
    browse: BTreeMap<String, BrowseState>,
    pub(super) staged_hosts: Vec<String>,
    initial_sync: VecDeque<String>,
    sync_worker: Option<std::thread::JoinHandle<()>>,
    finished_sync: Option<HostReply>,
    sync_failed: bool,
}

impl HostUi {
    pub(super) fn new(home: &Path, local_rows: &[Row]) -> Self {
        let (registry, registry_error) = match HostRegistry::load(home) {
            Ok(registry) => (Some(registry), None),
            Err(error) => (None, Some(error)),
        };
        let (tx, rx) = mpsc::channel();
        Self {
            registry,
            registry_error,
            tx,
            rx,
            cancel: None,
            request: 0,
            local_rows: local_rows.to_vec(),
            browse: BTreeMap::new(),
            staged_hosts: Vec::new(),
            initial_sync: VecDeque::new(),
            sync_worker: None,
            finished_sync: None,
            sync_failed: false,
        }
    }

    pub(super) fn labels(&self) -> Vec<String> {
        let mut labels = vec!["Local".to_owned()];
        if let Some(registry) = &self.registry {
            labels.extend(registry.followers().map(|host| host.alias.to_owned()));
        }
        labels
    }

    fn stash_browse(&mut self, model: &Model, index: usize) {
        if let Some(alias) = model.host_labels.get(index) {
            self.browse.insert(
                alias.clone(),
                BrowseState {
                    selected: model.selected,
                    collapsed: model.collapsed.clone(),
                    filter: model.filter.clone(),
                },
            );
        }
    }

    fn restore_browse(&self, model: &mut Model) {
        let selected = model
            .host_labels
            .get(model.host_index)
            .and_then(|alias| self.browse.get(alias));
        if let Some(browse) = selected {
            model.selected = browse.selected.min(model.rows.len().saturating_sub(1));
            model.collapsed = browse.collapsed.clone();
            model.filter = browse.filter.clone();
        } else {
            model.selected = 0;
            model.collapsed.clear();
            model.filter.clear();
        }
    }

    pub(super) fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.request = self.request.wrapping_add(1);
        self.finished_sync = None;
    }

    pub(super) fn select_host(
        &mut self,
        model: &mut Model,
        index: usize,
        library_error: Option<&str>,
    ) {
        if index >= model.host_labels.len() {
            return;
        }
        model.host_index = index;
        model.host_syncing = false;
        if index == 0 {
            self.cancel();
            model.rows = self.local_rows.clone();
            model.host_hostname = None;
            model.unavailable = self.registry_error.clone();
            model.scope_error = library_error.map(str::to_owned);
        } else {
            self.inspect_selected(model);
        }
        self.restore_browse(model);
    }

    fn resume_interrupted_inspection(&mut self, model: &mut Model) {
        if model.host_index != 0
            && model
                .unavailable
                .as_deref()
                .is_some_and(|text| text.starts_with("Inspecting "))
        {
            self.inspect_selected(model);
        }
    }

    fn inspect_selected(&mut self, model: &mut Model) {
        self.cancel();
        model.host_syncing = false;
        model.rows.clear();
        model.selected = 0;
        model.scope_error = None;
        model.unavailable = None;
        let Some(alias) = model.host_labels.get(model.host_index).cloned() else {
            return;
        };
        let Some(host) = self.registry.as_ref().and_then(|registry| {
            registry
                .followers()
                .find(|host| host.alias == alias.as_str())
        }) else {
            model.unavailable = Some("Follower is not configured.".to_owned());
            return;
        };
        model.host_hostname = host.hostname.map(str::to_owned);
        model.unavailable = Some(format!("Inspecting {alias} replica…"));
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        let sender = self.tx.clone();
        let request = self.request;
        let destination = host.destination.to_owned();
        std::thread::spawn(move || {
            let result = hosts::inspect(&destination, &cancel);
            let _ = sender.send(HostReply::Inspect {
                request,
                alias,
                result,
            });
        });
    }

    pub(super) fn save_registry(&mut self) -> Result<bool, String> {
        let registry = self.registry.as_mut().ok_or("Host registry unavailable")?;
        if !registry.dirty() {
            return Ok(false);
        }
        registry.save()?;
        if self.initial_sync.is_empty() {
            self.sync_failed = false;
        }
        self.initial_sync
            .extend(std::mem::take(&mut self.staged_hosts));
        Ok(true)
    }

    pub(super) fn discard_registry(&mut self) {
        if let Some(registry) = self.registry.as_mut() {
            registry.discard();
        }
        self.staged_hosts.clear();
    }

    pub(super) fn offer_initial_sync(&mut self, model: &mut Model) -> bool {
        if model.dirty || model.host_dirty || model.host_syncing || model.overlay != Overlay::None {
            return false;
        }
        let Some(alias) = self.initial_sync.pop_front() else {
            return false;
        };
        model.overlay = Overlay::ConfirmFollowerSync {
            alias,
            initial: true,
        };
        true
    }

    fn start_sync(&mut self, paths: &AppPaths, model: &mut Model, alias: String) {
        if model.dirty || model.host_dirty || self.registry.as_ref().is_none_or(HostRegistry::dirty)
        {
            model.overlay = Overlay::Notice(
                "Save or discard pending changes on Local before syncing.".to_owned(),
            );
            return;
        }
        if self
            .sync_worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            model.overlay = Overlay::Notice(
                "Previous sync is still stopping; try again after it completes.".to_owned(),
            );
            return;
        }
        if let Some(worker) = self.sync_worker.take() {
            let _ = worker.join();
        }
        let Some(host) = self
            .registry
            .as_ref()
            .and_then(|registry| registry.followers().find(|host| host.alias == alias))
        else {
            model.overlay = Overlay::FollowerSyncResult {
                title: "Follower sync failed".to_owned(),
                body: format!("Follower {alias} is not configured."),
            };
            self.sync_failed = true;
            return;
        };
        let destination = host.destination.to_owned();
        let hostname = host.hostname.map(str::to_owned);
        let Some(index) = model.host_labels.iter().position(|label| label == &alias) else {
            return;
        };
        self.stash_browse(model, model.host_index);
        if model.host_index == 0 {
            self.local_rows = model.rows.clone();
            model.rows.clear();
        } else if model.host_index != index {
            model.rows.clear();
        }
        self.cancel();
        model.host_index = index;
        model.host_hostname = hostname;
        model.scope_error = None;
        model.unavailable = Some(format!(
            "Syncing {alias}… Esc or leaving the host/scope cancels."
        ));
        model.host_syncing = true;
        self.restore_browse(model);
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        let sender = self.tx.clone();
        let request = self.request;
        let paths = paths.clone();
        self.sync_worker = Some(std::thread::spawn(move || {
            let result = crate::remote::run_for_tui(&paths, &alias, &destination, &cancel)
                .map_err(|error| error.to_string());
            let _ = sender.send(HostReply::Sync {
                request,
                alias,
                result,
            });
        }));
    }

    fn cancel_sync(&mut self, model: &mut Model) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        model.unavailable =
            Some("Canceling sync… waiting for transport shutdown and export cleanup.".to_owned());
    }

    fn apply_sync_result(&mut self, model: &mut Model) -> bool {
        if !matches!(model.overlay, Overlay::None | Overlay::Notice(_)) {
            return false;
        }
        let Some(HostReply::Sync {
            request,
            alias,
            result,
        }) = self.finished_sync.take()
        else {
            return false;
        };
        if request != self.request || model.host_labels.get(model.host_index) != Some(&alias) {
            return false;
        }
        let canceled = model
            .unavailable
            .as_deref()
            .is_some_and(|text| text.starts_with("Canceling sync"));
        model.host_syncing = false;
        let (success, body) = match result {
            Ok(report) => (report.exit_status == 0, report.text_with_color(false)),
            Err(error) => (false, error),
        };
        self.sync_failed |= !success;
        if success {
            self.stash_browse(model, model.host_index);
            self.inspect_selected(model);
        } else {
            model.unavailable = Some(format!("{alias} sync failed; see diagnostic."));
        }
        model.detail_scroll = 0;
        model.overlay = Overlay::FollowerSyncResult {
            title: if success {
                "Follower sync complete"
            } else if canceled {
                "Follower sync canceled"
            } else {
                "Follower sync failed"
            }
            .to_owned(),
            body,
        };
        true
    }

    pub(super) fn poll(&mut self, model: &mut Model) -> bool {
        let mut changed = false;
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                HostReply::Probe {
                    request,
                    name,
                    result,
                } if request == self.request
                    && matches!(&model.overlay, Overlay::FollowerProbe { name: pending } if pending == &name) =>
                {
                    changed = true;
                    self.cancel.take();
                    model.overlay = Overlay::None;
                    match result.and_then(|probe| {
                        let registry = self
                            .registry
                            .as_mut()
                            .ok_or_else(|| "Host registry unavailable".to_owned())?;
                        registry.stage(&name, &probe.hostname)?;
                        Ok(probe)
                    }) {
                        Ok(probe) => {
                            model.host_labels = self.labels();
                            model.host_dirty = true;
                            self.staged_hosts.push(name.clone());
                            model.overlay = Overlay::Diagnostic {
                                title: "Follower registration staged".to_owned(),
                                body: match probe.warning {
                                    Some(warning) => format!(
                                        "Follower {name} is staged. SSH reports hostname {}.\n\n{warning}\n\nSave or discard staged changes before selecting this follower. After saving, you can initialize its replica.",
                                        probe.hostname,
                                    ),
                                    None => format!(
                                        "Follower {name} is staged. SSH reports hostname {}.\n\nSave or discard staged changes before selecting this follower. After saving, you can initialize its replica.",
                                        probe.hostname,
                                    ),
                                },
                            };
                        }
                        Err(error) => {
                            model.overlay = Overlay::Diagnostic {
                                title: "Follower probe failed".to_owned(),
                                body: format!("Cannot register follower {name}:\n\n{error}"),
                            };
                        }
                    }
                    self.resume_interrupted_inspection(model);
                }
                HostReply::Inspect {
                    request,
                    alias,
                    result,
                } if request == self.request
                    && model.host_labels.get(model.host_index) == Some(&alias) =>
                {
                    changed = true;
                    self.cancel.take();
                    match result {
                        Ok(inventory) => {
                            let warning = inventory.warning.clone();
                            model.rows = replica_rows(&alias, inventory);
                            model.unavailable = None;
                            model.selected = 0;
                            self.restore_browse(model);
                            if let Some(warning) = warning {
                                if let Overlay::Diagnostic { body, .. }
                                | Overlay::FollowerSyncResult { body, .. } = &mut model.overlay
                                {
                                    body.push_str("\n\nReplica inspection: ");
                                    body.push_str(&warning);
                                } else {
                                    model.overlay = Overlay::Diagnostic {
                                        title: "Follower replica warning".to_owned(),
                                        body: format!("{alias}:\n\n{warning}"),
                                    };
                                }
                            }
                        }
                        Err(error) => {
                            model.rows.clear();
                            model.selected = 0;
                            model.unavailable =
                                Some(format!("{alias} replica unavailable: {error}"));
                            if let Overlay::FollowerSyncResult { body, .. } = &mut model.overlay {
                                body.push_str(&format!("\n\nCannot inspect {alias}:\n\n{error}"));
                            } else {
                                model.overlay = Overlay::Diagnostic {
                                    title: "Follower replica unavailable".to_owned(),
                                    body: format!("Cannot inspect {alias}:\n\n{error}"),
                                };
                            }
                        }
                    }
                }
                reply @ HostReply::Sync { .. } => {
                    if let HostReply::Sync { request, alias, .. } = &reply
                        && *request == self.request
                        && model.host_labels.get(model.host_index) == Some(alias)
                        && model.host_syncing
                    {
                        self.cancel.take();
                        self.finished_sync = Some(reply);
                    }
                }
                _ => {}
            }
        }
        if self
            .sync_worker
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
            && let Some(worker) = self.sync_worker.take()
            && worker.join().is_err()
            && model.host_syncing
        {
            self.finished_sync = Some(HostReply::Sync {
                request: self.request,
                alias: model.host_labels[model.host_index].clone(),
                result: Err("Follower sync worker failed unexpectedly.".to_owned()),
            });
        }
        changed |= self.apply_sync_result(model);
        changed
    }
}

impl Drop for HostUi {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.sync_worker.take() {
            let _ = worker.join();
        }
    }
}

pub(super) fn replica_rows(alias: &str, inventory: ReplicaInventory) -> Vec<Row> {
    let mut rows = vec![Row::location(format!("{alias}:{}", inventory.path))];
    rows[0].description = "Read-only follower replica".to_owned();
    if let Some(warning) = inventory.warning {
        rows.push(Row::diagnostic(warning));
    }
    let mut by_source: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for skill in inventory.skills {
        by_source
            .entry(skill.source_key.clone())
            .or_default()
            .push(skill);
    }
    for (source, skills) in by_source {
        let mut source_row = Row::source(source.clone(), CheckState::Unchecked);
        source_row.check = None;
        let source_path = format!("{alias}:{}/{source}/_skills", inventory.path);
        source_row.details = source_path.clone();
        rows.push(source_row);
        for skill in skills {
            let name = skill
                .skill_path
                .rsplit('/')
                .next()
                .unwrap_or(&skill.skill_path)
                .to_owned();
            let mut row = Row::skill(
                source.clone(),
                name,
                skill.description,
                false,
                false,
                MaterializationKind::Linked,
                "Read-only replica",
            );
            row.check = None;
            row.mode = None;
            row.details = if skill.skill_path == "." {
                format!("{source_path}/SKILL.md")
            } else {
                format!("{source_path}/{}/SKILL.md", skill.skill_path)
            };
            row.frontmatter = skill.document;
            row.warnings = skill.warnings;
            row.metadata_errors = skill.diagnostic.iter().cloned().collect();
            row.valid = skill.diagnostic.is_none();
            row.action.clear();
            rows.push(row);
            if let Some(diagnostic) = skill.diagnostic {
                rows.push(Row::diagnostic(diagnostic));
            }
        }
    }
    rows
}

fn refresh_local_after_save(
    paths: &AppPaths,
    session: &crate::app::LibrarySession,
    model: &mut Model,
    hosts: &mut HostUi,
    refresh: &mut LibraryRefresh,
) {
    let snapshot = Arc::new(LibraryWorkflow::snapshot(paths, &session.config));
    let rows = library_rows(&session.config, &snapshot);
    refresh.invalidate();
    refresh.snapshot = Some(snapshot);
    replace_local_rows(model, hosts, rows);
}

fn review_library_save(
    session: &crate::app::LibrarySession,
    model: &mut Model,
    config: LibraryConfig,
    acquisitions: Vec<LibraryAcquisition>,
    staged: &mut Option<(LibraryConfig, Vec<LibraryAcquisition>)>,
) {
    let affected = std::env::current_dir()
        .ok()
        .and_then(|directory| TargetWorkflow::load(directory).ok())
        .filter(|target| !target.first_run)
        .map(|target| {
            LibraryWorkflow::affected_references(&session.config, &config, &target.config)
        })
        .unwrap_or_default();
    *staged = (model.dirty || session.first_run).then_some((config, acquisitions));
    model.overlay = if affected.is_empty()
        && staged
            .as_ref()
            .is_none_or(|(_, acquisitions)| acquisitions.is_empty())
    {
        if model.host_index == 0 {
            Overlay::ConfirmSave
        } else {
            Overlay::ConfirmSaveWarning(
                "Save local changes? Follower inventory is read-only. Only local Library and host registry changes can be saved.\ny/Enter save · n/Esc return".to_owned(),
            )
        }
    } else {
        let count = staged
            .as_ref()
            .map_or(0, |(_, acquisitions)| acquisitions.len());
        Overlay::ConfirmSaveWarning(format!(
            "Save library changes? {count} skills to add to the library. The current repository will no longer be able to find {} enabled skills.\ny/Enter proceed · n/Esc return",
            affected.len(),
        ))
    };
}

pub(super) fn load_library_for_tui(
    paths: &AppPaths,
) -> Result<(crate::app::LibrarySession, Option<String>), WorkflowError> {
    match LibraryWorkflow::load(paths) {
        Ok(session) => Ok((session, None)),
        Err(error @ WorkflowError::InvalidInput { .. }) => Ok((
            crate::app::LibrarySession {
                config: LibraryConfig::empty(),
                fingerprint: Fingerprint::Absent,
                first_run: false,
            },
            Some(error.to_string()),
        )),
        Err(error) => Err(error),
    }
}

pub(super) fn run_library_once(
    paths: &AppPaths,
    return_target: Option<&Path>,
    browsing: &mut SessionNavigation,
    terminal: &mut AppTerminal,
) -> Result<Navigation, WorkflowError> {
    let (mut session, mut library_error) = load_library_for_tui(paths)?;
    let mut cached = browsing.library.take();
    let reused = cached.as_ref().is_some_and(|view| {
        view.session.fingerprint == session.fingerprint && view.error == library_error
    });
    let refresh = cached
        .as_ref()
        .map(|view| view.refresh.clone())
        .unwrap_or_else(|| Rc::new(RefCell::new(LibraryRefresh::default())));
    let (mut working_config, mut model, hosts) = if reused {
        let view = cached.take().expect("matching cached Library");
        (view.working_config, view.model, view.hosts)
    } else {
        let snapshot = browsing
            .library_snapshot
            .as_ref()
            .filter(|(fingerprint, _)| *fingerprint == session.fingerprint)
            .map(|(_, snapshot)| snapshot.clone());
        {
            let mut refresh = refresh.borrow_mut();
            refresh.invalidate();
            refresh.snapshot = snapshot.clone();
        }
        let mut model = initial_library_model(&session, snapshot.as_deref());
        if let Some(error) = &library_error {
            model.rows = vec![Row::diagnostic(error.clone())];
            model.scope_error = Some(error.clone());
        }
        let hosts = Rc::new(RefCell::new(HostUi::new(paths.home(), &model.rows)));
        (session.config.clone(), model, hosts)
    };
    {
        let mut hosts = hosts.borrow_mut();
        if reused {
            (hosts.registry, hosts.registry_error) = match HostRegistry::load(paths.home()) {
                Ok(registry) => (Some(registry), None),
                Err(error) => (None, Some(error)),
            };
        }
        let previous_alias = model.host_labels.get(model.host_index).cloned();
        model.host_labels = hosts.labels();
        hosts.browse = std::mem::take(&mut browsing.library_browse);
        let index = browsing
            .library_host
            .as_ref()
            .and_then(|alias| model.host_labels.iter().position(|label| label == alias))
            .unwrap_or(0);
        if index == 0 && model.host_index != 0 {
            hosts.select_host(&mut model, index, library_error.as_deref());
        } else {
            model.host_index = index;
            if index != 0 {
                let retained = (reused && previous_alias.as_ref() == model.host_labels.get(index))
                    .then(|| std::mem::take(&mut model.rows));
                hosts.inspect_selected(&mut model);
                if let Some(rows) = retained {
                    model.rows = rows;
                }
            }
        }
        hosts.restore_browse(&mut model);
        if model.host_index == 0 {
            model.unavailable = hosts.registry_error.clone();
        }
    }
    if library_error.is_none() {
        let mut refresh = refresh.borrow_mut();
        if let Err(error) = refresh.collect(paths) {
            model.unavailable = Some(error);
        }
        refresh.request(paths, &working_config);
        if model.host_index == 0 && model.unavailable.is_none() {
            model.unavailable = Some("Refreshing Library inventory…".to_owned());
        }
    }
    let tick_hosts = hosts.clone();
    let tick_refresh = refresh.clone();
    let mut staged: Option<(LibraryConfig, Vec<LibraryAcquisition>)> = None;
    let mut pending_switch: Option<(Scope, Option<usize>)> = None;
    let (exit, mut exited_model) = run_interactive_with_tick(
        terminal,
        model,
        |model, effect| match effect {
            Effect::StartFollowerProbe(name) => {
                let mut hosts = hosts.borrow_mut();
                let Some(registry) = hosts.registry.as_ref() else {
                    model.overlay = Overlay::Diagnostic {
                        title: "Follower registry unavailable".to_owned(),
                        body: hosts.registry_error.clone().unwrap_or_default(),
                    };
                    return Ok(None);
                };
                if let Err(error) = registry.validate_name(&name) {
                    model.overlay = Overlay::Diagnostic {
                        title: "Invalid follower name".to_owned(),
                        body: error,
                    };
                    return Ok(None);
                }
                if model.host_index == 0 {
                    hosts.local_rows = model.rows.clone();
                    hosts.stash_browse(model, 0);
                }
                hosts.cancel();
                model.host_syncing = false;
                let cancel = Arc::new(AtomicBool::new(false));
                hosts.cancel = Some(cancel.clone());
                let sender = hosts.tx.clone();
                let request = hosts.request;
                model.overlay = Overlay::FollowerProbe { name: name.clone() };
                std::thread::spawn(move || {
                    let result = hosts::probe(&name, &cancel);
                    let _ = sender.send(HostReply::Probe {
                        request,
                        name,
                        result,
                    });
                });
                Ok(None)
            }
            Effect::CancelFollowerProbe => {
                let mut hosts = hosts.borrow_mut();
                hosts.cancel();
                hosts.resume_interrupted_inspection(model);
                Ok(None)
            }
            Effect::StartFollowerSync(alias) => {
                hosts.borrow_mut().start_sync(paths, model, alias);
                Ok(None)
            }
            Effect::CancelFollowerSync => {
                hosts.borrow_mut().cancel_sync(model);
                Ok(None)
            }
            Effect::FinishFollowerInit => {
                let mut hosts = hosts.borrow_mut();
                if hosts.offer_initial_sync(model) {
                    return Ok(None);
                }
                if let Some((destination, host_to)) = pending_switch.take() {
                    if let Some(index) = host_to {
                        hosts.select_host(model, index, library_error.as_deref());
                        Ok(None)
                    } else {
                        Ok(Some(InteractionExit::Scope(destination)))
                    }
                } else if model.exit_after_save {
                    Ok(Some(InteractionExit::Exit(u8::from(hosts.sync_failed))))
                } else {
                    Ok(None)
                }
            }
            Effect::DirectoryChanged { from, to } => {
                model.exit_after_save = false;
                pending_switch = None;
                let mut hosts = hosts.borrow_mut();
                hosts.stash_browse(model, from);
                if from == 0 {
                    hosts.local_rows = model.rows.clone();
                }
                hosts.select_host(model, to, library_error.as_deref());
                Ok(None)
            }
            Effect::SwitchToScope {
                destination,
                host_to,
                save,
                discard,
            } => {
                if !save {
                    model.exit_after_save = false;
                    pending_switch = None;
                }
                let mut hosts = hosts.borrow_mut();
                hosts.stash_browse(model, model.host_index);
                if discard {
                    hosts.cancel();
                    hosts.discard_registry();
                    model.host_labels = hosts.labels();
                    if model.host_index >= model.host_labels.len() {
                        model.host_index = 0;
                    }
                    model.host_dirty = false;
                    working_config = session.config.clone();
                    let mut refresh = refresh.borrow_mut();
                    refresh.invalidate();
                    let rows = initial_library_model(&session, refresh.snapshot.as_deref()).rows;
                    model.dirty = false;
                    replace_local_rows(model, &mut hosts, rows);
                    refresh.request(paths, &working_config);
                }
                if save {
                    model.exit_after_save = false;
                    if model.host_index == 0 {
                        hosts.local_rows = model.rows.clone();
                    }
                    let config = library_config_from_rows(&working_config, &hosts.local_rows)?;
                    let acquisitions = library_acquisitions_from_rows(&hosts.local_rows);
                    review_library_save(&session, model, config, acquisitions, &mut staged);
                    pending_switch = Some((destination, host_to));
                    return Ok(None);
                }
                if let Some(index) = host_to {
                    hosts.select_host(model, index, library_error.as_deref());
                    return Ok(None);
                }
                Ok(Some(InteractionExit::Scope(destination)))
            }
            Effect::Quit { status } => Ok(Some(InteractionExit::Exit(status))),
            Effect::PrepareSave { fast } => {
                if model.host_index == 0 {
                    hosts.borrow_mut().local_rows = model.rows.clone();
                }
                let local_rows = hosts.borrow().local_rows.clone();
                let config = library_config_from_rows(&working_config, &local_rows)?;
                let acquisitions = library_acquisitions_from_rows(&local_rows);
                if fast
                    && acquisitions.is_empty()
                    && library_fast_save_is_safe(&session.config, &config)
                {
                    let mut outcomes = Vec::new();
                    if model.dirty || session.first_run {
                        LibraryWorkflow::save_with_acquisitions(
                            paths,
                            &session,
                            &config,
                            &acquisitions,
                            true,
                        )?;
                        session = LibraryWorkflow::load(paths)?;
                        working_config = session.config.clone();
                        refresh_local_after_save(
                            paths,
                            &session,
                            model,
                            &mut hosts.borrow_mut(),
                            &mut refresh.borrow_mut(),
                        );
                        model.dirty = false;
                        outcomes.push("Library inventory: saved");
                    } else {
                        outcomes.push("Library inventory: unchanged");
                    }
                    let mut hosts = hosts.borrow_mut();
                    if hosts.registry.as_ref().is_some_and(HostRegistry::dirty) {
                        if let Err(error) = hosts.save_registry() {
                            model.overlay = Overlay::Diagnostic {
                                title: "Host registry save failed".to_owned(),
                                body: format!("{}.\n\n{error}", outcomes.join(" · ")),
                            };
                            return Ok(None);
                        }
                        model.host_dirty = false;
                    }
                    if hosts.offer_initial_sync(model) {
                        return Ok(None);
                    }
                    Ok(Some(InteractionExit::Exit(0)))
                } else {
                    review_library_save(&session, model, config, acquisitions, &mut staged);
                    Ok(None)
                }
            }
            Effect::CommitSave => {
                let mut outcomes = Vec::new();
                let mut local_failed = false;
                if let Some((config, acquisitions)) = staged.take() {
                    match LibraryWorkflow::save_with_acquisitions(
                        paths,
                        &session,
                        &config,
                        &acquisitions,
                        true,
                    ) {
                        Ok(_) => {
                            session = LibraryWorkflow::load(paths)?;
                            working_config = session.config.clone();
                            refresh_local_after_save(
                                paths,
                                &session,
                                model,
                                &mut hosts.borrow_mut(),
                                &mut refresh.borrow_mut(),
                            );
                            model.dirty = false;
                            outcomes.push("Library inventory: saved".to_owned());
                        }
                        Err(error) => {
                            staged = Some((config, acquisitions));
                            outcomes.push(format!("Library inventory: failed ({error})"));
                            local_failed = true;
                        }
                    }
                } else {
                    outcomes.push("Library inventory: unchanged".to_owned());
                }
                if hosts.borrow().registry.is_some() {
                    match hosts.borrow_mut().save_registry() {
                        Ok(saved) => {
                            model.host_dirty = false;
                            outcomes.push(
                                if saved {
                                    "Host registry: saved"
                                } else {
                                    "Host registry: unchanged"
                                }
                                .to_owned(),
                            );
                        }
                        Err(error) => outcomes.push(format!("Host registry: failed ({error})")),
                    }
                } else {
                    outcomes.push("Host registry: unavailable".to_owned());
                }
                let failure = local_failed || model.host_dirty;
                model.overlay = if failure {
                    Overlay::Diagnostic {
                        title: "Library save failed".to_owned(),
                        body: outcomes.join("\n\n"),
                    }
                } else {
                    Overlay::Notice(outcomes.join(" · "))
                };
                if failure {
                    pending_switch = None;
                    Ok(None)
                } else if !hosts.borrow().initial_sync.is_empty() {
                    model.overlay = Overlay::None;
                    hosts.borrow_mut().offer_initial_sync(model);
                    Ok(None)
                } else if let Some((destination, host_to)) = pending_switch.take() {
                    if let Some(index) = host_to {
                        hosts
                            .borrow_mut()
                            .select_host(model, index, library_error.as_deref());
                        Ok(None)
                    } else {
                        Ok(Some(InteractionExit::Scope(destination)))
                    }
                } else if model.exit_after_save {
                    Ok(Some(InteractionExit::Exit(0)))
                } else {
                    Ok(None)
                }
            }
            Effect::CancelSave => {
                staged = None;
                pending_switch = None;
                Ok(None)
            }
            Effect::Undo => {
                let (loaded, error) = load_library_for_tui(paths)?;
                let mut refresh = refresh.borrow_mut();
                refresh.invalidate();
                if session.fingerprint != loaded.fingerprint || library_error != error {
                    refresh.snapshot = None;
                }
                session = loaded;
                library_error = error;
                model.scope_error = library_error.clone();
                working_config = session.config.clone();
                let mut hosts = hosts.borrow_mut();
                hosts.cancel();
                hosts.discard_registry();
                model.host_syncing = false;
                model.host_dirty = false;
                model.host_labels = hosts.labels();
                let rows = if let Some(error) = &library_error {
                    vec![Row::diagnostic(error.clone())]
                } else {
                    initial_library_model(&session, refresh.snapshot.as_deref()).rows
                };
                model.dirty = false;
                replace_local_rows(model, &mut hosts, rows);
                if model.host_index >= model.host_labels.len() {
                    hosts.select_host(model, 0, library_error.as_deref());
                } else {
                    hosts.resume_interrupted_inspection(model);
                }
                if library_error.is_none() {
                    refresh.request(paths, &working_config);
                }
                model.overlay = Overlay::None;
                hosts.offer_initial_sync(model);
                Ok(None)
            }
            Effect::ChangeTargetTo(value) => {
                Ok(Some(InteractionExit::Target(PathBuf::from(value))))
            }
            Effect::ApplyLocationEdit { edit, value } => {
                let value = value.trim();
                if value.is_empty() {
                    model.overlay = Overlay::Notice("Folder path cannot be empty.".to_owned());
                    return Ok(None);
                }
                let previous_checks = library_skill_checks(&model.rows);
                working_config = library_config_from_rows(&working_config, &model.rows)?;
                let mut locations = working_config.locations().to_vec();
                if edit {
                    let Some(index) = model.selected_row().and_then(|row| row.location_index)
                    else {
                        model.overlay =
                            Overlay::Notice("Select a library folder row to edit.".to_owned());
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
                refresh.borrow_mut().invalidate();
                let snapshot = LibraryWorkflow::snapshot(paths, &working_config);
                model.rows = if edit {
                    library_rows(&working_config, &snapshot)
                } else {
                    library_rows_after_location_add(&working_config, &snapshot, &previous_checks)
                };
                model.selected = model.selected.min(model.rows.len().saturating_sub(1));
                model.dirty = true;
                Ok(None)
            }
            Effect::RefreshLibrary => {
                let mut refresh = refresh.borrow_mut();
                let completed = refresh.collect(paths);
                refresh.request(paths, &working_config);
                if let Err(error) = completed {
                    model.unavailable = Some(error);
                } else if model.host_index == 0 && hosts.borrow().registry_error.is_none() {
                    model.unavailable = Some("Refreshing Library inventory…".to_owned());
                }
                Ok(None)
            }
            Effect::ApplySourceKey(_value) => {
                model.overlay = Overlay::Notice(
                    "Source names come from their library folders and cannot be edited here."
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
                    model.overlay =
                        Overlay::Notice("Select a library folder row to remove.".to_owned());
                    return Ok(None);
                };
                working_config = library_config_from_rows(&working_config, &model.rows)?;
                let mut locations = working_config.locations().to_vec();
                locations.remove(index);
                working_config = LibraryConfig::new(locations).map_err(config_issues)?;
                refresh.borrow_mut().invalidate();
                let snapshot = LibraryWorkflow::snapshot(paths, &working_config);
                model.rows = library_rows(&working_config, &snapshot);
                model.selected = model.selected.min(model.rows.len().saturating_sub(1));
                model.dirty = true;
                Ok(None)
            }
            _ => Ok(None),
        },
        |model| {
            let mut hosts = tick_hosts.borrow_mut();
            let mut changed = hosts.poll(model);
            let mut refresh = tick_refresh.borrow_mut();
            if let Err(error) = refresh.collect(paths) {
                model.unavailable = Some(error);
                changed = true;
            }
            changed |= refresh.apply(model, &mut hosts);
            Ok(changed)
        },
    )?;
    {
        let mut hosts = tick_hosts.borrow_mut();
        hosts.stash_browse(&exited_model, exited_model.host_index);
        hosts.cancel();
        exited_model.host_syncing = false;
        browsing.library_browse = std::mem::take(&mut hosts.browse);
        browsing.library_host = exited_model
            .host_labels
            .get(exited_model.host_index)
            .cloned();
    }
    exited_model.overlay = Overlay::None;
    Ok(finish_library_navigation(
        browsing,
        LibraryView {
            session,
            working_config,
            error: library_error,
            model: exited_model,
            hosts,
            refresh,
        },
        exit,
        return_target,
    ))
}

fn finish_library_navigation(
    browsing: &mut SessionNavigation,
    view: LibraryView,
    exit: InteractionExit,
    return_target: Option<&Path>,
) -> Navigation {
    if matches!(exit, InteractionExit::Target(_)) {
        view.refresh.borrow_mut().invalidate();
        browsing.library = None;
        browsing.library_snapshot = None;
    } else {
        if let Some(snapshot) = view.refresh.borrow().snapshot.as_ref() {
            browsing.library_snapshot = Some((view.session.fingerprint.clone(), snapshot.clone()));
        }
        browsing.library = Some(view);
    }
    match exit {
        InteractionExit::Target(path) => Navigation::Target(path, Scope::Repo),
        InteractionExit::Scope(destination) => Navigation::Target(
            return_target.unwrap_or_else(|| Path::new(".")).to_owned(),
            destination,
        ),
        InteractionExit::Exit(status) => Navigation::Exit(status),
        InteractionExit::Reload => Navigation::Library {
            return_target: return_target.map(Path::to_owned),
        },
    }
}

pub(super) fn library_rows(config: &LibraryConfig, snapshot: &LibrarySnapshot) -> Vec<Row> {
    let mut rows = snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| Row::diagnostic(diagnostic.message.clone()))
        .collect::<Vec<_>>();
    for (index, location) in config.locations().iter().enumerate() {
        let mut location_row = Row::location(location.path());
        location_row.location_index = Some(index);
        location_row.description = if index == 0 {
            "Writable library folder".to_owned()
        } else {
            "Read-only library folder".to_owned()
        };
        if let Some(observed) = snapshot.locations().get(index) {
            location_row.details = format!(
                "Configured path: {} · Full path: {} · {}",
                observed.expression(),
                observed
                    .resolved()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "unavailable".to_owned()),
                if observed.available() {
                    "available"
                } else {
                    "unavailable"
                }
            );
        }
        if snapshot.location_has_overlap_advisory(index) {
            location_row.details.push_str(
                " · WARNING: overlaps another library folder; allowed in the configuration",
            );
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
                index,
                source.relative_path().to_owned(),
                source.available(),
                source.key_collision(),
            );
            let source_kind = match source.kind() {
                crate::library::SourceKind::Local => "Local folder",
                crate::library::SourceKind::Git => "Git repository",
                crate::library::SourceKind::Unknown => "Unknown source",
            };
            source_row.description = format!(
                "{} · {} skill{}",
                source_kind,
                skill_count,
                if skill_count == 1 { "" } else { "s" }
            );
            source_row.details = format!(
                "{} · Folder: {} · Git remote: {}{}",
                source_kind,
                source
                    .root()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "unavailable".to_owned()),
                source.origin().unwrap_or("none"),
                if snapshot.location_has_overlap_advisory(index) {
                    " · WARNING: library folder overlap"
                } else {
                    ""
                }
            );
            if source.key_collision() {
                source_row.state = "Duplicate name".to_owned();
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
                row.warnings = skill.warnings().to_vec();
                row.metadata_errors = skill.diagnostics().to_vec();
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

fn library_rows_after_location_add(
    config: &LibraryConfig,
    snapshot: &LibrarySnapshot,
    previous_checks: &BTreeMap<(String, String), (CheckState, CheckState)>,
) -> Vec<Row> {
    let mut rows = library_rows(config, snapshot);
    preserve_library_checks_after_location_add(&mut rows, previous_checks);
    rows
}

pub(super) fn library_skill_checks(
    rows: &[Row],
) -> BTreeMap<(String, String), (CheckState, CheckState)> {
    rows.iter()
        .filter(|row| row.kind == RowKind::Skill && row.valid)
        .filter_map(|row| {
            Some((
                (row.inventory_id.clone()?, row.skill_path.clone()?),
                (row.check?, row.initial_check?),
            ))
        })
        .collect()
}

pub(super) fn preserve_library_checks_after_location_add(
    rows: &mut Vec<Row>,
    previous_checks: &BTreeMap<(String, String), (CheckState, CheckState)>,
) {
    for row in rows.iter_mut().filter(|row| row.kind == RowKind::Skill) {
        if !row.valid {
            continue;
        }
        let key = row.inventory_id.clone().zip(row.skill_path.clone());
        let (check, initial_check) = key
            .and_then(|key| previous_checks.get(&key).copied())
            .unwrap_or((CheckState::Unchecked, CheckState::Unchecked));
        row.check = Some(check);
        row.initial_check = Some(initial_check);
        refresh_library_visibility(row);
    }
    let groups = rows
        .iter()
        .filter(|row| row.kind == RowKind::Source)
        .filter_map(row_identity)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut model = Model::new(Workspace::Library, std::mem::take(rows));
    for group in groups {
        model.recompute_group(&group);
    }
    for row in model
        .rows
        .iter_mut()
        .filter(|row| row.kind == RowKind::Source)
    {
        row.initial_check = row.check;
    }
    *rows = model.rows;
}

pub(super) fn library_config_from_rows(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_local_refresh_preserves_selected_skill_when_returning_from_a_follower() {
        let home = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_environment(home.path().to_owned(), BTreeMap::new());
        let catalog = home.path().join("catalog");
        let alpha = catalog.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        std::fs::write(
            alpha.join("SKILL.md"),
            "---\nname: alpha\ndescription: Selection fixture skill\n---\n",
        )
        .unwrap();
        std::fs::create_dir_all(paths.library_config().parent().unwrap()).unwrap();
        std::fs::write(
            paths.library_config(),
            "version: 1\nlocations:\n  - path: ~/catalog\n",
        )
        .unwrap();
        let session = LibraryWorkflow::load(&paths).unwrap();
        let original = LibraryWorkflow::snapshot(&paths, &session.config);
        let mut model = initial_library_model(&session, Some(&original));
        model.selected = model
            .rows
            .iter()
            .position(|row| row.name == "alpha")
            .unwrap();
        model.filter = "skill".to_owned();
        let mut hosts = HostUi::new(paths.home(), &model.rows);
        hosts.stash_browse(&model, 0);
        model.host_labels.push("follower".to_owned());
        model.host_index = 1;
        model.rows.clear();
        model.selected = 0;

        let earlier = catalog.join("aardvark");
        std::fs::create_dir(&earlier).unwrap();
        std::fs::write(
            earlier.join("SKILL.md"),
            "---\nname: aardvark\ndescription: Selection fixture skill\n---\n",
        )
        .unwrap();
        let snapshot = Arc::new(LibraryWorkflow::snapshot(&paths, &session.config));
        let mut refresh = LibraryRefresh {
            ready: Some(LibraryDiscovery {
                generation: 0,
                rows: library_rows(&session.config, &snapshot),
                snapshot,
            }),
            ..LibraryRefresh::default()
        };
        assert!(refresh.apply(&mut model, &mut hosts));
        hosts.select_host(&mut model, 0, None);
        assert_eq!(model.selected_row().unwrap().name, "alpha");
        assert_eq!(model.filter, "skill");
        assert!(model.rows.iter().any(|row| row.name == "aardvark"));
    }

    #[test]
    fn confirmed_target_change_discards_library_edits_and_derived_inventory() {
        for change_target in [false, true] {
            let home = tempfile::tempdir().unwrap();
            let paths = AppPaths::with_environment(home.path().to_owned(), BTreeMap::new());
            let skill = home.path().join("catalog/alpha");
            std::fs::create_dir_all(&skill).unwrap();
            std::fs::write(
                skill.join("SKILL.md"),
                "---\nname: alpha\ndescription: Fixture\n---\n",
            )
            .unwrap();
            std::fs::create_dir_all(paths.library_config().parent().unwrap()).unwrap();
            let saved = b"version: 1\nlocations:\n  - path: ~/catalog\n";
            std::fs::write(paths.library_config(), saved).unwrap();
            let session = LibraryWorkflow::load(&paths).unwrap();
            let snapshot = Arc::new(LibraryWorkflow::snapshot(&paths, &session.config));
            let mut model = initial_library_model(&session, Some(&snapshot));
            let row = model
                .rows
                .iter_mut()
                .find(|row| row.name == "alpha")
                .unwrap();
            row.check = Some(CheckState::Unchecked);
            refresh_library_visibility(row);
            model.dirty = true;
            let working_config = library_config_from_rows(&session.config, &model.rows).unwrap();
            let hosts = Rc::new(RefCell::new(HostUi::new(paths.home(), &model.rows)));
            hosts
                .borrow_mut()
                .registry
                .as_mut()
                .unwrap()
                .stage("discarded-host", "verified-host")
                .unwrap();
            let refresh = Rc::new(RefCell::new(LibraryRefresh {
                snapshot: Some(Arc::new(LibraryWorkflow::snapshot(&paths, &working_config))),
                ..LibraryRefresh::default()
            }));
            let mut browsing = SessionNavigation::default();
            finish_library_navigation(
                &mut browsing,
                LibraryView {
                    session,
                    working_config,
                    error: None,
                    model,
                    hosts,
                    refresh,
                },
                if change_target {
                    InteractionExit::Target(home.path().join("next-repo"))
                } else {
                    InteractionExit::Scope(Scope::User)
                },
                None,
            );
            if change_target {
                assert!(browsing.library.is_none());
                assert!(browsing.library_snapshot.is_none());
                let session = LibraryWorkflow::load(&paths).unwrap();
                let snapshot = LibraryWorkflow::snapshot(&paths, &session.config);
                let restored = initial_library_model(&session, Some(&snapshot));
                assert_eq!(
                    restored
                        .rows
                        .iter()
                        .find(|row| row.name == "alpha")
                        .unwrap()
                        .check,
                    Some(CheckState::Checked)
                );
                assert!(
                    HostRegistry::load(paths.home())
                        .unwrap()
                        .followers()
                        .next()
                        .is_none()
                );
            } else {
                let retained = browsing.library.as_ref().unwrap();
                assert!(retained.model.dirty);
                assert_eq!(
                    retained
                        .model
                        .rows
                        .iter()
                        .find(|row| row.name == "alpha")
                        .unwrap()
                        .check,
                    Some(CheckState::Unchecked)
                );
                assert!(retained.hosts.borrow().registry.as_ref().unwrap().dirty());
                assert!(browsing.library_snapshot.is_some());
            }
            assert_eq!(std::fs::read(paths.library_config()).unwrap(), saved);
            assert!(!home.path().join(".skillator/config.yaml").exists());
        }
    }
}
