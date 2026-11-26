//! Synchronization from a primary checkout into a linked worktree.

use super::*;

pub struct WorktreeSyncWorkflow;

impl WorktreeSyncWorkflow {
    pub fn run(
        paths: &AppPaths,
        target_path: impl AsRef<Path>,
        mode: SyncMode,
    ) -> Result<CommandReport, WorkflowError> {
        let (destination, primary) = select_worktree_pair(target_path.as_ref())?;
        let locks = TargetLocks::acquire(&[&primary, &destination])?;
        let (desired, configuration_bytes, source_fingerprint) =
            load_primary_target_configuration(&primary)?;
        let destination_state =
            inspect_destination_configuration(&destination, &source_fingerprint)?;

        let (original, configuration_expected, configuration_guard) =
            match destination_planning_state(destination_state, &source_fingerprint) {
                Ok(state) => state,
                Err(message) => {
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

        PreparedWorktreeSync {
            primary,
            planner,
            source_fingerprint,
        }
        .finish(paths, mode)
    }
}

#[derive(Debug)]
pub(super) enum DestinationConfiguration {
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
pub(super) enum ConfigurationGuard {
    Guarded,
    GuardedWithMessage(String),
}

pub(super) fn load_primary_target_configuration(
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

pub(super) fn inspect_destination_configuration(
    target: &Target,
    desired_fingerprint: &Fingerprint,
) -> Result<DestinationConfiguration, WorkflowError> {
    let path = target.root().join(LOCAL_TARGET_CONFIG);
    if let Err(error) = validate_target_config_path(target, &path, "Destination Repository") {
        return Ok(DestinationConfiguration::Blocked {
            message: error.to_string(),
        });
    }
    if let Some(blocked) = destination_tracking_blocker(target) {
        return Ok(blocked);
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
    classify_destination_bytes(&bytes, desired_fingerprint)
}

fn classify_destination_bytes(
    bytes: &[u8],
    desired_fingerprint: &Fingerprint,
) -> Result<DestinationConfiguration, WorkflowError> {
    let fingerprint = Fingerprint::for_bytes(bytes);
    if &fingerprint == desired_fingerprint {
        return match RepositoryConfigCodec::parse(bytes) {
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
    let message = match RepositoryConfigCodec::parse(bytes) {
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

pub(super) fn configuration_guard_report(
    root: &Path,
    mode: &str,
    guard: ConfigurationGuard,
) -> CommandReport {
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

pub(super) fn configuration_blocked_report(
    root: &Path,
    mode: &str,
    message: String,
) -> CommandReport {
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

pub(super) fn add_configuration_check_change(
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

fn destination_planning_state(
    destination_state: DestinationConfiguration,
    source_fingerprint: &Fingerprint,
) -> Result<(RepositoryConfig, Fingerprint, Option<ConfigurationGuard>), String> {
    Ok(match destination_state {
        DestinationConfiguration::Missing { fingerprint } => {
            (RepositoryConfig::empty(), fingerprint, None)
        }
        DestinationConfiguration::Valid {
            config,
            fingerprint,
        } => {
            if &fingerprint == source_fingerprint {
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
        DestinationConfiguration::Blocked { message } => return Err(message),
    })
}

struct PreparedWorktreeSync {
    primary: Target,
    planner: TargetStatePlanner,
    source_fingerprint: Fingerprint,
}

impl PreparedWorktreeSync {
    fn finish(self, paths: &AppPaths, mode: SyncMode) -> Result<CommandReport, WorkflowError> {
        let Self {
            primary,
            planner,
            source_fingerprint,
        } = self;
        let destination = planner.target.clone();
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
                    planner.configuration_guard.as_ref(),
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

fn destination_tracking_blocker(target: &Target) -> Option<DestinationConfiguration> {
    let facts = match target
        .repository()
        .facts_for(Path::new(LOCAL_TARGET_CONFIG))
    {
        Ok(facts) => facts,
        Err(error) => {
            return Some(DestinationConfiguration::Blocked {
                message: format!("cannot inspect linked worktree configuration: {error}"),
            });
        }
    };
    if configuration_has_index_changes(&facts) {
        return Some(DestinationConfiguration::Blocked {
            message: format!(
                "linked worktree configuration is still tracked; run `git rm --cached -- {LOCAL_TARGET_CONFIG}` before replacing it (Skillator never changes the Git index)"
            ),
        });
    }
    None
}

fn select_worktree_pair(target_path: &Path) -> Result<(Target, Target), WorkflowError> {
    let destination = Target::select(target_path).map_err(|error| WorkflowError::InvalidInput {
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
    Ok((destination, primary))
}
