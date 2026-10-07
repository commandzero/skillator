//! Shared User/Repo directory editing, loading and save lifecycle.
use super::model::{
    BrowseState, CheckState, Effect, Model, Overlay, Row, RowKind, Scope, SkillInventoryRow,
    TargetTabScope, Workspace, row_identity, same_row_identity,
};
use super::{
    AppTerminal, InteractionExit, Navigation, SessionNavigation, fatal, invalid, library, repo,
    run_interactive_with_tick, user,
};
use crate::app::{
    AppPaths, LibraryWorkflow, PreparedTargetSave, PreparedUserScopeSave, ReportStatus,
    TargetSession, TargetWorkflow, UserScopeSession, UserScopeWorkflow, WorkflowError,
};
use crate::config::{LibraryConfig, RepositoryConfig, SkillDirectoryConfig};
use crate::domain::{
    Enablement, MaterializationKind, RepositoryRelativePath, SkillDirectoryKey, SkillKey,
    SkillPath, SourceKey,
};
use crate::library::{LibrarySnapshot, SkillValidity};
use crate::reconcile::Authorization;
use crate::target::{MaterializationState, ObservedState, Target};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, mpsc};

#[derive(Clone)]
pub(super) struct TargetTab {
    pub(super) scope: TargetTabScope,
    pub(super) directory: SkillDirectoryConfig,
    pub(super) rows: Vec<Row>,
}

pub(super) struct LoadedTargetState {
    pub(super) user: UserScopeSession,
    pub(super) repository: TargetSession,
    pub(super) tabs: Vec<TargetTab>,
}

pub(super) struct TargetData {
    pub(super) state: LoadedTargetState,
    pub(super) library_session: crate::app::LibrarySession,
    pub(super) library: Arc<LibrarySnapshot>,
    pub(super) library_error: Option<String>,
    pub(super) user_error: Option<String>,
    pub(super) repository_error: Option<String>,
}

pub(super) struct TargetView {
    pub(super) model: Model,
    pub(super) ui: Rc<RefCell<TargetUi>>,
}

pub(super) struct TargetUi {
    pub(super) directory: PathBuf,
    pub(super) loaded: Option<TargetData>,
    pub(super) dirty_scopes: BTreeSet<TargetTabScope>,
    pub(super) refresh: TargetRefresh,
}

pub(super) struct TargetRequest {
    pub(super) directory: PathBuf,
}

pub(super) struct TargetReply {
    generation: u64,
    result: Result<TargetData, WorkflowError>,
}

pub(super) struct TargetScan {
    generation: u64,
    pub(super) receiver: mpsc::Receiver<TargetReply>,
}

#[derive(Default)]
pub(super) struct TargetRefresh {
    generation: u64,
    pub(super) flight: Option<TargetScan>,
    requested: Option<TargetRequest>,
    ready: Option<Result<TargetData, WorkflowError>>,
}

impl TargetRefresh {
    pub(super) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.requested = None;
        self.ready = None;
    }

    pub(super) fn request(&mut self, paths: &AppPaths, request: TargetRequest) {
        self.invalidate();
        self.requested = Some(request);
        self.start(paths);
    }

    fn start(&mut self, paths: &AppPaths) {
        if self.flight.is_some() {
            return;
        }
        let Some(request) = self.requested.take() else {
            return;
        };
        let paths = paths.clone();
        let generation = self.generation;
        let (sender, receiver) = mpsc::channel();
        self.flight = Some(TargetScan {
            generation,
            receiver,
        });
        std::thread::spawn(move || {
            let result = load_target_data(&paths, &request.directory);
            let _ = sender.send(TargetReply { generation, result });
        });
    }

    pub(super) fn complete(&mut self, paths: &AppPaths, reply: TargetReply) {
        self.flight = None;
        if reply.generation == self.generation {
            self.ready = Some(reply.result);
        }
        self.start(paths);
    }

    fn collect(&mut self, paths: &AppPaths) {
        let Some(flight) = &self.flight else {
            return;
        };
        match flight.receiver.try_recv() {
            Ok(reply) => self.complete(paths, reply),
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                let current = flight.generation == self.generation;
                self.flight = None;
                self.start(paths);
                if current {
                    self.ready = Some(Err(fatal(
                        "Target inventory refresh failed; retained state is unchanged.",
                    )));
                }
            }
        }
    }
}

impl TargetUi {
    pub(super) fn activate(&self, model: &mut Model, scope: Scope, paths: &AppPaths) {
        if let Some(data) = &self.loaded {
            let git_available = data.state.repository.target.git_repository().is_some();
            model.target_path = git_available
                .then(|| user_relative_path(data.state.repository.target.root(), paths.home()));
            activate_directory_scope(
                model,
                &data.state.tabs,
                &self.dirty_scopes,
                scope,
                git_available,
                if scope == Scope::User {
                    data.user_error.as_deref()
                } else {
                    data.repository_error.as_deref()
                },
            );
        } else {
            model.scope = scope;
            model.rows.clear();
            model.dirty = false;
            model.scope_error = Some("Loading User/Repo inventory…".to_owned());
            model.unavailable = model.scope_error.clone();
        }
    }

    pub(super) fn apply(&mut self, model: &mut Model, paths: &AppPaths) -> bool {
        if model.dirty || !self.dirty_scopes.is_empty() || model.overlay != Overlay::None {
            return false;
        }
        let Some(result) = self.refresh.ready.take() else {
            return false;
        };
        let data = match result {
            Ok(data) => data,
            Err(error) => {
                model.scope_error = Some(error.to_string());
                model.unavailable = model.scope_error.clone();
                return true;
            }
        };
        let first_load = self.loaded.is_none();
        let filter = first_load.then(|| model.filter.clone());
        if let Some(previous) = &self.loaded {
            stash_target_browse(model, &previous.state.tabs, model.directory_index);
            for ((scope, key), browse) in &mut model.browse {
                let old = previous
                    .state
                    .tabs
                    .iter()
                    .find(|tab| tab.scope == *scope && tab.directory.key().as_str() == key);
                let new = data
                    .state
                    .tabs
                    .iter()
                    .find(|tab| tab.scope == *scope && tab.directory.key().as_str() == key);
                if let Some(new) = new {
                    browse.selected = old
                        .and_then(|tab| tab.rows.get(browse.selected))
                        .and_then(|selected| {
                            new.rows
                                .iter()
                                .position(|row| same_row_identity(row, selected))
                        })
                        .unwrap_or(browse.selected)
                        .min(new.rows.len().saturating_sub(1));
                }
            }
        }
        if data.state.repository.target.git_repository().is_some() {
            self.directory = data.state.repository.target.root().to_owned();
        }
        sync_target_tab_model(model, &data.state.tabs, &self.dirty_scopes);
        self.loaded = Some(data);
        self.activate(model, model.scope, paths);
        if let Some(filter) = filter {
            model.filter = filter;
        }
        true
    }
}

pub(super) fn load_target_data(
    paths: &AppPaths,
    directory: &Path,
) -> Result<TargetData, WorkflowError> {
    let (library_session, library_error) = library::load_library_for_tui(paths)?;
    let library = Arc::new(LibraryWorkflow::snapshot(paths, &library_session.config));
    let (state, user_error, repository_error) = reload_target_for_tui(
        paths,
        directory,
        &library,
        &library_session.config,
        library_error.as_deref(),
    )?;
    Ok(TargetData {
        state,
        library_session,
        library,
        library_error,
        user_error,
        repository_error,
    })
}

pub(super) enum PreparedScopeSave {
    User(PreparedUserScopeSave),
    Repository(PreparedTargetSave),
}

impl PreparedScopeSave {
    pub(super) fn plan(&self) -> &crate::reconcile::Plan {
        match self {
            Self::User(prepared) => prepared.plan(),
            Self::Repository(prepared) => prepared.plan(),
        }
    }

    pub(super) fn commit(
        self,
        paths: &AppPaths,
    ) -> Result<crate::app::CommandReport, WorkflowError> {
        let authorization = if self.plan().has_guarded() {
            Authorization::AllGuarded
        } else {
            Authorization::SafeOnly
        };
        match self {
            Self::User(prepared) => UserScopeWorkflow::commit_save(paths, prepared, authorization),
            Self::Repository(prepared) => {
                TargetWorkflow::commit_save_registered(paths, prepared, authorization)
            }
        }
    }
}

fn reload_target_for_tui(
    paths: &AppPaths,
    directory: &Path,
    library: &LibrarySnapshot,
    library_config: &LibraryConfig,
    library_error: Option<&str>,
) -> Result<(LoadedTargetState, Option<String>, Option<String>), WorkflowError> {
    let (user, user_error) = user::load_user_for_tui(paths)?;
    let (repository, repository_error) = repo::load_repository_for_tui(paths, directory)?;
    let mut state = build_target_state(user, repository, library, library_config);
    if let Some(error) = library_error {
        for tab in &mut state.tabs {
            tab.rows
                .push(Row::diagnostic(format!("Library unavailable: {error}")));
        }
    }
    Ok((state, user_error, repository_error))
}

fn activate_directory_scope(
    model: &mut Model,
    tabs: &[TargetTab],
    dirty_scopes: &BTreeSet<TargetTabScope>,
    destination: Scope,
    git_available: bool,
    error: Option<&str>,
) {
    model.scope = destination;
    model.scope_error = error.map(str::to_owned);
    let scope = if destination == Scope::User {
        TargetTabScope::User
    } else {
        TargetTabScope::Repository
    };
    if let Some(index) = preferred_target_index(model, tabs, scope)
        && model.scope_error.is_none()
        && (destination != Scope::Repo || git_available)
    {
        activate_target_tab(model, tabs, dirty_scopes, index);
        model.unavailable = None;
    } else {
        model.rows.clear();
        model.selected = 0;
        model.dirty = false;
        model.unavailable = model.scope_error.clone().or_else(|| {
            Some(if destination == Scope::Repo && !git_available {
                "Repo unavailable outside a Git worktree; press t to choose one.".to_owned()
            } else {
                "No configured skill directories; press Ctrl+T to add one.".to_owned()
            })
        });
    }
}

pub(super) fn run_target_once(
    paths: &AppPaths,
    directory: &Path,
    starting_scope: Scope,
    onboard: bool,
    browsing: &mut SessionNavigation,
    terminal: &mut AppTerminal,
) -> Result<Navigation, WorkflowError> {
    let directory = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_owned());
    let (mut model, ui) = match browsing.target.take() {
        Some(view) => (view.model, view.ui),
        None => (
            Model::new(Workspace::Target, Vec::new()),
            Rc::new(RefCell::new(TargetUi {
                directory: directory.clone(),
                loaded: None,
                dirty_scopes: BTreeSet::new(),
                refresh: TargetRefresh::default(),
            })),
        ),
    };
    {
        let mut ui = ui.borrow_mut();
        if ui.directory != directory {
            ui.directory = directory.clone();
            ui.loaded = None;
            ui.dirty_scopes.clear();
            ui.refresh.invalidate();
            model = Model::new(Workspace::Target, Vec::new());
        }
        if onboard {
            let data = load_target_data(paths, &directory)?;
            let git_available = data.state.repository.target.git_repository().is_some();
            let at_home = paths.home().canonicalize().ok().as_ref() == Some(&directory);
            let scope = if at_home || !git_available {
                Scope::User
            } else {
                Scope::Repo
            };
            model = initial_target_model(&data.state.tabs);
            if git_available {
                ui.directory = data.state.repository.target.root().to_owned();
            }
            ui.loaded = Some(data);
            ui.activate(&mut model, scope, paths);
        } else {
            if ui.loaded.is_none() || model.scope != starting_scope {
                ui.activate(&mut model, starting_scope, paths);
            }
            ui.refresh.collect(paths);
            ui.refresh.request(
                paths,
                TargetRequest {
                    directory: directory.clone(),
                },
            );
        }
    }
    let tick_ui = ui.clone();
    let mut pending: Option<PreparedScopeSave> = None;
    let mut navigate_after_save: Option<Scope> = None;
    let (exit, mut exited_model) = run_interactive_with_tick(
        terminal,
        model,
        |model, effect| {
            let mut ui = ui.borrow_mut();
            let TargetUi {
                loaded,
                dirty_scopes,
                refresh,
                ..
            } = &mut *ui;
            let Some(TargetData {
                state,
                library_session,
                library,
                library_error,
                user_error,
                repository_error,
            }) = loaded
            else {
                return match effect {
                    Effect::Quit { status } => Ok(Some(InteractionExit::Exit(status))),
                    Effect::SwitchToScope {
                        destination: Scope::Library,
                        ..
                    } => Ok(Some(InteractionExit::Scope(Scope::Library))),
                    Effect::SwitchToScope { destination, .. } => {
                        model.scope = destination;
                        Ok(None)
                    }
                    Effect::ChangeTargetTo(value) => {
                        Ok(Some(InteractionExit::Target(PathBuf::from(value))))
                    }
                    _ => {
                        model.overlay = Overlay::Notice("Loading User/Repo inventory…".to_owned());
                        Ok(None)
                    }
                };
            };
            let git_available = state.repository.target.git_repository().is_some();
            let reload_edited = |model: &mut Model,
                                 state: &mut LoadedTargetState,
                                 user_error: &mut Option<String>,
                                 repository_error: &mut Option<String>,
                                 dirty_scopes: &mut BTreeSet<TargetTabScope>,
                                 refresh: &mut TargetRefresh|
             -> Result<(), WorkflowError> {
                refresh.invalidate();
                let (reloaded, loaded_user_error, loaded_repository_error) = reload_target_for_tui(
                    paths,
                    &directory,
                    library,
                    &library_session.config,
                    library_error.as_deref(),
                )?;
                *state = reloaded;
                *user_error = loaded_user_error;
                *repository_error = loaded_repository_error;
                dirty_scopes.clear();
                sync_reloaded_target_model(
                    model,
                    state,
                    dirty_scopes,
                    user_error.as_deref(),
                    repository_error.as_deref(),
                );
                Ok(())
            };
            match effect {
                Effect::Quit { status } => Ok(Some(InteractionExit::Exit(status))),
                Effect::StartFollowerProbe(_)
                | Effect::CancelFollowerProbe
                | Effect::StartFollowerSync(_)
                | Effect::CancelFollowerSync
                | Effect::FinishFollowerInit => Ok(None),
                Effect::SwitchToScope {
                    destination,
                    save,
                    discard,
                    ..
                } => {
                    stash_target_browse(model, &state.tabs, model.directory_index);
                    store_active_target_tab(model, &mut state.tabs);
                    if save {
                        let Some(prepared) = prepare_scope_save_handling_busy(
                            paths,
                            state,
                            active_target_scope(model),
                            model,
                        )?
                        else {
                            return Ok(None);
                        };
                        if !plan_is_safe(prepared.plan()) {
                            model.overlay = save_review_overlay(prepared.plan());
                            navigate_after_save = Some(destination);
                            pending = Some(prepared);
                            return Ok(None);
                        }
                        let report = prepared.commit(paths)?;
                        if report.status != ReportStatus::InSync {
                            show_save_result(model, &report);
                            return Ok(None);
                        }
                    }
                    if save || discard {
                        reload_edited(
                            model,
                            state,
                            user_error,
                            repository_error,
                            dirty_scopes,
                            refresh,
                        )?;
                    }
                    if destination == Scope::Library {
                        return Ok(Some(InteractionExit::Scope(Scope::Library)));
                    }
                    activate_directory_scope(
                        model,
                        &state.tabs,
                        dirty_scopes,
                        destination,
                        git_available,
                        if destination == Scope::User {
                            user_error.as_deref()
                        } else {
                            repository_error.as_deref()
                        },
                    );
                    Ok(None)
                }
                Effect::DirectoryChanged { from, to } => {
                    if let Some(tab) = state.tabs.get_mut(from) {
                        tab.rows = model.rows.clone();
                        if model.dirty {
                            dirty_scopes.insert(tab.scope);
                        }
                    }
                    stash_target_browse(model, &state.tabs, from);
                    activate_target_tab(model, &state.tabs, dirty_scopes, to);
                    Ok(None)
                }
                Effect::PrepareSave { fast } => {
                    if let Some(error) = &model.scope_error {
                        model.overlay = Overlay::Notice(error.clone());
                        return Ok(None);
                    }
                    if model.scope == Scope::Repo && !git_available {
                        model.overlay = Overlay::Notice(
                            "Repo unavailable: choose a Git worktree with t.".to_owned(),
                        );
                        return Ok(None);
                    }
                    store_active_target_tab(model, &mut state.tabs);
                    let Some(prepared) = prepare_scope_save_handling_busy(
                        paths,
                        state,
                        active_target_scope(model),
                        model,
                    )?
                    else {
                        return Ok(None);
                    };
                    if fast && plan_is_safe(prepared.plan()) {
                        let report = prepared.commit(paths)?;
                        if report.status == ReportStatus::InSync {
                            refresh.invalidate();
                            Ok(Some(InteractionExit::Exit(0)))
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
                    let Some(prepared) = prepare_scope_save_handling_busy(
                        paths,
                        state,
                        active_target_scope(model),
                        model,
                    )?
                    else {
                        return Ok(None);
                    };
                    model.overlay = save_review_overlay(prepared.plan());
                    pending = Some(prepared);
                    Ok(None)
                }
                Effect::CommitSave => {
                    let Some(prepared) = pending.take() else {
                        return Ok(None);
                    };
                    let report = prepared.commit(paths)?;
                    if report.status != ReportStatus::InSync {
                        navigate_after_save = None;
                        show_save_result(model, &report);
                        return Ok(None);
                    }
                    refresh.invalidate();
                    if let Some(destination) = navigate_after_save.take() {
                        reload_edited(
                            model,
                            state,
                            user_error,
                            repository_error,
                            dirty_scopes,
                            refresh,
                        )?;
                        if destination == Scope::Library {
                            return Ok(Some(InteractionExit::Scope(Scope::Library)));
                        }
                        activate_directory_scope(
                            model,
                            &state.tabs,
                            dirty_scopes,
                            destination,
                            git_available,
                            if destination == Scope::User {
                                user_error.as_deref()
                            } else {
                                repository_error.as_deref()
                            },
                        );
                        Ok(None)
                    } else {
                        Ok(Some(if model.exit_after_save {
                            InteractionExit::Exit(0)
                        } else {
                            InteractionExit::Reload
                        }))
                    }
                }
                Effect::CancelSave => {
                    pending.take();
                    navigate_after_save.take();
                    Ok(None)
                }
                Effect::ApplyDirectoryEdit { edit, value } => {
                    if model.scope == Scope::Repo && !git_available {
                        model.overlay = Overlay::Notice(
                            "Repo unavailable: choose a Git worktree with t.".to_owned(),
                        );
                        return Ok(None);
                    }
                    let scope = active_target_scope(model);
                    let candidate = match if edit {
                        parse_directory_editor(&value)
                    } else {
                        parse_chooser_directory(&value, &state.tabs, scope)
                    } {
                        Ok(candidate) => candidate,
                        Err(message) => {
                            model.overlay = Overlay::Notice(message);
                            return Ok(None);
                        }
                    };
                    let root = if scope == TargetTabScope::User {
                        paths.home()
                    } else {
                        state.repository.target.root()
                    };
                    if let Err(message) =
                        validate_directory_containment(root, candidate.path().as_str())
                    {
                        model.overlay = Overlay::Notice(message);
                        return Ok(None);
                    }
                    if edit && state.tabs.is_empty() {
                        model.overlay = Overlay::Notice("No skill folder is selected.".to_owned());
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
                                "Disable all skills and save before changing this directory path."
                                    .to_owned(),
                            );
                            return Ok(None);
                        }
                    }
                    store_active_target_tab(model, &mut state.tabs);
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
                        let rows = match scope {
                            TargetTabScope::User => user::rows_for_new_directory(
                                &candidate,
                                &state.user,
                                library,
                                &library_session.config,
                            ),
                            TargetTabScope::Repository => repo::rows_for_new_directory(
                                &candidate,
                                &state.repository,
                                &state.user.config,
                                library,
                                &library_session.config,
                            ),
                        };
                        state.tabs.push(TargetTab {
                            scope,
                            rows,
                            directory: candidate,
                        });
                        model.directory_index = state.tabs.len() - 1;
                    }
                    dirty_scopes.insert(scope);
                    sync_target_tab_model(model, &state.tabs, dirty_scopes);
                    activate_target_tab(model, &state.tabs, dirty_scopes, model.directory_index);
                    model.unavailable = None;
                    Ok(None)
                }
                Effect::DeleteDirectory => {
                    delete_target_directory(model, &mut state.tabs, dirty_scopes);
                    Ok(None)
                }
                Effect::ChangeTargetTo(value) => {
                    Ok(Some(InteractionExit::Target(PathBuf::from(value))))
                }
                Effect::Undo => Ok(Some(InteractionExit::Reload)),
                Effect::ApplyLocationEdit { .. } => Ok(None),
                Effect::RefreshLibrary => Ok(None),
                Effect::ApplySourceKey(_) => Ok(None),
            }
        },
        |model| {
            let mut ui = tick_ui.borrow_mut();
            ui.refresh.collect(paths);
            Ok(ui.apply(model, paths))
        },
    )?;
    let return_directory;
    let scope = exited_model.scope;
    {
        let mut ui = tick_ui.borrow_mut();
        if let Some(data) = &mut ui.loaded {
            let selected_index = exited_model.directory_index;
            stash_target_browse(&mut exited_model, &data.state.tabs, selected_index);
            if data.library_error.is_none() {
                browsing.library_snapshot = Some((
                    data.library_session.fingerprint.clone(),
                    data.library.clone(),
                ));
            }
        }
        return_directory = ui.directory.clone();
        if matches!(exit, InteractionExit::Target(_) | InteractionExit::Reload) {
            ui.refresh.invalidate();
            ui.loaded = None;
            ui.dirty_scopes.clear();
            ui.activate(&mut exited_model, scope, paths);
        }
    }
    exited_model.overlay = Overlay::None;
    browsing.target = Some(TargetView {
        model: exited_model,
        ui,
    });
    match exit {
        InteractionExit::Target(directory) => Ok(Navigation::Target(directory, Scope::Repo)),
        InteractionExit::Scope(Scope::Library) => Ok(Navigation::Library {
            return_target: Some(return_directory),
        }),
        InteractionExit::Scope(destination) => {
            Ok(Navigation::Target(return_directory, destination))
        }
        InteractionExit::Reload => Ok(Navigation::Target(return_directory, scope)),
        InteractionExit::Exit(status) => Ok(Navigation::Exit(status)),
    }
}

fn sync_reloaded_target_model(
    model: &mut Model,
    state: &LoadedTargetState,
    dirty_scopes: &BTreeSet<TargetTabScope>,
    user_error: Option<&str>,
    repository_error: Option<&str>,
) {
    sync_target_tab_model(model, &state.tabs, dirty_scopes);
    activate_directory_scope(
        model,
        &state.tabs,
        dirty_scopes,
        model.scope,
        state.repository.target.git_repository().is_some(),
        if model.scope == Scope::User {
            user_error
        } else {
            repository_error
        },
    );
}

pub(super) fn build_target_state(
    user: UserScopeSession,
    repository: TargetSession,
    library: &LibrarySnapshot,
    library_config: &LibraryConfig,
) -> LoadedTargetState {
    let mut tabs = user::tabs(&user, library, library_config);
    tabs.extend(repo::tabs(
        &repository,
        &user.config,
        library,
        library_config,
    ));
    LoadedTargetState {
        user,
        repository,
        tabs,
    }
}

fn target_tab_label(tab: &TargetTab) -> String {
    let label = tab.directory.label().unwrap_or_default();
    if label.is_empty() || label == "User" {
        tab.directory
            .path()
            .as_str()
            .split('/')
            .next()
            .unwrap_or(tab.directory.key().as_str())
            .to_owned()
    } else {
        label.to_owned()
    }
}

pub(super) fn initial_target_tab_index(tabs: &[TargetTab]) -> usize {
    tabs.iter()
        .position(|tab| tab.scope == TargetTabScope::Repository)
        .unwrap_or(0)
}

pub(super) fn user_relative_path(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(relative) if relative.as_os_str().is_empty() => "~".to_owned(),
        Ok(relative) => format!("~/{}", relative.display()),
        Err(_) => path.display().to_string(),
    }
}

pub(super) fn initial_target_model(tabs: &[TargetTab]) -> Model {
    let index = initial_target_tab_index(tabs);
    let mut model = Model::new(
        Workspace::Target,
        tabs.get(index)
            .map(|tab| tab.rows.clone())
            .unwrap_or_default(),
    );
    model.directory_index = index;
    model.scope = if tabs
        .get(index)
        .is_some_and(|tab| tab.scope == TargetTabScope::User)
    {
        Scope::User
    } else {
        Scope::Repo
    };
    sync_target_tab_model(&mut model, tabs, &BTreeSet::new());
    model
}

fn sync_target_tab_model(
    model: &mut Model,
    tabs: &[TargetTab],
    dirty_scopes: &BTreeSet<TargetTabScope>,
) {
    model.directory_count = tabs.len().max(1);
    model.directory_labels = tabs.iter().map(target_tab_label).collect();
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

pub(super) fn stash_target_browse(model: &mut Model, tabs: &[TargetTab], index: usize) {
    if let Some(tab) = tabs.get(index)
        && (model.scope == Scope::User && tab.scope == TargetTabScope::User
            || model.scope == Scope::Repo && tab.scope == TargetTabScope::Repository)
    {
        let key = tab.directory.key().as_str().to_owned();
        model.browse.insert(
            (tab.scope, key.clone()),
            BrowseState {
                selected: model.selected,
                collapsed: model.collapsed.clone(),
                filter: model.filter.clone(),
            },
        );
        model.last_directory_keys.insert(tab.scope, key);
    }
}

fn preferred_target_index(
    model: &Model,
    tabs: &[TargetTab],
    scope: TargetTabScope,
) -> Option<usize> {
    model
        .last_directory_keys
        .get(&scope)
        .and_then(|key| {
            tabs.iter()
                .position(|tab| tab.scope == scope && tab.directory.key().as_str() == key)
        })
        .or_else(|| tabs.iter().position(|tab| tab.scope == scope))
}

pub(super) fn activate_target_tab(
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
    let tab = &tabs[model.directory_index];
    model.rows = tab.rows.clone();
    let key = (tab.scope, tab.directory.key().as_str().to_owned());
    if let Some(browse) = model.browse.get(&key) {
        model.selected = browse.selected.min(model.rows.len().saturating_sub(1));
        model.collapsed = browse.collapsed.clone();
        model.filter = browse.filter.clone();
    } else {
        model.selected = model.selected.min(model.rows.len().saturating_sub(1));
        model.collapsed.clear();
        model.filter.clear();
    }
    model.last_directory_keys.insert(tab.scope, key.1);
    model.dirty = dirty_scopes.contains(&tab.scope);
}

pub(super) fn store_active_target_tab(model: &Model, tabs: &mut [TargetTab]) {
    if let Some(tab) = tabs.get_mut(model.directory_index)
        && (model.scope == Scope::User && tab.scope == TargetTabScope::User
            || model.scope == Scope::Repo && tab.scope == TargetTabScope::Repository)
    {
        tab.rows = model.rows.clone();
    }
}

pub(super) fn delete_target_directory(
    model: &mut Model,
    tabs: &mut Vec<TargetTab>,
    dirty_scopes: &mut BTreeSet<TargetTabScope>,
) {
    store_active_target_tab(model, tabs);
    let scope = active_target_scope(model);
    if tabs.iter().filter(|tab| tab.scope == scope).count() <= 1 {
        model.overlay = Overlay::Notice(
            "Keep at least one skill folder for your user account and one for this repository."
                .to_owned(),
        );
        return;
    }
    let index = model.directory_index.min(tabs.len() - 1);
    if tabs[index].rows.iter().any(|row| {
        row.kind == RowKind::Skill
            && (row.check == Some(CheckState::Checked)
                || row.initial_check == Some(CheckState::Checked))
    }) {
        model.overlay = Overlay::Notice(
            "Disable all skills in this folder and save before removing the folder from the configuration."
                .to_owned(),
        );
        return;
    }
    let removed = tabs.remove(index);
    model
        .browse
        .remove(&(scope, removed.directory.key().as_str().to_owned()));
    dirty_scopes.insert(scope);
    let target = tabs
        .iter()
        .enumerate()
        .find(|(position, tab)| *position >= index && tab.scope == scope)
        .or_else(|| {
            tabs.iter()
                .enumerate()
                .rev()
                .find(|(_, tab)| tab.scope == scope)
        })
        .map(|(position, _)| position)
        .expect("deletion preserves a directory in the active scope");
    model.directory_index = target;
    sync_target_tab_model(model, tabs, dirty_scopes);
    activate_target_tab(model, tabs, dirty_scopes, target);
}

fn active_target_scope(model: &Model) -> TargetTabScope {
    if model.scope == Scope::Repo {
        TargetTabScope::Repository
    } else {
        TargetTabScope::User
    }
}

pub(super) fn scope_config(
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

pub(super) fn prepare_scope_save(
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
            repo::repository_skill_exceptions(tabs),
        )
        .map(PreparedScopeSave::Repository),
    }
}

fn prepare_scope_save_handling_busy(
    paths: &AppPaths,
    state: &LoadedTargetState,
    scope: TargetTabScope,
    model: &mut Model,
) -> Result<Option<PreparedScopeSave>, WorkflowError> {
    match prepare_scope_save(paths, state, &state.tabs, scope) {
        Ok(prepared) => Ok(Some(prepared)),
        Err(WorkflowError::Busy) => {
            model.overlay = Overlay::Busy;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn plan_is_safe(plan: &crate::reconcile::Plan) -> bool {
    plan.items()
        .iter()
        .all(|item| item.safety() == crate::reconcile::Safety::Safe)
}

pub(super) fn save_review_overlay(plan: &crate::reconcile::Plan) -> Overlay {
    if plan_is_safe(plan) {
        Overlay::ConfirmSave
    } else {
        let mut message = String::from("Review changes before saving:\n");
        for item in plan
            .items()
            .iter()
            .filter(|item| item.safety() != crate::reconcile::Safety::Safe)
        {
            message.push_str(&format!(
                "• {}: {}\n  {}\n",
                match item.safety() {
                    crate::reconcile::Safety::Guarded => "Needs confirmation",
                    crate::reconcile::Safety::Blocked => "Cannot change",
                    crate::reconcile::Safety::Safe => unreachable!("safe changes are filtered out"),
                },
                item.path().display(),
                item.reason()
            ));
        }
        if plan
            .items()
            .iter()
            .any(|item| item.safety() == crate::reconcile::Safety::Blocked)
        {
            message.push_str("\nItems marked 'Cannot change' will be skipped.\n");
        }
        message.push_str("\ny/Enter confirm and save · n/Esc return");
        Overlay::GuardedConfirmation(message)
    }
}

fn show_save_result(model: &mut Model, report: &crate::app::CommandReport) {
    let mut summary = String::from("Some changes still need attention:\n");
    summary.push_str(&crate::cli::render_text(
        report,
        crate::cli::ColorPolicy::Never,
    ));
    summary.push_str("\nPress Enter to exit.");
    model.overlay = Overlay::Result(summary);
}

pub(super) fn parse_chooser_directory(
    input: &str,
    tabs: &[TargetTab],
    scope: TargetTabScope,
) -> Result<SkillDirectoryConfig, String> {
    let path = RepositoryRelativePath::parse(input.trim()).map_err(|error| error.to_string())?;
    if tabs
        .iter()
        .any(|tab| tab.scope == scope && tab.directory.path() == &path)
    {
        return Err(format!(
            "Skill directory collision: `{path}` is already configured."
        ));
    }
    let agent = path.as_str().split('/').next().unwrap_or_default();
    let base = agent
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase() || character.is_ascii_digit() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let base = base.trim_matches('-');
    let base = if base.is_empty() { "custom" } else { base };
    let mut key = base.to_owned();
    let mut suffix = 2;
    while tabs
        .iter()
        .any(|tab| tab.scope == scope && tab.directory.key().as_str() == key)
    {
        key = format!("{base}-{suffix}");
        suffix += 1;
    }
    let label = agent.to_owned();
    Ok(SkillDirectoryConfig::new(
        SkillDirectoryKey::parse(key).map_err(|error| error.to_string())?,
        path,
        Some(label),
    ))
}

pub(super) fn validate_directory_containment(root: &Path, path: &str) -> Result<(), String> {
    let mut current = root.to_owned();
    for segment in path.split('/') {
        current.push(segment);
        match current.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "Skill directory cannot traverse a symbolic link: {}",
                    current.display()
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!(
                    "Skill directory path is not a directory: {}",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(format!("Cannot inspect {}: {error}", current.display())),
        }
    }
    Ok(())
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

pub(super) fn target_rows(
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

pub(super) fn rows_for_directory(
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
                        .unwrap_or("unavailable")
                        .to_owned(),
                    "Skill not found or unreadable".to_owned(),
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
                        .unwrap_or("unavailable")
                        .to_owned(),
                    "Enabled for your user account".to_owned(),
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
        rows.extend(repo::repository_skill_rows(directory, target, observation));
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
            " · Overlapping library folders may affect this skill"
        } else {
            ""
        };
        let mut row = if desired.is_none() && inherited {
            Row::inherited_user(source.clone(), name, description, available, "User account")
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
        if let Some(skill) = library.resolve(&skill_key) {
            row.warnings = skill.warnings().to_vec();
            row.metadata_errors = skill.diagnostics().to_vec();
            row.valid = skill.validity() == SkillValidity::Valid;
        }
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
        row.inherited_user = inherited;
        if inherited {
            row.available = available && library.resolve(&skill_key).is_some();
            row.details
                .push_str(" · also enabled for your user account");
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
        MaterializationState::DivergedCopy => "Copy has changed",
        MaterializationState::UnknownExpectedEntry | MaterializationState::Uninspectable => {
            "Unavailable"
        }
        MaterializationState::NoncanonicalLink => "Link needs updating",
        MaterializationState::BrokenLink => "Link destination missing",
        MaterializationState::MisdirectedLink => "Link points elsewhere",
        MaterializationState::CopyIneligible => "Cannot copy safely",
        MaterializationState::WrongKind => "Path already occupied",
        MaterializationState::ExpectedEntryCollision => "Duplicate name",
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

pub(super) fn repository_config_from_rows(
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
