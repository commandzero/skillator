//! Library behavior scenarios.

use super::*;

#[test]
fn library_command_rejects_non_tty_without_writes() {
    let home = support::TestHome::new();
    Command::cargo_bin("skillator")
        .unwrap()
        .arg("library")
        .env("HOME", home.path())
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("interactive terminal"));
    assert!(!home.library_config().exists());
}

#[test]
fn library_locations_can_be_added_listed_filtered_and_removed() {
    let home = support::TestHome::new();
    let skills = home.path().join("elastic/agent-skills");
    std::fs::create_dir_all(skills.join("skills/esdiag")).unwrap();
    std::fs::write(
        skills.join("skills/esdiag/SKILL.md"),
        "---\nname: esdiag\ndescription: Diagnose Elasticsearch\n---\n",
    )
    .unwrap();
    std::process::Command::new("git")
        .args(["init", "--quiet"])
        .arg(&skills)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/elastic/agent-skills.git",
        ])
        .current_dir(&skills)
        .status()
        .unwrap();

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["library", "add"])
        .arg(&skills)
        .env("HOME", home.path())
        .assert()
        .success();

    let json = Command::cargo_bin("skillator")
        .unwrap()
        .args(["library", "list", "elastic", "--format=json"])
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(json.status.success());
    assert!(String::from_utf8_lossy(&json.stdout).contains("elastic/agent-skills"));
    assert!(String::from_utf8_lossy(&json.stdout).contains("skills/esdiag"));
    let yaml = Command::cargo_bin("skillator")
        .unwrap()
        .args(["library", "list", "elastic", "--format=yaml"])
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(yaml.status.success());
    let json_value: Value = serde_json::from_slice(&json.stdout).unwrap();
    let yaml_value: Value = serde_saphyr::from_slice(&yaml.stdout).unwrap();
    assert_eq!(json_value, yaml_value);

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["library", "remove"])
        .arg(&skills)
        .env("HOME", home.path())
        .assert()
        .success();
    assert!(skills.exists());
}

#[test]
fn library_remove_reports_registered_target_enablements_that_lose_resolution() {
    let fixture = Fixture::new();
    fixture.command().arg("init").assert().success();

    fixture
        .command()
        .args(["library", "remove", "./library", "--check", "--format=json"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("enablement_will_be_unresolved"))
        .stdout(predicate::str::contains("local/library"))
        .stdout(predicate::str::contains("release-checklist"));
}

#[test]
fn library_remove_reports_unavailable_registered_targets() {
    let fixture = Fixture::new();
    let missing = fixture.home.path().join("missing-target");
    std::fs::write(
        fixture.home.path().join(".skillator/targets.yaml"),
        format!("version: 1\ntargets:\n  - {:?}\n", missing),
    )
    .unwrap();

    fixture
        .command()
        .args(["library", "remove", "./library", "--check", "--format=json"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("registered_target_unavailable"));
}

#[test]
fn library_prune_removes_missing_locations_and_preserves_unresolved_expressions() {
    let fixture = Fixture::new();
    fixture.command().arg("init").assert().success();
    let broken = fixture.home.path().join("broken-library");
    #[cfg(unix)]
    std::os::unix::fs::symlink(fixture.home.path().join("missing-target"), &broken).unwrap();
    std::fs::write(
        fixture.home.library_config(),
        format!(
            "version: 1\nlocations:\n  - path: \"./library\"\n    exclusions: []\n    allow_overlap: false\n  - path: {}\n    exclusions: []\n    allow_overlap: false\n  - path: \"${{DETACHED_LIBRARY}}\"\n    exclusions: []\n    allow_overlap: false\n",
            serde_json::to_string(broken.to_str().unwrap()).unwrap()
        ),
    )
    .unwrap();
    std::fs::remove_dir_all(fixture.home.path().join(".skillator/library")).unwrap();
    let before = std::fs::read(fixture.home.library_config()).unwrap();

    fixture
        .command()
        .args(["target", "list", "--format=json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"resolution\": \"unresolved\""));

    fixture
        .command()
        .args(["library", "prune", "--check", "--format=json"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("prune_library_location"))
        .stdout(predicate::str::contains("library_location_preserved"))
        .stdout(predicate::str::contains("enablement_will_be_unresolved"));
    assert_eq!(
        std::fs::read(fixture.home.library_config()).unwrap(),
        before
    );

    fixture
        .command()
        .args(["library", "prune"])
        .assert()
        .success();
    let pruned = std::fs::read_to_string(fixture.home.library_config()).unwrap();
    assert!(!pruned.contains("./library"));
    assert!(!pruned.contains(broken.to_string_lossy().as_ref()));
    assert!(pruned.contains("${DETACHED_LIBRARY}"));
    assert!(fixture.target.join(".agents/skillator.yaml").is_file());
}
