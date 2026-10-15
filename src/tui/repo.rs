//! Repository-owned directory loading, physical skills and tracking exceptions.
use super::model::{CheckState, Row, TargetTabScope};
use super::target::{self, TargetTab};
use super::{invalid, user};
use crate::app::{AppPaths, TargetSession, TargetWorkflow, WorkflowError};
use crate::config::{Fingerprint, LibraryConfig, RepositoryConfig, SkillDirectoryConfig};
use crate::library::{LibrarySnapshot, inspect_skill_metadata};
use crate::target::{
    CONTROL_FILE_EXCEPTIONS, RepositorySkillExceptions, Target, control_file_path, observe,
    repository_tracking_rule,
};
use std::collections::BTreeSet;
use std::path::Path;

pub(super) fn load_repository_for_tui(
    paths: &AppPaths,
    directory: &Path,
) -> Result<(TargetSession, Option<String>), WorkflowError> {
    match TargetWorkflow::load(directory) {
        Ok(session) => Ok((session, None)),
        Err(error @ WorkflowError::InvalidInput { .. }) => {
            let target = Target::select(directory)
                .or_else(|_| Target::user(paths.home()))
                .map_err(invalid)?;
            Ok((
                TargetSession {
                    target,
                    config: RepositoryConfig::empty(),
                    fingerprint: Fingerprint::Absent,
                    first_run: false,
                    recommendations: Vec::new(),
                },
                Some(error.to_string()),
            ))
        }
        Err(error) => Err(error),
    }
}

pub(super) fn repository_skill_exceptions(tabs: &[TargetTab]) -> RepositorySkillExceptions {
    let mut exceptions = RepositorySkillExceptions::new();
    for tab in tabs
        .iter()
        .filter(|tab| tab.scope == TargetTabScope::Repository)
    {
        let names = tab
            .rows
            .iter()
            .filter(|row| row.repository_candidate && row.check == Some(CheckState::Repository))
            .filter_map(|row| row.repository_name.clone())
            .collect::<BTreeSet<_>>();
        if !names.is_empty() {
            exceptions.insert(tab.directory.key().as_str().to_owned(), names);
        }
    }
    exceptions
}

pub(super) fn repository_skill_rows(
    directory: &SkillDirectoryConfig,
    target: &Target,
    observation: &crate::target::DirectoryObservation,
) -> Vec<Row> {
    let control = target.root().join(control_file_path(directory));
    let exceptions = std::fs::read_to_string(control)
        .ok()
        .and_then(|content| {
            content
                .split_once(CONTROL_FILE_EXCEPTIONS)
                .map(|(_, suffix)| suffix.lines().map(str::to_owned).collect::<BTreeSet<_>>())
        })
        .unwrap_or_default();
    let mut skills = Vec::new();
    for path in observation.unmanaged_entries() {
        let Ok(metadata) = std::fs::symlink_metadata(path) else {
            continue;
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        let Ok(document) = std::fs::read_to_string(path.join("SKILL.md")) else {
            continue;
        };
        let Some(repository_name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        let Ok(metadata) = inspect_skill_metadata(document.as_bytes(), Some(&repository_name))
        else {
            continue;
        };
        if !metadata.errors.is_empty() {
            continue;
        }
        let excepted = repository_tracking_rule(directory, &repository_name)
            .is_some_and(|rule| exceptions.contains(&rule));
        let relative = path.strip_prefix(target.root()).unwrap_or(path);
        let tracked = target
            .repository()
            .facts_for(relative)
            .is_ok_and(|facts| facts.tracked);
        let mut row = Row::repository_skill(
            repository_name,
            metadata.name,
            metadata.description,
            document,
            path,
            tracked,
            excepted,
        );
        row.warnings = metadata.warnings;
        skills.push(row);
    }
    if skills.is_empty() {
        return Vec::new();
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    let mut source = Row::source("Repository", CheckState::Unchecked);
    source.description = format!(
        "{} repository-owned skill{}",
        skills.len(),
        if skills.len() == 1 { "" } else { "s" }
    );
    let mut rows = vec![source];
    rows.extend(skills);
    rows
}

pub(super) fn tabs(
    session: &TargetSession,
    user_config: &RepositoryConfig,
    library: &LibrarySnapshot,
    library_config: &LibraryConfig,
) -> Vec<TargetTab> {
    let observed = observe(&session.target, &session.config, library);
    let inherited = user::user_enabled_skills(user_config);
    let mut tabs = session
        .config
        .skill_directories()
        .iter()
        .cloned()
        .zip(target::target_rows(
            &session.config,
            library,
            library_config,
            &observed,
            &inherited,
            Some(&session.target),
        ))
        .map(|(directory, rows)| TargetTab {
            scope: TargetTabScope::Repository,
            directory,
            rows,
        })
        .collect::<Vec<_>>();
    if session.first_run
        && let Some(first) = tabs.first_mut()
    {
        for recommendation in session.recommendations.iter().rev() {
            let mut row = Row::diagnostic(format!(
                "[ ] {} exists; press `a` to add this skill folder",
                recommendation.path()
            ));
            row.name = "Recommendation".to_owned();
            row.state = "Unchecked".to_owned();
            first.rows.insert(0, row);
        }
    }
    tabs
}

pub(super) fn rows_for_new_directory(
    directory: &SkillDirectoryConfig,
    session: &TargetSession,
    user_config: &RepositoryConfig,
    library: &LibrarySnapshot,
    library_config: &LibraryConfig,
) -> Vec<Row> {
    let observed = observe(&session.target, &session.config, library);
    let inherited = user::user_enabled_skills(user_config);
    target::rows_for_directory(
        directory,
        &session.config,
        library,
        library_config,
        &observed,
        &inherited,
        Some(&session.target),
    )
}
