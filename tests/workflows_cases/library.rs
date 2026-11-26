//! Library behavior scenarios.

use super::*;

#[test]
fn library_inventory_changes_do_not_rewrite_target_references() {
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

    assert!(affected.is_empty());
    assert_eq!(
        std::fs::read(fixture.target.join(".agents/skillator.yaml")).unwrap(),
        repository_bytes
    );
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

    std::assert_matches!(error, WorkflowError::InvalidInput { .. });
    assert!(!fixture.target.join(".agents/skills").exists());
}

#[test]
fn first_run_library_remains_staged_until_confirmed_save() {
    let home = support::TestHome::new();
    let paths = AppPaths::new(home.path().to_owned());
    let session = LibraryWorkflow::load(&paths).unwrap();
    assert!(session.first_run);
    assert!(!home.path().join(".skillator").exists());

    let cancelled = LibraryWorkflow::save(&paths, &session, &session.config, false).unwrap_err();
    std::assert_matches!(cancelled, WorkflowError::Cancelled);
    assert!(!home.path().join(".skillator").exists());

    LibraryWorkflow::save(&paths, &session, &session.config, true).unwrap();
    assert!(home.library_config().exists());
    assert!(home.path().join(".skillator/library").is_dir());
}
