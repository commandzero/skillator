mod support;
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use support::{git, git_output};

struct Fixture {
    temp: tempfile::TempDir,
    home: PathBuf,
    library: PathBuf,
    remote: PathBuf,
    seed: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = root.join("home");
        let library = root.join("library");
        let remote = root.join("remote.git");
        let seed = root.join("seed");
        for path in [&home, &library, &seed] {
            fs::create_dir_all(path).unwrap();
        }
        git(
            &root,
            &["init", "--bare", "--quiet", remote.to_str().unwrap()],
        );
        git(&seed, &["init", "--quiet", "-b", "main"]);
        author(&seed);
        fs::write(seed.join("content"), "first").unwrap();
        fs::create_dir(seed.join("example")).unwrap();
        fs::write(
            seed.join("example/SKILL.md"),
            "---\nname: example\ndescription: Example skill\n---\n",
        )
        .unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-qm", "initial"]);
        git(
            &seed,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&seed, &["push", "-qu", "origin", "main"]);
        git(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        let f = Self {
            temp,
            home,
            library,
            remote,
            seed,
        };
        f.locations(std::slice::from_ref(&f.library));
        f
    }
    fn clone_repo(&self, name: &str) -> PathBuf {
        let path = self.library.join(name);
        git(
            &self.library,
            &[
                "clone",
                "--quiet",
                self.remote.to_str().unwrap(),
                path.to_str().unwrap(),
            ],
        );
        author(&path);
        path
    }
    fn advance(&self) {
        fs::write(self.seed.join("content"), "second").unwrap();
        git(&self.seed, &["commit", "-qam", "advance"]);
        git(&self.seed, &["push", "-q"]);
    }
    fn locations(&self, paths: &[PathBuf]) {
        let config = json!({"version":1,"locations":paths.iter().map(|p| json!({"path":p,"allow_overlap":true})).collect::<Vec<_>>()});
        fs::create_dir_all(self.home.join(".skillator")).unwrap();
        fs::write(
            self.home.join(".skillator/library.yaml"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_skillator"));
        c.current_dir(&self.home)
            .env("HOME", &self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_ALLOW_PROTOCOL", "file")
            .env("LC_ALL", "C")
            .args(["library", "update"])
            .stdin(Stdio::null());
        c
    }
    fn run(&self, args: &[&str], status: i32) -> Value {
        let output = self
            .command()
            .args(["--format", "json"])
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(status), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["exit_status"], status);
        value
    }
}
fn author(path: &Path) {
    git(path, &["config", "user.email", "test@example.invalid"]);
    git(path, &["config", "user.name", "Test"]);
}
fn outcome<'a>(report: &'a Value, root: &Path) -> &'a str {
    report["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["path"] == root.to_str().unwrap())
        .unwrap()["outcome"]
        .as_str()
        .unwrap()
}
fn code(report: &Value, code: &str) -> bool {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == code)
}
fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
fn tree(path: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut result = Vec::new();
    for entry in fs::read_dir(path).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            result.extend(tree(&p));
        } else {
            result.push((
                p.clone(),
                fs::read(&p).unwrap(),
                fs::metadata(&p).unwrap().modified().unwrap(),
            ));
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

#[test]
fn fast_forward_preview_and_repeated_update_preserve_copies_and_declarations() {
    let f = Fixture::new();
    let repo = f.clone_repo("repo");
    f.advance();
    let link = f.home.join("linked");
    symlink(repo.join("content"), &link).unwrap();
    let copy = f.home.join("copy");
    fs::copy(repo.join("content"), &copy).unwrap();
    let config = fs::read(f.home.join(".skillator/library.yaml")).unwrap();
    let before = tree(&repo);
    let preview = f.run(&["--check"], 1);
    assert_eq!(outcome(&preview, &repo), "would_apply");
    assert!(code(&preview, "remote_state_not_checked"));
    assert_eq!(tree(&repo), before);
    let text = f.command().arg("--check").output().unwrap();
    assert!(
        String::from_utf8(text.stdout)
            .unwrap()
            .contains("Would attempt pull; remote state not checked.")
    );
    let report = f.run(&[], 0);
    assert_eq!(outcome(&report, &repo), "applied");
    assert_eq!(fs::read_to_string(link).unwrap(), "second");
    assert_eq!(fs::read_to_string(copy).unwrap(), "first");
    assert_eq!(
        fs::read(f.home.join(".skillator/library.yaml")).unwrap(),
        config
    );
    let json = f.run(&[], 0);
    assert_eq!(outcome(&json, &repo), "unchanged");
    let yaml = f.command().args(["--format", "yaml"]).output().unwrap();
    let yaml: Value = serde_saphyr::from_slice(&yaml.stdout).unwrap();
    assert_eq!(json, yaml);
}

#[test]
fn blockers_and_divergence_do_not_prevent_independent_pulls() {
    let f = Fixture::new();
    let dirty = f.clone_repo("a-dirty");
    let detached = f.clone_repo("b-detached");
    let no_upstream = f.clone_repo("c-no-upstream");
    let divergent = f.clone_repo("d-diverged");
    let operation = f.clone_repo("e-operation");
    let clean = f.clone_repo("z-clean");
    fs::write(dirty.join("untracked"), "keep").unwrap();
    git(&detached, &["checkout", "--detach", "-q"]);
    git(&no_upstream, &["branch", "--unset-upstream"]);
    fs::write(divergent.join("local"), "local").unwrap();
    git(&divergent, &["add", "local"]);
    git(&divergent, &["commit", "-qm", "local"]);
    let local_head = git_output(&divergent, &["rev-parse", "HEAD"]);
    fs::write(
        operation.join(".git/MERGE_HEAD"),
        git_output(&operation, &["rev-parse", "HEAD"]),
    )
    .unwrap();
    f.advance();
    let report = f.run(&[], 1);
    for r in [&dirty, &detached, &no_upstream, &operation] {
        assert_eq!(outcome(&report, r), "blocked");
    }
    assert_eq!(outcome(&report, &divergent), "failed");
    assert_eq!(outcome(&report, &clean), "applied");
    for c in [
        "dirty_checkout",
        "detached_head",
        "missing_upstream",
        "operation_in_progress",
        "pull_failed",
    ] {
        assert!(code(&report, c), "{report}");
    }
    assert_eq!(git_output(&divergent, &["rev-parse", "HEAD"]), local_head);
    assert_eq!(fs::read_to_string(dirty.join("untracked")).unwrap(), "keep");
}

#[test]
fn non_origin_upstream_and_conflicting_settings_are_respected() {
    let f = Fixture::new();
    let repo = f.clone_repo("repo");
    git(&repo, &["remote", "rename", "origin", "source"]);
    for (key, val) in [
        ("pull.rebase", "true"),
        ("pull.ff", "false"),
        ("merge.autoStash", "true"),
        ("rebase.autoStash", "true"),
        ("submodule.recurse", "true"),
    ] {
        git(&repo, &["config", key, val]);
    }
    fs::write(repo.join(".git/info/exclude"), "ignored\n").unwrap();
    fs::write(repo.join("ignored"), "keep").unwrap();
    f.advance();
    assert_eq!(outcome(&f.run(&[], 0), &repo), "applied");
    fs::write(repo.join("local"), "local").unwrap();
    git(&repo, &["add", "local"]);
    git(&repo, &["commit", "-qm", "ahead"]);
    assert_eq!(outcome(&f.run(&[], 0), &repo), "unchanged");
}

#[test]
fn canonical_dedup_and_worktree_identity_do_not_depend_on_source_keys() {
    let f = Fixture::new();
    let first = f.clone_repo("first");
    let second = f.clone_repo("second");
    let worktree = f.library.join("worktree");
    git(
        &first,
        &[
            "worktree",
            "add",
            "-qb",
            "other",
            worktree.to_str().unwrap(),
            "origin/main",
        ],
    );
    f.locations(&[f.library.clone(), first.clone()]);
    f.advance();
    let report = f.run(&[], 0);
    assert_eq!(report["changes"].as_array().unwrap().len(), 3);
    for r in [&first, &second, &worktree] {
        assert_eq!(outcome(&report, r), "applied");
    }
}

#[test]
fn exclusions_links_and_enclosing_roots_stay_outside_the_plan() {
    let f = Fixture::new();
    let excluded = f.clone_repo("excluded");
    let external = f.clone_repo("external");
    let external_new = f.temp.path().join("external");
    fs::rename(&external, &external_new).unwrap();
    symlink(&external_new, f.library.join("linked")).unwrap();
    let config = json!({"version":1,"locations":[{"path":f.library,"exclusions":["excluded"]},{"path":f.seed.join("example")}]});
    fs::write(
        f.home.join(".skillator/library.yaml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    f.advance();
    let report = f.run(&[], 0);
    assert!(report["changes"].as_array().unwrap().is_empty());
    assert_eq!(
        fs::read_to_string(excluded.join("content")).unwrap(),
        "first"
    );
    assert_eq!(
        fs::read_to_string(external_new.join("content")).unwrap(),
        "first"
    );
}

#[test]
fn submodules_are_skipped_even_as_direct_locations_on_tracking_branches() {
    let f = Fixture::new();
    let parent = f.clone_repo("parent");
    git(
        &parent,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            f.remote.to_str().unwrap(),
            "module",
        ],
    );
    git(&parent, &["commit", "-qm", "submodule"]);
    let module = parent.join("module");
    git(&module, &["checkout", "-q", "main"]);
    f.locations(&[f.library.clone(), module.clone()]);
    let report = f.run(&[], 0);
    assert_eq!(report["changes"].as_array().unwrap().len(), 1);
    assert!(code(&report, "submodule_skipped"));
    f.locations(&[module]);
    let report = f.run(&[], 0);
    assert!(report["changes"].as_array().unwrap().is_empty());
}

#[test]
fn missing_locations_are_partial_but_missing_configuration_is_empty_success() {
    let f = Fixture::new();
    let repo = f.clone_repo("repo");
    f.advance();
    f.locations(&[f.library.clone(), f.temp.path().join("missing")]);
    let report = f.run(&[], 1);
    assert!(code(&report, "location_unavailable"));
    assert_eq!(outcome(&report, &repo), "applied");
    fs::remove_file(f.home.join(".skillator/library.yaml")).unwrap();
    assert!(f.run(&[], 0)["changes"].as_array().unwrap().is_empty());
    assert!(!f.home.join(".skillator/library.yaml").exists());
}

#[test]
fn invalid_options_and_configuration_fail_before_updates() {
    let f = Fixture::new();
    let repo = f.clone_repo("repo");
    f.advance();
    for args in [
        vec!["--timeout", "0"],
        vec!["--timeout", "-1"],
        vec!["--timeout", "1.5"],
        vec!["--timeout", "x"],
        vec!["--timeout", "4294967296"],
        vec!["--force"],
        vec!["selector"],
        vec!["--format", "bad"],
    ] {
        let output = f.command().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    let help = f.command().arg("--help").output().unwrap();
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("[default: 30]")
    );
    fs::write(f.home.join(".skillator/library.yaml"), "version: nope\n").unwrap();
    let output = f.command().output().unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read_to_string(repo.join("content")).unwrap(), "first");
}

fn stalled_hook(repo: &Path, marker: &Path) {
    executable(
        &repo.join(".git/hooks/post-merge"),
        &format!(
            "#!/bin/sh\n(sleep 3; echo survived > '{}') &\necho started > '{}.started'\nwait\n",
            marker.display(),
            marker.display()
        ),
    );
}
fn wait_for(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "missing {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn timeout_kills_descendants_and_continues_after_a_partially_applied_pull() {
    let f = Fixture::new();
    let slow = f.clone_repo("a-slow");
    let next = f.clone_repo("z-next");
    f.advance();
    let marker = f.temp.path().join("survived");
    stalled_hook(&slow, &marker);
    let start = Instant::now();
    let report = f.run(&["--timeout", "1"], 1);
    assert!(start.elapsed() < Duration::from_secs(10));
    assert!(code(&report, "pull_timeout"));
    assert_eq!(outcome(&report, &slow), "failed");
    assert_eq!(outcome(&report, &next), "applied");
    std::thread::sleep(Duration::from_secs(3));
    assert!(!marker.exists());
    assert_eq!(fs::read_to_string(slow.join("content")).unwrap(), "second");
}

#[test]
fn interrupt_stops_the_batch_and_retains_prior_success() {
    let f = Fixture::new();
    let first = f.clone_repo("a-first");
    let slow = f.clone_repo("b-slow");
    let last = f.clone_repo("z-last");
    f.advance();
    let marker = f.temp.path().join("survived");
    stalled_hook(&slow, &marker);
    let child = f
        .command()
        .args(["--format", "json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for(&f.temp.path().join("survived.started"));
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(130), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(outcome(&report, &first), "applied");
    assert_eq!(outcome(&report, &slow), "failed");
    assert_eq!(outcome(&report, &last), "blocked");
    assert_eq!(fs::read_to_string(last.join("content")).unwrap(), "first");
    std::thread::sleep(Duration::from_secs(3));
    assert!(!marker.exists());
}

#[test]
fn failed_transport_is_captured_without_ansi_or_prompts() {
    let f = Fixture::new();
    let repo = f.clone_repo("repo");
    let helper = f.temp.path().join("ssh-helper");
    executable(
        &helper,
        "#!/bin/sh\nif [ \"$GIT_TERMINAL_PROMPT\" != 0 ]; then exit 99; fi\nprintf '\x1b[31mauthentication failed\x1b[0m\\n' >&2\nexit 1\n",
    );
    git(
        &repo,
        &[
            "remote",
            "set-url",
            "origin",
            "ssh://example.invalid/skills",
        ],
    );
    let output = f
        .command()
        .env("GIT_ALLOW_PROTOCOL", "ssh")
        .env("GIT_SSH_COMMAND", &helper)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains(&27));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(code(&report, "pull_failed"));
    assert!(
        report["diagnostics"]
            .to_string()
            .contains("authentication failed")
    );
}

#[test]
fn unreadable_subtrees_report_incomplete_discovery_without_stopping_updates() {
    let f = Fixture::new();
    let repo = f.clone_repo("repo");
    let hidden = f.library.join("unreadable");
    fs::create_dir(&hidden).unwrap();
    fs::set_permissions(&hidden, fs::Permissions::from_mode(0o0)).unwrap();
    f.advance();
    let output = f.command().args(["--format", "json"]).output().unwrap();
    fs::set_permissions(&hidden, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(code(&report, "discovery_incomplete"));
    assert_eq!(outcome(&report, &repo), "applied");
}

#[test]
fn checkout_replacement_after_planning_is_blocked() {
    let f = Fixture::new();
    let first = f.clone_repo("a-first");
    let replaced = f.clone_repo("z-replaced");
    let moved = f.temp.path().join("old-checkout");
    executable(
        &first.join(".git/hooks/post-merge"),
        &format!(
            "#!/bin/sh\nmv '{}' '{}'\nmkdir '{}'\ngit init -q '{}'\n",
            replaced.display(),
            moved.display(),
            replaced.display(),
            replaced.display()
        ),
    );
    f.advance();
    let report = f.run(&[], 1);
    assert_eq!(outcome(&report, &first), "applied");
    assert_eq!(outcome(&report, &replaced), "blocked");
    assert!(code(&report, "checkout_changed"));
    assert_eq!(fs::read_to_string(moved.join("content")).unwrap(), "first");
}

#[test]
fn ordinary_nested_repositories_without_skills_are_updated() {
    let f = Fixture::new();
    let parent = f.clone_repo("parent");
    let nested = parent.join("nested");
    git(
        &parent,
        &["clone", "-q", f.remote.to_str().unwrap(), "nested"],
    );
    author(&nested);
    git(&nested, &["rm", "-qr", "example"]);
    git(&nested, &["commit", "-qm", "remove skill"]);
    fs::write(parent.join(".git/info/exclude"), "nested/\n").unwrap();
    f.advance();
    let report = f.run(&[], 1);
    assert_eq!(outcome(&report, &parent), "applied");
    // Even without Skills, the nested Source is selected and its divergence reported.
    assert_eq!(outcome(&report, &nested), "failed");
}

#[test]
fn preview_does_not_contact_an_unavailable_remote_and_bare_locations_are_not_updated() {
    let f = Fixture::new();
    let repo = f.clone_repo("repo");
    git(
        &repo,
        &[
            "remote",
            "set-url",
            "origin",
            "/definitely/missing/remote.git",
        ],
    );
    f.locations(&[repo.clone(), f.remote.clone()]);
    assert_eq!(outcome(&f.run(&["--check"], 1), &repo), "would_apply");
    let report = f.run(&[], 1);
    assert_eq!(report["changes"].as_array().unwrap().len(), 1);
    assert_eq!(outcome(&report, &repo), "failed");
}

#[test]
fn staged_unstaged_and_locked_checkouts_preserve_user_state() {
    let f = Fixture::new();
    let staged = f.clone_repo("staged");
    let unstaged = f.clone_repo("unstaged");
    let locked = f.clone_repo("locked");
    fs::write(staged.join("content"), "staged edit").unwrap();
    git(&staged, &["add", "content"]);
    fs::write(unstaged.join("content"), "unstaged edit").unwrap();
    fs::write(locked.join(".git/index.lock"), "keep lock").unwrap();
    f.advance();
    let report = f.run(&[], 1);
    assert_eq!(outcome(&report, &staged), "blocked");
    assert_eq!(outcome(&report, &unstaged), "blocked");
    assert_eq!(outcome(&report, &locked), "failed");
    assert_eq!(
        fs::read_to_string(locked.join(".git/index.lock")).unwrap(),
        "keep lock"
    );
    assert_eq!(
        fs::read_to_string(staged.join("content")).unwrap(),
        "staged edit"
    );
    assert_eq!(
        fs::read_to_string(unstaged.join("content")).unwrap(),
        "unstaged edit"
    );
}

#[test]
fn branch_and_tag_with_the_same_name_use_the_branch_upstream() {
    let f = Fixture::new();
    let repo = f.clone_repo("repo");
    git(&repo, &["tag", "main"]);
    f.advance();
    let preview = f.run(&["--check"], 1);
    assert_eq!(outcome(&preview, &repo), "would_apply");
    assert!(!code(&preview, "missing_upstream"));
    assert_eq!(outcome(&f.run(&[], 0), &repo), "applied");
}

#[test]
fn ssh_batch_options_precede_conflicting_environment_and_repository_options() {
    for environment_override in [true, false] {
        let f = Fixture::new();
        let repo = f.clone_repo("repo");
        // A transparent helper records OpenSSH's resolved options without any
        // network request. Its quoted path also tests command preservation.
        let helper = f.temp.path().join("ssh helper");
        let effective = f.temp.path().join("ssh-options");
        executable(
            &helper,
            &format!(
                "#!/bin/sh\n/usr/bin/ssh -F /dev/null -G \"$@\" > '{}'\nexit 1\n",
                effective.display()
            ),
        );
        let configured = format!(
            "'{}' -oBatchMode=no -o NumberOfPasswordPrompts=7 -o User=skillator-test",
            helper.display()
        );
        git(
            &repo,
            &[
                "remote",
                "set-url",
                "origin",
                "ssh://example.invalid/skills",
            ],
        );
        let mut command = f.command();
        command
            .env("GIT_ALLOW_PROTOCOL", "ssh")
            .env("GIT_SSH_VARIANT", "ssh");
        if environment_override {
            // The environment command must retain precedence over repository config.
            git(
                &repo,
                &["config", "core.sshCommand", "/definitely/missing/ssh"],
            );
            command.env("GIT_SSH_COMMAND", &configured);
        } else {
            command.env_remove("GIT_SSH_COMMAND");
            git(&repo, &["config", "core.sshCommand", &configured]);
        }
        let output = command.args(["--format", "json"]).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let options = fs::read_to_string(effective).unwrap();
        assert!(
            options.lines().any(|line| line == "batchmode yes"),
            "{options}"
        );
        assert!(
            options
                .lines()
                .any(|line| line == "numberofpasswordprompts 0"),
            "{options}"
        );
        assert!(
            options.lines().any(|line| line == "user skillator-test"),
            "{options}"
        );
    }
}
