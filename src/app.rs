//! Command-shaped application workflows.

mod selector;
pub use selector::SkillSelector;
mod paths;
pub use paths::AppPaths;
mod library_workflow;
pub use library_workflow::{LibrarySession, LibraryWorkflow};
mod registry;
pub use registry::TargetRegistryWorkflow;
use registry::{
    append_registration_preview, load_registry, register_target, report_permits_target_registration,
};
mod worktree;
pub use worktree::WorktreeSyncWorkflow;
use worktree::{ConfigurationGuard, configuration_blocked_report};
mod user_scope;
pub use user_scope::{PreparedUserScopeSave, UserScopeSession, UserScopeWorkflow};
mod reports;
use reports::*;

use crate::acquisition::{AcquisitionError, LibraryAcquisition, PreparedAcquisitions};
use crate::config::{
    Fingerprint, LibraryConfig, LoadResult, RepositoryConfig, RepositoryConfigCodec, SaveError,
    TargetRegistry, load_library, load_repository, load_target_registry, save_bytes, save_library,
    save_repository, save_target_registry,
};
use crate::domain::{MaterializationKind, SkillDirectoryKey, SkillKey, SkillPath, SourceKey};
use crate::library::{LibrarySnapshot, SkillValidity, expand_location, scan_library};
use crate::reconcile::{
    Action, ApplyResult, Authorization, Outcome, Plan, PreparedPlan, Safety, TargetBusy,
    TargetLocks, execute, prepare_apply, prepare_check, prepare_transition,
    prepare_transition_with_locks_and_repository_skills,
};
use crate::target::RepositorySkillExceptions;
use crate::target::Target;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const LOCAL_TARGET_CONFIG: &str = ".agents/skillator.yaml";

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("{message}")]
    InvalidInput { message: String },
    #[error("Another Skillator process is saving changes")]
    Busy,
    #[error("{message}")]
    Fatal { message: String },
    #[error("save was cancelled")]
    Cancelled,
}

impl WorkflowError {
    pub fn exit_status(&self) -> u8 {
        match self {
            Self::InvalidInput { .. } => 3,
            Self::Busy => 4,
            Self::Fatal { .. } => 5,
            Self::Cancelled => 0,
        }
    }
}

impl From<TargetBusy> for WorkflowError {
    fn from(_: TargetBusy) -> Self {
        Self::Busy
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    Check,
    Apply { force: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    InSync,
    NotConverged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportOutcome {
    Unchanged,
    WouldApply,
    WouldRequireForce,
    Applied,
    NotAuthorized,
    Blocked,
    Failed,
    RolledBack,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportChange {
    pub path: String,
    pub action: String,
    pub safety: String,
    pub outcome: ReportOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportDiagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandReport {
    pub format_version: u8,
    pub status: ReportStatus,
    pub exit_status: u8,
    pub mode: String,
    pub target: String,
    pub changes: Vec<ReportChange>,
    pub diagnostics: Vec<ReportDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibrarySkillReport {
    pub path: String,
    pub name: Option<String>,
    pub valid: bool,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibrarySourceReport {
    pub key: String,
    pub skills: Vec<LibrarySkillReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryInventoryReport {
    pub format_version: u8,
    pub filter: Option<String>,
    pub sources: Vec<LibrarySourceReport>,
    pub diagnostics: Vec<ReportDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryLocationReport {
    pub expression: String,
    pub resolved: Option<String>,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryLocationsReport {
    pub format_version: u8,
    pub locations: Vec<LibraryLocationReport>,
    pub diagnostics: Vec<ReportDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeEnablementReport {
    pub source: String,
    pub skill: String,
    pub materialized_name: Option<String>,
    pub materialization: String,
    pub resolution: String,
    pub observed_state: String,
    pub comparison: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeDirectoryReport {
    pub key: String,
    pub path: String,
    pub enablements: Vec<ScopeEnablementReport>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeEnablementsReport {
    pub format_version: u8,
    pub scope: String,
    pub root: String,
    pub directories: Vec<ScopeDirectoryReport>,
    pub diagnostics: Vec<ReportDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredTargetReport {
    pub path: String,
    pub status: String,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetRegistryReport {
    pub format_version: u8,
    pub targets: Vec<RegisteredTargetReport>,
    pub diagnostics: Vec<ReportDiagnostic>,
}

pub struct SyncWorkflow;

impl SyncWorkflow {
    pub fn run(
        paths: &AppPaths,
        target_path: impl AsRef<Path>,
        mode: SyncMode,
    ) -> Result<CommandReport, WorkflowError> {
        let target =
            Target::select(target_path.as_ref()).map_err(|error| WorkflowError::InvalidInput {
                message: error.to_string(),
            })?;
        let repository_path = target.root().join(".agents/skillator.yaml");
        validate_target_config_path(&target, &repository_path, "Repository")?;
        let repository = match load_repository(&repository_path).map_err(fatal)? {
            LoadResult::Missing => {
                return Err(WorkflowError::InvalidInput {
                    message: format!(
                        "repository configuration is missing at {}; run `skillator init {}` first",
                        repository_path.display(),
                        target.root().display()
                    ),
                });
            }
            LoadResult::Valid(loaded) => loaded.value().clone(),
            LoadResult::Unsupported { version, .. } => {
                return Err(WorkflowError::InvalidInput {
                    message: format!("unsupported repository configuration version {version}"),
                });
            }
            LoadResult::Invalid { issues } => {
                return Err(WorkflowError::InvalidInput {
                    message: format_issues("invalid repository configuration", &issues),
                });
            }
        };
        let (library, mut diagnostics) = load_library_snapshot(paths)?;
        match mode {
            SyncMode::Check => {
                let prepared = prepare_check(&target, &repository, &library)?;
                Ok(report_check(
                    target.root(),
                    prepared.plan(),
                    &mut diagnostics,
                ))
            }
            SyncMode::Apply { force } => {
                let prepared = prepare_apply(&target, &repository, &library)?;
                let result = execute(
                    prepared,
                    if force {
                        Authorization::AllGuarded
                    } else {
                        Authorization::SafeOnly
                    },
                    &target,
                    &repository,
                    &library,
                );
                Ok(report_apply(
                    target.root(),
                    &result,
                    force,
                    &mut diagnostics,
                ))
            }
        }
    }
}

fn load_library_snapshot(
    paths: &AppPaths,
) -> Result<(LibrarySnapshot, Vec<ReportDiagnostic>), WorkflowError> {
    let session = LibraryWorkflow::load(paths)?;
    let snapshot = LibraryWorkflow::snapshot(paths, &session.config);
    let diagnostics = library_diagnostics(&snapshot);
    Ok((snapshot, diagnostics))
}

fn library_diagnostics(snapshot: &LibrarySnapshot) -> Vec<ReportDiagnostic> {
    snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| ReportDiagnostic {
            code: diagnostic.code.to_owned(),
            severity: "warning".to_owned(),
            message: diagnostic.message.clone(),
            data: diagnostic.path.as_ref().map(|path| {
                BTreeMap::from([("path".to_owned(), path.to_string_lossy().into_owned())])
            }),
        })
        .collect()
}

fn simple_report(
    target: PathBuf,
    mode: SyncMode,
    changes: Vec<ReportChange>,
    diagnostics: Vec<ReportDiagnostic>,
) -> CommandReport {
    let preview_changes = mode == SyncMode::Check && !changes.is_empty();
    CommandReport {
        format_version: 1,
        status: if preview_changes {
            ReportStatus::NotConverged
        } else {
            ReportStatus::InSync
        },
        exit_status: if preview_changes { 1 } else { 0 },
        mode: match mode {
            SyncMode::Check => "check",
            SyncMode::Apply { .. } => "apply",
        }
        .to_owned(),
        target: target.to_string_lossy().into_owned(),
        changes,
        diagnostics,
    }
}

fn format_issues(prefix: &str, issues: &[crate::config::ConfigIssue]) -> String {
    let details = issues
        .iter()
        .map(|issue| format!("{}: {}", issue.path, issue.message))
        .collect::<Vec<_>>()
        .join("; ");
    format!("{prefix}: {details}")
}

fn fatal(error: impl std::fmt::Display) -> WorkflowError {
    WorkflowError::Fatal {
        message: error.to_string(),
    }
}

fn acquisition_error(error: AcquisitionError) -> WorkflowError {
    match error {
        AcquisitionError::Invalid(message) => WorkflowError::InvalidInput { message },
        AcquisitionError::Failed(message) | AcquisitionError::RecoveryRequired(message) => {
            WorkflowError::Fatal { message }
        }
    }
}

#[derive(Debug)]
pub struct TargetSession {
    pub target: Target,
    pub config: RepositoryConfig,
    pub fingerprint: Fingerprint,
    pub first_run: bool,
    pub recommendations: Vec<crate::config::SkillDirectoryConfig>,
}

pub struct TargetWorkflow;

pub struct PreparedTargetSave {
    planner: TargetStatePlanner,
}

impl PreparedTargetSave {
    pub fn plan(&self) -> &Plan {
        self.planner.plan()
    }

    fn check(self) -> CommandReport {
        self.planner.check_report()
    }

    fn rejects_apply(&self, force: bool) -> bool {
        self.plan().items().iter().any(|item| {
            item.safety() == Safety::Blocked || (item.safety() == Safety::Guarded && !force)
        })
    }

    fn rejected_apply_report(self) -> CommandReport {
        let mut report = self.check();
        report.mode = "apply".to_owned();
        for change in &mut report.changes {
            if change.outcome == ReportOutcome::WouldRequireForce {
                change.outcome = ReportOutcome::NotAuthorized;
            }
        }
        report
    }
}

struct TargetStateRequest {
    target: Target,
    original: RepositoryConfig,
    desired: RepositoryConfig,
    configuration_bytes: Vec<u8>,
    configuration_expected: Fingerprint,
    configuration_guard: Option<ConfigurationGuard>,
    repository_skills: RepositorySkillExceptions,
}

struct TargetStatePlanner {
    target: Target,
    desired: RepositoryConfig,
    configuration_bytes: Vec<u8>,
    configuration_expected: Fingerprint,
    configuration_guard: Option<ConfigurationGuard>,
    library: LibrarySnapshot,
    prepared: PreparedPlan,
    diagnostics: Vec<ReportDiagnostic>,
}

impl std::fmt::Debug for TargetStatePlanner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TargetStatePlanner")
            .field("target", &self.target.root())
            .field("desired", &self.desired)
            .field("configuration_expected", &self.configuration_expected)
            .field("prepared", &self.prepared)
            .finish_non_exhaustive()
    }
}

impl TargetStatePlanner {
    fn prepare(
        paths: &AppPaths,
        session: &TargetSession,
        desired: RepositoryConfig,
    ) -> Result<Self, WorkflowError> {
        Self::prepare_with_repository_skills(
            paths,
            session,
            desired,
            RepositorySkillExceptions::new(),
        )
    }

    fn prepare_with_repository_skills(
        paths: &AppPaths,
        session: &TargetSession,
        desired: RepositoryConfig,
        repository_skills: RepositorySkillExceptions,
    ) -> Result<Self, WorkflowError> {
        let configuration_bytes = RepositoryConfigCodec::render(&desired).map_err(fatal)?;
        let locks = TargetLocks::acquire(&[&session.target])?;
        Self::prepare_with_locks(
            paths,
            TargetStateRequest {
                target: session.target.clone(),
                original: session.config.clone(),
                desired,
                configuration_bytes: configuration_bytes.into_bytes(),
                configuration_expected: session.fingerprint.clone(),
                configuration_guard: None,
                repository_skills,
            },
            locks,
        )
    }

    fn prepare_with_locks(
        paths: &AppPaths,
        request: TargetStateRequest,
        locks: TargetLocks,
    ) -> Result<Self, WorkflowError> {
        let TargetStateRequest {
            target,
            original,
            desired,
            configuration_bytes,
            configuration_expected,
            configuration_guard,
            repository_skills,
        } = request;
        validate_target_config_path(
            &target,
            &target.root().join(LOCAL_TARGET_CONFIG),
            "Repository",
        )?;
        let (library, diagnostics) = load_library_snapshot(paths)?;
        let prepared = prepare_transition_with_locks_and_repository_skills(
            &target,
            &original,
            &desired,
            &library,
            &repository_skills,
            locks,
        )?;
        Ok(Self {
            target,
            desired,
            configuration_bytes,
            configuration_expected,
            configuration_guard,
            library,
            prepared,
            diagnostics,
        })
    }

    fn plan(&self) -> &Plan {
        self.prepared.plan()
    }

    fn check_report(mut self) -> CommandReport {
        let mut report = report_check(
            self.target.root(),
            self.prepared.plan(),
            &mut self.diagnostics,
        );
        if self.configuration_expected != Fingerprint::for_bytes(&self.configuration_bytes) {
            report.changes.push(ReportChange {
                path: LOCAL_TARGET_CONFIG.to_owned(),
                action: "write_target_configuration".to_owned(),
                safety: "safe".to_owned(),
                outcome: ReportOutcome::WouldApply,
            });
        }
        if !report.changes.is_empty() {
            report.status = ReportStatus::NotConverged;
            report.exit_status = 1;
        }
        report
    }

    fn commit(self, authorization: Authorization) -> Result<CommandReport, WorkflowError> {
        self.commit_internal(authorization, None)
    }

    fn commit_with_source(
        self,
        authorization: Authorization,
        source_path: &Path,
        source_expected: &Fingerprint,
    ) -> Result<CommandReport, WorkflowError> {
        self.commit_internal(authorization, Some((source_path, source_expected)))
    }

    fn configuration_validation_failure(
        &self,
        worktree_sync: bool,
        error: WorkflowError,
    ) -> Result<Option<CommandReport>, WorkflowError> {
        if worktree_sync {
            Ok(Some(configuration_blocked_report(
                self.target.root(),
                "worktree_sync",
                error.to_string(),
            )))
        } else {
            Err(error)
        }
    }

    fn validate_configuration_at_commit(
        &self,
        config_path: &Path,
        worktree_sync: bool,
    ) -> Result<Option<CommandReport>, WorkflowError> {
        if let Err(error) = validate_target_config_path(&self.target, config_path, "Repository") {
            return self.configuration_validation_failure(worktree_sync, error);
        }
        let config_facts = self
            .target
            .repository()
            .facts_for(Path::new(LOCAL_TARGET_CONFIG))
            .map_err(fatal)?;
        if configuration_has_index_changes(&config_facts) {
            let message = format!(
                "local skill configuration is still tracked; run `git rm --cached -- {LOCAL_TARGET_CONFIG}` before saving (Skillator never changes the Git index)"
            );
            return self.configuration_validation_failure(
                worktree_sync,
                WorkflowError::InvalidInput { message },
            );
        }

        if worktree_sync
            && let Err(message) = validate_destination_configuration_at_commit(
                config_path,
                &self.configuration_expected,
            )
        {
            return Ok(Some(configuration_blocked_report(
                self.target.root(),
                "worktree_sync",
                message,
            )));
        }
        let current_config = fingerprint_path(config_path).map_err(fatal)?;
        if current_config != self.configuration_expected {
            return if worktree_sync {
                Ok(Some(configuration_blocked_report(
                    self.target.root(),
                    "worktree_sync",
                    "linked worktree configuration changed while worktree sync was being prepared"
                        .to_owned(),
                )))
            } else {
                Err(save_error(SaveError::Stale))
            };
        }
        Ok(None)
    }

    fn commit_internal(
        self,
        authorization: Authorization,
        source: Option<(&Path, &Fingerprint)>,
    ) -> Result<CommandReport, WorkflowError> {
        if let Some((source_path, source_expected)) = source
            && source_configuration_changed(source_path, source_expected).map_err(fatal)?
        {
            return Ok(configuration_blocked_report(
                self.target.root(),
                "worktree_sync",
                "primary worktree configuration changed while worktree sync was being prepared"
                    .to_owned(),
            ));
        }
        let config_path = self.target.root().join(LOCAL_TARGET_CONFIG);

        if let Some(report) =
            self.validate_configuration_at_commit(&config_path, source.is_some())?
        {
            return Ok(report);
        }
        let configuration_changed =
            self.configuration_expected != Fingerprint::for_bytes(&self.configuration_bytes);
        if configuration_changed {
            validate_target_config_path(&self.target, &config_path, "Repository")?;
        }
        if configuration_changed {
            validate_target_config_path(&self.target, &config_path, "Repository")?;
            save_bytes(
                &config_path,
                &self.configuration_bytes,
                &self.configuration_expected,
            )
            .map_err(save_error)?;
        }

        let result = execute(
            self.prepared,
            authorization,
            &self.target,
            &self.desired,
            &self.library,
        );
        let mut diagnostics = self.diagnostics;
        let mut report = report_apply(
            self.target.root(),
            &result,
            authorization == Authorization::AllGuarded,
            &mut diagnostics,
        );
        if configuration_changed {
            report.changes.push(ReportChange {
                path: LOCAL_TARGET_CONFIG.to_owned(),
                action: "write_target_configuration".to_owned(),
                safety: if self.configuration_guard.is_some() {
                    "guarded"
                } else {
                    "safe"
                }
                .to_owned(),
                outcome: ReportOutcome::Applied,
            });
        }
        Ok(report)
    }
}

impl TargetWorkflow {
    pub fn inspect(
        paths: &AppPaths,
        target_path: impl AsRef<Path>,
    ) -> Result<ScopeEnablementsReport, WorkflowError> {
        let session = Self::load(target_path)?;
        if session.first_run {
            return Err(WorkflowError::InvalidInput {
                message: format!(
                    "Repository Configuration is missing at {}; run `skillator init {}` first",
                    session.target.root().join(LOCAL_TARGET_CONFIG).display(),
                    session.target.root().display()
                ),
            });
        }
        let (library, diagnostics) = load_library_snapshot(paths)?;
        Ok(scope_enablements_report(
            "target",
            (&session.target, &session.config),
            crate::target::observe(&session.target, &session.config, &library),
            diagnostics,
        ))
    }

    pub fn load(path: impl AsRef<Path>) -> Result<TargetSession, WorkflowError> {
        let target =
            Target::select(path.as_ref()).map_err(|error| WorkflowError::InvalidInput {
                message: error.to_string(),
            })?;
        let config_path = target.root().join(".agents/skillator.yaml");
        validate_target_config_path(&target, &config_path, "Repository")?;
        match load_repository(&config_path).map_err(fatal)? {
            LoadResult::Missing => {
                let recommendations = recognized_skill_directories(target.root());
                Ok(TargetSession {
                    target,
                    config: RepositoryConfig::first_run(),
                    fingerprint: Fingerprint::Absent,
                    first_run: true,
                    recommendations,
                })
            }
            LoadResult::Valid(loaded) => Ok(TargetSession {
                target,
                config: loaded.value().clone(),
                fingerprint: loaded.fingerprint().clone(),
                first_run: false,
                recommendations: Vec::new(),
            }),
            LoadResult::Unsupported { version, .. } => Err(WorkflowError::InvalidInput {
                message: format!("unsupported repository configuration version {version}"),
            }),
            LoadResult::Invalid { issues } => Err(WorkflowError::InvalidInput {
                message: format_issues("invalid repository configuration", &issues),
            }),
        }
    }

    pub fn save_configuration(
        session: &TargetSession,
        staged: &RepositoryConfig,
    ) -> Result<Fingerprint, WorkflowError> {
        let path = session.target.root().join(".agents/skillator.yaml");
        validate_target_config_path(&session.target, &path, "Repository")?;
        let _locks = TargetLocks::acquire(&[&session.target])?;
        let facts = session
            .target
            .repository()
            .facts_for(Path::new(LOCAL_TARGET_CONFIG))
            .map_err(fatal)?;
        if configuration_has_index_changes(&facts) {
            return Err(WorkflowError::InvalidInput {
                message: format!(
                    "local skill configuration is still tracked; run `git rm --cached -- {LOCAL_TARGET_CONFIG}` before saving (Skillator never changes the Git index)"
                ),
            });
        }
        if fingerprint_path(&path).map_err(fatal)? != session.fingerprint {
            return Err(save_error(SaveError::Stale));
        }
        save_repository(&path, staged, &session.fingerprint).map_err(save_error)
    }

    pub fn prepare_save(
        paths: &AppPaths,
        session: &TargetSession,
        staged: RepositoryConfig,
    ) -> Result<PreparedTargetSave, WorkflowError> {
        Ok(PreparedTargetSave {
            planner: TargetStatePlanner::prepare(paths, session, staged)?,
        })
    }

    pub fn prepare_save_with_repository_skills(
        paths: &AppPaths,
        session: &TargetSession,
        staged: RepositoryConfig,
        repository_skills: RepositorySkillExceptions,
    ) -> Result<PreparedTargetSave, WorkflowError> {
        Ok(PreparedTargetSave {
            planner: TargetStatePlanner::prepare_with_repository_skills(
                paths,
                session,
                staged,
                repository_skills,
            )?,
        })
    }

    pub fn commit_save(
        prepared: PreparedTargetSave,
        authorization: Authorization,
    ) -> Result<CommandReport, WorkflowError> {
        prepared.planner.commit(authorization)
    }

    pub fn commit_save_registered(
        paths: &AppPaths,
        prepared: PreparedTargetSave,
        authorization: Authorization,
    ) -> Result<CommandReport, WorkflowError> {
        let target = prepared.planner.target.clone();
        let report = Self::commit_save(prepared, authorization)?;
        if report_permits_target_registration(&report) {
            register_target(paths, &target)?;
        }
        Ok(report)
    }

    pub fn init(
        paths: &AppPaths,
        target_path: impl AsRef<Path>,
        mode: SyncMode,
    ) -> Result<CommandReport, WorkflowError> {
        let session = Self::load(target_path)?;
        if !session.first_run {
            if matches!(mode, SyncMode::Apply { .. }) {
                register_target(paths, &session.target)?;
            }
            return Ok(simple_report(
                session.target.root().to_owned(),
                mode,
                Vec::new(),
                Vec::new(),
            ));
        }
        let prepared = Self::prepare_save(paths, &session, session.config.clone())?;
        match mode {
            SyncMode::Check => {
                let mut report = prepared.check();
                append_registration_preview(paths, &session.target, &mut report)?;
                Ok(report)
            }
            SyncMode::Apply { force } => {
                if prepared.rejects_apply(force) {
                    Ok(prepared.rejected_apply_report())
                } else {
                    Self::commit_save_registered(
                        paths,
                        prepared,
                        if force {
                            Authorization::AllGuarded
                        } else {
                            Authorization::SafeOnly
                        },
                    )
                }
            }
        }
    }

    pub fn mutate_enablement(
        paths: &AppPaths,
        target_path: impl AsRef<Path>,
        selector: &SkillSelector,
        directory: Option<&str>,
        materialization: Option<MaterializationKind>,
        mode: SyncMode,
    ) -> Result<CommandReport, WorkflowError> {
        let session = Self::load(target_path)?;
        let staged = stage_enablement(
            paths,
            &session.config,
            selector,
            EnablementEdit {
                directory,
                materialization,
                configuration_name: "Repository Configuration",
            },
        )?;
        let prepared = Self::prepare_save(paths, &session, staged)?;
        match mode {
            SyncMode::Check => {
                let mut report = prepared.check();
                append_registration_preview(paths, &session.target, &mut report)?;
                annotate_selector(&mut report, selector);
                Ok(report)
            }
            SyncMode::Apply { force } => {
                let mut report = if prepared.rejects_apply(force) {
                    prepared.rejected_apply_report()
                } else {
                    Self::commit_save_registered(
                        paths,
                        prepared,
                        if force {
                            Authorization::AllGuarded
                        } else {
                            Authorization::SafeOnly
                        },
                    )?
                };
                report.mode = "apply".to_owned();
                annotate_selector(&mut report, selector);
                Ok(report)
            }
        }
    }
}

fn select_directory(
    config: &RepositoryConfig,
    requested: Option<&str>,
) -> Result<SkillDirectoryKey, WorkflowError> {
    if let Some(requested) = requested {
        let key =
            SkillDirectoryKey::parse(requested).map_err(|error| WorkflowError::InvalidInput {
                message: error.to_string(),
            })?;
        return config
            .skill_directories()
            .iter()
            .find(|directory| directory.key() == &key)
            .map(|directory| directory.key().clone())
            .ok_or_else(|| WorkflowError::InvalidInput {
                message: format!("unknown Skill Directory `{requested}`"),
            });
    }
    if let Some(agents) = config
        .skill_directories()
        .iter()
        .find(|directory| directory.key().as_str() == "agents")
    {
        return Ok(agents.key().clone());
    }
    if let [only] = config.skill_directories() {
        return Ok(only.key().clone());
    }
    Err(WorkflowError::InvalidInput {
        message: format!(
            "Skill Directory is ambiguous; choose --directory from: {}",
            config
                .skill_directories()
                .iter()
                .map(|directory| directory.key().as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    })
}

fn validate_destination_configuration_at_commit(
    path: &Path,
    expected: &Fingerprint,
) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return if expected == &Fingerprint::Absent {
                Ok(())
            } else {
                Err("linked worktree configuration became unavailable after planning".to_owned())
            };
        }
        Err(error) => {
            return Err(format!(
                "linked worktree configuration cannot be inspected after planning: {error}"
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(
            "linked worktree configuration changed to a non-regular file after planning".to_owned(),
        );
    }
    let bytes = fs::read(path).map_err(|error| {
        format!("linked worktree configuration is unreadable after planning: {error}")
    })?;
    if &Fingerprint::for_bytes(&bytes) != expected {
        return Err("linked worktree configuration changed after planning".to_owned());
    }
    Ok(())
}

fn fingerprint_path(path: &Path) -> Result<Fingerprint, std::io::Error> {
    match fs::read(path) {
        Ok(bytes) => Ok(Fingerprint::for_bytes(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Fingerprint::Absent),
        Err(error) => Err(error),
    }
}

fn source_configuration_changed(
    path: &Path,
    expected: &Fingerprint,
) -> Result<bool, std::io::Error> {
    Ok(fingerprint_path(path)? != *expected)
}

fn recognized_skill_directories(root: &Path) -> Vec<crate::config::SkillDirectoryConfig> {
    [
        crate::config::SkillDirectoryConfig::claude_preset(),
        crate::config::SkillDirectoryConfig::github_preset(),
        crate::config::SkillDirectoryConfig::cursor_preset(),
        crate::config::SkillDirectoryConfig::gemini_preset(),
    ]
    .into_iter()
    .filter(|directory| root.join(directory.path().as_str()).is_dir())
    .collect()
}

fn save_error(error: SaveError) -> WorkflowError {
    match error {
        SaveError::Stale => WorkflowError::InvalidInput {
            message: error.to_string(),
        },
        _ => fatal(error),
    }
}

fn validate_target_config_path(
    target: &Target,
    config_path: &Path,
    scope: &str,
) -> Result<(), WorkflowError> {
    if !validate_configuration_parent(target, scope)? {
        return Ok(());
    }
    match std::fs::symlink_metadata(config_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(WorkflowError::InvalidInput {
                message: format!(
                    "{scope} configuration must be a regular file, not a link: {}",
                    config_path.display()
                ),
            })
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(fatal(error)),
    }
}

fn load_target_repository(
    target: &Target,
    scope: &str,
) -> Result<LoadResult<RepositoryConfig>, WorkflowError> {
    let config_path = target.root().join(LOCAL_TARGET_CONFIG);
    validate_target_config_path(target, &config_path, scope)?;
    load_repository(&config_path).map_err(fatal)
}

impl LibraryWorkflow {
    pub fn save_with_acquisitions(
        paths: &AppPaths,
        session: &LibrarySession,
        staged: &LibraryConfig,
        acquisitions: &[LibraryAcquisition],
        confirmed: bool,
    ) -> Result<Fingerprint, WorkflowError> {
        if !confirmed {
            return Err(WorkflowError::Cancelled);
        }
        Self::save_confirmed_acquisitions(paths, session, staged, acquisitions)
    }
}

fn configuration_has_index_changes(facts: &crate::git::PathFacts) -> bool {
    facts.tracked || facts.staged || facts.unmerged
}

struct EnablementEdit<'a> {
    directory: Option<&'a str>,
    materialization: Option<MaterializationKind>,
    configuration_name: &'a str,
}

fn stage_enablement(
    paths: &AppPaths,
    config: &RepositoryConfig,
    selector: &SkillSelector,
    edit: EnablementEdit<'_>,
) -> Result<RepositoryConfig, WorkflowError> {
    let directory = select_directory(config, edit.directory)?;
    if edit.materialization.is_some() {
        validate_registered_selector(paths, selector)?;
    }
    let mut enablements = config.enablements().to_vec();
    enablements.retain(|enablement| {
        enablement.directory() != &directory || enablement.skill() != selector.key()
    });
    if let Some(kind) = edit.materialization {
        enablements.push(crate::domain::Enablement::new(
            directory,
            selector.key().clone(),
            kind,
        ));
    }
    config
        .with_enablements(enablements)
        .map_err(|issues| WorkflowError::InvalidInput {
            message: format_issues(&format!("invalid {}", edit.configuration_name), &issues),
        })
}

fn validate_registered_selector(
    paths: &AppPaths,
    selector: &SkillSelector,
) -> Result<(), WorkflowError> {
    let (snapshot, _) = load_library_snapshot(paths)?;
    if snapshot.resolve(selector.key()).is_none() {
        return Err(WorkflowError::InvalidInput {
            message: format!("Skill is not registered and valid: {selector}"),
        });
    }
    Ok(())
}

fn validate_configuration_parent(target: &Target, scope: &str) -> Result<bool, WorkflowError> {
    let parent = target.root().join(".agents");
    match std::fs::symlink_metadata(&parent) {
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(WorkflowError::InvalidInput {
                message: format!(
                    "{scope} configuration parent must be a directory, not a link: {}",
                    parent.display()
                ),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(fatal(error)),
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_skill_selector_uses_the_final_colon_boundary() {
        let selector = SkillSelector::parse("elastic/agent-skills:skills/esdiag").unwrap();
        assert_eq!(selector.key().source().as_str(), "elastic/agent-skills");
        assert_eq!(selector.key().path().as_str(), "skills/esdiag");
        assert!(SkillSelector::parse("esdiag").is_err());
        assert!(SkillSelector::parse("Elastic/agent-skills:skills/esdiag").is_err());
        assert!(SkillSelector::parse("elastic/agent-skills:../esdiag").is_err());
    }

    #[test]
    fn failed_target_reports_do_not_permit_registration() {
        let report_with = |outcome| CommandReport {
            format_version: 1,
            status: ReportStatus::NotConverged,
            exit_status: 1,
            mode: "apply".to_owned(),
            target: "/target".to_owned(),
            changes: vec![ReportChange {
                path: ".agents/skillator.yaml".to_owned(),
                action: "write_target_configuration".to_owned(),
                safety: "safe".to_owned(),
                outcome,
            }],
            diagnostics: Vec::new(),
        };

        for outcome in [
            ReportOutcome::NotAuthorized,
            ReportOutcome::Blocked,
            ReportOutcome::Failed,
            ReportOutcome::RolledBack,
            ReportOutcome::RecoveryRequired,
        ] {
            assert!(!report_permits_target_registration(&report_with(outcome)));
        }
        assert!(report_permits_target_registration(&report_with(
            ReportOutcome::Applied
        )));
    }

    #[test]
    fn worktree_commit_blocks_a_changed_primary_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let git = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(directory.path())
            .output()
            .unwrap();
        assert!(git.status.success(), "git init failed: {git:?}");

        let source_path = directory.path().join("primary-target.yaml");
        let original = b"primary configuration";
        std::fs::write(&source_path, original).unwrap();
        let expected = Fingerprint::for_bytes(original);
        let paths = AppPaths::new(directory.path().to_owned());
        let target = Target::select(directory.path()).unwrap();
        let desired = RepositoryConfig::empty();
        let configuration_bytes = RepositoryConfigCodec::render(&desired)
            .unwrap()
            .into_bytes();
        let planner = TargetStatePlanner::prepare_with_locks(
            &paths,
            TargetStateRequest {
                target: target.clone(),
                original: RepositoryConfig::empty(),
                desired,
                configuration_bytes,
                configuration_expected: Fingerprint::Absent,
                configuration_guard: None,
                repository_skills: RepositorySkillExceptions::new(),
            },
            TargetLocks::acquire(&[&target]).unwrap(),
        )
        .unwrap();

        std::fs::write(&source_path, b"changed primary configuration").unwrap();

        let report = planner
            .commit_with_source(Authorization::SafeOnly, &source_path, &expected)
            .unwrap();
        assert_eq!(report.status, ReportStatus::NotConverged);
        assert!(report.changes.iter().any(|change| {
            change.path == LOCAL_TARGET_CONFIG && change.outcome == ReportOutcome::Blocked
        }));
        assert!(!directory.path().join(LOCAL_TARGET_CONFIG).exists());
    }

    #[cfg(unix)]
    #[test]
    fn target_configuration_commit_rechecks_the_physical_parent() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let git = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(directory.path())
            .output()
            .unwrap();
        assert!(git.status.success(), "git init failed: {git:?}");

        let source_path = directory.path().join("primary-target.yaml");
        let source_bytes = b"primary configuration";
        std::fs::write(&source_path, source_bytes).unwrap();
        let paths = AppPaths::new(directory.path().to_owned());
        let target = Target::select(directory.path()).unwrap();
        let desired = RepositoryConfig::empty();
        let configuration_bytes = RepositoryConfigCodec::render(&desired)
            .unwrap()
            .into_bytes();
        let planner = TargetStatePlanner::prepare_with_locks(
            &paths,
            TargetStateRequest {
                target: target.clone(),
                original: RepositoryConfig::empty(),
                desired,
                configuration_bytes,
                configuration_expected: Fingerprint::Absent,
                configuration_guard: None,
                repository_skills: RepositorySkillExceptions::new(),
            },
            TargetLocks::acquire(&[&target]).unwrap(),
        )
        .unwrap();

        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), directory.path().join(".agents")).unwrap();

        let result = planner.commit_with_source(
            Authorization::SafeOnly,
            &source_path,
            &Fingerprint::for_bytes(source_bytes),
        );

        let report = result.expect("parent replacement should be reported as blocked");
        assert_eq!(report.status, ReportStatus::NotConverged);
        assert!(report.changes.iter().any(|change| {
            change.path == LOCAL_TARGET_CONFIG && change.outcome == ReportOutcome::Blocked
        }));
        assert!(!outside.path().join("skillator.yaml").exists());
        assert!(!directory.path().join(".gitignore").exists());
    }
}
