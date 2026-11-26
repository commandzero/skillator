//! Registry behavior scenarios.

use super::*;

#[test]
fn target_registry_keeps_linked_worktrees_as_distinct_targets() {
    let home = support::TestHome::new();
    let primary = home.git_repo("primary");
    support::git(&primary, &["config", "user.name", "Skillator Tests"]);
    support::git(&primary, &["config", "user.email", "tests@example.invalid"]);
    std::fs::write(primary.join("seed"), "seed").unwrap();
    support::git(&primary, &["add", "seed"]);
    support::git(&primary, &["commit", "--quiet", "-m", "seed"]);
    let linked = home.path().join("linked");
    support::git(
        &primary,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );

    for target in [&primary, &linked] {
        Command::cargo_bin("skillator")
            .unwrap()
            .arg("init")
            .arg(target)
            .env("HOME", home.path())
            .assert()
            .success();
    }
    let registry = std::fs::read_to_string(home.path().join(".skillator/targets.yaml")).unwrap();
    assert!(registry.contains(primary.to_string_lossy().as_ref()));
    assert!(registry.contains(linked.to_string_lossy().as_ref()));
}

#[test]
fn scope_and_target_registry_lists_are_machine_readable() {
    let fixture = Fixture::new();
    fixture.command().arg("init").assert().success();

    let target_json = fixture
        .command()
        .args(["target", "list", "--format=json"])
        .output()
        .unwrap();
    assert!(target_json.status.success());
    let target_yaml = fixture
        .command()
        .args(["target", "list", "--format=yaml"])
        .output()
        .unwrap();
    assert!(target_yaml.status.success());
    let json_value: Value = serde_json::from_slice(&target_json.stdout).unwrap();
    let yaml_value: Value = serde_saphyr::from_slice(&target_yaml.stdout).unwrap();
    assert_eq!(json_value, yaml_value);
    assert_eq!(json_value["directories"][0]["key"], "agents");
    assert_eq!(
        json_value["directories"][0]["enablements"][0]["source"],
        "local/library"
    );

    fixture
        .command()
        .args(["user", "list", "--format=json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"directories\": []"));

    fixture
        .command()
        .args(["targets", "list", "--format=json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"available\""))
        .stdout(predicate::str::contains(
            fixture.target.to_string_lossy().as_ref(),
        ));
}

#[test]
fn target_registry_remove_and_prune_are_previewable_and_non_destructive() {
    let home = support::TestHome::new();
    let configured = home.git_repo("configured");
    let unconfigured = home.git_repo("unconfigured");
    let invalid = home.git_repo("invalid");
    let missing = home.path().join("missing");
    for target in [&configured, &invalid] {
        std::fs::create_dir_all(target.join(".agents")).unwrap();
    }
    std::fs::write(
        configured.join(".agents/skillator.yaml"),
        "version: 1\nskill_directories: []\nenablements: []\n",
    )
    .unwrap();
    std::fs::write(invalid.join(".agents/skillator.yaml"), "version: nope\n").unwrap();
    std::fs::create_dir_all(home.path().join(".skillator")).unwrap();
    let registry_path = home.path().join(".skillator/targets.yaml");
    std::fs::write(
        &registry_path,
        format!(
            "version: 1\ntargets:\n  - {}\n  - {}\n  - {}\n  - {}\n",
            serde_json::to_string(configured.to_str().unwrap()).unwrap(),
            serde_json::to_string(invalid.to_str().unwrap()).unwrap(),
            serde_json::to_string(missing.to_str().unwrap()).unwrap(),
            serde_json::to_string(unconfigured.to_str().unwrap()).unwrap(),
        ),
    )
    .unwrap();
    let before = std::fs::read(&registry_path).unwrap();

    assert_registry_lists_all_states(&home);

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["targets", "prune", "--check", "--format=json"])
        .env("HOME", home.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("prune_target_registration"))
        .stdout(predicate::str::contains("target_registration_preserved"));
    assert_eq!(std::fs::read(&registry_path).unwrap(), before);

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["targets", "prune"])
        .env("HOME", home.path())
        .assert()
        .success();
    let pruned = std::fs::read_to_string(&registry_path).unwrap();
    assert!(pruned.contains(configured.to_string_lossy().as_ref()));
    assert!(pruned.contains(invalid.to_string_lossy().as_ref()));
    assert!(!pruned.contains(missing.to_string_lossy().as_ref()));
    assert!(!pruned.contains(unconfigured.to_string_lossy().as_ref()));
    assert!(unconfigured.is_dir());
    assert!(invalid.join(".agents/skillator.yaml").is_file());

    for _ in 0..2 {
        Command::cargo_bin("skillator")
            .unwrap()
            .args(["targets", "remove"])
            .arg(&invalid)
            .env("HOME", home.path())
            .assert()
            .success();
    }
    assert!(invalid.join(".agents/skillator.yaml").is_file());
}

#[cfg(unix)]
#[test]
fn registered_target_inspection_does_not_follow_repository_config_symlinks() {
    let home = support::TestHome::new();
    let target = home.git_repo("symlinked-config");
    std::fs::create_dir_all(target.join(".agents")).unwrap();
    let external_config = home.path().join("external-skillator.yaml");
    std::fs::write(
        &external_config,
        "version: 1\nskill_directories: []\nenablements: []\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&external_config, target.join(".agents/skillator.yaml")).unwrap();

    let parent_target = home.git_repo("symlinked-parent");
    let external_parent = home.path().join("external-agents");
    std::fs::create_dir_all(&external_parent).unwrap();
    std::fs::write(
        external_parent.join("skillator.yaml"),
        "version: 1\nskill_directories: []\nenablements: []\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&external_parent, parent_target.join(".agents")).unwrap();

    std::fs::create_dir_all(home.path().join(".skillator")).unwrap();
    let registry_path = home.path().join(".skillator/targets.yaml");
    std::fs::write(
        &registry_path,
        format!(
            "version: 1\ntargets:\n  - {}\n  - {}\n",
            serde_json::to_string(target.to_str().unwrap()).unwrap(),
            serde_json::to_string(parent_target.to_str().unwrap()).unwrap(),
        ),
    )
    .unwrap();
    let before = std::fs::read(&registry_path).unwrap();

    assert_registry_rejects_linked_configuration(&home);

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["targets", "prune", "--format=json"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("target_registration_preserved"));
    assert_eq!(std::fs::read(&registry_path).unwrap(), before);

    std::fs::write(
        home.path().join(".skillator/library.yaml"),
        "version: 1\nlocations:\n  - path: \"./missing-library\"\n    exclusions: []\n    allow_overlap: false\n",
    )
    .unwrap();
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["library", "prune", "--check", "--format=json"])
        .env("HOME", home.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("registered_target_unavailable"))
        .stdout(predicate::str::contains(
            "Repository configuration must be a regular file, not a link",
        ))
        .stdout(predicate::str::contains(
            "Repository configuration parent must be a directory, not a link",
        ));
}

fn assert_registry_lists_all_states(home: &support::TestHome) {
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["targets", "list", "--format=json"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"available\""))
        .stdout(predicate::str::contains("\"status\": \"unavailable\""))
        .stdout(predicate::str::contains("\"status\": \"unconfigured\""))
        .stdout(predicate::str::contains("\"status\": \"invalid\""));
}

#[cfg(unix)]
fn assert_registry_rejects_linked_configuration(home: &support::TestHome) {
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["targets", "list", "--format=json"])
        .env("HOME", home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"invalid\""))
        .stdout(predicate::str::contains(
            "Repository configuration must be a regular file, not a link",
        ))
        .stdout(predicate::str::contains(
            "Repository configuration parent must be a directory, not a link",
        ));
}
