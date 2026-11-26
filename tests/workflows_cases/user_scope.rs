//! User scope behavior scenarios.

use super::*;

#[test]
fn first_user_scope_save_creates_desired_state_without_git_control_files() {
    let home = support::TestHome::new();
    let paths = AppPaths::new(home.path().to_owned());
    let session = UserScopeWorkflow::load(&paths).unwrap();

    assert!(session.first_run);
    assert_eq!(session.config.skill_directories()[0].label(), Some("User"));
    assert!(!paths.user_config().exists());

    let prepared =
        UserScopeWorkflow::prepare_save(&paths, &session, session.config.clone()).unwrap();
    assert!(prepared.plan().items().iter().any(|item| {
        item.action() == Action::CreateDirectory
            && item.safety() == Safety::Safe
            && item.path() == home.path().canonicalize().unwrap().join(".agents/skills")
    }));
    assert!(
        prepared
            .plan()
            .items()
            .iter()
            .all(|item| item.action() != Action::WriteControlFile)
    );

    UserScopeWorkflow::commit_save(&paths, prepared, Authorization::SafeOnly).unwrap();

    assert!(paths.user_config().is_file());
    assert!(home.path().join(".agents/skills").is_dir());
    assert!(!home.path().join(".agents/.gitignore").exists());
}

#[cfg(unix)]
#[test]
fn first_user_skill_link_previews_without_creating_agents_and_activates_on_apply() {
    let fixture = Fixture::new();
    let paths = &fixture.paths;
    let selector = SkillSelector::parse("local/library:release-checklist").unwrap();
    let agents = paths.home().join(".agents");
    let library_bytes = std::fs::read(paths.library_config()).unwrap();

    assert!(!agents.exists());
    let preview = UserScopeWorkflow::mutate_enablement(
        paths,
        &selector,
        Some(MaterializationKind::Linked),
        SyncMode::Check,
    )
    .unwrap();

    assert_eq!(preview.status, ReportStatus::NotConverged, "{preview:#?}");
    for (path, action) in [
        (".agents/skills", "create_directory"),
        (".agents/skills/release-checklist", "link"),
        (".agents/skillator.yaml", "write_user_configuration"),
    ] {
        assert!(preview.changes.iter().any(|change| {
            change.path == path
                && change.action == action
                && change.outcome == ReportOutcome::WouldApply
        }));
    }
    assert!(!agents.exists());
    assert!(!paths.target_registry().exists());
    assert_eq!(
        std::fs::read(paths.library_config()).unwrap(),
        library_bytes
    );
    assert!(UserScopeWorkflow::load(paths).unwrap().first_run);

    let applied = UserScopeWorkflow::mutate_enablement(
        paths,
        &selector,
        Some(MaterializationKind::Linked),
        SyncMode::Apply { force: false },
    )
    .unwrap();

    assert_eq!(applied.status, ReportStatus::InSync, "{applied:#?}");
    let link = agents.join("skills/release-checklist");
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        paths
            .home()
            .join(".skillator/library/release-checklist")
            .canonicalize()
            .unwrap()
    );
    assert_saved_user_link(paths, &selector);
    let converged = UserScopeWorkflow::mutate_enablement(
        paths,
        &selector,
        Some(MaterializationKind::Linked),
        SyncMode::Check,
    )
    .unwrap();
    assert_eq!(converged.status, ReportStatus::InSync, "{converged:#?}");
    assert!(converged.changes.is_empty());
    assert!(!agents.join(".gitignore").exists());
    assert!(!paths.target_registry().exists());
}

#[cfg(unix)]
#[test]
fn first_user_skill_link_refuses_linked_or_occupied_destination_ancestors() {
    for relative in [".agents", ".agents/skills"] {
        for linked in [false, true] {
            let fixture = Fixture::new();
            let paths = &fixture.paths;
            let selector = SkillSelector::parse("local/library:release-checklist").unwrap();
            let ancestor = paths.home().join(relative);
            std::fs::create_dir_all(ancestor.parent().unwrap()).unwrap();
            let outside = paths.home().join("outside");
            if linked {
                std::fs::create_dir(&outside).unwrap();
                std::fs::write(outside.join("keep"), "untouched").unwrap();
                std::os::unix::fs::symlink(&outside, &ancestor).unwrap();
            } else {
                std::fs::write(&ancestor, "occupied").unwrap();
            }

            for mode in [
                SyncMode::Check,
                SyncMode::Apply { force: false },
                SyncMode::Apply { force: true },
            ] {
                let result = UserScopeWorkflow::mutate_enablement(
                    paths,
                    &selector,
                    Some(MaterializationKind::Linked),
                    mode,
                );
                if relative == ".agents" {
                    std::assert_matches!(result, Err(WorkflowError::InvalidInput { .. }));
                } else {
                    let report = result.unwrap();
                    assert_eq!(report.status, ReportStatus::NotConverged, "{report:#?}");
                    assert!(report.changes.iter().any(|change| {
                        change.path == relative && change.outcome == ReportOutcome::Blocked
                    }));
                }
                assert!(!paths.user_config().exists());
                assert!(!paths.target_registry().exists());
                if linked {
                    assert_eq!(std::fs::read_link(&ancestor).unwrap(), outside);
                    assert_eq!(
                        std::fs::read_to_string(outside.join("keep")).unwrap(),
                        "untouched"
                    );
                    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
                } else {
                    assert_eq!(std::fs::read_to_string(&ancestor).unwrap(), "occupied");
                }
            }
        }
    }
}

#[cfg(unix)]
fn assert_saved_user_link(paths: &AppPaths, selector: &SkillSelector) {
    let saved = UserScopeWorkflow::load(paths).unwrap();
    assert!(!saved.first_run);
    assert_eq!(saved.config.enablements().len(), 1);
    assert_eq!(saved.config.enablements()[0].skill(), selector.key());
    assert_eq!(
        saved.config.enablements()[0].materialization(),
        MaterializationKind::Linked
    );
}
