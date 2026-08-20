//! Command-shaped application workflows.

use crate::acquisition::{AcquisitionError, LibraryAcquisition, PreparedAcquisitions};
use crate::config::{
    Fingerprint, LibraryConfig, LoadResult, RepositoryConfig, SaveError, load_library,
    load_repository, save_library, save_repository,
};
use crate::domain::SkillKey;
use crate::library::{LibrarySnapshot, Registration, scan_library};
use crate::reconcile::{
    Action, ApplyResult, Authorization, Outcome, Plan, PreparedPlan, Safety, TargetBusy, execute,
    prepare_apply, prepare_check, prepare_transition,
};
use crate::target::Target;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    home: PathBuf,
    environment: BTreeMap<String, String>,
}

impl AppPaths {
    pub fn new(home: PathBuf) -> Self {
        Self {
            home,
            environment: std::env::vars().collect(),
        }
    }

    pub fn with_environment(home: PathBuf, environment: BTreeMap<String, String>) -> Self {
        Self { home, environment }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn library_config(&self) -> PathBuf {
        self.home.join(".skillator/library.yaml")
    }

    pub fn user_config(&self) -> PathBuf {
        self.home.join(".agents/skillator.yaml")
    }

    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("{message}")]
    InvalidInput { message: String },
    #[error("Target Busy")]
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
                        "Repository Configuration is missing at {}; run `skillator {}` first",
                        repository_path.display(),
                        target.root().display()
                    ),
                });
            }
            LoadResult::Valid(loaded) => loaded.value().clone(),
            LoadResult::Unsupported { version, .. } => {
                return Err(WorkflowError::InvalidInput {
                    message: format!("unsupported Repository Configuration version {version}"),
                });
            }
            LoadResult::Invalid { issues } => {
                return Err(WorkflowError::InvalidInput {
                    message: format_issues("invalid Repository Configuration", &issues),
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
    let path = paths.library_config();
    let config = match load_library(&path).map_err(fatal)? {
        LoadResult::Missing => LibraryConfig::empty(),
        LoadResult::Valid(loaded) => loaded.value().clone(),
        LoadResult::Unsupported { version, .. } => {
            return Err(WorkflowError::InvalidInput {
                message: format!("unsupported Library Configuration version {version}"),
            });
        }
        LoadResult::Invalid { issues } => {
            return Err(WorkflowError::InvalidInput {
                message: format_issues("invalid Library Configuration", &issues),
            });
        }
    };
    let snapshot = scan_library(&config, &path, paths.home(), &paths.environment);
    let mut diagnostics = snapshot
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
        .collect::<Vec<_>>();
    for source in snapshot.sources() {
        if source.registration() == Registration::Unregistered {
            diagnostics.push(ReportDiagnostic {
                code: "unregistered_source".to_owned(),
                severity: "advisory".to_owned(),
                message: format!(
                    "discovered Unregistered Source `{}`",
                    source.suggested_key()
                ),
                data: None,
            });
        }
        for skill in source.skills() {
            if skill.registration() == Registration::Unregistered {
                diagnostics.push(ReportDiagnostic {
                    code: "unregistered_skill".to_owned(),
                    severity: "advisory".to_owned(),
                    message: format!(
                        "discovered Unregistered Skill `{}/{}`",
                        source.key(),
                        skill.path()
                    ),
                    data: None,
                });
            }
        }
    }
    Ok((snapshot, diagnostics))
}

fn report_check(
    root: &Path,
    plan: &Plan,
    diagnostics: &mut Vec<ReportDiagnostic>,
) -> CommandReport {
    let changes = plan
        .items()
        .iter()
        .map(|item| ReportChange {
            path: display_path(root, item.path()),
            action: action_name(item.action()).to_owned(),
            safety: safety_name(item.safety()).to_owned(),
            outcome: match item.safety() {
                Safety::Safe => ReportOutcome::WouldApply,
                Safety::Guarded => ReportOutcome::WouldRequireForce,
                Safety::Blocked => ReportOutcome::Blocked,
            },
        })
        .collect::<Vec<_>>();
    add_plan_diagnostics(root, plan, diagnostics);
    let converged = changes.is_empty();
    CommandReport {
        format_version: 1,
        status: if converged {
            ReportStatus::InSync
        } else {
            ReportStatus::NotConverged
        },
        exit_status: if converged { 0 } else { 1 },
        mode: "check".to_owned(),
        target: root.to_string_lossy().into_owned(),
        changes,
        diagnostics: std::mem::take(diagnostics),
    }
}

fn report_apply(
    root: &Path,
    result: &ApplyResult,
    force: bool,
    diagnostics: &mut Vec<ReportDiagnostic>,
) -> CommandReport {
    let changes = result
        .outcomes()
        .iter()
        .map(|item| ReportChange {
            path: display_path(root, &item.path),
            action: action_name(item.action).to_owned(),
            safety: safety_name(item.safety).to_owned(),
            outcome: match item.outcome {
                Outcome::Applied => ReportOutcome::Applied,
                Outcome::NotAuthorized => ReportOutcome::NotAuthorized,
                Outcome::Blocked => ReportOutcome::Blocked,
                Outcome::Failed => ReportOutcome::Failed,
                Outcome::RolledBack => ReportOutcome::RolledBack,
                Outcome::RecoveryRequired => ReportOutcome::RecoveryRequired,
            },
        })
        .collect();
    for item in result.outcomes() {
        if item.outcome != Outcome::Applied {
            diagnostics.push(ReportDiagnostic {
                code: outcome_code(item.outcome).to_owned(),
                severity: if item.outcome == Outcome::NotAuthorized {
                    "warning"
                } else {
                    "error"
                }
                .to_owned(),
                message: item.message.clone(),
                data: Some(BTreeMap::from([(
                    "path".to_owned(),
                    display_path(root, &item.path),
                )])),
            });
        }
    }
    if !result
        .final_observed()
        .directories()
        .iter()
        .all(|directory| {
            *directory.control_file() == crate::target::ControlFileState::NotRequired
                || directory.control_tracked()
                || *directory.control_file() != crate::target::ControlFileState::Canonical
        })
    {
        for directory in result.final_observed().directories() {
            if *directory.control_file() == crate::target::ControlFileState::Canonical
                && !directory.control_tracked()
            {
                let relative = display_path(root, directory.path());
                diagnostics.push(ReportDiagnostic {
                    code: "control_file_untracked".to_owned(),
                    severity: "warning".to_owned(),
                    message: format!("run `git add -f -- {relative}/.gitignore`"),
                    data: None,
                });
            }
        }
    }
    for directory in result
        .final_observed()
        .directories()
        .iter()
        .filter(|directory| directory.comparison() != crate::target::Comparison::InSync)
    {
        diagnostics.push(ReportDiagnostic {
            code: "final_directory_state".to_owned(),
            severity: "warning".to_owned(),
            message: format!(
                "Skill Directory `{}` finished {:?}",
                display_path(root, directory.path()),
                directory.comparison()
            ),
            data: Some(BTreeMap::from([
                ("path".to_owned(), display_path(root, directory.path())),
                (
                    "comparison".to_owned(),
                    format!("{:?}", directory.comparison()).to_ascii_lowercase(),
                ),
            ])),
        });
    }
    for enablement in result
        .final_observed()
        .enablements()
        .filter(|enablement| enablement.comparison() != crate::target::Comparison::InSync)
    {
        diagnostics.push(ReportDiagnostic {
            code: "final_enablement_state".to_owned(),
            severity: "warning".to_owned(),
            message: format!(
                "Enablement `{}/{}` finished {:?}: {:?}",
                enablement.enablement().skill().source(),
                enablement.enablement().skill().path(),
                enablement.comparison(),
                enablement.state()
            ),
            data: enablement
                .path()
                .map(|path| BTreeMap::from([("path".to_owned(), display_path(root, path))])),
        });
    }
    for enablement in result
        .final_observed()
        .enablements()
        .filter(|enablement| enablement.overlap_advisory())
    {
        diagnostics.push(ReportDiagnostic {
            code: "library_location_overlap".to_owned(),
            severity: "warning".to_owned(),
            message: format!(
                "Library Location overlap affects Enablement `{}/{}`",
                enablement.enablement().skill().source(),
                enablement.enablement().skill().path()
            ),
            data: None,
        });
    }
    let converged = result.converged();
    CommandReport {
        format_version: 1,
        status: if converged {
            ReportStatus::InSync
        } else {
            ReportStatus::NotConverged
        },
        exit_status: if converged { 0 } else { 1 },
        mode: if force { "sync_force" } else { "sync" }.to_owned(),
        target: root.to_string_lossy().into_owned(),
        changes,
        diagnostics: std::mem::take(diagnostics),
    }
}

fn add_plan_diagnostics(root: &Path, plan: &Plan, diagnostics: &mut Vec<ReportDiagnostic>) {
    for item in plan
        .items()
        .iter()
        .filter(|item| item.safety() == Safety::Blocked)
    {
        diagnostics.push(ReportDiagnostic {
            code: if item.action() == Action::TrackControlFile {
                "control_file_untracked"
            } else {
                "blocked"
            }
            .to_owned(),
            severity: "warning".to_owned(),
            message: item.reason().to_owned(),
            data: Some(BTreeMap::from([(
                "path".to_owned(),
                display_path(root, item.path()),
            )])),
        });
    }
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn action_name(action: Action) -> &'static str {
    match action {
        Action::CreateDirectory => "create_directory",
        Action::WriteControlFile => "write_control_file",
        Action::Link => "link",
        Action::Copy => "copy",
        Action::Replace => "replace",
        Action::RemoveUnmanaged => "remove_unmanaged",
        Action::TrackControlFile => "track_control_file",
        Action::Recover => "recover",
    }
}

fn safety_name(safety: Safety) -> &'static str {
    match safety {
        Safety::Safe => "safe",
        Safety::Guarded => "guarded",
        Safety::Blocked => "blocked",
    }
}

fn outcome_code(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Applied => "applied",
        Outcome::NotAuthorized => "not_authorized",
        Outcome::Blocked => "blocked",
        Outcome::Failed => "failed",
        Outcome::RolledBack => "rolled_back",
        Outcome::RecoveryRequired => "recovery_required",
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

#[derive(Debug, Clone)]
pub struct LibrarySession {
    pub config: LibraryConfig,
    pub fingerprint: Fingerprint,
    pub first_run: bool,
}

pub struct LibraryWorkflow;

impl LibraryWorkflow {
    pub fn load(paths: &AppPaths) -> Result<LibrarySession, WorkflowError> {
        match load_library(&paths.library_config()).map_err(fatal)? {
            LoadResult::Missing => Ok(LibrarySession {
                config: LibraryConfig::first_run(),
                fingerprint: Fingerprint::Absent,
                first_run: true,
            }),
            LoadResult::Valid(loaded) => Ok(LibrarySession {
                config: loaded.value().clone(),
                fingerprint: loaded.fingerprint().clone(),
                first_run: false,
            }),
            LoadResult::Unsupported { version, .. } => Err(WorkflowError::InvalidInput {
                message: format!("unsupported Library Configuration version {version}"),
            }),
            LoadResult::Invalid { issues } => Err(WorkflowError::InvalidInput {
                message: format_issues("invalid Library Configuration", &issues),
            }),
        }
    }

    pub fn save(
        paths: &AppPaths,
        session: &LibrarySession,
        staged: &LibraryConfig,
        confirmed: bool,
    ) -> Result<Fingerprint, WorkflowError> {
        Self::save_with_acquisitions(paths, session, staged, &[], confirmed)
    }

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
        let snapshot = Self::snapshot(paths, staged);
        if snapshot
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "overlapping_locations")
        {
            return Err(WorkflowError::InvalidInput {
                message:
                    "overlapping Library Locations require explicit allow_overlap on both Locations"
                        .to_owned(),
            });
        }
        if session.first_run {
            for location in staged.locations() {
                if location.path() == "./library" {
                    std::fs::create_dir_all(paths.home.join(".skillator/library"))
                        .map_err(fatal)?;
                }
            }
        }
        if acquisitions.is_empty() {
            return save_library(&paths.library_config(), staged, &session.fingerprint)
                .map_err(save_error);
        }
        let local_root = snapshot
            .locations()
            .first()
            .and_then(|location| location.resolved())
            .ok_or_else(|| WorkflowError::InvalidInput {
                message: "the first Library Location must be available for acquisition".to_owned(),
            })?;
        let mut prepared =
            PreparedAcquisitions::prepare(local_root, acquisitions).map_err(acquisition_error)?;
        prepared.publish().map_err(acquisition_error)?;
        let fingerprint = match save_library(&paths.library_config(), staged, &session.fingerprint)
        {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                return match prepared.rollback() {
                    Ok(()) => Err(save_error(error)),
                    Err(rollback) => Err(acquisition_error(rollback)),
                };
            }
        };
        prepared.finish().map_err(acquisition_error)?;
        Ok(fingerprint)
    }

    pub fn snapshot(paths: &AppPaths, config: &LibraryConfig) -> LibrarySnapshot {
        scan_library(
            config,
            &paths.library_config(),
            paths.home(),
            &paths.environment,
        )
    }

    pub fn affected_references(
        original: &LibraryConfig,
        staged: &LibraryConfig,
        repository: &RepositoryConfig,
    ) -> Vec<SkillKey> {
        let registered = |config: &LibraryConfig| {
            config
                .locations()
                .iter()
                .flat_map(|location| location.sources())
                .flat_map(|source| {
                    source
                        .skills()
                        .iter()
                        .map(|skill| SkillKey::new(source.key().clone(), skill.path().clone()))
                })
                .collect::<std::collections::BTreeSet<_>>()
        };
        let removed = registered(original)
            .difference(&registered(staged))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        repository
            .enablements()
            .iter()
            .map(|enablement| enablement.skill())
            .filter(|skill| removed.contains(*skill))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
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
    target: Target,
    staged: RepositoryConfig,
    expected: Fingerprint,
    library: LibrarySnapshot,
    prepared: PreparedPlan,
    diagnostics: Vec<ReportDiagnostic>,
}

impl PreparedTargetSave {
    pub fn plan(&self) -> &Plan {
        self.prepared.plan()
    }
}

impl TargetWorkflow {
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
                message: format!("unsupported Repository Configuration version {version}"),
            }),
            LoadResult::Invalid { issues } => Err(WorkflowError::InvalidInput {
                message: format_issues("invalid Repository Configuration", &issues),
            }),
        }
    }

    pub fn save_configuration(
        session: &TargetSession,
        staged: &RepositoryConfig,
    ) -> Result<Fingerprint, WorkflowError> {
        let path = session.target.root().join(".agents/skillator.yaml");
        validate_target_config_path(&session.target, &path, "Repository")?;
        save_repository(&path, staged, &session.fingerprint).map_err(save_error)
    }

    pub fn prepare_save(
        paths: &AppPaths,
        session: &TargetSession,
        staged: RepositoryConfig,
    ) -> Result<PreparedTargetSave, WorkflowError> {
        let (library, diagnostics) = load_library_snapshot(paths)?;
        let prepared = prepare_transition(&session.target, &session.config, &staged, &library)?;
        Ok(PreparedTargetSave {
            target: session.target.clone(),
            staged,
            expected: session.fingerprint.clone(),
            library,
            prepared,
            diagnostics,
        })
    }

    pub fn commit_save(
        prepared: PreparedTargetSave,
        authorization: Authorization,
    ) -> Result<CommandReport, WorkflowError> {
        let path = prepared.target.root().join(".agents/skillator.yaml");
        validate_target_config_path(&prepared.target, &path, "Repository")?;
        save_repository(&path, &prepared.staged, &prepared.expected).map_err(save_error)?;
        let result = execute(
            prepared.prepared,
            authorization,
            &prepared.target,
            &prepared.staged,
            &prepared.library,
        );
        let mut diagnostics = prepared.diagnostics;
        Ok(report_apply(
            prepared.target.root(),
            &result,
            authorization == Authorization::AllGuarded,
            &mut diagnostics,
        ))
    }
}

#[derive(Debug)]
pub struct UserScopeSession {
    pub target: Target,
    pub config: RepositoryConfig,
    pub fingerprint: Fingerprint,
    pub first_run: bool,
}

pub struct UserScopeWorkflow;

pub struct PreparedUserScopeSave {
    target: Target,
    staged: RepositoryConfig,
    expected: Fingerprint,
    library: LibrarySnapshot,
    prepared: PreparedPlan,
    diagnostics: Vec<ReportDiagnostic>,
}

impl PreparedUserScopeSave {
    pub fn plan(&self) -> &Plan {
        self.prepared.plan()
    }
}

impl UserScopeWorkflow {
    pub fn load(paths: &AppPaths) -> Result<UserScopeSession, WorkflowError> {
        let target = Target::user(paths.home()).map_err(|error| WorkflowError::InvalidInput {
            message: error.to_string(),
        })?;
        let config_path = paths.user_config();
        validate_target_config_path(&target, &config_path, "User Scope")?;
        match load_repository(&config_path).map_err(fatal)? {
            LoadResult::Missing => Ok(UserScopeSession {
                target,
                config: RepositoryConfig::user_first_run(),
                fingerprint: Fingerprint::Absent,
                first_run: true,
            }),
            LoadResult::Valid(loaded) => Ok(UserScopeSession {
                target,
                config: loaded.value().clone(),
                fingerprint: loaded.fingerprint().clone(),
                first_run: false,
            }),
            LoadResult::Unsupported { version, .. } => Err(WorkflowError::InvalidInput {
                message: format!("unsupported User Scope Configuration version {version}"),
            }),
            LoadResult::Invalid { issues } => Err(WorkflowError::InvalidInput {
                message: format_issues("invalid User Scope Configuration", &issues),
            }),
        }
    }

    pub fn prepare_save(
        paths: &AppPaths,
        session: &UserScopeSession,
        staged: RepositoryConfig,
    ) -> Result<PreparedUserScopeSave, WorkflowError> {
        let (library, diagnostics) = load_library_snapshot(paths)?;
        let prepared = prepare_transition(&session.target, &session.config, &staged, &library)?;
        Ok(PreparedUserScopeSave {
            target: session.target.clone(),
            staged,
            expected: session.fingerprint.clone(),
            library,
            prepared,
            diagnostics,
        })
    }

    pub fn commit_save(
        paths: &AppPaths,
        prepared: PreparedUserScopeSave,
        authorization: Authorization,
    ) -> Result<CommandReport, WorkflowError> {
        let path = paths.user_config();
        validate_target_config_path(&prepared.target, &path, "User Scope")?;
        save_repository(&path, &prepared.staged, &prepared.expected).map_err(save_error)?;
        let result = execute(
            prepared.prepared,
            authorization,
            &prepared.target,
            &prepared.staged,
            &prepared.library,
        );
        let mut diagnostics = prepared.diagnostics;
        Ok(report_apply(
            prepared.target.root(),
            &result,
            authorization == Authorization::AllGuarded,
            &mut diagnostics,
        ))
    }
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
    let parent = target.root().join(".agents");
    match std::fs::symlink_metadata(&parent) {
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(WorkflowError::InvalidInput {
                message: format!(
                    "{scope} Configuration parent must be a physical directory: {}",
                    parent.display()
                ),
            });
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(fatal(error)),
    }
    match std::fs::symlink_metadata(config_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(WorkflowError::InvalidInput {
                message: format!(
                    "{scope} Configuration must be a physical file: {}",
                    config_path.display()
                ),
            })
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(fatal(error)),
    }
}
