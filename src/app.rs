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
                        "Repository Configuration is missing at {}; run `skillator init {}` first",
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
    let mut scopes = Vec::new();
    let mut diagnostics = Vec::new();
    if let Ok(LoadResult::Valid(loaded)) = load_repository(&paths.user_config()) {
        scopes.push(("user".to_owned(), loaded.value().clone()));
    }
    match load_registry(paths) {
        Ok((registry, _)) => {
            for root in registry.targets() {
                let config = root.join(LOCAL_TARGET_CONFIG);
                match load_repository(&config) {
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
            if original_library.resolve(enablement.skill()).is_some()
                && staged_library.resolve(enablement.skill()).is_none()
            {
                diagnostics.push(ReportDiagnostic {
                    code: "enablement_will_be_unresolved".to_owned(),
                    severity: "warning".to_owned(),
                    message: format!(
                        "Enablement `{}/{}` in `{scope}` will be unresolved",
                        enablement.skill().source(),
                        enablement.skill().path()
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
                "Skill Directory `{}` finished {:?}: {}",
                display_path(root, directory.path()),
                directory.comparison(),
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
        register_target(paths, &target)?;
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

    pub fn mutate_enablement(
        paths: &AppPaths,
        selector: &SkillSelector,
        materialization: Option<MaterializationKind>,
        mode: SyncMode,
    ) -> Result<CommandReport, WorkflowError> {
        let session = Self::load(paths)?;
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
    fn canonical_skill_selector_uses_the_final_colon_boundary() {
        let selector = SkillSelector::parse("elastic/agent-skills:skills/esdiag").unwrap();
        assert_eq!(selector.key().source().as_str(), "elastic/agent-skills");
        assert_eq!(selector.key().path().as_str(), "skills/esdiag");
        assert!(SkillSelector::parse("esdiag").is_err());
        assert!(SkillSelector::parse("Elastic/agent-skills:skills/esdiag").is_err());
        assert!(SkillSelector::parse("elastic/agent-skills:../esdiag").is_err());
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
