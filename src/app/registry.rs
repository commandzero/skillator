//! Registered target lifecycle and inspection.

use super::*;

pub(super) fn load_registry(
    paths: &AppPaths,
) -> Result<(TargetRegistry, Fingerprint), WorkflowError> {
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

pub(super) fn register_target(paths: &AppPaths, target: &Target) -> Result<(), WorkflowError> {
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

pub(super) fn report_permits_target_registration(report: &CommandReport) -> bool {
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

pub(super) fn append_registration_preview(
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
            .map(|(path, report)| registration_prune_diagnostic(path, report))
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

pub(super) fn registered_target_candidate(directory: &Path) -> Result<PathBuf, WorkflowError> {
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

pub(super) fn inspect_registered_target(path: &Path) -> RegisteredTargetReport {
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
        Ok(target) => registered_configuration_status(&target, &mut diagnostics),
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

fn registration_prune_diagnostic(path: &Path, report: &RegisteredTargetReport) -> ReportDiagnostic {
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
}

fn registered_configuration_status(target: &Target, diagnostics: &mut Vec<String>) -> &'static str {
    match load_target_repository(target, "Repository") {
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
    }
}
