//! Scope mutations behavior scenarios.

use super::*;

#[cfg(unix)]
#[test]
fn target_and_user_commands_materialize_canonical_selectors() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.target.join(".agents/skillator.yaml"),
        "version: 1\nskill_directories:\n  - key: \"agents\"\n    path: \".agents/skills\"\nenablements: []\n",
    )
    .unwrap();

    fixture
        .command()
        .args([
            "target",
            "link",
            "local/library:release-checklist",
            "--check",
        ])
        .assert()
        .code(1);
    assert!(
        !fixture
            .target
            .join(".agents/skills/release-checklist")
            .exists()
    );

    fixture
        .command()
        .args(["target", "link", "local/library:release-checklist"])
        .assert()
        .success();
    assert_target_linked(&fixture);

    assert_target_copy(&fixture);

    assert_user_link(&fixture);
    fixture
        .command()
        .args([
            "target",
            "remove",
            "local/library:release-checklist",
            "--force",
        ])
        .assert()
        .success();
    assert!(
        !fixture
            .target
            .join(".agents/skills/release-checklist")
            .exists()
    );

    fixture
        .command()
        .args(["user", "remove", "local/library:release-checklist"])
        .assert()
        .success();
    assert!(
        !fixture
            .home
            .path()
            .join(".agents/skills/release-checklist")
            .exists()
    );
}

#[test]
fn canonical_selector_errors_do_not_write() {
    let home = support::TestHome::new();
    let target = home.git_repo("target");
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["target", "link", "release-checklist"])
        .current_dir(&target)
        .env("HOME", home.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains("source-key"));
    assert!(!target.join(".agents").exists());
}

#[test]
fn target_mutation_does_not_publish_configuration_when_entry_is_guarded() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.target.join(".agents/skillator.yaml"),
        "version: 1\nskill_directories:\n  - key: \"agents\"\n    path: \".agents/skills\"\nenablements: []\n",
    )
    .unwrap();
    let occupant = fixture.target.join(".agents/skills/release-checklist");
    std::fs::create_dir_all(occupant.parent().unwrap()).unwrap();
    std::fs::write(&occupant, "mine").unwrap();

    fixture
        .command()
        .args(["target", "link", "local/library:release-checklist"])
        .assert()
        .code(1);
    assert_eq!(std::fs::read_to_string(&occupant).unwrap(), "mine");
    let config = std::fs::read_to_string(fixture.target.join(".agents/skillator.yaml")).unwrap();
    assert!(config.contains("enablements: []"));
}

#[test]
fn user_mutation_preserves_an_unmanaged_collision() {
    let fixture = Fixture::new();
    let occupant = fixture.home.path().join(".agents/skills/release-checklist");
    std::fs::create_dir_all(occupant.parent().unwrap()).unwrap();
    std::fs::write(&occupant, "mine").unwrap();

    fixture
        .command()
        .args(["user", "link", "local/library:release-checklist"])
        .assert()
        .code(1);
    assert_eq!(std::fs::read_to_string(&occupant).unwrap(), "mine");
    assert!(!fixture.home.path().join(".agents/skillator.yaml").exists());
}

#[test]
fn user_remove_on_uninitialized_scope_is_unchanged_and_write_free() {
    let home = support::TestHome::new();

    Command::cargo_bin("skillator")
        .unwrap()
        .args([
            "user",
            "remove",
            "local/library:release-checklist",
            "--format=json",
        ])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"in_sync\""))
        .stdout(predicate::str::contains("\"changes\": []"));

    assert!(!home.path().join(".agents/skillator.yaml").exists());
    assert!(!home.path().join(".agents/skills").exists());
}

#[cfg(unix)]
fn assert_user_link(fixture: &Fixture) {
    fixture
        .command()
        .args(["user", "link", "local/library:release-checklist"])
        .assert()
        .success();
    assert!(
        std::fs::symlink_metadata(fixture.home.path().join(".agents/skills/release-checklist"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
fn assert_target_linked(fixture: &Fixture) {
    assert!(
        std::fs::symlink_metadata(fixture.target.join(".agents/skills/release-checklist"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
fn assert_target_copy(fixture: &Fixture) {
    fixture
        .command()
        .args(["target", "copy", "local/library:release-checklist"])
        .assert()
        .success();
    assert!(
        fixture
            .target
            .join(".agents/skills/release-checklist")
            .is_dir()
    );
}
