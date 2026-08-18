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
        .stdout(predicate::str::contains("skillator sync"))
        .stderr(predicate::str::is_empty());
    Command::cargo_bin("skillator")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("skillator 0.1.0"));
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
        .args(["sync", "--check"])
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
    forced.command().args(["sync", "--force"]).assert().code(1);
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
