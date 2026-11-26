//! Worktree behavior scenarios.

use super::*;

#[cfg(unix)]
#[test]
fn worktree_sync_projects_primary_local_state_and_materializes_skills() {
    let fixture = WorktreeFixture::new();

    let report = WorktreeSyncWorkflow::run(
        &fixture.paths,
        &fixture.linked,
        SyncMode::Apply { force: false },
    )
    .unwrap();

    assert_eq!(report.status, ReportStatus::InSync, "{report:#?}");
    assert_eq!(report.mode, "worktree_sync");
    assert_configuration_matches_primary(&fixture);
    assert!(
        std::fs::symlink_metadata(fixture.linked.join(".agents/skills/release-checklist"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let registry = std::fs::read_to_string(fixture.paths.target_registry()).unwrap();
    assert!(registry.contains(fixture.linked.to_string_lossy().as_ref()));
}

#[test]
fn worktree_sync_check_does_not_create_destination_configuration() {
    let fixture = WorktreeFixture::new();

    let report =
        WorktreeSyncWorkflow::run(&fixture.paths, &fixture.linked, SyncMode::Check).unwrap();

    assert_eq!(report.status, ReportStatus::NotConverged);
    assert!(report.changes.iter().any(|change| {
        change.path == ".agents/skillator.yaml" && change.outcome == ReportOutcome::WouldApply
    }));
    assert!(report.changes.iter().any(|change| {
        change.action == "register_target" && change.outcome == ReportOutcome::WouldApply
    }));
    assert!(!fixture.linked.join(".agents/skillator.yaml").exists());
    assert!(!fixture.linked.join(".agents/skills").exists());
    assert_not_registered(&fixture);
}

#[test]
fn worktree_sync_does_not_edit_root_ignore() {
    let fixture = WorktreeFixture::new();
    let root_ignore = fixture.linked.join(".gitignore");
    let before = std::fs::read(&root_ignore).unwrap();

    let report =
        WorktreeSyncWorkflow::run(&fixture.paths, &fixture.linked, SyncMode::Check).unwrap();

    assert_eq!(report.status, ReportStatus::NotConverged);
    assert_eq!(std::fs::read(root_ignore).unwrap(), before);
    assert!(!fixture.linked.join(".agents/skillator.yaml").exists());
}

#[test]
fn worktree_sync_requires_force_for_differing_untracked_configuration() {
    let fixture = WorktreeFixture::new();
    std::fs::create_dir_all(fixture.linked.join(".agents")).unwrap();
    std::fs::write(
        fixture.linked.join(".agents/skillator.yaml"),
        "version: 1\nskill_directories: []\nenablements: []\n",
    )
    .unwrap();
    let before = std::fs::read(fixture.linked.join(".agents/skillator.yaml")).unwrap();

    let guarded = WorktreeSyncWorkflow::run(
        &fixture.paths,
        &fixture.linked,
        SyncMode::Apply { force: false },
    )
    .unwrap();
    assert_eq!(guarded.exit_status, 1);
    assert!(
        guarded
            .changes
            .iter()
            .any(|change| change.outcome == ReportOutcome::WouldRequireForce)
    );
    assert_eq!(
        std::fs::read(fixture.linked.join(".agents/skillator.yaml")).unwrap(),
        before
    );
    assert_not_registered(&fixture);

    let forced = WorktreeSyncWorkflow::run(
        &fixture.paths,
        &fixture.linked,
        SyncMode::Apply { force: true },
    )
    .unwrap();
    assert_eq!(forced.mode, "worktree_sync_force");
    assert_eq!(forced.status, ReportStatus::InSync, "{forced:#?}");
    assert!(
        forced.changes.iter().any(|change| {
            change.path == ".agents/skillator.yaml" && change.safety == "guarded"
        })
    );
    assert_configuration_matches_primary(&fixture);
    assert!(fixture.paths.target_registry().is_file());
}

#[test]
fn worktree_sync_blocks_a_tracked_destination_configuration_even_with_force() {
    let fixture = WorktreeFixture::new();
    std::fs::create_dir_all(fixture.linked.join(".agents")).unwrap();
    let path = fixture.linked.join(".agents/skillator.yaml");
    std::fs::write(
        &path,
        "version: 1\nskill_directories: []\nenablements: []\n",
    )
    .unwrap();
    support::git(&fixture.linked, &["add", "-f", ".agents/skillator.yaml"]);
    let before = std::fs::read(&path).unwrap();

    let report = WorktreeSyncWorkflow::run(
        &fixture.paths,
        &fixture.linked,
        SyncMode::Apply { force: true },
    )
    .unwrap();

    assert_eq!(report.status, ReportStatus::NotConverged);
    assert_eq!(report.exit_status, 1);
    assert!(report.changes.iter().any(|change| {
        change.path == ".agents/skillator.yaml" && change.outcome == ReportOutcome::Blocked
    }));
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert_not_registered(&fixture);
}

#[cfg(unix)]
#[test]
fn worktree_sync_blocks_an_unreadable_destination_configuration() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = WorktreeFixture::new();
    std::fs::create_dir_all(fixture.linked.join(".agents")).unwrap();
    let path = fixture.linked.join(".agents/skillator.yaml");
    std::fs::write(
        &path,
        "version: 1\nskill_directories: []\nenablements: []\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

    let result = WorktreeSyncWorkflow::run(
        &fixture.paths,
        &fixture.linked,
        SyncMode::Apply { force: true },
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let report = result.unwrap();

    assert_eq!(report.status, ReportStatus::NotConverged);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("unreadable") })
    );
    assert!(report.changes.iter().any(|change| {
        change.path == ".agents/skillator.yaml" && change.outcome == ReportOutcome::Blocked
    }));
    assert_not_registered(&fixture);
}

#[test]
fn worktree_sync_rejects_missing_primary_configuration_without_writes() {
    let fixture = WorktreeFixture::new();
    std::fs::remove_file(fixture.primary.join(".agents/skillator.yaml")).unwrap();

    let error =
        WorktreeSyncWorkflow::run(&fixture.paths, &fixture.linked, SyncMode::Check).unwrap_err();
    assert!(error.to_string().contains("primary worktree configuration"));
    assert!(!fixture.linked.join(".agents").exists());
    assert_not_registered(&fixture);
}

#[cfg(unix)]
#[test]
fn worktree_sync_keeps_resolvable_skills_when_one_source_is_unavailable() {
    let fixture = WorktreeFixture::new();
    std::fs::write(
        fixture.primary.join(".agents/skillator.yaml"),
        "version: 1\nskill_directories:\n  - key: agents\n    path: .agents/skills\nenablements:\n  - directory: agents\n    skill:\n      source: local/library\n      path: release-checklist\n    materialization: linked\n  - directory: agents\n    skill:\n      source: missing/source\n      path: ghost\n    materialization: linked\n",
    )
    .unwrap();

    let report = WorktreeSyncWorkflow::run(
        &fixture.paths,
        &fixture.linked,
        SyncMode::Apply { force: false },
    )
    .unwrap();

    assert_eq!(report.status, ReportStatus::NotConverged);
    assert!(
        std::fs::symlink_metadata(fixture.linked.join(".agents/skills/release-checklist"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "final_enablement_state" || diagnostic.message.contains("unavailable")
    }));
}

fn assert_not_registered(fixture: &WorktreeFixture) {
    assert!(!fixture.paths.target_registry().exists());
}
fn assert_configuration_matches_primary(fixture: &WorktreeFixture) {
    assert_eq!(
        std::fs::read(fixture.linked.join(".agents/skillator.yaml")).unwrap(),
        std::fs::read(fixture.primary.join(".agents/skillator.yaml")).unwrap()
    );
}
