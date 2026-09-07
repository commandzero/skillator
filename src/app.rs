//! Command-shaped application workflows.

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

    pub fn target_registry(&self) -> PathBuf {
        self.home.join(".skillator/targets.yaml")
    }

    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
}

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
                "primary worktree configuration changed while worktree sync was being prepared"
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
                configuration_guard: configuration_guard.clone(),
                repository_skills: RepositorySkillExceptions::new(),
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
                        "primary worktree configuration changed while worktree sync was being prepared"
                            .to_owned(),
                    ));
                }
                add_configuration_check_change(
                    &mut report,
                    &planner.configuration_expected,
                    &planner.configuration_bytes,
                    configuration_guard.as_ref(),
                );
                append_registration_preview(paths, &destination, &mut report)?;
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
                if report_permits_target_registration(&report) {
                    register_target(paths, &destination)?;
                }
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
            "cannot read primary worktree configuration at {}: {error}",
            path.display()
        ),
    })?;
    let fingerprint = Fingerprint::for_bytes(&bytes);
    let config = match RepositoryConfigCodec::parse(&bytes) {
        LoadResult::Valid(loaded) => loaded.value().clone(),
        LoadResult::Unsupported { version, .. } => {
            return Err(WorkflowError::InvalidInput {
                message: format!("unsupported primary repository configuration version {version}"),
            });
        }
        LoadResult::Invalid { issues } => {
            return Err(WorkflowError::InvalidInput {
                message: format_issues("invalid primary repository configuration", &issues),
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
                message: format!("cannot inspect linked worktree configuration: {error}"),
            });
        }
    };
    if facts.tracked || facts.staged || facts.unmerged {
        return Ok(DestinationConfiguration::Blocked {
            message: format!(
                "linked worktree configuration is still tracked; run `git rm --cached -- {LOCAL_TARGET_CONFIG}` before replacing it (Skillator never changes the Git index)"
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
                message: format!("cannot inspect linked worktree configuration: {error}"),
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(DestinationConfiguration::Blocked {
            message: format!(
                "linked worktree configuration must be a regular file, not a link: {}",
                path.display()
            ),
        });
    }
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Ok(DestinationConfiguration::Blocked {
                message: format!("linked worktree configuration is unreadable: {error}"),
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
                    "linked worktree configuration matches the primary worktree file, but contains invalid settings"
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
            format!("linked worktree configuration has unsupported version {version}")
        }
        LoadResult::Invalid { issues } => {
            format_issues("linked worktree configuration is invalid", &issues)
        }
        LoadResult::Missing => "linked worktree configuration is unavailable".to_owned(),
    };
    Ok(DestinationConfiguration::Guarded {
        fingerprint,
        message,
    })
}

fn configuration_guard_report(root: &Path, mode: &str, guard: ConfigurationGuard) -> CommandReport {
    let message = match guard {
        ConfigurationGuard::Guarded => {
            "linked worktree configuration differs from the primary; rerun with `--force` to replace it"
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
                message: format!("unsupported library configuration version {version}"),
            });
        }
        LoadResult::Invalid { issues } => {
            return Err(WorkflowError::InvalidInput {
                message: format_issues("invalid library configuration", &issues),
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

fn load_registry(paths: &AppPaths) -> Result<(TargetRegistry, Fingerprint), WorkflowError> {
    match load_target_registry(&paths.target_registry()).map_err(fatal)? {
        LoadResult::Missing => Ok((TargetRegistry::default(), Fingerprint::Absent)),
        LoadResult::Valid(loaded) => Ok((loaded.value().clone(), loaded.fingerprint().clone())),
        LoadResult::Unsupported { version, .. } => Err(WorkflowError::InvalidInput {
            message: format!("unsupported Target Registry version {version}"),
        }),
        LoadResult::Invalid { issues } => Err(WorkflowError::InvalidInput {
            message: format_issues("invalid Target Registry", &issues),
        }),
    }
}

fn register_target(paths: &AppPaths, target: &Target) -> Result<(), WorkflowError> {
    let (registry, expected) = load_registry(paths)?;
    let root = target.root().canonicalize().map_err(fatal)?;
    let staged = registry
        .with_target(root)
        .map_err(|issues| WorkflowError::InvalidInput {
            message: format_issues("invalid Target Registry", &issues),
        })?;
    save_target_registry(&paths.target_registry(), &staged, &expected).map_err(save_error)?;
    Ok(())
}

fn report_permits_target_registration(report: &CommandReport) -> bool {
    report.changes.iter().all(|change| {
        !matches!(
            change.outcome,
            ReportOutcome::NotAuthorized
                | ReportOutcome::Blocked
                | ReportOutcome::Failed
                | ReportOutcome::RolledBack
                | ReportOutcome::RecoveryRequired
        )
    })
}

fn append_registration_preview(
    paths: &AppPaths,
    target: &Target,
    report: &mut CommandReport,
) -> Result<(), WorkflowError> {
    let (registry, _) = load_registry(paths)?;
    let root = target.root().canonicalize().map_err(fatal)?;
    if !registry.targets().contains(&root) {
        report.changes.push(ReportChange {
            path: paths.target_registry().to_string_lossy().into_owned(),
            action: "register_target".to_owned(),
            safety: "safe".to_owned(),
            outcome: ReportOutcome::WouldApply,
        });
        report.status = ReportStatus::NotConverged;
        report.exit_status = 1;
    }
    Ok(())
}

pub struct TargetRegistryWorkflow;

impl TargetRegistryWorkflow {
    pub fn list(paths: &AppPaths) -> Result<TargetRegistryReport, WorkflowError> {
        let (registry, _) = load_registry(paths)?;
        Ok(TargetRegistryReport {
            format_version: 1,
            targets: registry
                .targets()
                .iter()
                .map(|path| inspect_registered_target(path))
                .collect(),
            diagnostics: Vec::new(),
        })
    }

    pub fn remove(
        paths: &AppPaths,
        directory: &Path,
        mode: SyncMode,
    ) -> Result<CommandReport, WorkflowError> {
        let (registry, expected) = load_registry(paths)?;
        let candidate = registered_target_candidate(directory)?;
        if !registry.targets().contains(&candidate) {
            return Ok(simple_report(
                paths.target_registry(),
                mode,
                Vec::new(),
                Vec::new(),
            ));
        }
        let staged =
            registry
                .without_target(&candidate)
                .map_err(|issues| WorkflowError::InvalidInput {
                    message: format_issues("invalid Target Registry", &issues),
                })?;
        let outcome = if mode == SyncMode::Check {
            ReportOutcome::WouldApply
        } else {
            save_target_registry(&paths.target_registry(), &staged, &expected)
                .map_err(save_error)?;
            ReportOutcome::Applied
        };
        Ok(simple_report(
            paths.target_registry(),
            mode,
            vec![ReportChange {
                path: candidate.to_string_lossy().into_owned(),
                action: "remove_target_registration".to_owned(),
                safety: "safe".to_owned(),
                outcome,
            }],
            Vec::new(),
        ))
    }

    pub fn prune(paths: &AppPaths, mode: SyncMode) -> Result<CommandReport, WorkflowError> {
        let (registry, expected) = load_registry(paths)?;
        let inspected = registry
            .targets()
            .iter()
            .map(|path| (path, inspect_registered_target(path)))
            .collect::<Vec<_>>();
        let stale = inspected
            .iter()
            .filter(|(_, report)| matches!(report.status.as_str(), "unavailable" | "unconfigured"))
            .map(|(path, _)| (*path).clone())
            .collect::<Vec<_>>();
        let diagnostics = inspected
            .iter()
            .filter(|(_, report)| report.status != "available")
            .map(|(path, report)| {
                let preserved = matches!(report.status.as_str(), "invalid" | "uninspectable");
                ReportDiagnostic {
                    code: if preserved {
                        "target_registration_preserved"
                    } else {
                        "target_registration_stale"
                    }
                    .to_owned(),
                    severity: if preserved { "warning" } else { "info" }.to_owned(),
                    message: format!(
                        "Registered Target is {} and will {}: {}",
                        report.status,
                        if preserved {
                            "be preserved"
                        } else {
                            "be pruned"
                        },
                        path.display()
                    ),
                    data: Some(BTreeMap::from([
                        ("target".to_owned(), path.to_string_lossy().into_owned()),
                        ("state".to_owned(), report.status.clone()),
                    ])),
                }
            })
            .collect::<Vec<_>>();
        if stale.is_empty() {
            return Ok(simple_report(
                paths.target_registry(),
                mode,
                Vec::new(),
                diagnostics,
            ));
        }
        let staged = registry
            .retaining(|path| !stale.iter().any(|stale| stale == path))
            .map_err(|issues| WorkflowError::InvalidInput {
                message: format_issues("invalid Target Registry", &issues),
            })?;
        let outcome = if mode == SyncMode::Check {
            ReportOutcome::WouldApply
        } else {
            save_target_registry(&paths.target_registry(), &staged, &expected)
                .map_err(save_error)?;
            ReportOutcome::Applied
        };
        let changes = stale
            .into_iter()
            .map(|path| ReportChange {
                path: path.to_string_lossy().into_owned(),
                action: "prune_target_registration".to_owned(),
                safety: "safe".to_owned(),
                outcome,
            })
            .collect();
        Ok(simple_report(
            paths.target_registry(),
            mode,
            changes,
            diagnostics,
        ))
    }
}

fn registered_target_candidate(directory: &Path) -> Result<PathBuf, WorkflowError> {
    if let Ok(target) = Target::select(directory) {
        return target.root().canonicalize().map_err(fatal);
    }
    let absolute = if directory.is_absolute() {
        directory.to_owned()
    } else {
        std::env::current_dir().map_err(fatal)?.join(directory)
    };
    Ok(absolute.canonicalize().unwrap_or(absolute))
}

fn inspect_registered_target(path: &Path) -> RegisteredTargetReport {
    let mut diagnostics = Vec::new();
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return RegisteredTargetReport {
                path: path.to_string_lossy().into_owned(),
                status: "unavailable".to_owned(),
                diagnostics,
            };
        }
        Err(error) => {
            diagnostics.push(error.to_string());
            return RegisteredTargetReport {
                path: path.to_string_lossy().into_owned(),
                status: "uninspectable".to_owned(),
                diagnostics,
            };
        }
        Ok(_) => {}
    }
    let status = match Target::select(path) {
        Ok(target) => match load_target_repository(&target, "Repository") {
            Ok(LoadResult::Valid(_)) => "available",
            Ok(LoadResult::Missing) => "unconfigured",
            Ok(LoadResult::Unsupported { version, .. }) => {
                diagnostics.push(format!(
                    "unsupported Repository Configuration version {version}"
                ));
                "invalid"
            }
            Ok(LoadResult::Invalid { issues }) => {
                diagnostics.push(format_issues("invalid Repository Configuration", &issues));
                "invalid"
            }
            Err(error @ WorkflowError::InvalidInput { .. }) => {
                diagnostics.push(error.to_string());
                "invalid"
            }
            Err(error) => {
                diagnostics.push(error.to_string());
                "uninspectable"
            }
        },
        Err(
            crate::target::TargetError::Missing(_)
            | crate::target::TargetError::NotDirectory(_)
            | crate::target::TargetError::NotGit(_)
            | crate::target::TargetError::Bare(_),
        ) => "unavailable",
        Err(error) => {
            diagnostics.push(error.to_string());
            "uninspectable"
        }
    };
    RegisteredTargetReport {
        path: path.to_string_lossy().into_owned(),
        status: status.to_owned(),
        diagnostics,
    }
}

fn scope_enablements_report(
    scope: &str,
    target: &Target,
    config: &RepositoryConfig,
    library: &LibrarySnapshot,
    mut diagnostics: Vec<ReportDiagnostic>,
) -> ScopeEnablementsReport {
    let observed = crate::target::observe(target, config, library);
    let observations = observed.enablements().collect::<Vec<_>>();
    let directories = config
        .skill_directories()
        .iter()
        .map(|directory| {
            let enablements = observations
                .iter()
                .filter(|observation| observation.enablement().directory() == directory.key())
                .map(|observation| ScopeEnablementReport {
                    source: observation.enablement().skill().source().to_string(),
                    skill: observation.enablement().skill().path().to_string(),
                    materialized_name: observation.expected_entry().map(str::to_owned),
                    materialization: match observation.enablement().materialization() {
                        MaterializationKind::Linked => "linked",
                        MaterializationKind::Copied => "copied",
                    }
                    .to_owned(),
                    resolution: if observation.unresolved() {
                        "unresolved"
                    } else {
                        "resolved"
                    }
                    .to_owned(),
                    observed_state: materialization_state_name(observation.state()).to_owned(),
                    comparison: comparison_name(observation.comparison()).to_owned(),
                })
                .collect();
            let directory_diagnostics = observed
                .directories()
                .iter()
                .find(|observed| observed.key() == directory.key().as_str())
                .map(|observed| observed.diagnostics().to_vec())
                .unwrap_or_default();
            ScopeDirectoryReport {
                key: directory.key().to_string(),
                path: directory.path().to_string(),
                enablements,
                diagnostics: directory_diagnostics,
            }
        })
        .collect();
    diagnostics.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then_with(|| left.message.cmp(&right.message))
    });
    ScopeEnablementsReport {
        format_version: 1,
        scope: scope.to_owned(),
        root: target.root().to_string_lossy().into_owned(),
        directories,
        diagnostics,
    }
}

fn materialization_state_name(state: &crate::target::MaterializationState) -> &'static str {
    use crate::target::MaterializationState;
    match state {
        MaterializationState::Missing => "missing",
        MaterializationState::CanonicalLink => "canonical_link",
        MaterializationState::NoncanonicalLink => "noncanonical_link",
        MaterializationState::BrokenLink => "broken_link",
        MaterializationState::MisdirectedLink => "misdirected_link",
        MaterializationState::EquivalentCopy => "equivalent_copy",
        MaterializationState::DivergedCopy => "diverged_copy",
        MaterializationState::CopyIneligible => "copy_ineligible",
        MaterializationState::WrongKind => "wrong_kind",
        MaterializationState::Uninspectable => "uninspectable",
        MaterializationState::ExpectedEntryCollision => "expected_entry_collision",
        MaterializationState::UnknownExpectedEntry => "unknown_expected_entry",
    }
}

fn comparison_name(comparison: crate::target::Comparison) -> &'static str {
    match comparison {
        crate::target::Comparison::InSync => "in_sync",
        crate::target::Comparison::Drifted => "drifted",
        crate::target::Comparison::Unverifiable => "unverifiable",
    }
}

fn annotate_selector(report: &mut CommandReport, selector: &SkillSelector) {
    report.diagnostics.push(ReportDiagnostic {
        code: "selected_skill".to_owned(),
        severity: "info".to_owned(),
        message: format!("Selected Skill `{selector}`"),
        data: Some(BTreeMap::from([
            ("source".to_owned(), selector.key().source().to_string()),
            ("skill".to_owned(), selector.key().path().to_string()),
        ])),
    });
}

fn affected_enablement_diagnostics(
    paths: &AppPaths,
    original_library: &LibrarySnapshot,
    staged_library: &LibrarySnapshot,
) -> Vec<ReportDiagnostic> {
    enablement_resolution_diagnostics(paths, Some(original_library), staged_library)
}

fn unresolved_enablement_diagnostics(
    paths: &AppPaths,
    staged_library: &LibrarySnapshot,
) -> Vec<ReportDiagnostic> {
    enablement_resolution_diagnostics(paths, None, staged_library)
}

fn enablement_resolution_diagnostics(
    paths: &AppPaths,
    original_library: Option<&LibrarySnapshot>,
    staged_library: &LibrarySnapshot,
) -> Vec<ReportDiagnostic> {
    let mut scopes = Vec::new();
    let mut diagnostics = Vec::new();
    if let Ok(LoadResult::Valid(loaded)) = load_repository(&paths.user_config()) {
        scopes.push(("user".to_owned(), loaded.value().clone()));
    }
    match load_registry(paths) {
        Ok((registry, _)) => {
            for root in registry.targets() {
                let loaded = Target::select(root)
                    .map_err(|error| error.to_string())
                    .and_then(|target| {
                        load_target_repository(&target, "Repository")
                            .map_err(|error| error.to_string())
                    });
                match loaded {
                    Ok(LoadResult::Valid(loaded)) => {
                        scopes.push((root.to_string_lossy().into_owned(), loaded.value().clone()));
                    }
                    other => diagnostics.push(ReportDiagnostic {
                        code: "registered_target_unavailable".to_owned(),
                        severity: "warning".to_owned(),
                        message: format!(
                            "Registered Target could not be inspected: {}",
                            root.display()
                        ),
                        data: Some(BTreeMap::from([
                            ("target".to_owned(), root.to_string_lossy().into_owned()),
                            ("state".to_owned(), format!("{other:?}")),
                        ])),
                    }),
                }
            }
        }
        Err(error) => diagnostics.push(ReportDiagnostic {
            code: "target_registry_invalid".to_owned(),
            severity: "warning".to_owned(),
            message: error.to_string(),
            data: None,
        }),
    }
    for (scope, config) in scopes {
        for enablement in config.enablements() {
            if staged_library.resolve(enablement.skill()).is_none()
                && original_library
                    .is_none_or(|library| library.resolve(enablement.skill()).is_some())
            {
                let newly_unresolved = original_library.is_some();
                diagnostics.push(ReportDiagnostic {
                    code: "enablement_will_be_unresolved".to_owned(),
                    severity: "warning".to_owned(),
                    message: format!(
                        "Enablement `{}/{}` in `{scope}` {} unresolved",
                        enablement.skill().source(),
                        enablement.skill().path(),
                        if newly_unresolved {
                            "will be"
                        } else {
                            "remains"
                        }
                    ),
                    data: Some(BTreeMap::from([
                        ("scope".to_owned(), scope.clone()),
                        ("directory".to_owned(), enablement.directory().to_string()),
                        ("source".to_owned(), enablement.skill().source().to_string()),
                        ("skill".to_owned(), enablement.skill().path().to_string()),
                    ])),
                });
            }
        }
    }
    diagnostics
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
                "skill folder `{}`: {}. {}",
                display_path(root, directory.path()),
                directory.comparison().description(),
                directory.diagnostics().join("; ")
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
                "Skill `{}/{}`: {}. {}",
                enablement.enablement().skill().source(),
                enablement.enablement().skill().path(),
                enablement.comparison().description(),
                enablement.state().description()
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
                "Overlapping library folders affect skill `{}/{}`",
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
                message: format!("unsupported library configuration version {version}"),
            }),
            LoadResult::Invalid { issues } => Err(WorkflowError::InvalidInput {
                message: format_issues("invalid library configuration", &issues),
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
                    "Library folders overlap; set allow_overlap: true for both folders to allow this"
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
                message: "Cannot add skills: the first library folder is unavailable".to_owned(),
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

    pub fn load_cli(paths: &AppPaths) -> Result<LibrarySession, WorkflowError> {
        match load_library(&paths.library_config()).map_err(fatal)? {
            LoadResult::Missing => Ok(LibrarySession {
                config: LibraryConfig::empty(),
                fingerprint: Fingerprint::Absent,
                first_run: false,
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

    pub fn inventory(
        paths: &AppPaths,
        filter: Option<&str>,
    ) -> Result<LibraryInventoryReport, WorkflowError> {
        let session = Self::load_cli(paths)?;
        let snapshot = Self::snapshot(paths, &session.config);
        let prefix = filter.map(str::to_ascii_lowercase);
        let sources = snapshot
            .sources()
            .filter(|source| {
                prefix.as_ref().is_none_or(|prefix| {
                    source
                        .key()
                        .as_str()
                        .to_ascii_lowercase()
                        .starts_with(prefix)
                })
            })
            .map(|source| LibrarySourceReport {
                key: source.key().as_str().to_owned(),
                skills: source
                    .skills()
                    .map(|skill| LibrarySkillReport {
                        path: skill.path().to_owned(),
                        name: skill.name().map(str::to_owned),
                        valid: skill.validity() == SkillValidity::Valid,
                        diagnostics: skill.diagnostics().to_vec(),
                    })
                    .collect(),
            })
            .collect();
        Ok(LibraryInventoryReport {
            format_version: 1,
            filter: filter.map(str::to_owned),
            sources,
            diagnostics: library_diagnostics(&snapshot),
        })
    }

    pub fn locations(paths: &AppPaths) -> Result<LibraryLocationsReport, WorkflowError> {
        let session = Self::load_cli(paths)?;
        let snapshot = Self::snapshot(paths, &session.config);
        Ok(LibraryLocationsReport {
            format_version: 1,
            locations: snapshot
                .locations()
                .iter()
                .map(|location| LibraryLocationReport {
                    expression: location.expression().to_owned(),
                    resolved: location
                        .resolved()
                        .map(|path| path.to_string_lossy().into_owned()),
                    available: location.available(),
                })
                .collect(),
            diagnostics: library_diagnostics(&snapshot),
        })
    }

    pub fn add_location(
        paths: &AppPaths,
        expression: String,
        allow_overlap: bool,
        mode: SyncMode,
    ) -> Result<CommandReport, WorkflowError> {
        let session = Self::load_cli(paths)?;
        let candidate = expand_location(
            &expression,
            paths
                .library_config()
                .parent()
                .unwrap_or_else(|| Path::new(".")),
            paths.home(),
            paths.environment(),
        )
        .map_err(|message| WorkflowError::InvalidInput { message })?;
        let snapshot = Self::snapshot(paths, &session.config);
        let duplicate = snapshot.locations().iter().any(|location| {
            location.expression() == expression
                || match (location.resolved(), candidate.canonicalize().ok()) {
                    (Some(existing), Some(candidate)) => existing == candidate,
                    _ => false,
                }
        });
        if duplicate {
            return Ok(simple_report(
                paths.library_config(),
                mode,
                Vec::new(),
                Vec::new(),
            ));
        }
        let mut locations = session.config.locations().to_vec();
        locations.push(crate::config::LibraryLocationConfig::new(
            expression.clone(),
            Vec::new(),
            allow_overlap,
        ));
        let staged = session.config.with_locations(locations).map_err(|issues| {
            WorkflowError::InvalidInput {
                message: format_issues("invalid Library Configuration", &issues),
            }
        })?;
        let staged_snapshot = Self::snapshot(paths, &staged);
        if staged_snapshot
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == "overlapping_locations")
        {
            return Err(WorkflowError::InvalidInput {
                message: "overlapping Library Locations require --allow-overlap and compatible existing configuration".to_owned(),
            });
        }
        let outcome = if mode == SyncMode::Check {
            ReportOutcome::WouldApply
        } else {
            save_library(&paths.library_config(), &staged, &session.fingerprint)
                .map_err(save_error)?;
            ReportOutcome::Applied
        };
        Ok(simple_report(
            paths.library_config(),
            mode,
            vec![ReportChange {
                path: expression,
                action: "add_library_location".to_owned(),
                safety: "safe".to_owned(),
                outcome,
            }],
            library_diagnostics(&staged_snapshot),
        ))
    }

    pub fn remove_location(
        paths: &AppPaths,
        expression: &str,
        mode: SyncMode,
    ) -> Result<CommandReport, WorkflowError> {
        let session = Self::load_cli(paths)?;
        let original_snapshot = Self::snapshot(paths, &session.config);
        let supplied = expand_location(
            expression,
            paths
                .library_config()
                .parent()
                .unwrap_or_else(|| Path::new(".")),
            paths.home(),
            paths.environment(),
        )
        .ok()
        .and_then(|path| path.canonicalize().ok());
        let matches = original_snapshot
            .locations()
            .iter()
            .enumerate()
            .filter(|(_, location)| {
                location.expression() == expression
                    || supplied
                        .as_ref()
                        .zip(location.resolved())
                        .is_some_and(|(left, right)| left == right)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(WorkflowError::InvalidInput {
                message: "Library Location matches more than one configured entry".to_owned(),
            });
        }
        let Some(index) = matches.first().copied() else {
            return Err(WorkflowError::InvalidInput {
                message: format!("Library Location is not configured: {expression}"),
            });
        };
        let mut locations = session.config.locations().to_vec();
        locations.remove(index);
        let staged = session.config.with_locations(locations).map_err(|issues| {
            WorkflowError::InvalidInput {
                message: format_issues("invalid Library Configuration", &issues),
            }
        })?;
        let staged_snapshot = Self::snapshot(paths, &staged);
        let mut diagnostics =
            affected_enablement_diagnostics(paths, &original_snapshot, &staged_snapshot);
        diagnostics.extend(library_diagnostics(&staged_snapshot));
        let outcome = if mode == SyncMode::Check {
            ReportOutcome::WouldApply
        } else {
            save_library(&paths.library_config(), &staged, &session.fingerprint)
                .map_err(save_error)?;
            ReportOutcome::Applied
        };
        Ok(simple_report(
            paths.library_config(),
            mode,
            vec![ReportChange {
                path: expression.to_owned(),
                action: "remove_library_location".to_owned(),
                safety: "safe".to_owned(),
                outcome,
            }],
            diagnostics,
        ))
    }

    pub fn prune_locations(
        paths: &AppPaths,
        mode: SyncMode,
    ) -> Result<CommandReport, WorkflowError> {
        let session = Self::load_cli(paths)?;
        let original_snapshot = Self::snapshot(paths, &session.config);
        let library_config_path = paths.library_config();
        let config_parent = library_config_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let mut stale = Vec::new();
        let mut diagnostics = Vec::new();
        for (index, location) in session.config.locations().iter().enumerate() {
            match classify_library_location(
                location.path(),
                config_parent,
                paths.home(),
                paths.environment(),
            ) {
                LibraryLocationState::Present => {}
                LibraryLocationState::Stale => stale.push(index),
                LibraryLocationState::Preserve(message) => {
                    diagnostics.push(ReportDiagnostic {
                        code: "library_location_preserved".to_owned(),
                        severity: "warning".to_owned(),
                        message,
                        data: Some(BTreeMap::from([(
                            "location".to_owned(),
                            location.path().to_owned(),
                        )])),
                    });
                }
            }
        }
        if stale.is_empty() {
            diagnostics.extend(library_diagnostics(&original_snapshot));
            return Ok(simple_report(
                paths.library_config(),
                mode,
                Vec::new(),
                diagnostics,
            ));
        }
        let staged_locations = session
            .config
            .locations()
            .iter()
            .enumerate()
            .filter(|(index, _)| !stale.contains(index))
            .map(|(_, location)| location.clone())
            .collect();
        let staged = session
            .config
            .with_locations(staged_locations)
            .map_err(|issues| WorkflowError::InvalidInput {
                message: format_issues("invalid Library Configuration", &issues),
            })?;
        let staged_snapshot = Self::snapshot(paths, &staged);
        diagnostics.extend(unresolved_enablement_diagnostics(paths, &staged_snapshot));
        diagnostics.extend(library_diagnostics(&staged_snapshot));
        let outcome = if mode == SyncMode::Check {
            ReportOutcome::WouldApply
        } else {
            save_library(&paths.library_config(), &staged, &session.fingerprint)
                .map_err(save_error)?;
            ReportOutcome::Applied
        };
        let changes = stale
            .into_iter()
            .map(|index| ReportChange {
                path: session.config.locations()[index].path().to_owned(),
                action: "prune_library_location".to_owned(),
                safety: "safe".to_owned(),
                outcome,
            })
            .collect();
        Ok(simple_report(
            paths.library_config(),
            mode,
            changes,
            diagnostics,
        ))
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

enum LibraryLocationState {
    Present,
    Stale,
    Preserve(String),
}

fn classify_library_location(
    expression: &str,
    config_parent: &Path,
    home: &Path,
    environment: &BTreeMap<String, String>,
) -> LibraryLocationState {
    let path = match expand_location(expression, config_parent, home, environment) {
        Ok(path) => path,
        Err(message) => return LibraryLocationState::Preserve(message),
    };
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LibraryLocationState::Stale;
        }
        Err(error) => {
            return LibraryLocationState::Preserve(format!(
                "Library Location could not be inspected: {}: {error}",
                path.display()
            ));
        }
    };
    if metadata.file_type().is_symlink() {
        return match path.canonicalize() {
            Ok(resolved) if resolved.is_dir() => LibraryLocationState::Present,
            Ok(_) => LibraryLocationState::Stale,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                LibraryLocationState::Stale
            }
            Err(error) => LibraryLocationState::Preserve(format!(
                "Library Location could not be resolved: {}: {error}",
                path.display()
            )),
        };
    }
    if metadata.is_dir() {
        LibraryLocationState::Present
    } else {
        LibraryLocationState::Stale
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
                "local skill configuration is still tracked; run `git rm --cached -- {LOCAL_TARGET_CONFIG}` before saving (Skillator never changes the Git index)"
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
                    "linked worktree configuration changed while worktree sync was being prepared"
                        .to_owned(),
                ));
            }
            return Err(save_error(SaveError::Stale));
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
            &session.target,
            &session.config,
            &library,
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
        if facts.tracked || facts.staged || facts.unmerged {
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
        let directory = select_directory(&session.config, directory)?;
        if materialization.is_some() {
            let snapshot =
                LibraryWorkflow::snapshot(paths, &LibraryWorkflow::load_cli(paths)?.config);
            if snapshot.resolve(&selector.key).is_none() {
                return Err(WorkflowError::InvalidInput {
                    message: format!("Skill is not registered and valid: {selector}"),
                });
            }
        }
        let mut enablements = session.config.enablements().to_vec();
        enablements.retain(|enablement| {
            enablement.directory() != &directory || enablement.skill() != &selector.key
        });
        if let Some(kind) = materialization {
            enablements.push(crate::domain::Enablement::new(
                directory,
                selector.key.clone(),
                kind,
            ));
        }
        let staged = session
            .config
            .with_enablements(enablements)
            .map_err(|issues| WorkflowError::InvalidInput {
                message: format_issues("invalid Repository Configuration", &issues),
            })?;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSelector {
    key: SkillKey,
}

impl SkillSelector {
    pub fn parse(value: &str) -> Result<Self, WorkflowError> {
        let Some((source, path)) = value.rsplit_once(':') else {
            return Err(WorkflowError::InvalidInput {
                message: "Skill selector must use <source-key>:<skill-path>".to_owned(),
            });
        };
        let source = SourceKey::parse(source).map_err(|error| WorkflowError::InvalidInput {
            message: error.to_string(),
        })?;
        let path = SkillPath::parse(path).map_err(|error| WorkflowError::InvalidInput {
            message: error.to_string(),
        })?;
        Ok(Self {
            key: SkillKey::new(source, path),
        })
    }

    pub fn key(&self) -> &SkillKey {
        &self.key
    }
}

impl std::fmt::Display for SkillSelector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.key.source(), self.key.path())
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

    fn check(mut self) -> CommandReport {
        let mut report = report_check(
            self.target.root(),
            self.prepared.plan(),
            &mut self.diagnostics,
        );
        if self.expected
            != Fingerprint::for_bytes(
                RepositoryConfigCodec::render(&self.staged)
                    .expect("validated configuration renders")
                    .as_bytes(),
            )
        {
            report.changes.push(ReportChange {
                path: ".agents/skillator.yaml".to_owned(),
                action: "write_user_configuration".to_owned(),
                safety: "safe".to_owned(),
                outcome: ReportOutcome::WouldApply,
            });
            report.status = ReportStatus::NotConverged;
            report.exit_status = 1;
        }
        report
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

impl UserScopeWorkflow {
    pub fn inspect(paths: &AppPaths) -> Result<ScopeEnablementsReport, WorkflowError> {
        let session = Self::load(paths)?;
        if session.first_run {
            return Ok(ScopeEnablementsReport {
                format_version: 1,
                scope: "user".to_owned(),
                root: session.target.root().to_string_lossy().into_owned(),
                directories: Vec::new(),
                diagnostics: Vec::new(),
            });
        }
        let (library, diagnostics) = load_library_snapshot(paths)?;
        Ok(scope_enablements_report(
            "user",
            &session.target,
            &session.config,
            &library,
            diagnostics,
        ))
    }

    pub fn load(paths: &AppPaths) -> Result<UserScopeSession, WorkflowError> {
        let target = Target::user(paths.home()).map_err(|error| WorkflowError::InvalidInput {
            message: error.to_string(),
        })?;
        let config_path = paths.user_config();
        validate_target_config_path(&target, &config_path, "User account")?;
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
                message: format!("unsupported user configuration version {version}"),
            }),
            LoadResult::Invalid { issues } => Err(WorkflowError::InvalidInput {
                message: format_issues("invalid user configuration", &issues),
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
        validate_target_config_path(&prepared.target, &path, "User account")?;
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

    pub fn mutate_enablement(
        paths: &AppPaths,
        selector: &SkillSelector,
        materialization: Option<MaterializationKind>,
        mode: SyncMode,
    ) -> Result<CommandReport, WorkflowError> {
        let session = Self::load(paths)?;
        if session.first_run && materialization.is_none() {
            let mut report = simple_report(
                session.target.root().to_owned(),
                mode,
                Vec::new(),
                Vec::new(),
            );
            annotate_selector(&mut report, selector);
            return Ok(report);
        }
        let directory = select_directory(&session.config, None)?;
        if materialization.is_some() {
            let snapshot =
                LibraryWorkflow::snapshot(paths, &LibraryWorkflow::load_cli(paths)?.config);
            if snapshot.resolve(selector.key()).is_none() {
                return Err(WorkflowError::InvalidInput {
                    message: format!("Skill is not registered and valid: {selector}"),
                });
            }
        }
        let mut enablements = session.config.enablements().to_vec();
        enablements.retain(|enablement| {
            enablement.directory() != &directory || enablement.skill() != selector.key()
        });
        if let Some(kind) = materialization {
            enablements.push(crate::domain::Enablement::new(
                directory,
                selector.key().clone(),
                kind,
            ));
        }
        let staged = session
            .config
            .with_enablements(enablements)
            .map_err(|issues| WorkflowError::InvalidInput {
                message: format_issues("invalid User Scope Configuration", &issues),
            })?;
        let prepared = Self::prepare_save(paths, &session, staged)?;
        match mode {
            SyncMode::Check => {
                let mut report = prepared.check();
                annotate_selector(&mut report, selector);
                Ok(report)
            }
            SyncMode::Apply { force } => {
                let mut report = if prepared.rejects_apply(force) {
                    prepared.rejected_apply_report()
                } else {
                    Self::commit_save(
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
                    "{scope} configuration parent must be a directory, not a link: {}",
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
