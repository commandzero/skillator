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

    fn problem(&mut self, host: Option<&str>, code: &str, message: &str) {
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

    fn transfer(&mut self, host: &str, action: &'static str, check: bool, result: Result<bool>) {
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
                eprintln!("{host}: {error}");
                self.record(host, action, ReportOutcome::Failed);
                self.problem(
                    Some(host),
                    "library_transfer_failed",
                    "Skill delivery failed; see stderr for details.",
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
    if let Some(leader) = config.leader() {
        if options.hosts.is_some() {
            return Err(Error::argument(
                "--hosts selects followers on a leader; it cannot be used on a follower",
            ));
        }
        transport::check_rsync()?;
        pull(paths, &leader.destination, options.check, &mut report);
    } else {
        let followers = config.select(options.hosts.as_deref())?;
        transport::check_rsync()?;
        let export = export::prepare(paths)?;
        for (alias, follower) in followers {
            let result = (|| {
                let replica = transport::remote_replica(&follower.destination, !options.check)?;
                if options.check && !replica.exists {
                    return Ok(true);
                }
                let changed = transport::rsync(
                    &follower.destination,
                    export.path(),
                    &replica.path,
                    Direction::Push,
                    options.check,
                )?;
                Ok(changed || replica.created)
            })();
            report.transfer(alias, "push", options.check, result);
        }
        let export = export.keep();
        if let Err(error) = std::fs::remove_dir_all(&export) {
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
    Ok(report)
}

fn pull(paths: &AppPaths, destination: &str, check: bool, report: &mut Report) {
    let replica = match transport::local_replica(paths.home(), false) {
        Ok(replica) => replica,
        Err(error) => {
            report.transfer("leader", "pull", check, Err(error));
            return;
        }
    };
    let export = match transport::remote_export(destination) {
        Ok(export) => export,
        Err(error) => {
            report.transfer("leader", "pull", check, Err(error));
            return;
        }
    };
    let result = (|| {
        if check && !replica.exists {
            return Ok(true);
        }
        let replica = transport::local_replica(paths.home(), !check)?;
        let changed =
            transport::rsync(destination, &export, &replica.path, Direction::Pull, check)?;
        Ok(changed || replica.created)
    })();
    report.transfer("leader", "pull", check, result);
    if let Err(error) = transport::cleanup_export(destination, &export) {
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
