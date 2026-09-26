use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;

#[test]
fn policy_options_are_explicit_and_invalid_arguments_are_rejected() {
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["library", "rsync", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--hosts")
                .and(predicate::str::contains("--missing"))
                .and(predicate::str::contains("--conflict")),
        );
    for args in [
        vec!["--missing", "delete"],
        vec!["--conflict", "newest"],
        vec!["--force"],
        vec!["--format", "json", "--color", "never"],
    ] {
        Command::cargo_bin("skillator")
            .unwrap()
            .args(["library", "rsync"])
            .args(args)
            .assert()
            .code(2)
            .stdout("");
    }
}

#[test]
fn required_host_configuration_errors_write_nothing() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("skillator")
        .unwrap()
        .env("HOME", home.path())
        .args(["library", "rsync"])
        .assert()
        .code(3)
        .stdout("");
    assert!(fs::read_dir(home.path()).unwrap().next().is_none());
    fs::create_dir(home.path().join(".skillator")).unwrap();
    fs::write(
        home.path().join(".skillator/config.yaml"),
        "version: 1\nhosts: {dev: {destination: dev}}\n",
    )
    .unwrap();
    for aliases in ["", "missing", "dev,", "dev,missing"] {
        Command::cargo_bin("skillator")
            .unwrap()
            .env("HOME", home.path())
            .args(["library", "rsync", "--hosts", aliases])
            .assert()
            .code(2)
            .stdout("");
    }
    assert_eq!(
        fs::read_dir(home.path().join(".skillator"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn remote_protocol_rejects_mutation_before_preflight_and_unknown_fields() {
    let home = tempfile::tempdir().unwrap();
    for request in [
        "{\"command\":\"begin\",\"token\":\"untrusted\"}\n",
        "{\"command\":\"inspect\",\"sources\":[],\"extra\":true}\n",
        "{\"command\":\"publish\",\"path\":\"../escape\",\"expected\":null,\"desired\":null,\"stage\":null}\n",
    ] {
        let output = Command::cargo_bin("skillator")
            .unwrap()
            .env("HOME", home.path())
            .arg("__rsync")
            .write_stdin(request)
            .assert()
            .success()
            .stderr("")
            .get_output()
            .stdout
            .clone();
        let value: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(value["result"], "error");
        assert!(fs::read_dir(home.path()).unwrap().next().is_none());
    }
}

#[test]
fn protocol_observation_is_framed_and_write_free() {
    let home = tempfile::tempdir().unwrap();
    let output = Command::cargo_bin("skillator")
        .unwrap()
        .env("HOME", home.path())
        .arg("__rsync")
        .write_stdin("{\"command\":\"inspect\",\"sources\":[]}\n")
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["result"], "snapshot");
    assert_eq!(value["snapshot"]["protocol"], 4);
    assert!(fs::read_dir(home.path()).unwrap().next().is_none());
}

#[test]
fn missing_dependencies_fail_before_remote_writes() {
    let home = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    for dependency in ["Git", "rsync"] {
        if dependency == "rsync" {
            std::os::unix::fs::symlink("/usr/bin/git", bin.path().join("git")).unwrap();
        }
        let output = Command::cargo_bin("skillator")
            .unwrap()
            .env("HOME", home.path())
            .env("PATH", bin.path())
            .arg("__rsync")
            .write_stdin("{\"command\":\"inspect\",\"sources\":[]}\n")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let value: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(value["result"], "error");
        assert!(value["message"].as_str().unwrap().contains(dependency));
        assert!(fs::read_dir(home.path()).unwrap().next().is_none());
    }
}

#[test]
fn machine_previews_are_equivalent_and_global_preflight_is_write_free() {
    use std::os::unix::fs::PermissionsExt;
    let local = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    fs::create_dir_all(local.path().join(".skillator/library/demo")).unwrap();
    fs::write(
        local.path().join(".skillator/config.yaml"),
        "version: 1\nhosts: {a: {destination: a}, b: {destination: b}}\n",
    )
    .unwrap();
    fs::write(
        local.path().join(".skillator/library.yaml"),
        "version: 1\nlocations: [{path: '~/.skillator/library'}]\n",
    )
    .unwrap();
    fs::write(
        local.path().join(".skillator/library/demo/SKILL.md"),
        "---\nname: demo\ndescription: Preview test\n---\n",
    )
    .unwrap();
    let binary = assert_cmd::cargo::cargo_bin("skillator");
    let script = bin.path().join("ssh");
    // A process adapter exercises the framed CLI without claiming real SSH coverage.
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nexec env HOME='{}' '{}' __rsync\n",
            remote.path().display(),
            binary.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap()
    );
    let invoke = |format: &str| {
        Command::cargo_bin("skillator")
            .unwrap()
            .env("HOME", local.path())
            .env("PATH", &path)
            .args([
                "library", "rsync", "--hosts", "a", "--check", "--format", format,
            ])
            .assert()
            .code(1)
            .stderr("")
            .get_output()
            .stdout
            .clone()
    };
    let json: Value = serde_json::from_slice(&invoke("json")).unwrap();
    let yaml: Value = serde_saphyr::from_slice(&invoke("yaml")).unwrap();
    assert_eq!(json, yaml);
    for (color, ansi) in [("always", true), ("never", false), ("auto", false)] {
        let output = Command::cargo_bin("skillator")
            .unwrap()
            .env("HOME", local.path())
            .env("PATH", &path)
            .env("TERM", "xterm")
            .env_remove("NO_COLOR")
            .args([
                "library", "rsync", "--hosts", "a", "--check", "--color", color,
            ])
            .assert()
            .code(1)
            .stderr("")
            .get_output()
            .stdout
            .clone();
        assert_eq!(output.contains(&0x1b), ansi, "color={color}");
    }
    let output = Command::cargo_bin("skillator")
        .unwrap()
        .env("HOME", local.path())
        .env("PATH", &path)
        .env("NO_COLOR", "1")
        .args(["library", "rsync", "--hosts", "a", "--check"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    assert!(!output.contains(&0x1b));
    assert_eq!(json["mode"], "library_rsync");
    assert!(
        json["changes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|change| change["host"] == "a" && change["outcome"] == "would_apply")
    );
    assert!(fs::read_dir(remote.path()).unwrap().next().is_none());
    assert!(!local.path().join(".skillator/rsync").exists());
    fs::write(&script, format!("#!/bin/sh\nfor arg do if [ \"$arg\" = b ]; then printf '%s\\n' '{{\"result\":\"error\",\"code\":3,\"message\":\"Skillator is not installed on remote host\"}}'; exit; fi; done\nexec env HOME='{}' '{}' __rsync\n", remote.path().display(), binary.display())).unwrap();
    Command::cargo_bin("skillator")
        .unwrap()
        .env("HOME", local.path())
        .env("PATH", &path)
        .args(["library", "rsync", "--format", "json"])
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicate::str::contains("remote host b"));
    assert!(fs::read_dir(remote.path()).unwrap().next().is_none());
    assert!(!local.path().join(".skillator/rsync").exists());
}
