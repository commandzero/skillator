//! User account configuration and enablement workflows.

use super::*;

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
            (&session.target, &session.config),
            crate::target::observe(&session.target, &session.config, &library),
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
        let staged = stage_enablement(
            paths,
            &session.config,
            selector,
            EnablementEdit {
                directory: None,
                materialization,
                configuration_name: "User Scope Configuration",
            },
        )?;
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
