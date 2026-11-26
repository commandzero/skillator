//! Library settings, discovery, and location maintenance workflows.

use super::*;

#[derive(Debug, Clone)]
pub struct LibrarySession {
    pub config: LibraryConfig,
    pub fingerprint: Fingerprint,
    pub first_run: bool,
}

pub struct LibraryWorkflow;

impl LibraryWorkflow {
    pub fn load(paths: &AppPaths) -> Result<LibrarySession, WorkflowError> {
        load_library_session(paths, true)
    }

    pub fn save(
        paths: &AppPaths,
        session: &LibrarySession,
        staged: &LibraryConfig,
        confirmed: bool,
    ) -> Result<Fingerprint, WorkflowError> {
        Self::save_with_acquisitions(paths, session, staged, &[], confirmed)
    }

    pub(super) fn save_confirmed_acquisitions(
        paths: &AppPaths,
        session: &LibrarySession,
        staged: &LibraryConfig,
        acquisitions: &[LibraryAcquisition],
    ) -> Result<Fingerprint, WorkflowError> {
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
        initialize_library_directory(paths, session, staged)?;
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
        load_library_session(paths, false)
    }

    pub fn inventory(
        paths: &AppPaths,
        filter: Option<&str>,
    ) -> Result<LibraryInventoryReport, WorkflowError> {
        let session = Self::load(paths)?;
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
                        diagnostics: skill
                            .diagnostics()
                            .iter()
                            .cloned()
                            .chain(
                                skill
                                    .warnings()
                                    .iter()
                                    .map(|warning| format!("Warning: {warning}")),
                            )
                            .collect(),
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
        let outcome = save_location_change(paths, &session, &staged, mode)?;
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
        let outcome = save_location_change(paths, &session, &staged, mode)?;
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
        let (stale, mut diagnostics) = stale_library_locations(paths, &session.config);
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
        let outcome = save_location_change(paths, &session, &staged, mode)?;
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
        return classify_linked_library_location(&path);
    }
    if metadata.is_dir() {
        LibraryLocationState::Present
    } else {
        LibraryLocationState::Stale
    }
}

fn load_library_session(
    paths: &AppPaths,
    first_run: bool,
) -> Result<LibrarySession, WorkflowError> {
    let description = if first_run {
        "library configuration"
    } else {
        "Library Configuration"
    };
    match load_library(&paths.library_config()).map_err(fatal)? {
        LoadResult::Missing => Ok(LibrarySession {
            config: if first_run {
                LibraryConfig::first_run()
            } else {
                LibraryConfig::empty()
            },
            fingerprint: Fingerprint::Absent,
            first_run,
        }),
        LoadResult::Valid(loaded) => Ok(LibrarySession {
            config: loaded.value().clone(),
            fingerprint: loaded.fingerprint().clone(),
            first_run: false,
        }),
        LoadResult::Unsupported { version, .. } => Err(WorkflowError::InvalidInput {
            message: format!("unsupported {description} version {version}"),
        }),
        LoadResult::Invalid { issues } => Err(WorkflowError::InvalidInput {
            message: format_issues(&format!("invalid {description}"), &issues),
        }),
    }
}

fn initialize_library_directory(
    paths: &AppPaths,
    session: &LibrarySession,
    staged: &LibraryConfig,
) -> Result<(), WorkflowError> {
    if session.first_run {
        for location in staged.locations() {
            if location.path() == "./library" {
                std::fs::create_dir_all(paths.home.join(".skillator/library")).map_err(fatal)?;
            }
        }
    }
    Ok(())
}

fn classify_linked_library_location(path: &Path) -> LibraryLocationState {
    match path.canonicalize() {
        Ok(resolved) if resolved.is_dir() => LibraryLocationState::Present,
        Ok(_) => LibraryLocationState::Stale,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => LibraryLocationState::Stale,
        Err(error) => LibraryLocationState::Preserve(format!(
            "Library Location could not be resolved: {}: {error}",
            path.display()
        )),
    }
}

fn save_location_change(
    paths: &AppPaths,
    session: &LibrarySession,
    staged: &LibraryConfig,
    mode: SyncMode,
) -> Result<ReportOutcome, WorkflowError> {
    if mode == SyncMode::Check {
        return Ok(ReportOutcome::WouldApply);
    }
    save_library(&paths.library_config(), staged, &session.fingerprint).map_err(save_error)?;
    Ok(ReportOutcome::Applied)
}

fn stale_library_locations(
    paths: &AppPaths,
    config: &LibraryConfig,
) -> (Vec<usize>, Vec<ReportDiagnostic>) {
    let mut stale = Vec::new();
    let mut diagnostics = Vec::new();
    let library_config_path = paths.library_config();
    let config_parent = library_config_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    for (index, location) in config.locations().iter().enumerate() {
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
    (stale, diagnostics)
}
