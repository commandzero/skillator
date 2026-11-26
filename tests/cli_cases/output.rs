//! Output behavior scenarios.

use super::*;

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
                .and(predicate::str::contains("targets"))
                .and(predicate::str::contains("user"))
                .and(predicate::str::contains("hook"))
                .and(predicate::str::contains("\n  worktree").not()),
        )
        .stderr(predicate::str::is_empty());
    assert_subcommand_help();
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
        .stdout(predicate::str::contains(concat!(
            "skillator ",
            env!("CARGO_PKG_VERSION")
        )));
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
            "repository configuration is missing",
        ));
}

#[test]
fn text_reports_translate_internal_statuses_without_changing_machine_fields() {
    use skillator::app::{CommandReport, ReportChange, ReportOutcome, ReportStatus};
    use skillator::cli::{ColorPolicy, render_text};

    let outcomes = [
        (ReportOutcome::WouldApply, "Would change"),
        (ReportOutcome::WouldRequireForce, "Needs --force"),
        (ReportOutcome::Applied, "Saved"),
        (ReportOutcome::NotAuthorized, "Skipped: needs confirmation"),
        (ReportOutcome::Blocked, "Cannot change"),
        (ReportOutcome::Failed, "Failed"),
        (ReportOutcome::RolledBack, "Failed; original restored"),
        (ReportOutcome::RecoveryRequired, "Manual recovery needed"),
    ];
    for (outcome, label) in outcomes {
        let report = CommandReport {
            format_version: 1,
            status: ReportStatus::NotConverged,
            exit_status: 1,
            mode: "check".to_owned(),
            target: "/repo".to_owned(),
            changes: vec![ReportChange {
                path: ".agents/.gitignore".to_owned(),
                action: "write_control_file".to_owned(),
                safety: "guarded".to_owned(),
                outcome,
            }],
            diagnostics: vec![],
        };
        for color in [ColorPolicy::Never, ColorPolicy::Always] {
            let text = render_text(&report, color);
            assert!(text.contains(label), "{text}");
            assert!(text.contains("write .gitignore"), "{text}");
            assert!(!text.contains("guarded"), "{text}");
            assert!(!text.contains("write_control_file"), "{text}");
        }
        let machine = serde_json::to_value(&report).unwrap();
        assert_eq!(machine["changes"][0]["safety"], "guarded");
        assert_eq!(machine["changes"][0]["action"], "write_control_file");
    }
}

fn assert_subcommand_help() {
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["sync", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("target").and(predicate::str::contains("worktree")));
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["library", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("prune"));
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["target", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"));
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["user", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"));
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["targets", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("list")
                .and(predicate::str::contains("remove"))
                .and(predicate::str::contains("prune")),
        );
    Command::cargo_bin("skillator")
        .unwrap()
        .args(["hook", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("install")
                .and(predicate::str::contains("status"))
                .and(predicate::str::contains("uninstall")),
        );
}
