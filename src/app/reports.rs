//! Reports and diagnostics derived from workflow results.

use super::*;

pub(super) fn scope_enablements_report(
    scope: &str,
    context: (&Target, &RepositoryConfig),
    observed: crate::target::ObservedState,
    mut diagnostics: Vec<ReportDiagnostic>,
) -> ScopeEnablementsReport {
    let (target, config) = context;
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

pub(super) fn materialization_state_name(
    state: &crate::target::MaterializationState,
) -> &'static str {
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

pub(super) fn comparison_name(comparison: crate::target::Comparison) -> &'static str {
    match comparison {
        crate::target::Comparison::InSync => "in_sync",
        crate::target::Comparison::Drifted => "drifted",
        crate::target::Comparison::Unverifiable => "unverifiable",
    }
}

pub(super) fn annotate_selector(report: &mut CommandReport, selector: &SkillSelector) {
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

pub(super) fn affected_enablement_diagnostics(
    paths: &AppPaths,
    original_library: &LibrarySnapshot,
    staged_library: &LibrarySnapshot,
) -> Vec<ReportDiagnostic> {
    enablement_resolution_diagnostics(paths, Some(original_library), staged_library)
}

pub(super) fn unresolved_enablement_diagnostics(
    paths: &AppPaths,
    staged_library: &LibrarySnapshot,
) -> Vec<ReportDiagnostic> {
    enablement_resolution_diagnostics(paths, None, staged_library)
}

pub(super) fn enablement_resolution_diagnostics(
    paths: &AppPaths,
    original_library: Option<&LibrarySnapshot>,
    staged_library: &LibrarySnapshot,
) -> Vec<ReportDiagnostic> {
    let mut scopes = Vec::new();
    let mut diagnostics = Vec::new();
    if let Ok(LoadResult::Valid(loaded)) = load_repository(&paths.user_config()) {
        scopes.push(("user".to_owned(), loaded.value().clone()));
    }
    collect_registered_scopes(paths, &mut scopes, &mut diagnostics);
    for (scope, config) in scopes {
        append_resolution_diagnostics(
            &scope,
            &config,
            (original_library, staged_library),
            &mut diagnostics,
        );
    }
    diagnostics
}

pub(super) fn report_check(
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

pub(super) fn report_apply(
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
    append_apply_diagnostics(root, result, diagnostics);
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

pub(super) fn add_plan_diagnostics(
    root: &Path,
    plan: &Plan,
    diagnostics: &mut Vec<ReportDiagnostic>,
) {
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

pub(super) fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

pub(super) fn action_name(action: Action) -> &'static str {
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

pub(super) fn safety_name(safety: Safety) -> &'static str {
    match safety {
        Safety::Safe => "safe",
        Safety::Guarded => "guarded",
        Safety::Blocked => "blocked",
    }
}

pub(super) fn outcome_code(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Applied => "applied",
        Outcome::NotAuthorized => "not_authorized",
        Outcome::Blocked => "blocked",
        Outcome::Failed => "failed",
        Outcome::RolledBack => "rolled_back",
        Outcome::RecoveryRequired => "recovery_required",
    }
}

fn append_apply_diagnostics(
    root: &Path,
    result: &ApplyResult,
    diagnostics: &mut Vec<ReportDiagnostic>,
) {
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
    append_directory_diagnostics(root, result, diagnostics);
    append_enablement_diagnostics(root, result, diagnostics);
    append_overlap_diagnostics(result, diagnostics);
}

fn append_directory_diagnostics(
    root: &Path,
    result: &ApplyResult,
    diagnostics: &mut Vec<ReportDiagnostic>,
) {
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
}

fn append_enablement_diagnostics(
    root: &Path,
    result: &ApplyResult,
    diagnostics: &mut Vec<ReportDiagnostic>,
) {
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
}

fn append_overlap_diagnostics(result: &ApplyResult, diagnostics: &mut Vec<ReportDiagnostic>) {
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
}

fn collect_registered_scopes(
    paths: &AppPaths,
    scopes: &mut Vec<(String, RepositoryConfig)>,
    diagnostics: &mut Vec<ReportDiagnostic>,
) {
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
}

fn append_resolution_diagnostics(
    scope: &str,
    config: &RepositoryConfig,
    libraries: (Option<&LibrarySnapshot>, &LibrarySnapshot),
    diagnostics: &mut Vec<ReportDiagnostic>,
) {
    let (original_library, staged_library) = libraries;
    for enablement in config.enablements() {
        if staged_library.resolve(enablement.skill()).is_none()
            && original_library.is_none_or(|library| library.resolve(enablement.skill()).is_some())
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
                    ("scope".to_owned(), scope.to_owned()),
                    ("directory".to_owned(), enablement.directory().to_string()),
                    ("source".to_owned(), enablement.skill().source().to_string()),
                    ("skill".to_owned(), enablement.skill().path().to_string()),
                ])),
            });
        }
    }
}
