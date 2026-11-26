//! Target save behavior scenarios.

use super::*;

#[test]
fn stale_target_configuration_blocks_save_before_reconciliation() {
    let fixture = Fixture::new();
    let session = TargetWorkflow::load(&fixture.target).unwrap();
    let prepared =
        TargetWorkflow::prepare_save(&fixture.paths, &session, session.config.clone()).unwrap();
    let path = fixture.target.join(".agents/skillator.yaml");
    std::fs::write(&path, "externally changed\n").unwrap();

    let error =
        TargetWorkflow::commit_save(prepared, skillator::reconcile::Authorization::SafeOnly)
            .unwrap_err();

    std::assert_matches!(error, WorkflowError::InvalidInput { .. });
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "externally changed\n"
    );
    assert!(!fixture.target.join(".agents/skills").exists());
}

#[test]
fn target_save_keeps_clone_local_rules_in_agents_control_file() {
    let fixture = Fixture::new();
    let session = TargetWorkflow::load(&fixture.target).unwrap();
    let prepared =
        TargetWorkflow::prepare_save(&fixture.paths, &session, session.config.clone()).unwrap();

    TargetWorkflow::commit_save(prepared, Authorization::SafeOnly).unwrap();

    assert!(!fixture.target.join(".gitignore").exists());
    assert_eq!(
        std::fs::read_to_string(fixture.target.join(".agents/.gitignore")).unwrap(),
        AGENTS_CONTROL
    );
}

#[test]
fn tracked_local_target_configuration_blocks_save_with_index_remediation() {
    let fixture = Fixture::new();
    support::git(&fixture.target, &["add", "-f", ".agents/skillator.yaml"]);
    let session = TargetWorkflow::load(&fixture.target).unwrap();
    let prepared =
        TargetWorkflow::prepare_save(&fixture.paths, &session, session.config.clone()).unwrap();

    let error = TargetWorkflow::commit_save(prepared, Authorization::SafeOnly).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("git rm --cached -- .agents/skillator.yaml")
    );
}

#[cfg(unix)]
#[test]
fn repository_configuration_parent_symlink_is_rejected_without_external_writes() {
    let home = support::TestHome::new();
    let target = home.git_repo("target");
    let outside = home.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, target.join(".agents")).unwrap();
    let paths = AppPaths::new(home.path().to_owned());

    let error = SyncWorkflow::run(&paths, &target, SyncMode::Check).unwrap_err();

    std::assert_matches!(error, WorkflowError::InvalidInput { .. });
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
}

#[test]
fn target_save_restores_a_unique_backup_only_after_configuration_is_saved() {
    let fixture = Fixture::new();
    let root = fixture.target.join(".agents/skills");
    std::fs::create_dir_all(&root).unwrap();
    let encoded = "release-checklist"
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let backup = root.join(format!(".skillator-backup-1-1-{encoded}"));
    std::fs::write(&backup, "preserved").unwrap();
    let session = TargetWorkflow::load(&fixture.target).unwrap();
    let prepared =
        TargetWorkflow::prepare_save(&fixture.paths, &session, session.config.clone()).unwrap();
    assert!(prepared.plan().items().iter().any(|item| {
        item.action() == skillator::reconcile::Action::Recover
            && item.safety() == skillator::reconcile::Safety::Safe
    }));

    let report =
        TargetWorkflow::commit_save(prepared, skillator::reconcile::Authorization::AllGuarded)
            .unwrap();

    assert_eq!(
        std::fs::read_to_string(root.join("release-checklist")).unwrap(),
        "preserved"
    );
    assert!(!backup.exists());
    assert_eq!(report.exit_status, 1);
    assert!(report.changes.iter().any(|change| {
        change.action == "recover" && change.outcome == skillator::app::ReportOutcome::Applied
    }));
}

#[test]
fn first_run_detects_existing_native_skill_directories_as_recommendations() {
    let home = support::TestHome::new();
    let target = home.git_repo("target");
    std::fs::create_dir_all(target.join(".claude/skills")).unwrap();
    std::fs::create_dir_all(target.join(".cursor/skills")).unwrap();

    let session = TargetWorkflow::load(&target).unwrap();

    assert!(session.first_run);
    assert_eq!(
        session
            .recommendations
            .iter()
            .map(|directory| directory.path().as_str())
            .collect::<Vec<_>>(),
        vec![".claude/skills", ".cursor/skills"]
    );
    assert_eq!(session.config.skill_directories().len(), 1);
}

#[test]
fn first_target_save_creates_required_paths_without_initialization_diagnostics() {
    let home = support::TestHome::new();
    let paths = AppPaths::new(home.path().to_owned());
    let target_root = home.git_repo("target");
    let session = TargetWorkflow::load(&target_root).unwrap();
    let staged = session
        .config
        .with_skill_directories(vec![
            session.config.skill_directories()[0].clone(),
            skillator::config::SkillDirectoryConfig::claude_preset(),
        ])
        .unwrap();
    let library_config = LibraryConfig::first_run();
    let library = scan_library(
        &library_config,
        &paths.library_config(),
        paths.home(),
        &BTreeMap::new(),
    );
    let observed = observe(&session.target, &staged, &library);

    assert!(!target_root.join(".agents").exists());
    for directory in observed.directories() {
        assert!(directory.diagnostics().iter().all(|message| {
            !message.contains("skill folder is not initialized")
                && !message.contains(".gitignore is missing")
        }));
    }

    let prepared = TargetWorkflow::prepare_save(&paths, &session, staged).unwrap();
    for relative in [".agents/skills", ".claude/skills"] {
        assert!(prepared.plan().items().iter().any(|item| {
            item.action() == Action::CreateDirectory
                && item.safety() == Safety::Safe
                && item.path() == session.target.root().join(relative)
        }));
    }
    TargetWorkflow::commit_save(prepared, Authorization::SafeOnly).unwrap();

    assert!(target_root.join(".agents/skillator.yaml").is_file());
    assert!(target_root.join(".agents/skills").is_dir());
    assert!(target_root.join(".claude/skills").is_dir());
    assert_eq!(
        std::fs::read_to_string(target_root.join(".agents/.gitignore")).unwrap(),
        AGENTS_CONTROL
    );
    assert_eq!(
        std::fs::read_to_string(target_root.join(".claude/.gitignore")).unwrap(),
        "# Generated by Skillator\n.gitignore\nskills/*\n\n# Exception list for repository tracking\n"
    );
    assert!(!target_root.join(".gitignore").exists());
}
