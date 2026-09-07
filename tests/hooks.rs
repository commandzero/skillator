mod support;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use skillator::git::GitRepository;
use skillator::hooks::{HookState, HookWorkflow};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const ZERO_REF: &str = "0000000000000000000000000000000000000000";

#[test]
fn hook_commands_are_non_interactive_and_report_state() {
    let home = support::TestHome::new();
    let repository = home.git_repo("target");

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "install", "--check", "--format=json"])
        .current_dir(&repository)
        .env("HOME", home.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("\"state\": \"absent\""));

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "install"])
        .current_dir(&repository)
        .env("HOME", home.path())
        .assert()
        .success();

    let status = Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "status", "--format=json"])
        .current_dir(&repository)
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(status.status.success());
    let value: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(value["state"], "installed");
    assert!(
        value["hook_path"]
            .as_str()
            .unwrap()
            .ends_with("post-checkout")
    );

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "uninstall", "--check"])
        .current_dir(&repository)
        .env("HOME", home.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("would_apply"));

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "uninstall"])
        .current_dir(&repository)
        .env("HOME", home.path())
        .assert()
        .success();
    assert_eq!(
        HookWorkflow::status(&repository).unwrap().state,
        HookState::Absent
    );
}

#[test]
fn hook_option_conflicts_are_parser_failures() {
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "install", "--check", "--force"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn hook_install_outside_git_is_invalid_without_writes() {
    let home = support::TestHome::new();
    let directory = home.path().join("plain");
    std::fs::create_dir_all(&directory).unwrap();

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "install"])
        .current_dir(&directory)
        .env("HOME", home.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains("not inside a Git worktree"));
    assert!(directory.read_dir().unwrap().next().is_none());
}

#[test]
fn hook_status_reports_an_unrelated_conflict() {
    let home = support::TestHome::new();
    let repository = home.git_repo("target");
    let hook = GitRepository::discover(&repository)
        .unwrap()
        .hooks_path()
        .unwrap()
        .join("post-checkout");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    let original = b"#!/bin/sh\nexit 0\n";
    std::fs::write(&hook, original).unwrap();

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "status", "--format=json"])
        .current_dir(&repository)
        .env("HOME", home.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("\"state\": \"conflict\""));
    assert_eq!(std::fs::read(&hook).unwrap(), original);
}

#[test]
fn forced_install_blockers_use_structured_reports() {
    let home = support::TestHome::new();
    let repository = home.git_repo("target");
    let hook = GitRepository::discover(&repository)
        .unwrap()
        .hooks_path()
        .unwrap()
        .join("post-checkout");
    let predecessor = hook
        .parent()
        .unwrap()
        .join("post-checkout.skillator-original");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(&predecessor, b"reserved\n").unwrap();

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "install", "--force", "--format=json"])
        .current_dir(&repository)
        .env("HOME", home.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("\"state\": \"blocked\""))
        .stdout(predicate::str::contains("hook_install_blocked"));
}

#[test]
fn non_regular_hook_path_is_blocked_without_replacement() {
    let home = support::TestHome::new();
    let repository = home.git_repo("target");
    let hook = GitRepository::discover(&repository)
        .unwrap()
        .hooks_path()
        .unwrap()
        .join("post-checkout");
    std::fs::create_dir_all(&hook).unwrap();

    let report = HookWorkflow::install(
        &repository,
        skillator::app::SyncMode::Apply { force: false },
    )
    .unwrap();
    assert_eq!(report.state, HookState::Blocked);
    assert!(hook.is_dir());
}

#[test]
fn concurrent_hook_installations_leave_one_valid_managed_hook() {
    let home = support::TestHome::new();
    let repository = home.git_repo("target");
    let binary = std::env::var_os("CARGO_BIN_EXE_skillator").expect("test binary path");
    let children = (0..8)
        .map(|_| {
            ProcessCommand::new(&binary)
                .args(["hook", "install", "--format=json"])
                .current_dir(&repository)
                .env("HOME", home.path())
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let outputs = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect::<Vec<_>>();
    assert!(outputs.iter().any(|output| output.status.success()));
    assert_eq!(
        HookWorkflow::status(&repository).unwrap().state,
        HookState::Installed
    );
}

#[test]
fn concurrent_hook_uninstallations_leave_no_managed_files() {
    let home = support::TestHome::new();
    let repository = home.git_repo("target");
    HookWorkflow::install(
        &repository,
        skillator::app::SyncMode::Apply { force: false },
    )
    .unwrap();
    let binary = std::env::var_os("CARGO_BIN_EXE_skillator").expect("test binary path");
    let children = (0..4)
        .map(|_| {
            ProcessCommand::new(&binary)
                .args(["hook", "uninstall", "--format=json"])
                .current_dir(&repository)
                .env("HOME", home.path())
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let outputs = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect::<Vec<_>>();
    assert!(outputs.iter().any(|output| output.status.success()));
    assert_eq!(
        HookWorkflow::status(&repository).unwrap().state,
        HookState::Absent
    );
}

#[cfg(unix)]
#[test]
fn post_checkout_hook_skips_clone_and_ordinary_checkout() {
    let home = support::TestHome::new();
    let source = home.git_repo("source");
    support::git(&source, &["config", "user.name", "Skillator Tests"]);
    support::git(&source, &["config", "user.email", "tests@example.invalid"]);
    std::fs::write(source.join("seed"), "seed").unwrap();
    support::git(&source, &["add", "seed"]);
    support::git(&source, &["commit", "--quiet", "-m", "seed"]);
    HookWorkflow::install(&source, skillator::app::SyncMode::Apply { force: false }).unwrap();

    let source_hook = GitRepository::discover(&source)
        .unwrap()
        .hooks_path()
        .unwrap()
        .join("post-checkout");
    let shared_hooks = home.path().join("shared-hooks");
    std::fs::create_dir_all(&shared_hooks).unwrap();
    let shared_hook = shared_hooks.join("post-checkout");
    std::fs::copy(&source_hook, &shared_hook).unwrap();
    set_executable(&shared_hook);

    let fake_bin = home.path().join("fake-bin");
    std::fs::create_dir_all(&fake_bin).unwrap();
    let sync_log = home.path().join("sync.log");
    let fake_skillator = fake_bin.join("skillator");
    std::fs::write(
        &fake_skillator,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n",
            sync_log.display()
        ),
    )
    .unwrap();
    set_executable(&fake_skillator);
    let fake_path = prepend_path(&fake_bin);

    let cloned = home.path().join("clone");
    let clone = ProcessCommand::new("git")
        .arg("-c")
        .arg(format!("core.hooksPath={}", shared_hooks.display()))
        .args(["clone", "--quiet"])
        .arg(&source)
        .arg(&cloned)
        .env("HOME", home.path())
        .env("PATH", &fake_path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(clone.status.success(), "git clone failed: {clone:?}");
    assert!(
        !sync_log.exists(),
        "clone must not sync as a linked worktree"
    );

    std::fs::write(cloned.join("seed"), "changed").unwrap();
    support::git(&cloned, &["checkout", "--quiet", "--", "seed"]);
    assert!(
        !sync_log.exists(),
        "ordinary file checkout must not sync as a linked worktree"
    );
}

#[cfg(unix)]
#[test]
fn unresolved_sync_reports_a_diagnostic_without_blocking_worktree_creation() {
    let home = support::TestHome::new();
    let primary = home.git_repo("primary");
    support::git(&primary, &["config", "user.name", "Skillator Tests"]);
    support::git(&primary, &["config", "user.email", "tests@example.invalid"]);
    std::fs::write(primary.join("seed"), "seed").unwrap();
    support::git(&primary, &["add", "seed"]);
    support::git(&primary, &["commit", "--quiet", "-m", "seed"]);
    std::fs::create_dir_all(primary.join(".agents")).unwrap();
    std::fs::write(
        primary.join(".agents/skillator.yaml"),
        "version: 1\nskill_directories:\n  - key: agents\n    path: .agents/skills\nenablements:\n  - directory: agents\n    skill:\n      source: missing/source\n      path: ghost\n    materialization: linked\n",
    )
    .unwrap();

    let binary = std::env::var_os("CARGO_BIN_EXE_skillator").expect("test binary path");
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(&binary, bin.join("skillator")).unwrap();
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "install"])
        .current_dir(&primary)
        .env("HOME", home.path())
        .env("PATH", prepend_path(&bin))
        .assert()
        .success();
    let linked = home.path().join("linked");
    let output = ProcessCommand::new("git")
        .args([
            "-C",
            primary.to_str().unwrap(),
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ])
        .env("HOME", home.path())
        .env("PATH", prepend_path(&bin))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git worktree add failed: {output:?}"
    );
    assert!(linked.is_dir());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("automatic worktree sync did not converge")
    );
}

#[cfg(unix)]
#[test]
fn post_checkout_hook_syncs_a_new_linked_worktree() {
    let home = support::TestHome::new();
    let primary = home.git_repo("primary");
    support::git(&primary, &["config", "user.name", "Skillator Tests"]);
    support::git(&primary, &["config", "user.email", "tests@example.invalid"]);
    std::fs::write(primary.join("seed"), "seed").unwrap();
    support::git(&primary, &["add", "seed"]);
    support::git(&primary, &["commit", "--quiet", "-m", "seed"]);
    std::fs::create_dir_all(primary.join(".agents")).unwrap();
    std::fs::write(
        primary.join(".agents/skillator.yaml"),
        "version: 1\nskill_directories: []\nenablements: []\n",
    )
    .unwrap();

    let binary = std::env::var_os("CARGO_BIN_EXE_skillator")
        .map(PathBuf::from)
        .expect("Cargo exposes the Skillator test binary");
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(&binary, bin.join("skillator")).unwrap();
    let path = prepend_path(&bin);

    Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "install"])
        .current_dir(&primary)
        .env("HOME", home.path())
        .env("PATH", &path)
        .assert()
        .success();

    let linked = home.path().join("linked");
    let output = ProcessCommand::new("git")
        .args([
            "-C",
            primary.to_str().unwrap(),
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ])
        .env("HOME", home.path())
        .env("PATH", &path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git worktree add failed: {output:?}"
    );
    assert_eq!(
        std::fs::read_to_string(linked.join(".agents/skillator.yaml")).unwrap(),
        "version: 1\nskill_directories: []\nenablements: []\n"
    );

    let hook = GitRepository::discover(&linked)
        .unwrap()
        .hooks_path()
        .unwrap()
        .join("post-checkout");
    let missing_binary = ProcessCommand::new(&hook)
        .args([ZERO_REF, "new", "1"])
        .current_dir(&linked)
        .env_remove("SKILLATOR_NO_AUTO_SYNC")
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(missing_binary.status.success());
    assert!(String::from_utf8_lossy(&missing_binary.stderr).contains("executable not found"));

    let fake_bin = home.path().join("fake-bin");
    std::fs::create_dir_all(&fake_bin).unwrap();
    let sync_log = home.path().join("sync.log");
    let fake_skillator = fake_bin.join("skillator");
    std::fs::write(
        &fake_skillator,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nexit 7\n",
            sync_log.display()
        ),
    )
    .unwrap();
    set_executable(&fake_skillator);
    let fake_path = prepend_path(&fake_bin);

    ProcessCommand::new(&hook)
        .args(["1111111111111111111111111111111111111111", "new", "1"])
        .current_dir(&linked)
        .env("PATH", &fake_path)
        .env("SKILLATOR_NO_AUTO_SYNC", "")
        .output()
        .unwrap();
    assert!(!sync_log.exists(), "branch checkout must not sync");

    ProcessCommand::new(&hook)
        .args([ZERO_REF, "new", "1"])
        .current_dir(&primary)
        .env("PATH", &fake_path)
        .output()
        .unwrap();
    assert!(!sync_log.exists(), "the primary worktree must not sync");

    let guarded = ProcessCommand::new(&hook)
        .args([ZERO_REF, "new", "1"])
        .current_dir(&linked)
        .env("PATH", &fake_path)
        .output()
        .unwrap();
    assert!(guarded.status.success());
    assert!(
        String::from_utf8_lossy(&guarded.stderr)
            .contains("automatic worktree sync did not converge")
    );

    let no_checkout = home.path().join("no-checkout");
    let no_checkout_output = ProcessCommand::new("git")
        .args([
            "-C",
            primary.to_str().unwrap(),
            "worktree",
            "add",
            "--quiet",
            "--no-checkout",
            "-b",
            "no-checkout",
            no_checkout.to_str().unwrap(),
        ])
        .env("HOME", home.path())
        .env("PATH", &path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        no_checkout_output.status.success(),
        "git worktree add --no-checkout failed: {no_checkout_output:?}"
    );
    assert!(!no_checkout.join(".agents/skillator.yaml").exists());
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["sync", "worktree", no_checkout.to_str().unwrap()])
        .current_dir(&primary)
        .env("HOME", home.path())
        .env("PATH", &path)
        .assert()
        .success();
    assert_eq!(
        std::fs::read_to_string(no_checkout.join(".agents/skillator.yaml")).unwrap(),
        "version: 1\nskill_directories: []\nenablements: []\n"
    );

    std::fs::remove_file(primary.join(".agents/skillator.yaml")).unwrap();
    let unavailable = home.path().join("unavailable");
    let unavailable_output = ProcessCommand::new("git")
        .args([
            "-C",
            primary.to_str().unwrap(),
            "worktree",
            "add",
            "--quiet",
            "-b",
            "unavailable",
            unavailable.to_str().unwrap(),
        ])
        .env("HOME", home.path())
        .env("PATH", &path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        unavailable_output.status.success(),
        "worktree creation must remain successful: {unavailable_output:?}"
    );
    assert!(unavailable.is_dir());
    assert!(!unavailable.join(".agents/skillator.yaml").exists());
    assert!(
        String::from_utf8_lossy(&unavailable_output.stderr)
            .contains("automatic worktree sync did not converge")
    );

    let opted_out = home.path().join("opted-out");
    let opted_out_output = ProcessCommand::new("git")
        .args([
            "-C",
            primary.to_str().unwrap(),
            "worktree",
            "add",
            "--quiet",
            "-b",
            "opted-out",
            opted_out.to_str().unwrap(),
        ])
        .env("HOME", home.path())
        .env("PATH", &path)
        .env("SKILLATOR_NO_AUTO_SYNC", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(opted_out_output.status.success());
    assert!(!opted_out.join(".agents/skillator.yaml").exists());
}

fn prepend_path(directory: &Path) -> String {
    let existing = std::env::var_os("PATH").unwrap_or_default();
    let mut value = directory.to_path_buf().into_os_string();
    value.push(":");
    value.push(existing);
    value.to_string_lossy().into_owned()
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn hook_status_uses_the_resolved_git_hook_path() {
    let home = support::TestHome::new();
    let repository = home.git_repo("target");
    let discovered = GitRepository::discover(&repository).unwrap();
    assert!(discovered.hooks_path().unwrap().ends_with("hooks"));
    assert!(!discovered.is_linked_worktree());
    assert_eq!(
        HookWorkflow::status(&repository).unwrap().repository,
        repository.canonicalize().unwrap().to_string_lossy()
    );
}

#[test]
fn hook_discovery_honors_a_configured_hooks_path() {
    let home = support::TestHome::new();
    let repository = home.git_repo("target");
    support::git(&repository, &["config", "core.hooksPath", ".githooks"]);
    let discovered = GitRepository::discover(&repository).unwrap();
    assert_eq!(
        discovered.hooks_path().unwrap(),
        repository.canonicalize().unwrap().join(".githooks")
    );

    let installed = HookWorkflow::install(
        &repository,
        skillator::app::SyncMode::Apply { force: false },
    )
    .unwrap();
    assert!(installed.hook_path.ends_with(".githooks/post-checkout"));
}

#[test]
fn hook_reports_have_equivalent_json_and_yaml_values() {
    let home = support::TestHome::new();
    let repository = home.git_repo("target");
    let json = Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "status", "--format=json"])
        .current_dir(&repository)
        .env("HOME", home.path())
        .output()
        .unwrap();
    let yaml = Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "status", "--format=yaml"])
        .current_dir(&repository)
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(json.status.success());
    assert!(yaml.status.success());
    let json_value: Value = serde_json::from_slice(&json.stdout).unwrap();
    let yaml_value: Value = serde_saphyr::from_slice(&yaml.stdout).unwrap();
    assert_eq!(json_value, yaml_value);
}

#[cfg(unix)]
#[test]
fn chained_predecessor_failure_is_forwarded() {
    let home = support::TestHome::new();
    let repository = home.git_repo("target");
    let hook = GitRepository::discover(&repository)
        .unwrap()
        .hooks_path()
        .unwrap()
        .join("post-checkout");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, b"#!/bin/sh\nexit 23\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).unwrap();

    HookWorkflow::install(&repository, skillator::app::SyncMode::Apply { force: true }).unwrap();
    let result = ProcessCommand::new(&hook)
        .args([ZERO_REF, "new", "1"])
        .current_dir(&repository)
        .env("SKILLATOR_NO_AUTO_SYNC", "1")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(23));
}
