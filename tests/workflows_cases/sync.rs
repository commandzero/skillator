//! Sync behavior scenarios.

use super::*;

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
fn ordinary_sync_applies_safe_work_without_staging_local_controls() {
    let fixture = Fixture::new();
    let report = SyncWorkflow::run(
        &fixture.paths,
        &fixture.target,
        SyncMode::Apply { force: false },
    )
    .unwrap();

    assert_eq!(report.exit_status, 0);
    assert!(
        fixture
            .target
            .join(".agents/skills/release-checklist")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| { !diagnostic.message.contains("git add -- .agents/.gitignore") })
    );
}

#[test]
fn sync_without_repository_configuration_is_invalid_input_and_does_not_write() {
    let home = support::TestHome::new();
    let target = home.git_repo("target");
    let paths = AppPaths::new(home.path().to_owned());

    let error = SyncWorkflow::run(&paths, &target, SyncMode::Check).unwrap_err();
    std::assert_matches!(error, WorkflowError::InvalidInput { .. });
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
    std::assert_matches!(error, WorkflowError::Busy);
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
    assert!(fixture.target.join(".agents/.gitignore").exists());
}
