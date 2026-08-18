mod support;

use skillator::app::{
    AppPaths, LibraryWorkflow, ReportOutcome, ReportStatus, SyncMode, SyncWorkflow, TargetWorkflow,
    WorkflowError,
};
use skillator::config::{LibraryConfigCodec, LoadResult, RepositoryConfigCodec};
use skillator::library::scan_library;
use skillator::reconcile::prepare_check;
use skillator::target::Target;
use std::collections::BTreeMap;

#[test]
fn check_reports_safe_work_without_writing_any_target_entry() {
    let fixture = Fixture::new();
    let report = SyncWorkflow::run(&fixture.paths, &fixture.target, SyncMode::Check).unwrap();

    assert_eq!(report.exit_status, 1);
    assert!(
        report
            .changes
            .iter()
            .any(|change| change.outcome == ReportOutcome::WouldApply)
    );
    assert!(!fixture.target.join(".agents/skills").exists());
}

#[cfg(unix)]
#[test]
fn ordinary_sync_applies_safe_work_but_keeps_tracking_remediation_nonconverged() {
    let fixture = Fixture::new();
    let report = SyncWorkflow::run(
        &fixture.paths,
        &fixture.target,
        SyncMode::Apply { force: false },
    )
    .unwrap();

    assert_eq!(report.exit_status, 1);
    assert!(
        fixture
            .target
            .join(".agents/skills/release-checklist")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("git add -f -- .agents/skills/.gitignore")
    }));
}

#[test]
fn sync_without_repository_configuration_is_invalid_input_and_does_not_write() {
    let home = support::TestHome::new();
    let target = home.git_repo("target");
    let paths = AppPaths::new(home.path().to_owned());

    let error = SyncWorkflow::run(&paths, &target, SyncMode::Check).unwrap_err();
    assert!(matches!(error, WorkflowError::InvalidInput { .. }));
    assert!(!target.join(".agents").exists());
}

#[cfg(unix)]
#[test]
fn repeated_sync_converges_without_rewriting_materializations() {
    let fixture = Fixture::new();
    SyncWorkflow::run(
        &fixture.paths,
        &fixture.target,
        SyncMode::Apply { force: false },
    )
    .unwrap();
    support::git(&fixture.target, &["add", "-f", ".agents/skills/.gitignore"]);
    let link = fixture.target.join(".agents/skills/release-checklist");
    let before = std::fs::symlink_metadata(&link)
        .unwrap()
        .modified()
        .unwrap();

    let report = SyncWorkflow::run(
        &fixture.paths,
        &fixture.target,
        SyncMode::Apply { force: false },
    )
    .unwrap();

    assert_eq!(report.status, ReportStatus::InSync, "{report:#?}");
    assert_eq!(report.exit_status, 0);
    assert!(report.changes.is_empty());
    assert_eq!(
        std::fs::symlink_metadata(link).unwrap().modified().unwrap(),
        before
    );
}

#[test]
fn active_target_owner_makes_check_busy_without_writes() {
    let fixture = Fixture::new();
    let library_bytes = std::fs::read(fixture.paths.library_config()).unwrap();
    let LoadResult::Valid(library_config) = LibraryConfigCodec::parse(&library_bytes) else {
        panic!("valid Library config")
    };
    let library = scan_library(
        library_config.value(),
        &fixture.paths.library_config(),
        fixture.paths.home(),
        &BTreeMap::new(),
    );
    let repository_bytes = std::fs::read(fixture.target.join(".agents/skillator.yaml")).unwrap();
    let LoadResult::Valid(repository) = RepositoryConfigCodec::parse(&repository_bytes) else {
        panic!("valid Repository config")
    };
    let target = Target::select(&fixture.target).unwrap();
    let _owner = prepare_check(&target, repository.value(), &library).unwrap();

    let error = SyncWorkflow::run(&fixture.paths, &fixture.target, SyncMode::Check).unwrap_err();
    assert!(matches!(error, WorkflowError::Busy));
    assert!(!fixture.target.join(".agents/skills").exists());
}

#[test]
fn library_unregistration_identifies_current_target_references_without_rewriting_them() {
    let fixture = Fixture::new();
    let original_bytes = std::fs::read(fixture.paths.library_config()).unwrap();
    let LoadResult::Valid(original) = LibraryConfigCodec::parse(&original_bytes) else {
        panic!("valid Library config")
    };
    let LoadResult::Valid(staged) = LibraryConfigCodec::parse(
        b"version: 1\nlocations:\n  - path: \"./library\"\n    exclusions: []\n    allow_overlap: false\n    sources:\n      - key: \"local/library\"\n        path: \".\"\n        skills: []\n",
    ) else {
        panic!("valid staged Library config")
    };
    let repository_bytes = std::fs::read(fixture.target.join(".agents/skillator.yaml")).unwrap();
    let LoadResult::Valid(repository) = RepositoryConfigCodec::parse(&repository_bytes) else {
        panic!("valid Repository config")
    };

    let affected =
        LibraryWorkflow::affected_references(original.value(), staged.value(), repository.value());

    assert_eq!(affected.len(), 1);
    assert_eq!(affected[0].source().as_str(), "local/library");
    assert_eq!(affected[0].path().as_str(), "release-checklist");
    assert_eq!(
        std::fs::read(fixture.target.join(".agents/skillator.yaml")).unwrap(),
        repository_bytes
    );
}

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

    assert!(matches!(error, WorkflowError::InvalidInput { .. }));
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "externally changed\n"
    );
    assert!(!fixture.target.join(".agents/skills").exists());
}

#[test]
fn absent_library_keeps_enablements_unresolved_without_creating_a_library() {
    let home = support::TestHome::new();
    let target = home.git_repo("target");
    std::fs::create_dir_all(target.join(".agents")).unwrap();
    std::fs::write(
        target.join(".agents/skillator.yaml"),
        "version: 1\nskill_directories:\n  - key: agents\n    path: .agents/skills\nenablements:\n  - directory: agents\n    skill:\n      source: missing/source\n      path: ghost\n    materialization: linked\n",
    )
    .unwrap();
    let paths = AppPaths::new(home.path().to_owned());

    let report = SyncWorkflow::run(&paths, &target, SyncMode::Check).unwrap();

    assert_eq!(report.exit_status, 1);
    assert!(!home.library_config().exists());
    assert!(!target.join(".agents/skills").exists());
}

#[test]
fn unsupported_library_configuration_is_read_only_invalid_input() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.paths.library_config(),
        "version: 2\nlocations: []\n",
    )
    .unwrap();

    let error = SyncWorkflow::run(&fixture.paths, &fixture.target, SyncMode::Check).unwrap_err();

    assert!(matches!(error, WorkflowError::InvalidInput { .. }));
    assert!(!fixture.target.join(".agents/skills").exists());
}

#[test]
fn ordinary_sync_partially_applies_safe_work_and_preserves_guarded_occupants() {
    let fixture = Fixture::new();
    let occupant = fixture.target.join(".agents/skills/release-checklist");
    std::fs::create_dir_all(occupant.parent().unwrap()).unwrap();
    std::fs::write(&occupant, "mine").unwrap();

    let report = SyncWorkflow::run(
        &fixture.paths,
        &fixture.target,
        SyncMode::Apply { force: false },
    )
    .unwrap();

    assert_eq!(report.exit_status, 1);
    assert_eq!(std::fs::read_to_string(occupant).unwrap(), "mine");
    assert!(
        report
            .changes
            .iter()
            .any(|change| change.outcome == ReportOutcome::NotAuthorized)
    );
    assert!(fixture.target.join(".agents/skills/.gitignore").exists());
}

#[test]
fn first_run_library_remains_staged_until_confirmed_save() {
    let home = support::TestHome::new();
    let paths = AppPaths::new(home.path().to_owned());
    let session = LibraryWorkflow::load(&paths).unwrap();
    assert!(session.first_run);
    assert!(!home.path().join(".skillator").exists());

    let cancelled = LibraryWorkflow::save(&paths, &session, &session.config, false).unwrap_err();
    assert!(matches!(cancelled, WorkflowError::Cancelled));
    assert!(!home.path().join(".skillator").exists());

    LibraryWorkflow::save(&paths, &session, &session.config, true).unwrap();
    assert!(home.library_config().exists());
    assert!(home.path().join(".skillator/library").is_dir());
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

    assert!(matches!(error, WorkflowError::InvalidInput { .. }));
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

struct Fixture {
    _home: support::TestHome,
    paths: AppPaths,
    target: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let home = support::TestHome::new();
        let library = home.path().join(".skillator/library/release-checklist");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(
            library.join("SKILL.md"),
            "---\nname: release-checklist\ndescription: Prepare a release\n---\n",
        )
        .unwrap();
        std::fs::write(
            home.library_config(),
            "version: 1\nlocations:\n  - path: \"./library\"\n    exclusions: []\n    allow_overlap: false\n    sources:\n      - key: \"local/library\"\n        path: \".\"\n        skills:\n          - path: \"release-checklist\"\n",
        )
        .unwrap();
        let target = home.git_repo("target");
        std::fs::create_dir_all(target.join(".agents")).unwrap();
        std::fs::write(
            target.join(".agents/skillator.yaml"),
            "version: 1\nskill_directories:\n  - key: \"agents\"\n    path: \".agents/skills\"\nenablements:\n  - directory: \"agents\"\n    skill:\n      source: \"local/library\"\n      path: \"release-checklist\"\n    materialization: \"linked\"\n",
        )
        .unwrap();
        Self {
            paths: AppPaths::new(home.path().to_owned()),
            _home: home,
            target,
        }
    }
}
