//! User-owned directory loading and tab construction.
use super::invalid;
use super::model::{Row, TargetTabScope};
use super::target::{self, TargetTab};
use crate::app::{AppPaths, UserScopeSession, UserScopeWorkflow, WorkflowError};
use crate::config::{Fingerprint, LibraryConfig, RepositoryConfig, SkillDirectoryConfig};
use crate::domain::SkillKey;
use crate::library::LibrarySnapshot;
use crate::target::{Target, observe};
use std::collections::BTreeSet;

pub(super) fn load_user_for_tui(
    paths: &AppPaths,
) -> Result<(UserScopeSession, Option<String>), WorkflowError> {
    match UserScopeWorkflow::load(paths) {
        Ok(session) => Ok((session, None)),
        Err(error @ WorkflowError::InvalidInput { .. }) => Ok((
            UserScopeSession {
                target: Target::user(paths.home()).map_err(invalid)?,
                config: RepositoryConfig::empty(),
                fingerprint: Fingerprint::Absent,
                first_run: false,
            },
            Some(error.to_string()),
        )),
        Err(error) => Err(error),
    }
}

pub(super) fn user_enabled_skills(config: &RepositoryConfig) -> BTreeSet<SkillKey> {
    config
        .enablements()
        .iter()
        .map(|enablement| enablement.skill().clone())
        .collect()
}

pub(super) fn tabs(
    session: &UserScopeSession,
    library: &LibrarySnapshot,
    library_config: &LibraryConfig,
) -> Vec<TargetTab> {
    let observed = observe(&session.target, &session.config, library);
    session
        .config
        .skill_directories()
        .iter()
        .cloned()
        .zip(target::target_rows(
            &session.config,
            library,
            library_config,
            &observed,
            &BTreeSet::new(),
            None,
        ))
        .map(|(directory, rows)| TargetTab {
            scope: TargetTabScope::User,
            directory,
            rows,
        })
        .collect()
}

pub(super) fn rows_for_new_directory(
    directory: &SkillDirectoryConfig,
    session: &UserScopeSession,
    library: &LibrarySnapshot,
    library_config: &LibraryConfig,
) -> Vec<Row> {
    let observed = observe(&session.target, &session.config, library);
    target::rows_for_directory(
        directory,
        &session.config,
        library,
        library_config,
        &observed,
        &BTreeSet::new(),
        None,
    )
}
