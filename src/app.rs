//! Command-shaped application workflows.

use crate::acquisition::{AcquisitionError, LibraryAcquisition, PreparedAcquisitions};
use crate::config::{
    Fingerprint, LibraryConfig, LoadResult, RepositoryConfig, RepositoryConfigCodec, SaveError,
    load_library, load_repository, save_bytes, save_library, save_repository,
};
use crate::domain::SkillKey;
use crate::library::{LibrarySnapshot, scan_library};
use crate::reconcile::{
    Action, ApplyResult, Authorization, Outcome, Plan, PreparedPlan, Safety, TargetBusy,
    TargetLocks, execute, prepare_apply, prepare_check, prepare_transition,
    prepare_transition_with_locks,
};
use crate::target::Target;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const LOCAL_TARGET_CONFIG: &str = ".agents/skillator.yaml";
const LOCAL_TARGET_CONTROL: &str = ".agents/.gitignore";

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

pub struct WorktreeSyncWorkflow;

impl WorktreeSyncWorkflow {
    pub fn run(
        paths: &AppPaths,
        target_path: impl AsRef<Path>,
        mode: SyncMode,
    ) -> Result<CommandReport, WorkflowError> {
        let destination =
            Target::select(target_path.as_ref()).map_err(|error| WorkflowError::InvalidInput {
                message: error.to_string(),
            })?;
        let pair = destination
            .repository()
            .linked_worktree_pair()
            .map_err(|error| WorkflowError::InvalidInput {
                message: format!("invalid linked worktree input: {error}"),
            })?;
        let primary =
            Target::select(pair.primary_root()).map_err(|error| WorkflowError::InvalidInput {
                message: format!("cannot open primary worktree: {error}"),
            })?;
        let locks = TargetLocks::acquire(&[&primary, &destination])?;
        let (desired, configuration_bytes, source_fingerprint) =
            load_primary_target_configuration(&primary)?;
        let destination_state =
            inspect_destination_configuration(&destination, &source_fingerprint)?;

        let (original, configuration_expected, configuration_guard) = match destination_state {
            DestinationConfiguration::Missing { fingerprint } => {
                (RepositoryConfig::empty(), fingerprint, None)
            }
            DestinationConfiguration::Valid {
                config,
                fingerprint,
            } => {
                if fingerprint == source_fingerprint {
                    (config, fingerprint, None)
                } else {
                    (config, fingerprint, Some(ConfigurationGuard::Guarded))
                }
            }
            DestinationConfiguration::Guarded {
                fingerprint,
                message,
            } => (
                RepositoryConfig::empty(),
                fingerprint,
                Some(ConfigurationGuard::GuardedWithMessage(message)),
            ),
            DestinationConfiguration::Blocked { message } => {
                return Ok(configuration_blocked_report(
                    destination.root(),
                    "worktree_sync",
                    message,
                ));
            }
        };

        if let Some(guard) = configuration_guard.as_ref()
            && matches!(mode, SyncMode::Apply { force: false })
        {
            return Ok(configuration_guard_report(
                destination.root(),
                "worktree_sync",
                guard.clone(),
            ));
        }

        if source_configuration_changed(
            &primary.root().join(LOCAL_TARGET_CONFIG),
            &source_fingerprint,
        )
        .map_err(fatal)?
        {
            return Ok(configuration_blocked_report(
                destination.root(),
                "worktree_sync",
                "primary Target configuration changed while worktree sync was being prepared"
                    .to_owned(),
            ));
        }
        let planner = TargetStatePlanner::prepare_with_locks(
            paths,
            TargetStateRequest {
                target: destination.clone(),
                original,
                desired,
                configuration_bytes,
                configuration_expected,
                root_ignore_policy: RootIgnorePolicy::Require,
                configuration_guard: configuration_guard.clone(),
            },
            locks,
        )?;

        match mode {
            SyncMode::Check => {
                let mut diagnostics = planner.diagnostics.clone();
                let mut report = report_check(destination.root(), planner.plan(), &mut diagnostics);
                report.mode = "worktree_check".to_owned();
                if source_configuration_changed(
                    &primary.root().join(LOCAL_TARGET_CONFIG),
                    &source_fingerprint,
                )
                .map_err(fatal)?
                {
                    return Ok(configuration_blocked_report(
                        destination.root(),
                        "worktree_check",
                        "primary Target configuration changed while worktree sync was being prepared"
                            .to_owned(),
                    ));
                }
                add_configuration_check_change(
                    &mut report,
                    &planner.configuration_expected,
                    &planner.configuration_bytes,
                    configuration_guard.as_ref(),
                );
                Ok(report)
            }
            SyncMode::Apply { force } => {
                let mut report = planner.commit_with_source(
                    if force {
                        Authorization::AllGuarded
                    } else {
                        Authorization::SafeOnly
                    },
                    &primary.root().join(LOCAL_TARGET_CONFIG),
                    &source_fingerprint,
                )?;
                report.mode = if force {
                    "worktree_sync_force".to_owned()
                } else {
                    "worktree_sync".to_owned()
                };
                Ok(report)
            }
        }
    }
}

#[derive(Debug)]
enum DestinationConfiguration {
    Missing {
        fingerprint: Fingerprint,
    },
    Valid {
        config: RepositoryConfig,
        fingerprint: Fingerprint,
    },
    Guarded {
        fingerprint: Fingerprint,
        message: String,
    },
    Blocked {
        message: String,
    },
}

#[derive(Debug, Clone)]
enum ConfigurationGuard {
    Guarded,
    GuardedWithMessage(String),
}

fn load_primary_target_configuration(
    primary: &Target,
) -> Result<(RepositoryConfig, Vec<u8>, Fingerprint), WorkflowError> {
    let path = primary.root().join(LOCAL_TARGET_CONFIG);
    validate_target_config_path(primary, &path, "Primary Repository")?;
    let bytes = fs::read(&path).map_err(|error| WorkflowError::InvalidInput {
        message: format!(
            "cannot read primary Target configuration at {}: {error}",
            path.display()
        ),
    })?;
    let fingerprint = Fingerprint::for_bytes(&bytes);
    let config = match RepositoryConfigCodec::parse(&bytes) {
        LoadResult::Valid(loaded) => loaded.value().clone(),
        LoadResult::Unsupported { version, .. } => {
            return Err(WorkflowError::InvalidInput {
                message: format!("unsupported primary Repository Configuration version {version}"),
            });
        }
        LoadResult::Invalid { issues } => {
            return Err(WorkflowError::InvalidInput {
                message: format_issues("invalid primary Repository Configuration", &issues),
            });
        }
        LoadResult::Missing => unreachable!("configuration bytes were read immediately above"),
    };
    Ok((config, bytes, fingerprint))
}

fn inspect_destination_configuration(
    target: &Target,
    desired_fingerprint: &Fingerprint,
) -> Result<DestinationConfiguration, WorkflowError> {
    let path = target.root().join(LOCAL_TARGET_CONFIG);
    if let Err(error) = validate_target_config_path(target, &path, "Destination Repository") {
        return Ok(DestinationConfiguration::Blocked {
            message: error.to_string(),
        });
    }
    let facts = match target
        .repository()
        .facts_for(Path::new(LOCAL_TARGET_CONFIG))
    {
        Ok(facts) => facts,
        Err(error) => {
            return Ok(DestinationConfiguration::Blocked {
                message: format!("cannot inspect destination Target configuration: {error}"),
            });
        }
    };
    if facts.tracked || facts.staged || facts.unmerged {
        return Ok(DestinationConfiguration::Blocked {
            message: format!(
                "destination Target configuration is still tracked; run `git rm --cached -- {LOCAL_TARGET_CONFIG}` before replacing it (Skillator never changes the Git index)"
            ),
        });
    }
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DestinationConfiguration::Missing {
                fingerprint: Fingerprint::Absent,
            });
        }
        Err(error) => {
            return Ok(DestinationConfiguration::Blocked {
                message: format!("cannot inspect destination Target configuration: {error}"),
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(DestinationConfiguration::Blocked {
            message: format!(
                "destination Target configuration must be a physical file: {}",
                path.display()
            ),
        });
    }
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Ok(DestinationConfiguration::Blocked {
                message: format!("destination Target configuration is unreadable: {error}"),
            });
        }
    };
    let fingerprint = Fingerprint::for_bytes(&bytes);
    if &fingerprint == desired_fingerprint {
        return match RepositoryConfigCodec::parse(&bytes) {
            LoadResult::Valid(loaded) => Ok(DestinationConfiguration::Valid {
                config: loaded.value().clone(),
                fingerprint,
            }),
            _ => Ok(DestinationConfiguration::Blocked {
                message:
                    "destination Target configuration matches the primary bytes but is invalid"
                        .to_owned(),
            }),
        };
    }
    let message = match RepositoryConfigCodec::parse(&bytes) {
        LoadResult::Valid(loaded) => {
            return Ok(DestinationConfiguration::Valid {
                config: loaded.value().clone(),
                fingerprint,
            });
        }
        LoadResult::Unsupported { version, .. } => {
            format!("destination Target configuration has unsupported version {version}")
        }
        LoadResult::Invalid { issues } => {
            format_issues("destination Target configuration is invalid", &issues)
        }
        LoadResult::Missing => "destination Target configuration is unavailable".to_owned(),
    };
    Ok(DestinationConfiguration::Guarded {
        fingerprint,
        message,
    })
}

fn configuration_guard_report(root: &Path, mode: &str, guard: ConfigurationGuard) -> CommandReport {
    let message = match guard {
        ConfigurationGuard::Guarded => {
            "destination Target configuration differs from the primary; rerun with `--force` to replace it"
                .to_owned()
        }
        ConfigurationGuard::GuardedWithMessage(message) => format!(
            "{message}; rerun with `--force` to replace it"
        ),
    };
    CommandReport {
        format_version: 1,
        status: ReportStatus::NotConverged,
        exit_status: 1,
        mode: mode.to_owned(),
        target: root.to_string_lossy().into_owned(),
        changes: vec![ReportChange {
            path: LOCAL_TARGET_CONFIG.to_owned(),
            action: "write_target_configuration".to_owned(),
            safety: "guarded".to_owned(),
            outcome: ReportOutcome::WouldRequireForce,
        }],
        diagnostics: vec![ReportDiagnostic {
            code: "configuration_guarded".to_owned(),
            severity: "warning".to_owned(),
            message,
            data: None,
        }],
    }
}

fn configuration_blocked_report(root: &Path, mode: &str, message: String) -> CommandReport {
    CommandReport {
        format_version: 1,
        status: ReportStatus::NotConverged,
        exit_status: 1,
        mode: mode.to_owned(),
        target: root.to_string_lossy().into_owned(),
        changes: vec![ReportChange {
            path: LOCAL_TARGET_CONFIG.to_owned(),
            action: "write_target_configuration".to_owned(),
            safety: "blocked".to_owned(),
            outcome: ReportOutcome::Blocked,
        }],
        diagnostics: vec![ReportDiagnostic {
            code: "configuration_blocked".to_owned(),
            severity: "error".to_owned(),
            message,
            data: None,
        }],
    }
}

fn add_configuration_check_change(
    report: &mut CommandReport,
    expected: &Fingerprint,
    desired: &[u8],
    guard: Option<&ConfigurationGuard>,
) {
    if expected == &Fingerprint::for_bytes(desired) {
        return;
    }
    report.changes.push(ReportChange {
        path: LOCAL_TARGET_CONFIG.to_owned(),
        action: "write_target_configuration".to_owned(),
        safety: if guard.is_some() { "guarded" } else { "safe" }.to_owned(),
        outcome: if guard.is_some() {
            ReportOutcome::WouldRequireForce
        } else {
            ReportOutcome::WouldApply
        },
    });
    report.status = ReportStatus::NotConverged;
    report.exit_status = 1;
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
    let diagnostics = snapshot
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
        _original: &LibraryConfig,
        _staged: &LibraryConfig,
        _repository: &RepositoryConfig,
    ) -> Vec<SkillKey> {
        // Locations are discovery roots, not a persisted source inventory.  A
        // reference is resolved against the fresh Snapshot immediately before
        // Target planning, so there is no configuration-only removal set.
        Vec::new()
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootIgnorePolicy {
    Ensure,
    Require,
}

#[derive(Debug)]
struct RootIgnorePublication {
    path: PathBuf,
    expected: Fingerprint,
    desired: Vec<u8>,
}

struct TargetStateRequest {
    target: Target,
    original: RepositoryConfig,
    desired: RepositoryConfig,
    configuration_bytes: Vec<u8>,
    configuration_expected: Fingerprint,
    root_ignore_policy: RootIgnorePolicy,
    configuration_guard: Option<ConfigurationGuard>,
}

struct TargetStatePlanner {
    target: Target,
    desired: RepositoryConfig,
    configuration_bytes: Vec<u8>,
    configuration_expected: Fingerprint,
    configuration_guard: Option<ConfigurationGuard>,
    root_ignore: RootIgnorePublication,
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
            .field("root_ignore", &self.root_ignore.path)
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
                root_ignore_policy: RootIgnorePolicy::Ensure,
                configuration_guard: None,
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
            root_ignore_policy,
            configuration_guard,
        } = request;
        validate_target_config_path(
            &target,
            &target.root().join(LOCAL_TARGET_CONFIG),
            "Repository",
        )?;
        let root_ignore = plan_root_ignore(&target, root_ignore_policy)?;
        let (library, diagnostics) = load_library_snapshot(paths)?;
        let prepared =
            prepare_transition_with_locks(&target, &original, &desired, &library, locks)?;
        Ok(Self {
            target,
            desired,
            configuration_bytes,
            configuration_expected,
            configuration_guard,
            root_ignore,
            library,
            prepared,
            diagnostics,
        })
    }

    fn plan(&self) -> &Plan {
        self.prepared.plan()
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
                "primary Target configuration changed while worktree sync was being prepared"
                    .to_owned(),
            ));
        }
        let config_path = self.target.root().join(LOCAL_TARGET_CONFIG);
        if let Err(error) = validate_target_config_path(&self.target, &config_path, "Repository") {
            if source.is_some() {
                return Ok(configuration_blocked_report(
                    self.target.root(),
                    "worktree_sync",
                    error.to_string(),
                ));
            }
            return Err(error);
        }
        let config_facts = self
            .target
            .repository()
            .facts_for(Path::new(LOCAL_TARGET_CONFIG))
            .map_err(fatal)?;
        if config_facts.tracked || config_facts.staged || config_facts.unmerged {
            let message = format!(
                "local Target configuration is still tracked; run `git rm --cached -- {LOCAL_TARGET_CONFIG}` before saving (Skillator never changes the Git index)"
            );
            if source.is_some() {
                return Ok(configuration_blocked_report(
                    self.target.root(),
                    "worktree_sync",
                    message,
                ));
            }
            return Err(WorkflowError::InvalidInput { message });
        }

        if source.is_some()
            && let Err(message) = validate_destination_configuration_at_commit(
                &config_path,
                &self.configuration_expected,
            )
        {
            return Ok(configuration_blocked_report(
                self.target.root(),
                "worktree_sync",
                message,
            ));
        }
        let current_config = fingerprint_path(&config_path).map_err(fatal)?;
        if current_config != self.configuration_expected {
            if source.is_some() {
                return Ok(configuration_blocked_report(
                    self.target.root(),
                    "worktree_sync",
                    "destination Target configuration changed while worktree sync was being prepared"
                        .to_owned(),
                ));
            }
            return Err(save_error(SaveError::Stale));
        }
        let current_root_ignore = fingerprint_path(&self.root_ignore.path).map_err(fatal)?;
        if current_root_ignore != self.root_ignore.expected {
            return Err(save_error(SaveError::Stale));
        }

        let root_ignore_changed =
            self.root_ignore.expected != Fingerprint::for_bytes(&self.root_ignore.desired);
        let configuration_changed =
            self.configuration_expected != Fingerprint::for_bytes(&self.configuration_bytes);
        if root_ignore_changed || configuration_changed {
            validate_target_config_path(&self.target, &config_path, "Repository")?;
        }
        if root_ignore_changed {
            save_bytes(
                &self.root_ignore.path,
                &self.root_ignore.desired,
                &self.root_ignore.expected,
            )
            .map_err(save_error)?;
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
        if root_ignore_changed {
            report.changes.push(ReportChange {
                path: display_path(self.target.root(), &self.root_ignore.path),
                action: "write_root_ignore".to_owned(),
                safety: "safe".to_owned(),
                outcome: ReportOutcome::Applied,
            });
        }
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
        let _locks = TargetLocks::acquire(&[&session.target])?;
        let root_ignore = plan_root_ignore(&session.target, RootIgnorePolicy::Ensure)?;
        let facts = session
            .target
            .repository()
            .facts_for(Path::new(LOCAL_TARGET_CONFIG))
            .map_err(fatal)?;
        if facts.tracked || facts.staged || facts.unmerged {
            return Err(WorkflowError::InvalidInput {
                message: format!(
                    "local Target configuration is still tracked; run `git rm --cached -- {LOCAL_TARGET_CONFIG}` before saving (Skillator never changes the Git index)"
                ),
            });
        }
        if fingerprint_path(&path).map_err(fatal)? != session.fingerprint {
            return Err(save_error(SaveError::Stale));
        }
        if root_ignore.expected != Fingerprint::for_bytes(&root_ignore.desired) {
            save_bytes(
                &root_ignore.path,
                &root_ignore.desired,
                &root_ignore.expected,
            )
            .map_err(save_error)?;
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

    pub fn commit_save(
        prepared: PreparedTargetSave,
        authorization: Authorization,
    ) -> Result<CommandReport, WorkflowError> {
        prepared.planner.commit(authorization)
    }
}

fn plan_root_ignore(
    target: &Target,
    policy: RootIgnorePolicy,
) -> Result<RootIgnorePublication, WorkflowError> {
    let root_ignore = target.root().join(".gitignore");
    let (current, expected) = match fs::read(&root_ignore) {
        Ok(bytes) => {
            let expected = Fingerprint::for_bytes(&bytes);
            (bytes, expected)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (Vec::new(), Fingerprint::Absent)
        }
        Err(error) => return Err(fatal(error)),
    };
    let mut desired = current.clone();
    for relative in [LOCAL_TARGET_CONFIG, LOCAL_TARGET_CONTROL] {
        let rule = format!("/{relative}");
        let has_exact_rule = current
            .split(|byte| *byte == b'\n')
            .any(|line| line == rule.as_bytes());
        let effective = target
            .repository()
            .facts_for(Path::new(relative))
            .map_err(fatal)?
            .ignored;
        if !has_exact_rule || !effective {
            if !desired.is_empty() && !desired.ends_with(b"\n") {
                desired.push(b'\n');
            }
            desired.extend_from_slice(rule.as_bytes());
            desired.push(b'\n');
        }
    }
    if policy == RootIgnorePolicy::Require && desired != current {
        return Err(WorkflowError::InvalidInput {
            message: format!(
                "linked worktree root .gitignore must contain `/{LOCAL_TARGET_CONFIG}` and `/{LOCAL_TARGET_CONTROL}`"
            ),
        });
    }
    Ok(RootIgnorePublication {
        path: root_ignore,
        expected,
        desired,
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
                Err("destination Target configuration became unavailable after planning".to_owned())
            };
        }
        Err(error) => {
            return Err(format!(
                "destination Target configuration cannot be inspected after planning: {error}"
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(
            "destination Target configuration changed to a non-regular file after planning"
                .to_owned(),
        );
    }
    let bytes = fs::read(path).map_err(|error| {
        format!("destination Target configuration is unreadable after planning: {error}")
    })?;
    if &Fingerprint::for_bytes(&bytes) != expected {
        return Err("destination Target configuration changed after planning".to_owned());
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

#[cfg(test)]
mod tests {
    use super::*;

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
                root_ignore_policy: RootIgnorePolicy::Ensure,
                configuration_guard: None,
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
                root_ignore_policy: RootIgnorePolicy::Ensure,
                configuration_guard: None,
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
