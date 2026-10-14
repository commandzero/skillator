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
        for name in ["rsync", "mkdir", "cmp", "rm", "rmdir", "find"] {
            let program = executable(name);
            symlink(&program, receiver_bin.join(name)).unwrap();
            symlink(&program, leader_bin.join(name)).unwrap();
        }
        symlink(executable("chmod"), leader_bin.join("chmod")).unwrap();
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

    fn invalid_local_input(&self, home: &Path, args: &[&str]) {
        self.command(home)
            .args(["library", "rsync"])
            .args(args)
            .args(["--format", "json"])
            .assert()
            .code(3)
            .stdout("");
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
    fixture.invalid_local_input(&fixture.follower, &[]);
    assert_eq!(fs::read_to_string(&existing).unwrap(), "keep");
    fs::remove_dir_all(fixture.follower.join(REPLICA)).unwrap();
    let outside = fixture.root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "keep").unwrap();
    symlink(&outside, fixture.follower.join(REPLICA)).unwrap();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 1);
    fixture.invalid_local_input(&fixture.follower, &[]);
    assert_eq!(fs::read_to_string(outside.join("keep")).unwrap(), "keep");
    assert!(!outside.join(MARKER).exists());
    fixture.assert_exports_cleaned();
}

#[test]
fn multiply_linked_replica_files_are_rejected_before_push_or_pull_even_in_check_mode() {
    for (pull, link_entry) in [(false, false), (true, false), (false, true), (true, true)] {
        let fixture = Fixture::new();
        fs::set_permissions(fixture.source(), fs::Permissions::from_mode(0o755)).unwrap();
        if link_entry {
            symlink("SKILL.md", fixture.source().with_file_name("alias.md")).unwrap();
        }
        fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
        if pull {
            fixture.configure_follower();
        }
        let outside = fixture.root.path().join("unrelated hard link");
        let original = fs::read(fixture.source()).unwrap();
        if link_entry {
            fs::write(fixture.root.path().join("SKILL.md"), &original).unwrap();
            symlink("SKILL.md", &outside).unwrap();
        } else {
            fs::write(&outside, &original).unwrap();
            fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let old_time =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        fs::File::open(&outside)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old_time))
            .unwrap();
        let received = if link_entry {
            fixture
                .received(&fixture.follower)
                .with_file_name("alias.md")
        } else {
            fixture.received(&fixture.follower)
        };
        fs::remove_file(&received).unwrap();
        if link_entry {
            assert!(
                std::process::Command::new("touch")
                    .args(["-h", "-t", "200001010000.00"])
                    .arg(&outside)
                    .status()
                    .unwrap()
                    .success()
            );
            assert!(
                std::process::Command::new("ln")
                    .arg("-P")
                    .arg(&outside)
                    .arg(&received)
                    .status()
                    .unwrap()
                    .success()
            );
            assert!(fs::symlink_metadata(&received).unwrap().is_symlink());
        } else {
            fs::hard_link(&outside, &received).unwrap();
        }
        let original_metadata = fs::symlink_metadata(&outside).unwrap();
        for check in [true, false] {
            let args: &[&str] = match (pull, check) {
                (false, true) => &["--hosts", "dev", "--check"],
                (false, false) => &["--hosts", "dev"],
                (true, true) => &["--check"],
                (true, false) => &[],
            };
            let home = if pull {
                &fixture.follower
            } else {
                &fixture.leader
            };
            if pull {
                fixture.invalid_local_input(home, args);
            } else {
                let report = fixture.report(home, args, 1);
                assert_eq!(report["changes"][0]["outcome"], "failed");
            }
            let metadata = fs::symlink_metadata(&outside).unwrap();
            assert_eq!(fs::read(&outside).unwrap(), original);
            assert_eq!(fs::read(&received).unwrap(), original);
            assert_eq!(
                metadata.permissions().mode(),
                original_metadata.permissions().mode()
            );
            assert_eq!(
                metadata.modified().unwrap(),
                original_metadata.modified().unwrap()
            );
        }
        fixture.assert_exports_cleaned();
    }
}

#[test]
fn multiply_linked_replica_markers_are_rejected_before_push_or_pull() {
    for pull in [false, true] {
        let fixture = Fixture::new();
        fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
        if pull {
            fixture.configure_follower();
        }
        let marker = fixture.follower.join(REPLICA).join(MARKER);
        let outside = fixture.root.path().join("linked marker outside replica");
        fs::hard_link(&marker, &outside).unwrap();
        let original = fs::read(&outside).unwrap();
        let original_metadata = fs::metadata(&outside).unwrap();
        for check in [true, false] {
            let args: &[&str] = match (pull, check) {
                (false, true) => &["--hosts", "dev", "--check"],
                (false, false) => &["--hosts", "dev"],
                (true, true) => &["--check"],
                (true, false) => &[],
            };
            let home = if pull {
                &fixture.follower
            } else {
                &fixture.leader
            };
            if pull {
                fixture.invalid_local_input(home, args);
            } else {
                let report = fixture.report(home, args, 1);
                assert_eq!(report["changes"][0]["outcome"], "failed");
            }
            let metadata = fs::metadata(&outside).unwrap();
            assert_eq!(fs::read(&outside).unwrap(), original);
            assert_eq!(
                metadata.permissions().mode(),
                original_metadata.permissions().mode()
            );
            assert_eq!(
                metadata.modified().unwrap(),
                original_metadata.modified().unwrap()
            );
        }
        fixture.assert_exports_cleaned();
    }
}

#[test]
fn receiver_traversal_errors_refuse_transfer() {
    let fixture = Fixture::new();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
    let received = fixture.received(&fixture.follower);
    let original = fs::read(&received).unwrap();
    let remote_find = fixture.root.path().join("receiver-bin/find");
    fs::remove_file(&remote_find).unwrap();
    fs::write(&remote_find, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&remote_find, fs::Permissions::from_mode(0o755)).unwrap();
    for args in [&["--hosts", "dev", "--check"][..], &["--hosts", "dev"][..]] {
        let report = fixture.report(&fixture.leader, args, 1);
        assert_eq!(report["changes"][0]["outcome"], "failed");
        assert_eq!(fs::read(&received).unwrap(), original);
    }
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
    let outside = fixture.root.path().join("skillator-library-export-outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "unrelated").unwrap();
    fs::copy(
        fixture.follower.join(REPLICA).join(MARKER),
        outside.join(MARKER),
    )
    .unwrap();
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
    assert!(outside.join(MARKER).is_file());
    fixture.assert_exports_cleaned();
}

#[test]
fn failed_export_cleanup_reports_the_retained_path_without_claiming_success() {
    let fixture = Fixture::new();
    fixture.configure_follower();
    fixture.report(&fixture.follower, &[], 0);
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
    assert_eq!(report["changes"], serde_json::json!([]));
    assert_eq!(report["diagnostics"][0]["data"]["host"], "leader");
    assert_eq!(
        report["diagnostics"][0]["data"]["path"],
        "~/.skillator/library/replica"
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
    fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
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
    assert_eq!(report["changes"], serde_json::json!([]));
    assert_eq!(report["diagnostics"][0]["data"]["host"], "leader");
    assert_eq!(
        report["diagnostics"][0]["data"]["path"],
        "~/.skillator/library/replica"
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
fn unsolicited_receiver_stdout_cannot_create_a_fresh_replica() {
    for args in [&["--hosts", "dev", "--check"][..], &["--hosts", "dev"][..]] {
        let fixture = Fixture::new();
        let adapter = fixture.root.path().join("bin/ssh");
        let script = fs::read_to_string(&adapter).unwrap();
        let receiver = format!(
            "receiver-a.internal) export HOME={} PATH={};;",
            quote(&fixture.follower),
            quote(&fixture.root.path().join("receiver-bin")),
        );
        let noisy = receiver.replace(";;", "; printf 'unsolicited receiver output\\n';;");
        assert!(script.contains(&receiver));
        fs::write(&adapter, script.replace(&receiver, &noisy)).unwrap();
        let original = fs::read(fixture.source()).unwrap();
        let report = fixture.report(&fixture.leader, args, 1);
        assert_eq!(report["changes"][0]["outcome"], "failed");
        assert!(!fixture.follower.join(".skillator").exists());
        assert!(!fixture.follower.join(REPLICA).join(MARKER).exists());
        assert_eq!(fs::read(fixture.source()).unwrap(), original);
        fixture.assert_exports_cleaned();
    }
}

#[test]
fn failed_remote_marker_initialization_rolls_back_and_retry_delivers() {
    let fixture = Fixture::new();
    let mkdir = fixture.root.path().join("receiver-bin/mkdir");
    fs::remove_file(&mkdir).unwrap();
    fs::write(
        &mkdir,
        format!(
            "#!/bin/sh\n{} \"$@\" || exit $?\nfor path do\n if [ \"$path\" = {} ]; then /bin/chmod 0555 \"$path\"; fi\ndone\n",
            quote(&executable("mkdir")),
            quote(&fs::canonicalize(&fixture.follower).unwrap().join(REPLICA)),
        ),
    )
    .unwrap();
    fs::set_permissions(&mkdir, fs::Permissions::from_mode(0o755)).unwrap();
    let original = fs::read(fixture.source()).unwrap();
    let unrelated = fixture.follower.join("unrelated.txt");
    fs::write(&unrelated, "keep").unwrap();
    let failed = fixture.report(&fixture.leader, &["--hosts", "dev"], 1);
    assert_eq!(failed["changes"][0]["outcome"], "failed");
    assert!(!fixture.follower.join(REPLICA).exists());
    assert!(!fixture.follower.join(REPLICA).join(MARKER).exists());
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "keep");
    assert_eq!(fs::read(fixture.source()).unwrap(), original);
    fixture.assert_exports_cleaned();

    fs::remove_file(&mkdir).unwrap();
    symlink(executable("mkdir"), &mkdir).unwrap();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        original
    );
    assert!(fixture.follower.join(REPLICA).join(MARKER).is_file());
    fixture.report(&fixture.leader, &["--hosts", "dev", "--check"], 0);
    fixture.assert_exports_cleaned();
}

#[test]
fn failed_local_marker_initialization_rolls_back_and_retry_delivers() {
    let fixture = Fixture::new();
    fixture.configure_follower();
    fs::create_dir_all(fixture.follower.join(".skillator/library")).unwrap();
    let unrelated = fixture.follower.join(".skillator/library/unrelated.txt");
    fs::write(&unrelated, "keep").unwrap();
    let adapter = fixture.root.path().join("bin/ssh");
    let script = fs::read_to_string(&adapter).unwrap();
    assert!(script.contains("exec /bin/sh -c"));
    fs::write(
        &adapter,
        script.replace("exec /bin/sh -c", "umask 022\nexec /bin/sh -c"),
    )
    .unwrap();
    let original = fs::read(fixture.source()).unwrap();
    let failed = Command::new("/bin/sh")
        .env("HOME", &fixture.follower)
        .env("PATH", &fixture.path)
        .env("TMPDIR", &fixture.temporary)
        .args(["-c", "umask 0222; exec \"$@\"", "marker-initialization"])
        .arg(assert_cmd::cargo::cargo_bin("skillator"))
        .args(["library", "rsync", "--format", "json"])
        .assert()
        .code(1);
    let report: Value = serde_json::from_slice(&failed.get_output().stdout).unwrap();
    assert_eq!(report["changes"][0]["outcome"], "failed");
    assert!(!fixture.follower.join(REPLICA).exists());
    assert!(!fixture.follower.join(REPLICA).join(MARKER).exists());
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "keep");
    assert_eq!(fs::read(fixture.source()).unwrap(), original);
    fixture.assert_exports_cleaned();

    fixture.report(&fixture.follower, &[], 0);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        original
    );
    assert!(fixture.follower.join(REPLICA).join(MARKER).is_file());
    assert_eq!(fs::read_to_string(&unrelated).unwrap(), "keep");
    fixture.report(&fixture.follower, &["--check"], 0);
    fixture.assert_exports_cleaned();
}

#[test]
fn post_publication_validation_failure_identifies_retained_export() {
    let fixture = Fixture::new();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
    fixture.configure_follower();
    let preserved = b"follower edit that must survive validation failure";
    fs::write(fixture.received(&fixture.follower), preserved).unwrap();
    let helper = fixture.root.path().join("leader-bin/skillator");
    fs::remove_file(&helper).unwrap();
    let disconnected = fixture.root.path().join("disconnect-after-export");
    fs::write(
        &helper,
        format!(
            "#!/bin/sh\nset -eu\nexport_path=$({} \"$@\")\nprintf '%s\\n' \"$export_path\"\n: > {}\n",
            quote(&assert_cmd::cargo::cargo_bin("skillator")),
            quote(&disconnected),
        ),
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    let adapter = fixture.root.path().join("bin/ssh");
    let script = fs::read_to_string(&adapter).unwrap();
    assert!(script.contains("exec /bin/sh -c"));
    fs::write(
        &adapter,
        script.replace(
            "exec /bin/sh -c",
            &format!(
                "if [ -e {} ]; then printf 'leader disconnected\\n' >&2; exit 255; fi\nexec /bin/sh -c",
                quote(&disconnected),
            ),
        ),
    )
    .unwrap();
    let assertion = fixture
        .command(&fixture.follower)
        .args(["library", "rsync", "--format", "json"])
        .assert()
        .code(1);
    let output = assertion.get_output();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["changes"][0]["outcome"], "failed");
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        preserved
    );
    assert!(disconnected.is_file());
    let retained: Vec<_> = fs::read_dir(&fixture.temporary)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(retained.len(), 1);
    assert!(retained[0].join(MARKER).is_file());
    let candidate = retained[0].to_str().unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains(candidate));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(candidate));
    fs::write(&adapter, script).unwrap();
    fs::remove_file(&disconnected).unwrap();
    fs::remove_file(&helper).unwrap();
    symlink(assert_cmd::cargo::cargo_bin("skillator"), &helper).unwrap();
    fs::remove_dir_all(&retained[0]).unwrap();
    fixture.assert_exports_cleaned();
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_physical_receiver_home_fails_before_fresh_replica_creation() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let fixture = Fixture::new();
    let physical = fixture
        .root
        .path()
        .join(OsStr::from_bytes(b"receiver-home-\xff"));
    fs::create_dir(&physical).unwrap();
    let alias = fixture.root.path().join("receiver-home-alias");
    symlink(&physical, &alias).unwrap();
    let adapter = fixture.root.path().join("bin/ssh");
    let script = fs::read_to_string(&adapter).unwrap();
    assert!(script.contains(&quote(&fixture.follower)));
    fs::write(
        &adapter,
        script.replace(&quote(&fixture.follower), &quote(&alias)),
    )
    .unwrap();
    assert_eq!(fs::canonicalize(&alias).unwrap(), physical);
    for args in [&["--hosts", "dev", "--check"][..], &["--hosts", "dev"][..]] {
        let report = fixture.report(&fixture.leader, args, 1);
        assert_eq!(report["changes"][0]["outcome"], "failed");
        assert!(!physical.join(".skillator").exists());
        assert!(!physical.join(REPLICA).join(MARKER).exists());
        fixture.assert_exports_cleaned();
    }
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

#[test]
fn local_home_variants_pull_into_the_same_physical_replica() {
    let fixture = Fixture::new();
    fixture.configure_follower();
    let alias = fixture.root.path().join("follower alias");
    symlink(&fixture.follower, &alias).unwrap();
    let homes = [
        PathBuf::from(format!("{}/", fixture.follower.display())),
        PathBuf::from(format!("{}/../follower home/", fixture.follower.display())),
        alias,
    ];
    for (index, home) in homes.iter().enumerate() {
        fs::write(
            fixture.source(),
            format!("---\nname: demo\ndescription: A skill\n---\nleader-version-{index}\n"),
        )
        .unwrap();
        let preview = fixture.report(home, &["--check"], 1);
        assert_eq!(preview["changes"][0]["action"], "pull");
        if index == 0 {
            assert!(!fixture.follower.join(REPLICA).exists());
        }
        let applied = fixture.report(home, &[], 0);
        assert_eq!(applied["changes"][0]["action"], "pull");
        assert_eq!(
            fs::read(fixture.received(&fixture.follower)).unwrap(),
            fs::read(fixture.source()).unwrap()
        );
        assert!(fixture.follower.join(REPLICA).join(MARKER).is_file());
        fixture.report(home, &["--check"], 0);
    }
    assert!(!fixture.spare.join(REPLICA).exists());
    fixture.assert_exports_cleaned();
}

#[test]
fn local_home_resolving_to_control_character_path_cannot_create_replica() {
    let fixture = Fixture::new();
    let physical = fixture.root.path().join("invalid\nhome");
    fs::create_dir_all(physical.join(".skillator")).unwrap();
    fs::write(
        physical.join(".skillator/config.yaml"),
        "version: 1\nleader: {destination: leader.internal}\n",
    )
    .unwrap();
    let alias = fixture.root.path().join("invalid home alias");
    symlink(&physical, &alias).unwrap();
    for home in [&physical, &alias] {
        for args in [&["--check"][..], &[][..]] {
            fixture.invalid_local_input(home, args);
        }
    }
    assert!(!physical.join(REPLICA).exists());
    fixture.assert_exports_cleaned();
}

#[test]
fn fresh_pull_and_check_clean_readonly_source_directories_without_changing_source_files() {
    let fixture = Fixture::new();
    fixture.configure_follower();
    let skill = fixture.source().parent().unwrap().to_path_buf();
    let nested = skill.join("nested");
    fs::create_dir(&nested).unwrap();
    let nested_file = nested.join("notes.txt");
    fs::write(&nested_file, "read-only nested content").unwrap();
    symlink("nested", skill.join("nested-alias")).unwrap();
    fs::set_permissions(fixture.source(), fs::Permissions::from_mode(0o444)).unwrap();
    fs::set_permissions(&nested_file, fs::Permissions::from_mode(0o444)).unwrap();
    fs::set_permissions(&nested, fs::Permissions::from_mode(0o555)).unwrap();
    fs::set_permissions(&skill, fs::Permissions::from_mode(0o555)).unwrap();
    let source_file = fs::metadata(fixture.source()).unwrap();
    let source_nested_file = fs::metadata(&nested_file).unwrap();
    let source_directory = fs::metadata(&skill).unwrap();
    let source_nested_directory = fs::metadata(&nested).unwrap();
    let original_skill = fs::read(fixture.source()).unwrap();
    let original_nested = fs::read(&nested_file).unwrap();

    let preview = fixture.report(&fixture.follower, &["--check"], 1);
    assert_eq!(preview["changes"][0]["action"], "pull");
    assert!(!fixture.follower.join(REPLICA).exists());
    fixture.assert_exports_cleaned();

    let applied = fixture.report(&fixture.follower, &[], 0);
    assert_eq!(applied["changes"][0]["action"], "pull");
    fixture.assert_exports_cleaned();
    let received_skill = fixture.received(&fixture.follower);
    let received_directory = received_skill.parent().unwrap();
    assert_eq!(
        fs::read(&received_skill).unwrap(),
        fs::read(fixture.source()).unwrap()
    );
    assert_eq!(
        fs::read(received_directory.join("nested/notes.txt")).unwrap(),
        fs::read(&nested_file).unwrap()
    );
    assert_eq!(
        fs::read_link(received_directory.join("nested-alias")).unwrap(),
        Path::new("nested")
    );
    let received_nested = received_directory.join("nested");
    for path in [received_directory, received_nested.as_path()] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o555
        );
    }
    assert_eq!(
        fs::metadata(&received_skill).unwrap().permissions().mode() & 0o777,
        0o444
    );
    fixture.report(&fixture.follower, &["--check"], 0);
    fixture.assert_exports_cleaned();
    for (path, before) in [
        (fixture.source(), source_file),
        (nested_file.clone(), source_nested_file),
        (skill.clone(), source_directory),
        (nested.clone(), source_nested_directory),
    ] {
        let after = fs::metadata(path).unwrap();
        assert_eq!(after.permissions().mode(), before.permissions().mode());
        assert_eq!(after.modified().unwrap(), before.modified().unwrap());
    }
    assert_eq!(fs::read(fixture.source()).unwrap(), original_skill);
    assert_eq!(fs::read(&nested_file).unwrap(), original_nested);
    assert_eq!(
        fs::metadata(&skill).unwrap().permissions().mode() & 0o777,
        0o555
    );
    assert_eq!(
        fs::metadata(&nested).unwrap().permissions().mode() & 0o777,
        0o555
    );
    fs::set_permissions(&nested, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&skill, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn missing_leader_cleanup_utilities_block_export_and_preserve_existing_replica() {
    for utility in ["cmp", "rm", "find", "chmod"] {
        let fixture = Fixture::new();
        fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
        fixture.configure_follower();
        let preserved = format!("follower edit retained when {utility} is missing");
        fs::write(fixture.received(&fixture.follower), &preserved).unwrap();
        fs::remove_file(fixture.root.path().join("leader-bin").join(utility)).unwrap();
        for args in [&["--check"][..], &[][..]] {
            let report = fixture.report(&fixture.follower, args, 1);
            assert_eq!(report["changes"][0]["outcome"], "failed", "{utility}");
            assert_eq!(
                fs::read_to_string(fixture.received(&fixture.follower)).unwrap(),
                preserved,
                "{utility}"
            );
            fixture.assert_exports_cleaned();
        }
    }
}

#[test]
fn failed_helper_stdout_removes_readonly_export_without_changing_source() {
    let fixture = Fixture::new();
    let skill = fixture.source().parent().unwrap().to_path_buf();
    let nested = skill.join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("notes.txt"), "source content").unwrap();
    for path in [&skill, &nested] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o555)).unwrap();
    }
    let before = fs::metadata(fixture.source()).unwrap();
    let original = fs::read(fixture.source()).unwrap();
    let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
    drop(reader);
    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin("skillator"))
        .env("HOME", &fixture.leader)
        .env("PATH", &fixture.path)
        .env("TMPDIR", &fixture.temporary)
        .args(["library", "rsync", "--prepare-export"])
        .stdout(std::process::Stdio::from(std::os::fd::OwnedFd::from(
            writer,
        )))
        .output()
        .unwrap();
    for path in [&skill, &nested] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o555
        );
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(output.status.code(), Some(5));
    fixture.assert_exports_cleaned();
    let after = fs::metadata(fixture.source()).unwrap();
    assert_eq!(after.permissions().mode(), before.permissions().mode());
    assert_eq!(after.modified().unwrap(), before.modified().unwrap());
    assert_eq!(fs::read(fixture.source()).unwrap(), original);
    assert_eq!(
        fs::read_to_string(nested.join("notes.txt")).unwrap(),
        "source content"
    );
}

#[test]
fn missing_receiver_utilities_block_writes_to_fresh_and_existing_replicas() {
    for utility in ["cmp", "mkdir"] {
        let fixture = Fixture::new();
        let remote_utility = fixture.root.path().join("receiver-bin").join(utility);
        fs::remove_file(&remote_utility).unwrap();
        for args in [&["--hosts", "dev", "--check"][..], &["--hosts", "dev"][..]] {
            fixture.report(&fixture.leader, args, 1);
            assert!(!fixture.follower.join(".skillator").exists(), "{utility}");
            fixture.assert_exports_cleaned();
        }
        symlink(executable(utility), &remote_utility).unwrap();
        fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
        fs::write(
            fixture.received(&fixture.follower),
            "preserved follower edit",
        )
        .unwrap();
        fs::remove_file(remote_utility).unwrap();
        for args in [&["--hosts", "dev", "--check"][..], &["--hosts", "dev"][..]] {
            fixture.report(&fixture.leader, args, 1);
            assert_eq!(
                fs::read_to_string(fixture.received(&fixture.follower)).unwrap(),
                "preserved follower edit",
                "{utility}"
            );
            fixture.assert_exports_cleaned();
        }
    }
}

#[test]
fn missing_rollback_utilities_block_fresh_replica_creation() {
    for utility in ["rm", "rmdir"] {
        let fixture = Fixture::new();
        fs::remove_file(fixture.root.path().join("receiver-bin").join(utility)).unwrap();
        fixture.report(&fixture.leader, &["--hosts", "dev"], 1);
        assert!(!fixture.follower.join(".skillator").exists(), "{utility}");
        fixture.assert_exports_cleaned();
    }
}

#[test]
fn unsupported_receiver_find_predicate_blocks_fresh_replica_writes() {
    let fixture = Fixture::new();
    let find = fixture.root.path().join("receiver-bin/find");
    fs::remove_file(&find).unwrap();
    fs::write(
        &find,
        format!(
            "#!/bin/sh\nfor argument do\n if [ \"$argument\" = -links ]; then printf 'unsupported find predicate\\n' >&2; exit 2; fi\ndone\nexec {} \"$@\"\n",
            quote(&executable("find")),
        ),
    )
    .unwrap();
    fs::set_permissions(&find, fs::Permissions::from_mode(0o755)).unwrap();
    let original = fs::read(fixture.source()).unwrap();
    for args in [&["--hosts", "dev", "--check"][..], &["--hosts", "dev"][..]] {
        let report = fixture.report(&fixture.leader, args, 1);
        assert_eq!(report["changes"][0]["outcome"], "failed");
        assert!(!fixture.follower.join(".skillator").exists());
        assert_eq!(fs::read(fixture.source()).unwrap(), original);
        fixture.assert_exports_cleaned();
    }
    fs::remove_file(&find).unwrap();
    symlink(executable("find"), &find).unwrap();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
    assert_eq!(
        fs::read(fixture.received(&fixture.follower)).unwrap(),
        original
    );
    fixture.report(&fixture.leader, &["--hosts", "dev", "--check"], 0);
    fixture.assert_exports_cleaned();
}

#[test]
fn rsync_ambiguous_destinations_fail_before_connection_or_replica_writes() {
    for destination in [
        "local:prod",
        "user@local:prod",
        "host::module",
        "host:22",
        "::1",
        "[::1]:22",
        "[invalid]",
        "user@-host",
    ] {
        let fixture = Fixture::new();
        let adapter = fixture.root.path().join("bin/ssh");
        let contacted = fixture.root.path().join("unexpected-connection");
        fs::write(
            &adapter,
            format!(
                "#!/bin/sh\nprintf connected > {}\nexit 99\n",
                quote(&contacted)
            ),
        )
        .unwrap();
        for role in ["hosts:\n  dev", "leader"] {
            fs::write(
                fixture.leader.join(".skillator/config.yaml"),
                format!("version: 1\n{role}: {{destination: '{destination}'}}\n"),
            )
            .unwrap();
            fixture
                .command(&fixture.leader)
                .args(["library", "rsync", "--format", "json"])
                .assert()
                .code(3)
                .stdout("");
            assert!(!contacted.exists(), "{destination}");
            assert!(!fixture.follower.join(REPLICA).exists(), "{destination}");
            fixture.assert_exports_cleaned();
        }
    }
}

#[test]
fn excluded_skill_metadata_blocks_push_and_pull_without_replacing_replica() {
    let fixture = Fixture::new();
    fixture.report(&fixture.leader, &["--hosts", "dev"], 0);
    fixture.configure_follower();
    fs::write(
        fixture.received(&fixture.follower),
        "preserved follower content",
    )
    .unwrap();
    fs::write(
        fixture.leader.join(".skillator/library.yaml"),
        "version: 1\nlocations:\n  - path: '~/.skillator/library'\n    exclusions: ['demo/SKILL.md']\n",
    )
    .unwrap();
    for args in [&["--hosts", "dev", "--check"][..], &["--hosts", "dev"][..]] {
        fixture
            .command(&fixture.leader)
            .args(["library", "rsync"])
            .args(args)
            .assert()
            .code(3)
            .stdout("");
    }
    for args in [&["--check"][..], &[][..]] {
        fixture.report(&fixture.follower, args, 1);
    }
    assert_eq!(
        fs::read_to_string(fixture.received(&fixture.follower)).unwrap(),
        "preserved follower content"
    );
    fixture.assert_exports_cleaned();
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_generated_and_physical_paths_fail_without_replica_or_export_writes() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let fixture = Fixture::new();
    let physical_temp = fixture
        .root
        .path()
        .join(OsStr::from_bytes(b"temporary-\xff"));
    fs::create_dir(&physical_temp).unwrap();
    let temp_alias = fixture.root.path().join("temporary-alias");
    symlink(&physical_temp, &temp_alias).unwrap();
    for temporary in [&physical_temp, &temp_alias] {
        for args in [
            &["library", "rsync", "--hosts", "dev"][..],
            &["library", "rsync", "--prepare-export"][..],
        ] {
            fixture
                .command(&fixture.leader)
                .env("TMPDIR", temporary)
                .args(args)
                .assert()
                .code(3)
                .stdout("");
            assert!(!fixture.follower.join(REPLICA).exists());
            assert!(fs::read_dir(&physical_temp).unwrap().next().is_none());
            fixture.assert_exports_cleaned();
        }
    }
    let physical_home = fixture.root.path().join(OsStr::from_bytes(b"home-\xff"));
    fs::create_dir_all(physical_home.join(".skillator")).unwrap();
    fs::write(
        physical_home.join(".skillator/config.yaml"),
        "version: 1\nleader: {destination: leader.internal}\n",
    )
    .unwrap();
    let home_alias = fixture.root.path().join("home-alias");
    symlink(&physical_home, &home_alias).unwrap();
    for home in [&physical_home, &home_alias] {
        for args in [&["--check"][..], &[][..]] {
            fixture.invalid_local_input(home, args);
            assert!(!physical_home.join(".skillator/library").exists());
            fixture.assert_exports_cleaned();
        }
    }
}

#[test]
fn missing_local_ipv6_capability_blocks_both_roles_before_connection_or_writes() {
    let fixture = Fixture::new();
    let bin = fixture.root.path().join("no-ipv6-bin");
    fs::create_dir(&bin).unwrap();
    let rsync = bin.join("rsync");
    fs::write(
        &rsync,
        format!(
            "#!/bin/sh\ncase \"$1\" in --version) printf 'Capabilities:\\n  no IPv6, symlinks\\n'; exit 0;; esac\nexec {} \"$@\"\n",
            quote(&executable("rsync"))
        ),
    )
    .unwrap();
    fs::set_permissions(&rsync, fs::Permissions::from_mode(0o755)).unwrap();
    let contacted = fixture.root.path().join("unexpected-ipv6-connection");
    fs::write(
        fixture.root.path().join("bin/ssh"),
        format!(
            "#!/bin/sh\nprintf connected > {}\nexit 99\n",
            quote(&contacted)
        ),
    )
    .unwrap();
    for role in ["hosts:\n  dev", "leader"] {
        fs::write(
            fixture.leader.join(".skillator/config.yaml"),
            format!("version: 1\n{role}: {{destination: '[::1]'}}\n"),
        )
        .unwrap();
        for args in [&["--check"][..], &[][..]] {
            fixture
                .command(&fixture.leader)
                .env("PATH", format!("{}:{}", bin.display(), fixture.path))
                .args(["library", "rsync"])
                .args(args)
                .assert()
                .code(3)
                .stdout("");
            assert!(!contacted.exists());
            assert!(!fixture.follower.join(REPLICA).exists());
            fixture.assert_exports_cleaned();
        }
    }
}
