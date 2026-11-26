//! Sync behavior scenarios.

use super::*;

#[test]
fn worktree_sync_rejects_the_primary_worktree_without_writes() {
    let home = support::TestHome::new();
    let primary = home.git_repo("primary");

    assert_worktree_rejected(&home, &primary, "primary worktree");
}

#[test]
fn worktree_sync_rejects_a_non_git_directory_without_writes() {
    let home = support::TestHome::new();

    assert_worktree_rejected(&home, home.path(), "not in a Git worktree");
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
            "cannot read primary worktree configuration",
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
fn sync_help_explains_force_in_plain_language() {
    for args in [vec!["sync", "--help"], vec!["sync", "worktree", "--help"]] {
        Command::cargo_bin("skillator")
            .unwrap()
            .args(args)
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "Allow replacing or removing existing files",
            ))
            .stdout(predicate::str::contains("Guarded").not());
    }
}

fn assert_worktree_rejected(home: &support::TestHome, directory: &std::path::Path, message: &str) {
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["sync", "worktree", "--check"])
        .current_dir(directory)
        .env("HOME", home.path())
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(message));
    assert!(!directory.join(".agents").exists());
}
