use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

const REPLICA: &str = ".skillator/library/replica";
const SKILL: &str = "local/library/_skills/demo/SKILL.md";
const MARKER: &str = ".skillator-rsync-owned";

fn executable(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("{name} must be installed for library rsync tests"))
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

struct Fixture {
    root: tempfile::TempDir,
    leader: PathBuf,
    follower: PathBuf,
    spare: PathBuf,
    temporary: PathBuf,
    path: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let leader = root.path().join("leader's home");
        let follower = root.path().join("follower home");
        let spare = root.path().join("spare home");
        let temporary = root.path().join("leader's temporary files");
        let bin = root.path().join("bin");
        let receiver_bin = root.path().join("receiver-bin");
        let leader_bin = root.path().join("leader-bin");
        for directory in [
            &leader,
            &follower,
            &spare,
            &temporary,
            &bin,
            &receiver_bin,
            &leader_bin,
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        for name in ["rsync", "mkdir", "cmp", "rm"] {
            let program = executable(name);
            symlink(&program, receiver_bin.join(name)).unwrap();
            symlink(&program, leader_bin.join(name)).unwrap();
        }
        symlink(executable("git"), leader_bin.join("git")).unwrap();
        symlink(
            assert_cmd::cargo::cargo_bin("skillator"),
            leader_bin.join("skillator"),
        )
        .unwrap();
        let adapter = bin.join("ssh");
        // This adapter launches the real remote commands and rsync server locally.
        // It substitutes process routing only; real SSH is a separate smoke check.
        fs::write(&adapter, format!(
            "#!/bin/sh\nset -eu\nwhile [ $# -gt 0 ]; do\n case \"$1\" in\n -o*) shift;;\n --) shift; break;;\n *) break;;\n esac\ndone\nhost=$1\nshift\ncase \"$host\" in\n receiver-a.internal) export HOME={} PATH={};;\n receiver-b.internal) export HOME={} PATH={};;\n leader.internal) export HOME={} PATH={};;\n *) printf 'SSH destination unavailable: %s\\n' \"$host\" >&2; exit 255;;\nesac\nexport TMPDIR={}\nexec /bin/sh -c \"$*\"\n",
            quote(&follower), quote(&receiver_bin), quote(&spare), quote(&receiver_bin), quote(&leader), quote(&leader_bin), quote(&temporary),
        )).unwrap();
        fs::set_permissions(&adapter, fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        fs::create_dir_all(leader.join(".skillator/library/demo")).unwrap();
        fs::write(leader.join(".skillator/config.yaml"), "version: 1\nhosts:\n  dev: {destination: receiver-a.internal}\n  spare: {destination: receiver-b.internal}\n").unwrap();
        fs::write(
            leader.join(".skillator/library.yaml"),
            "version: 1\nlocations: [{path: '~/.skillator/library'}]\n",
        )
        .unwrap();
        fs::write(
            leader.join(".skillator/library/demo/SKILL.md"),
            "---\nname: demo\ndescription: A skill\n---\nleader-v1\n",
        )
        .unwrap();
        Self {
            root,
            leader,
            follower,
            spare,
            temporary,
            path,
        }
    }

    fn command(&self, home: &Path) -> Command {
        let mut command = Command::cargo_bin("skillator").unwrap();
        command
            .env("HOME", home)
            .env("PATH", &self.path)
            .env("TMPDIR", &self.temporary);
        command
    }

    fn report(&self, home: &Path, args: &[&str], code: i32) -> Value {
        let assertion = self
            .command(home)
            .args(["library", "rsync"])
            .args(args)
            .args(["--format", "json"])
            .assert()
            .code(code);
        let output = assertion.get_output();
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["exit_status"], code);
        assert_eq!(report["mode"], "library_rsync");
        let text = String::from_utf8_lossy(&output.stdout);
        for endpoint in [
            "receiver-a.internal",
            "receiver-b.internal",
            "leader.internal",
            "unavailable.internal",
        ] {
            assert!(!text.contains(endpoint));
        }
        report
    }

    fn configure_follower(&self) {
        fs::create_dir_all(self.follower.join(".skillator")).unwrap();
        fs::write(
            self.follower.join(".skillator/config.yaml"),
            "version: 1\nleader: {destination: leader.internal}\n",
        )
        .unwrap();
    }

    fn source(&self) -> PathBuf {
        self.leader.join(".skillator/library/demo/SKILL.md")
    }

    fn received(&self, home: &Path) -> PathBuf {
        home.join(REPLICA).join(SKILL)
    }

    fn assert_exports_cleaned(&self) {
        assert!(fs::read_dir(&self.temporary).unwrap().next().is_none());
    }
}

#[test]
fn removed_policies_and_protocol_commands_are_invalid_invocations() {
    for args in [
        vec!["library", "rsync", "--conflict", "local"],
        vec!["library", "rsync", "--missing", "copy"],
        vec!["library", "rsync", "--force"],
        vec!["library", "rsync", "--format", "json", "--color", "never"],
        vec!["__rsync", "--operation", "snapshot"],
        vec!["__rsync-server", "--server", "--sender", ".", "/tmp"],
    ] {
        Command::cargo_bin("skillator")
            .unwrap()
            .args(args)
            .assert()
            .code(2)
            .stdout("");
    }
}

#[test]
fn invalid_configuration_and_host_selection_do_not_connect_or_write() {
    let fixture = Fixture::new();
    for selector in ["", "dev,", "unknown"] {
        fixture
            .command(&fixture.leader)
            .args(["library", "rsync", "--hosts", selector])
            .assert()
            .code(2)
            .stdout("");
    }
    fixture.configure_follower();
    fixture
        .command(&fixture.follower)
        .args(["library", "rsync", "--hosts", "dev"])
        .assert()
        .code(2)
        .stdout("");
    fs::write(
        fixture.follower.join(".skillator/config.yaml"),
        "version: 1\nhosts: {}\nleader: {destination: leader.internal}\n",
    )
    .unwrap();
    fixture
        .command(&fixture.follower)
        .args(["library", "rsync"])
        .assert()
        .code(3)
        .stdout("");
    assert!(!fixture.follower.join(REPLICA).exists());
    assert!(!fixture.spare.join(REPLICA).exists());
    fixture.assert_exports_cleaned();
}

#[test]
fn leader_pushes_complete_skills_and_replaces_only_selected_owned_replicas() {
    let fixture = Fixture::new();
    let script = fixture.leader.join(".skillator/library/demo/run.sh");
    fs::write(&script, "#!/bin/sh\necho demo\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o751)).unwrap();
    symlink("run.sh", script.parent().unwrap().join("alias.sh")).unwrap();
    fs::write(fixture.follower.join("unrelated.txt"), "keep").unwrap();
    let preview = fixture.report(&fixture.leader, &["--check"], 1);
    assert_eq!(preview["changes"][0]["action"], "push");
    assert!(!fixture.follower.join(REPLICA).exists());
    assert!(!fixture.spare.join(REPLICA).exists());
    fixture.report(&fixture.leader, &[], 0);
    let before = fs::read(fixture.source()).unwrap();
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        before
    );
    assert_eq!(fs::read(fixture.received(&fixture.spare)).unwrap(), before);
    let remote_script = fixture
        .received(&fixture.follower)
        .parent()
        .unwrap()
        .join("run.sh");
    assert_eq!(
        fs::metadata(&remote_script).unwrap().permissions().mode() & 0o777,
        0o751
    );
    assert_eq!(
        fs::read_link(remote_script.parent().unwrap().join("alias.sh")).unwrap(),
        Path::new("run.sh")
    );
    fixture.report(&fixture.leader, &["--check"], 0);
    let timestamp = fs::metadata(fixture.source()).unwrap().modified().unwrap();
    fs::write(
        fixture.source(),
        String::from_utf8(before.clone())
            .unwrap()
            .replace("leader-v1", "leader-v2"),
    )
    .unwrap();
    fs::File::open(fixture.source())
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(timestamp))
        .unwrap();
    let authoritative = fs::read(fixture.source()).unwrap();
    fs::write(
        fixture.received(&fixture.follower),
        String::from_utf8(authoritative.clone())
            .unwrap()
            .replace("leader-v2", "edited-v2"),
    )
    .unwrap();
    fs::File::open(fixture.received(&fixture.follower))
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(timestamp))
        .unwrap();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        authoritative
    );
    assert_eq!(fs::read(fixture.source()).unwrap(), authoritative);
    assert_eq!(fs::read(fixture.received(&fixture.spare)).unwrap(), before);
    fs::remove_dir_all(fixture.source().parent().unwrap()).unwrap();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
    assert!(!fixture.received(&fixture.follower).exists());
    assert!(fixture.received(&fixture.spare).exists());
    assert_eq!(
        fs::read_to_string(fixture.follower.join("unrelated.txt")).unwrap(),
        "keep"
    );
    assert!(fixture.follower.join(REPLICA).join(MARKER).is_file());
    fixture.report(&fixture.leader, &["--hosts", "dev", "--check"], 0);
    fixture.assert_exports_cleaned();
}

#[test]
fn follower_pulls_current_leader_skills_without_reading_local_library_or_uploading_edits() {
    let fixture = Fixture::new();
    fixture.report(&fixture.leader, &[], 0);
    fixture.configure_follower();
    fs::write(
        fixture.follower.join(".skillator/library.yaml"),
        "not valid library configuration",
    )
    .unwrap();
    fs::create_dir_all(fixture.follower.join(".agents")).unwrap();
    fs::write(
        fixture.follower.join(".agents/skillator.yaml"),
        "not valid user configuration",
    )
    .unwrap();
    fs::write(fixture.follower.join("unrelated.txt"), "keep").unwrap();
    let original = fs::read(fixture.source()).unwrap();
    fs::write(
        fixture.source(),
        String::from_utf8(original.clone())
            .unwrap()
            .replace("leader-v1", "leader-v2"),
    )
    .unwrap();
    let current = fs::read(fixture.source()).unwrap();
    let preview = fixture.report(&fixture.follower, &["--check"], 1);
    assert_eq!(preview["changes"][0]["action"], "pull");
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        original
    );
    fixture.assert_exports_cleaned();
    let applied = fixture.report(&fixture.follower, &[], 0);
    assert_eq!(applied["changes"][0]["host"], "leader");
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        current
    );
    assert_eq!(
        fs::read(fixture.received(&fixture.spare)).unwrap(),
        original
    );
    fs::write(fixture.received(&fixture.follower), "follower-only edit").unwrap();
    fixture.report(&fixture.follower, &[], 0);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        current
    );
    assert_eq!(fs::read(fixture.source()).unwrap(), current);
    fixture.report(&fixture.follower, &["--check"], 0);
    fs::remove_dir_all(fixture.source().parent().unwrap()).unwrap();
    fixture.report(&fixture.follower, &[], 0);
    assert!(!fixture.received(&fixture.follower).exists());
    assert_eq!(
        fs::read(fixture.received(&fixture.spare)).unwrap(),
        original
    );
    assert_eq!(
        fs::read_to_string(fixture.follower.join("unrelated.txt")).unwrap(),
        "keep"
    );
    assert_eq!(
        fs::read_to_string(fixture.follower.join(".skillator/library.yaml")).unwrap(),
        "not valid library configuration"
    );
    fixture.assert_exports_cleaned();
}

#[test]
fn initial_follower_preview_does_not_create_a_replica() {
    let fixture = Fixture::new();
    fixture.configure_follower();
    fixture.report(&fixture.follower, &["--check"], 1);
    assert!(!fixture.follower.join(REPLICA).exists());
    fixture.assert_exports_cleaned();
    fixture.report(&fixture.follower, &[], 0);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        fs::read(fixture.source()).unwrap()
    );
    fixture.assert_exports_cleaned();
}

#[test]
fn incomplete_or_absent_leader_configuration_cannot_delete_replica_content() {
    let fixture = Fixture::new();
    fixture.report(&fixture.leader, &[], 0);
    fixture.configure_follower();
    let original = fs::read(fixture.received(&fixture.follower)).unwrap();
    fs::write(fixture.source(), "invalid skill metadata").unwrap();
    fixture
        .command(&fixture.leader)
        .args(["library", "rsync"])
        .assert()
        .code(3)
        .stdout("");
    fixture.report(&fixture.follower, &[], 1);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        original
    );
    fs::remove_file(fixture.leader.join(".skillator/library.yaml")).unwrap();
    fixture
        .command(&fixture.leader)
        .args(["library", "rsync"])
        .assert()
        .code(3)
        .stdout("");
    fixture.report(&fixture.follower, &[], 1);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        original
    );
    assert_eq!(
        fs::read(fixture.received(&fixture.spare)).unwrap(),
        original
    );
    fixture.assert_exports_cleaned();
}

#[test]
fn unowned_or_redirected_remote_and_local_replicas_are_not_adopted() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.follower.join(REPLICA)).unwrap();
    let existing = fixture.follower.join(REPLICA).join("existing.txt");
    fs::write(&existing, "keep").unwrap();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 1);
    assert_eq!(fs::read_to_string(&existing).unwrap(), "keep");
    assert!(!fixture.follower.join(REPLICA).join(MARKER).exists());
    fixture.configure_follower();
    fixture.report(&fixture.follower, &[], 1);
    assert_eq!(fs::read_to_string(&existing).unwrap(), "keep");
    fs::remove_dir_all(fixture.follower.join(REPLICA)).unwrap();
    let outside = fixture.root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "keep").unwrap();
    symlink(&outside, fixture.follower.join(REPLICA)).unwrap();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 1);
    fixture.report(&fixture.follower, &[], 1);
    assert_eq!(fs::read_to_string(outside.join("keep")).unwrap(), "keep");
    assert!(!outside.join(MARKER).exists());
    fixture.assert_exports_cleaned();
}

#[test]
fn independent_host_failure_and_machine_previews_are_reported_consistently() {
    let fixture = Fixture::new();
    fs::write(fixture.leader.join(".skillator/config.yaml"), "version: 1\nhosts:\n  dev: {destination: receiver-a.internal}\n  unreachable: {destination: unavailable.internal}\n").unwrap();
    let json = fixture.report(&fixture.leader, &["--check"], 1);
    let yaml_output = fixture
        .command(&fixture.leader)
        .args(["library", "rsync", "--check", "--format", "yaml"])
        .assert()
        .code(1);
    let yaml: Value =
        serde_saphyr::from_str(std::str::from_utf8(&yaml_output.get_output().stdout).unwrap())
            .unwrap();
    assert_eq!(json, yaml);
    assert!(!fixture.follower.join(REPLICA).exists());
    let report = fixture.report(&fixture.leader, &[], 1);
    assert_eq!(report["changes"][0]["host"], "dev");
    assert_eq!(report["changes"][0]["outcome"], "applied");
    assert_eq!(report["changes"][1]["host"], "unreachable");
    assert_eq!(report["changes"][1]["outcome"], "failed");
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        fs::read(fixture.source()).unwrap()
    );
    fixture.assert_exports_cleaned();
}

#[test]
fn missing_leader_skillator_and_invalid_export_paths_preserve_existing_replica() {
    let fixture = Fixture::new();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
    fixture.configure_follower();
    let preserved = b"keep this follower edit until valid leader input is available";
    fs::write(fixture.received(&fixture.follower), preserved).unwrap();
    let helper = fixture.root.path().join("leader-bin/skillator");
    fs::remove_file(&helper).unwrap();
    fixture.report(&fixture.follower, &[], 1);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        preserved
    );
    let outside = fixture.root.path().join("unrelated export");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "unrelated").unwrap();
    fs::write(
        &helper,
        format!("#!/bin/sh\nprintf '%s\\n' {}\n", quote(&outside)),
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.report(&fixture.follower, &[], 1);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        preserved
    );
    assert_eq!(
        fs::read_to_string(outside.join("keep")).unwrap(),
        "unrelated"
    );
    fixture.assert_exports_cleaned();
}

#[test]
fn failed_export_cleanup_reports_the_retained_path_without_claiming_success() {
    let fixture = Fixture::new();
    fixture.configure_follower();
    let remote_rm = fixture.root.path().join("leader-bin/rm");
    fs::remove_file(&remote_rm).unwrap();
    fs::write(
        &remote_rm,
        "#!/bin/sh\necho 'cleanup unavailable' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&remote_rm, fs::Permissions::from_mode(0o755)).unwrap();
    let assertion = fixture
        .command(&fixture.follower)
        .args(["library", "rsync", "--format", "json"])
        .assert()
        .code(1);
    let output = assertion.get_output();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["diagnostics"][0]["code"],
        "leader_export_cleanup_failed"
    );
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        fs::read(fixture.source()).unwrap()
    );
    let retained = fs::read_dir(&fixture.temporary)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(retained.join(MARKER).is_file());
    assert!(String::from_utf8_lossy(&output.stderr).contains(retained.to_str().unwrap()));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(retained.to_str().unwrap()));
    fs::remove_file(&remote_rm).unwrap();
    symlink(executable("rm"), &remote_rm).unwrap();
    fixture.report(&fixture.follower, &[], 0);
    assert!(retained.exists());
    fs::remove_dir_all(retained).unwrap();
    fixture.assert_exports_cleaned();
}

#[test]
fn retry_converges_after_rsync_failure_leaves_partial_owned_content() {
    let fixture = Fixture::new();
    let remote_rsync = fixture.root.path().join("receiver-bin/rsync");
    fs::remove_file(&remote_rsync).unwrap();
    fs::write(&remote_rsync, format!(
        "#!/bin/sh\ncase \"$*\" in\n *--sender*) exec {} \"$@\";;\n *--server*) mkdir -p \"$HOME/{REPLICA}/local/library/_skills/demo\"; printf partial > \"$HOME/{REPLICA}/{SKILL}\"; exit 12;;\n *) exec {} \"$@\";;\nesac\n", quote(&executable("rsync")), quote(&executable("rsync"))
    )).unwrap();
    fs::set_permissions(&remote_rsync, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 1);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        b"partial"
    );
    let authoritative = fs::read(fixture.source()).unwrap();
    fs::remove_file(&remote_rsync).unwrap();
    symlink(executable("rsync"), &remote_rsync).unwrap();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        authoritative
    );
    assert_eq!(fs::read(fixture.source()).unwrap(), authoritative);
    fixture.report(&fixture.leader, &["--hosts", "dev", "--check"], 0);
    fixture.assert_exports_cleaned();
}

#[test]
fn non_normalized_leader_tmpdir_still_supports_fresh_pull_and_cleanup() {
    let fixture = Fixture::new();
    fixture.configure_follower();
    let adapter = fixture.root.path().join("bin/ssh");
    let temporary = fixture
        .temporary
        .join("..")
        .join(fixture.temporary.file_name().unwrap());
    let script = fs::read_to_string(&adapter)
        .unwrap()
        .replace(&quote(&fixture.temporary), &quote(&temporary));
    fs::write(&adapter, script).unwrap();
    fixture.report(&fixture.follower, &[], 0);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        fs::read(fixture.source()).unwrap()
    );
    fixture.assert_exports_cleaned();
    fixture.report(&fixture.follower, &["--check"], 0);
    fixture.assert_exports_cleaned();
}

#[test]
fn failed_local_export_cleanup_reports_failure_and_retained_path() {
    let fixture = Fixture::new();
    let adapter = fixture.root.path().join("bin/ssh");
    let script = fs::read_to_string(&adapter).unwrap().replace(
        "exec /bin/sh -c",
        "/bin/chmod 500 \"$TMPDIR\"\nexec /bin/sh -c",
    );
    fs::write(&adapter, script).unwrap();
    let output = fixture
        .command(&fixture.leader)
        .args(["library", "rsync", "--hosts", "dev", "--format", "json"])
        .output()
        .unwrap();
    fs::set_permissions(&fixture.temporary, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["diagnostics"][0]["code"],
        "leader_export_cleanup_failed"
    );
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        fs::read(fixture.source()).unwrap()
    );
    let retained = fs::read_dir(&fixture.temporary)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(String::from_utf8_lossy(&output.stderr).contains(retained.to_str().unwrap()));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(retained.to_str().unwrap()));
    fs::remove_dir_all(retained).unwrap();
    fixture.assert_exports_cleaned();
}

#[test]
fn first_empty_replica_creation_is_an_applied_action_in_both_directions() {
    let fixture = Fixture::new();
    fs::remove_dir_all(fixture.source().parent().unwrap()).unwrap();
    let adapter = fixture.root.path().join("bin/ssh");
    let script = fs::read_to_string(&adapter)
        .unwrap()
        .replace("exec /bin/sh -c", "umask 077\nexec /bin/sh -c");
    fs::write(&adapter, script).unwrap();
    fixture.report(&fixture.leader, &["--hosts", "dev", "--check"], 1);
    let pushed = fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
    assert_eq!(pushed["changes"][0]["action"], "push");
    assert_eq!(pushed["changes"][0]["outcome"], "applied");
    assert!(fixture.follower.join(REPLICA).join(MARKER).is_file());
    fixture.report(&fixture.leader, &["--hosts", "dev", "--check"], 0);
    fs::remove_dir_all(fixture.follower.join(REPLICA)).unwrap();
    fixture.configure_follower();
    let assertion = Command::new("/bin/sh")
        .env("HOME", &fixture.follower)
        .env("PATH", &fixture.path)
        .env("TMPDIR", &fixture.temporary)
        .args(["-c", "umask 077; exec \"$@\"", "empty-library-pull"])
        .arg(assert_cmd::cargo::cargo_bin("skillator"))
        .args(["library", "rsync", "--format", "json"])
        .assert()
        .code(0);
    let pulled: Value = serde_json::from_slice(&assertion.get_output().stdout).unwrap();
    assert_eq!(pulled["changes"][0]["action"], "pull");
    assert_eq!(pulled["changes"][0]["outcome"], "applied");
    assert!(fixture.follower.join(REPLICA).join(MARKER).is_file());
    fixture.report(&fixture.follower, &["--check"], 0);
    fixture.assert_exports_cleaned();
}

#[test]
fn registered_git_repository_delivers_only_skill_content_on_push_and_fresh_pull() {
    let fixture = Fixture::new();
    fs::remove_dir_all(fixture.source().parent().unwrap()).unwrap();
    let repository = fixture.leader.join("collection");
    let skill = repository.join("tools/check");
    fs::create_dir_all(skill.join("scripts")).unwrap();
    for arguments in [
        vec!["init", "--quiet"],
        vec![
            "remote",
            "add",
            "origin",
            "https://example.test/acme/skill-tools.git",
        ],
    ] {
        assert!(
            std::process::Command::new(executable("git"))
                .arg("-C")
                .arg(&repository)
                .args(arguments)
                .status()
                .unwrap()
                .success()
        );
    }
    let metadata = "---\nname: check\ndescription: Repository skill\n---\nInstructions\n";
    fs::write(skill.join("SKILL.md"), metadata).unwrap();
    fs::write(skill.join("template.txt"), "template-v1").unwrap();
    fs::write(skill.join("scripts/run.sh"), "#!/bin/sh\necho skill\n").unwrap();
    fs::set_permissions(
        skill.join("scripts/run.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    for path in [
        "README.md",
        "LICENSE",
        "src/main.rs",
        "docs/notes.md",
        ".github/workflows/ci.yaml",
    ] {
        let file = repository.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "repository-only content").unwrap();
    }
    fixture
        .command(&fixture.leader)
        .args(["library", "add", repository.to_str().unwrap()])
        .assert()
        .code(0);
    let delivered_relative = Path::new("acme/skill-tools/_skills/tools/check");
    let assert_skill_only = || {
        let replica = fixture.follower.join(REPLICA);
        let delivered = replica.join(delivered_relative);
        assert_eq!(
            fs::read_to_string(delivered.join("SKILL.md")).unwrap(),
            metadata
        );
        assert_eq!(
            fs::read(delivered.join("template.txt")).unwrap(),
            fs::read(skill.join("template.txt")).unwrap()
        );
        assert_eq!(
            fs::read(delivered.join("scripts/run.sh")).unwrap(),
            fs::read(skill.join("scripts/run.sh")).unwrap()
        );
        assert_eq!(
            fs::metadata(delivered.join("scripts/run.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        fn files(directory: &Path, relative: &Path, found: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(directory.join(relative)).unwrap() {
                let entry = entry.unwrap();
                let path = relative.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    files(directory, &path, found);
                } else {
                    found.push(path);
                }
            }
        }
        let mut actual = Vec::new();
        files(&replica, Path::new(""), &mut actual);
        actual.sort();
        let mut expected = vec![
            PathBuf::from(MARKER),
            delivered_relative.join("SKILL.md"),
            delivered_relative.join("template.txt"),
            delivered_relative.join("scripts/run.sh"),
        ];
        expected.sort();
        assert_eq!(actual, expected);
    };
    fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
    assert_skill_only();
    fixture.configure_follower();
    fs::write(skill.join("template.txt"), "template-v2-without-push").unwrap();
    fs::write(repository.join("README.md"), "unrelated repository update").unwrap();
    fixture.report(&fixture.follower, &[], 0);
    assert_skill_only();
    assert!(!fixture.spare.join(REPLICA).exists());
    assert_eq!(
        fs::read_to_string(repository.join("README.md")).unwrap(),
        "unrelated repository update"
    );
    assert!(repository.join(".git/config").is_file());
    fixture.assert_exports_cleaned();
}

#[test]
fn remote_home_is_normalized_before_replica_creation() {
    let fixture = Fixture::new();
    let adapter = fixture.root.path().join("bin/ssh");
    let script = fs::read_to_string(&adapter).unwrap();
    let home = PathBuf::from(format!("{}/../follower home/", fixture.follower.display()));
    fs::write(
        &adapter,
        script.replace(&quote(&fixture.follower), &quote(&home)),
    )
    .unwrap();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        fs::read(fixture.source()).unwrap()
    );
    fixture.report(&fixture.leader, &["--hosts", "dev", "--check"], 0);
    fixture.assert_exports_cleaned();
}

#[test]
fn remote_home_with_control_characters_is_rejected_without_replica_writes() {
    let fixture = Fixture::new();
    let adapter = fixture.root.path().join("bin/ssh");
    let script = fs::read_to_string(&adapter).unwrap();
    let home = fixture.follower.join("bad\nhome");
    fs::create_dir(&home).unwrap();
    fs::write(
        &adapter,
        script.replace(&quote(&fixture.follower), &quote(&home)),
    )
    .unwrap();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 1);
    assert!(!home.join(".skillator").exists());
    fixture.assert_exports_cleaned();
}

#[test]
fn unsupported_rsync_options_fail_before_replica_creation() {
    for endpoint in ["local", "receiver", "leader"] {
        let fixture = Fixture::new();
        let program = fixture.root.path().join(match endpoint {
            "local" => "bin/rsync",
            "receiver" => "receiver-bin/rsync",
            "leader" => "leader-bin/rsync",
            _ => unreachable!(),
        });
        if endpoint != "local" {
            fs::remove_file(&program).unwrap();
        }
        fs::write(
            &program,
            format!(
                "#!/bin/sh\nfor argument do\n if [ \"$argument\" = --delete-delay ]; then echo 'unsupported rsync option: --delete-delay' >&2; exit 1; fi\ndone\nexec {} \"$@\"\n",
                quote(&executable("rsync")),
            ),
        ).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        if endpoint == "local" {
            fixture
                .command(&fixture.leader)
                .args(["library", "rsync", "--hosts", "dev", "--format", "json"])
                .assert()
                .code(3)
                .stdout("");
        } else if endpoint == "receiver" {
            fixture.report(&fixture.leader, &["--hosts", "dev"], 1);
        } else {
            fixture.configure_follower();
            fixture.report(&fixture.follower, &[], 1);
        }
        assert!(!fixture.follower.join(REPLICA).exists(), "{endpoint}");
        assert!(!fixture.spare.join(REPLICA).exists(), "{endpoint}");
        fixture.assert_exports_cleaned();
    }
}
