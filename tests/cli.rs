mod support;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

#[test]
fn help_and_version_are_successful_text_on_stdout() {
    Command::cargo_bin("skillator")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("skillator sync")
                .and(predicate::str::contains("library"))
                .and(predicate::str::contains("init"))
                .and(predicate::str::contains("target"))
                .and(predicate::str::contains("user"))
                .and(predicate::str::contains("\n  worktree").not()),
        )
        .stderr(predicate::str::is_empty());
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["sync", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("target").and(predicate::str::contains("worktree")));
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["worktree", "sync"])
        .assert()
        .code(2);
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["target", "init"])
        .assert()
        .code(2);
    Command::cargo_bin("skillator")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("skillator 0.1.0"));
}

#[test]
fn worktree_sync_rejects_the_primary_worktree_without_writes() {
    let home = support::TestHome::new();
    let primary = home.git_repo("primary");

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["sync", "worktree", "--check"])
        .current_dir(&primary)
        .env("HOME", home.path())
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("primary worktree"));
    assert!(!primary.join(".agents").exists());
}

#[test]
fn worktree_sync_rejects_a_non_git_directory_without_writes() {
    let home = support::TestHome::new();

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["sync", "worktree", "--check"])
        .current_dir(home.path())
        .env("HOME", home.path())
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("not in a Git worktree"));
    assert!(!home.path().join(".agents").exists());
}

#[test]
fn conflicting_sync_options_are_parser_failures() {
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["sync", "--check", "--force"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("cannot be used with"));

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["sync", "--format=json", "--color", "always"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty());
}

#[test]
fn machine_formats_encode_the_same_compact_report() {
    let fixture = Fixture::new();
    let json = fixture
        .command()
        .args(["sync", "--check", "--format=json"])
        .output()
        .unwrap();
    assert_eq!(json.status.code(), Some(1));
    assert!(json.stderr.is_empty());
    let json_value: Value = serde_json::from_slice(&json.stdout).unwrap();

    let yaml = fixture
        .command()
        .args(["sync", "--check", "--format=yaml"])
        .output()
        .unwrap();
    assert_eq!(yaml.status.code(), Some(1));
    assert!(yaml.stderr.is_empty());
    assert!(yaml.stdout.starts_with(b"---\n"));
    assert!(yaml.stdout.ends_with(b"\n"));
    let yaml_value: Value = serde_saphyr::from_slice(&yaml.stdout).unwrap();
    assert_eq!(json_value, yaml_value);
    assert!(!yaml.stdout.contains(&0x1b));
}

#[test]
fn root_non_tty_and_missing_repository_configuration_use_stable_errors() {
    let home = support::TestHome::new();
    let target = home.git_repo("target");
    Command::cargo_bin("skillator")
        .unwrap()
        .arg(&target)
        .env("HOME", home.path())
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("interactive terminal"));

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["sync", "target", "--check"])
        .arg(&target)
        .env("HOME", home.path())
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "Repository Configuration is missing",
        ));
}

#[cfg(unix)]
#[test]
fn force_is_the_only_cli_boundary_that_replaces_guarded_content() {
    let ordinary = Fixture::new();
    let ordinary_path = ordinary.target.join(".agents/skills/release-checklist");
    std::fs::create_dir_all(ordinary_path.parent().unwrap()).unwrap();
    std::fs::write(&ordinary_path, "mine").unwrap();
    ordinary.command().arg("sync").assert().code(1);
    assert_eq!(std::fs::read_to_string(&ordinary_path).unwrap(), "mine");

    let forced = Fixture::new();
    let forced_path = forced.target.join(".agents/skills/release-checklist");
    std::fs::create_dir_all(forced_path.parent().unwrap()).unwrap();
    std::fs::write(&forced_path, "mine").unwrap();
    forced.command().args(["sync", "--force"]).assert().code(0);
    assert!(
        std::fs::symlink_metadata(forced_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

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
fn init_is_previewable_and_registers_only_after_apply() {
    let home = support::TestHome::new();
    let target = home.git_repo("target");

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["init", "--check"])
        .arg(&target)
        .env("HOME", home.path())
        .assert()
        .code(1);
    assert!(!target.join(".agents/skillator.yaml").exists());
    assert!(!home.path().join(".skillator/targets.yaml").exists());

    Command::cargo_bin("skillator")
        .unwrap()
        .arg("init")
        .current_dir(&target)
        .env("HOME", home.path())
        .assert()
        .success();
    assert!(target.join(".agents/skillator.yaml").is_file());
    let registry = std::fs::read_to_string(home.path().join(".skillator/targets.yaml")).unwrap();
    assert!(registry.contains(target.to_string_lossy().as_ref()));
}

#[test]
fn bare_sync_discovers_linked_worktrees_and_explicit_target_overrides_it() {
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

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["sync", "--check"])
        .current_dir(&linked)
        .env("HOME", home.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "cannot read primary Target configuration",
        ));

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["sync", "target", "--check"])
        .current_dir(&linked)
        .env("HOME", home.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains("run `skillator init"));
}

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
    assert!(
        std::fs::symlink_metadata(fixture.target.join(".agents/skills/release-checklist"))
            .unwrap()
            .file_type()
            .is_symlink()
    );

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

struct Fixture {
    home: support::TestHome,
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
        Self { home, target }
    }

    fn command(&self) -> Command {
        let mut command = Command::cargo_bin("skillator").unwrap();
        command
            .env("HOME", self.home.path())
            .current_dir(&self.target);
        command
    }
}
