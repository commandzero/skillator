//! Leader-authoritative skill delivery using ordinary SSH and rsync.

mod config;
mod export;
pub(crate) mod hosts;
// Bound TUI SSH work and stop inherited pipe writers through private process
// groups; unsafe code is confined to the POSIX signal used for cleanup.
#[allow(unsafe_code)]
mod process;
mod transport;

use crate::app::{AppPaths, ReportDiagnostic, ReportOutcome};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use transport::Direction;

pub(super) const MARKER_NAME: &str = ".skillator-rsync-owned";
pub(super) const MARKER_CONTENT: &str = "skillator library rsync replica v1\n";
pub(super) const EXPORT_PREFIX: &str = "skillator-library-export-";
pub(super) const REPLICA_RELATIVE: &str = ".skillator/library/replica";
const REPLICA_DISPLAY: &str = "~/.skillator/library/replica";

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub(crate) struct Error {
    pub code: u8,
    message: String,
}

impl Error {
    fn input(message: impl Into<String>) -> Self {
        Self {
            code: 3,
            message: message.into(),
        }
    }

    fn argument(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }

    fn input_display(error: impl fmt::Display) -> Self {
        Self::input(error.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for Error {}

pub(crate) struct Options {
    pub hosts: Option<String>,
    pub check: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct Report {
    format_version: u8,
    status: &'static str,
    pub exit_status: u8,
    mode: &'static str,
    target: String,
    changes: Vec<Change>,
    diagnostics: Vec<ReportDiagnostic>,
}

#[derive(Debug, Serialize)]
struct Change {
    host: String,
    path: &'static str,
    action: &'static str,
    safety: &'static str,
    outcome: ReportOutcome,
}

impl Report {
    fn new(paths: &AppPaths) -> Self {
        Self {
            format_version: 1,
            status: "in_sync",
            exit_status: 0,
            mode: "library_rsync",
            target: paths.home().to_string_lossy().into_owned(),
            changes: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn record(&mut self, host: &str, action: &'static str, outcome: ReportOutcome) {
        if matches!(outcome, ReportOutcome::WouldApply | ReportOutcome::Failed) {
            self.status = "not_converged";
            self.exit_status = 1;
        }
        self.changes.push(Change {
            host: host.into(),
            path: REPLICA_DISPLAY,
            action,
            safety: "safe",
            outcome,
        });
    }

    fn problem(&mut self, host: Option<&str>, code: &str, message: impl Into<String>) {
        self.status = "not_converged";
        self.exit_status = 1;
        self.diagnostics.push(ReportDiagnostic {
            code: code.into(),
            severity: "error".into(),
            message: message.into(),
            data: host.map(|host| {
                BTreeMap::from([
                    ("host".into(), host.into()),
                    ("path".into(), REPLICA_DISPLAY.into()),
                ])
            }),
        });
    }

    fn transfer(
        &mut self,
        host: &str,
        action: &'static str,
        check: bool,
        result: Result<bool>,
        execution: &Execution<'_>,
    ) {
        match result {
            Ok(false) => {}
            Ok(true) => self.record(
                host,
                action,
                if check {
                    ReportOutcome::WouldApply
                } else {
                    ReportOutcome::Applied
                },
            ),
            Err(error) => {
                if !execution.quiet() {
                    eprintln!("{host}: {error}");
                }
                self.record(host, action, ReportOutcome::Failed);
                self.problem(
                    Some(host),
                    "library_transfer_failed",
                    if execution.quiet() {
                        format!("Skill delivery failed: {error}")
                    } else {
                        "Skill delivery failed; see stderr for details.".to_owned()
                    },
                );
            }
        }
    }

    pub fn text_with_color(&self, color: bool) -> String {
        if self.changes.is_empty() && self.diagnostics.is_empty() {
            return "In sync.\n".into();
        }
        let mut text = String::new();
        for change in &self.changes {
            let verb = match (change.action, change.outcome) {
                ("push", ReportOutcome::WouldApply) => "Would push",
                ("pull", ReportOutcome::WouldApply) => "Would pull",
                ("push", ReportOutcome::Applied) => "Pushed",
                ("pull", ReportOutcome::Applied) => "Pulled",
                ("push", _) => "Failed to push",
                _ => "Failed to pull",
            };
            use std::fmt::Write;
            if color {
                let _ = writeln!(
                    text,
                    "{}: \x1b[36m{}\x1b[0m skills at {}",
                    change.host, verb, change.path
                );
            } else {
                let _ = writeln!(text, "{}: {} skills at {}", change.host, verb, change.path);
            }
        }
        for diagnostic in &self.diagnostics {
            use std::fmt::Write;
            let host = diagnostic
                .data
                .as_ref()
                .and_then(|data| data.get("host"))
                .map_or("leader", String::as_str);
            let _ = writeln!(text, "{host}: {}", diagnostic.message);
        }
        text
    }
}

struct Execution<'a> {
    cancel: Option<&'a AtomicBool>,
}

impl Execution<'_> {
    fn quiet(&self) -> bool {
        self.cancel.is_some()
    }

    fn ensure_running(&self) -> Result<()> {
        if self
            .cancel
            .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
        {
            return Err(Error::input("Skill delivery cancelled"));
        }
        Ok(())
    }

    fn capture(&self, command: &mut Command) -> Result<Vec<u8>> {
        match self.cancel {
            Some(cancel) => process::capture_transfer(command, cancel),
            None => process::capture(command),
        }
    }
}

/// Prepare current leader skills for one follower-initiated pull.
/// The invoking follower owns cleanup of the returned private temporary directory.
pub(crate) fn prepare_export(paths: &AppPaths) -> Result<PathBuf> {
    let config = config::Config::load(paths.home())?;
    if config.leader().is_some() {
        return Err(Error::input(
            "only a configured leader can prepare a skill export",
        ));
    }
    let export = export::prepare(paths)?;
    let path = export.path().canonicalize().map_err(Error::input_display)?;
    let _ = export.keep();
    Ok(path)
}

pub(crate) fn run(paths: &AppPaths, options: Options) -> Result<Report> {
    let config = config::Config::load(paths.home())?;
    let mut report = Report::new(paths);
    let execution = Execution { cancel: None };
    if let Some(leader) = config.leader() {
        if options.hosts.is_some() {
            return Err(Error::argument(
                "--hosts selects followers on a leader; it cannot be used on a follower",
            ));
        }
        transport::check_rsync(&execution)?;
        pull(
            paths,
            &leader.destination,
            options.check,
            &mut report,
            &execution,
        );
    } else {
        let followers = config.select(options.hosts.as_deref())?;
        push(paths, followers, options.check, &mut report, &execution)?;
    }
    Ok(report)
}

/// Deliver to exactly one follower selected from the currently saved leader configuration.
/// The destination guard prevents a queued TUI action from using a changed registration.
pub(crate) fn run_for_tui(
    paths: &AppPaths,
    alias: &str,
    expected_destination: &str,
    cancel: &AtomicBool,
) -> Result<Report> {
    let execution = Execution {
        cancel: Some(cancel),
    };
    execution.ensure_running()?;
    let config = config::Config::load(paths.home())?;
    let followers = config.select(Some(alias))?;
    let [(selected, follower)] = followers.as_slice() else {
        return Err(Error::argument("select exactly one follower for delivery"));
    };
    if *selected != alias {
        return Err(Error::argument(
            "select exactly one follower alias for delivery",
        ));
    }
    if follower.destination != expected_destination {
        return Err(Error::input(format!(
            "saved SSH destination for {selected} changed; select the host again before syncing"
        )));
    }
    let mut report = Report::new(paths);
    push(paths, followers, false, &mut report, &execution)?;
    Ok(report)
}

fn push(
    paths: &AppPaths,
    followers: Vec<(&str, &config::Host)>,
    check: bool,
    report: &mut Report,
    execution: &Execution<'_>,
) -> Result<()> {
    execution.ensure_running()?;
    transport::check_rsync(execution)?;
    execution.ensure_running()?;
    let export = export::prepare(paths)?;
    for (alias, follower) in followers {
        let result = (|| {
            execution.ensure_running()?;
            let replica = transport::remote_replica(&follower.destination, !check, execution)?;
            if check && !replica.exists {
                return Ok(true);
            }
            execution.ensure_running()?;
            let changed = transport::rsync(
                &follower.destination,
                export.path(),
                &replica.path,
                Direction::Push,
                check,
                execution,
            )?;
            Ok(changed || replica.created)
        })();
        let cancelled = execution.ensure_running().is_err();
        report.transfer(alias, "push", check, result, execution);
        if cancelled {
            break;
        }
    }
    let export = export.keep();
    if let Err(error) = std::fs::remove_dir_all(&export) {
        if !execution.quiet() {
            eprintln!(
                "leader: could not remove temporary export {}: {error}",
                export.display()
            );
        }
        report.problem(
            None,
            "leader_export_cleanup_failed",
            if execution.quiet() {
                format!(
                    "Could not remove temporary leader export {}: {error}",
                    export.display()
                )
            } else {
                "Could not remove the temporary leader export; see stderr for cleanup details."
                    .to_owned()
            },
        );
    }
    Ok(())
}

fn pull(
    paths: &AppPaths,
    destination: &str,
    check: bool,
    report: &mut Report,
    execution: &Execution<'_>,
) {
    let replica = match transport::local_replica(paths.home(), false) {
        Ok(replica) => replica,
        Err(error) => {
            report.transfer("leader", "pull", check, Err(error), execution);
            return;
        }
    };
    let export = match transport::remote_export(destination, execution) {
        Ok(export) => export,
        Err(error) => {
            report.transfer("leader", "pull", check, Err(error), execution);
            return;
        }
    };
    let result = (|| {
        if check && !replica.exists {
            return Ok(true);
        }
        let replica = transport::local_replica(paths.home(), !check)?;
        let changed = transport::rsync(
            destination,
            &export,
            &replica.path,
            Direction::Pull,
            check,
            execution,
        )?;
        Ok(changed || replica.created)
    })();
    report.transfer("leader", "pull", check, result, execution);
    if let Err(error) = transport::cleanup_export(destination, &export, execution) {
        eprintln!(
            "leader: could not remove temporary export {}: {error}",
            export.display()
        );
        report.problem(
            None,
            "leader_export_cleanup_failed",
            "Could not remove the temporary leader export; see stderr for cleanup details.",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn tui_rejects_changed_saved_destination_before_export_or_network() {
        let home = tempfile::tempdir().unwrap();
        let configuration = home.path().join(".skillator");
        fs::create_dir(&configuration).unwrap();
        fs::write(
            configuration.join("config.yaml"),
            "version: 1\nhosts:\n  dev: {destination: receiver-a.internal}\n",
        )
        .unwrap();
        let paths = AppPaths::new(home.path().to_owned());
        let cancel = AtomicBool::new(false);
        let error = run_for_tui(&paths, "dev", "receiver-b.internal", &cancel).unwrap_err();
        assert!(error.to_string().contains("changed"));
        assert!(!home.path().join(".skillator/library/replica").exists());
        assert_eq!(error.code, 3);
        let error = run_for_tui(&paths, "missing", "receiver-a.internal", &cancel).unwrap_err();
        assert_eq!(error.code, 2);
        fs::write(
            configuration.join("config.yaml"),
            "version: 1\nleader: {destination: authority.internal}\n",
        )
        .unwrap();
        let error = run_for_tui(&paths, "dev", "receiver-a.internal", &cancel).unwrap_err();
        assert_eq!(error.code, 2);
    }

    #[test]
    fn tui_report_exposes_real_subprocess_failure_instead_of_stderr_pointer() {
        let home = tempfile::tempdir().unwrap();
        let mut report = Report::new(&AppPaths::new(home.path().to_owned()));
        let cancel = AtomicBool::new(false);
        let execution = Execution {
            cancel: Some(&cancel),
        };
        let failure = execution.capture(
            Command::new("sh")
                .arg("-c")
                .arg("printf 'receiver storage is full\\n' >&2; exit 17"),
        );
        report.transfer("dev", "push", false, failure.map(|_| false), &execution);
        assert_eq!(report.exit_status, 1);
        let text = report.text_with_color(false);
        assert!(text.contains("receiver storage is full"), "{text}");
        assert!(!text.contains("see stderr"), "{text}");
    }

    #[test]
    fn tui_cancel_cleans_export_after_stopping_blocked_ssh() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        let temporary = fixture.path().join("temporary");
        let bin = fixture.path().join("bin");
        let receiver = fixture.path().join("receiver");
        for path in [&home, &temporary, &bin, &receiver] {
            fs::create_dir(path).unwrap();
        }
        let skill = home.join(".skillator/library/demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            home.join(".skillator/config.yaml"),
            "version: 1\nhosts:\n  dev: {destination: receiver.internal}\n",
        )
        .unwrap();
        fs::write(
            home.join(".skillator/library.yaml"),
            "version: 1\nlocations: [{path: '~/.skillator/library'}]\n",
        )
        .unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo\ndescription: A skill\n---\nleader skill\n",
        )
        .unwrap();
        let adapter = bin.join("ssh");
        fs::write(
            &adapter,
            format!(
                "#!/bin/sh\nset -eu\nwhile [ $# -gt 0 ]; do\n case \"$1\" in -o*) shift;; --) shift; break;; *) break;; esac\ndone\nhost=$1\nshift\n[ \"$host\" = receiver.internal ] || exit 255\nprintf ready > \"$SKILLATOR_TUI_READY\"\nsleep 5\nexport HOME='{}'\nexec /bin/sh -c \"$*\"\n",
                receiver.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&adapter, fs::Permissions::from_mode(0o755)).unwrap();
        let ready = fixture.path().join("ready");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "remote::tests::tui_delivery_cancellation_worker",
            ])
            .env("SKILLATOR_TUI_TEST_HOME", &home)
            .env("SKILLATOR_TUI_TEST_TMPDIR", &temporary)
            .env("SKILLATOR_TUI_READY", &ready)
            .env("HOME", &home)
            .env(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("TMPDIR", &temporary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "run only in the isolated tui cancellation fixture"]
    fn tui_delivery_cancellation_worker() {
        use std::sync::Arc;
        use std::time::{Duration, Instant};
        let home = PathBuf::from(std::env::var_os("SKILLATOR_TUI_TEST_HOME").unwrap());
        let temporary = PathBuf::from(std::env::var_os("SKILLATOR_TUI_TEST_TMPDIR").unwrap());
        let ready = PathBuf::from(std::env::var_os("SKILLATOR_TUI_READY").unwrap());
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker = std::thread::spawn(move || {
            run_for_tui(
                &AppPaths::new(home),
                "dev",
                "receiver.internal",
                &worker_cancel,
            )
        });
        let start = Instant::now();
        while !ready.exists() && start.elapsed() < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.exists(), "SSH did not begin after export");
        cancel.store(true, Ordering::Relaxed);
        let start = Instant::now();
        let report = worker.join().unwrap().unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(report.text_with_color(false).contains("cancelled"));
        assert!(fs::read_dir(temporary).unwrap().next().is_none());
    }
}
