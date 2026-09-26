//! Source updates are separate from desired-state and Materialization writes.
use crate::app::{
    AppPaths, CommandReport, ReportChange, ReportDiagnostic, ReportOutcome, ReportStatus,
    WorkflowError,
};
use crate::config::{LibraryConfig, LoadResult, load_library};
use crate::library::{SourceKind, scan_library};
use crate::update_process::{self, Completion, InterruptGuard};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "/usr/bin/false")
        .env("SSH_ASKPASS", "/usr/bin/false")
        .env("GIT_EDITOR", "/usr/bin/false")
        .env("GIT_SEQUENCE_EDITOR", "/usr/bin/false")
        .stdin(Stdio::null());
    // A caller's repository-selection environment must not redirect this update.
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_PREFIX",
    ] {
        command.env_remove(key);
    }
    command
}

// OpenSSH takes the first value for each option. Insert enforced arguments
// immediately after the executable, retaining its shell quoting and all the
// configured arguments verbatim. Transparent helpers receive the same options.
fn noninteractive_ssh(configured: &str) -> String {
    let configured = configured.trim_start();
    let mut quote = None;
    let mut escaped = false;
    let mut end = configured.len();
    for (index, character) in configured.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
        } else if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
        } else if character == '\'' || character == '"' {
            quote = Some(character);
        } else if character.is_ascii_whitespace() {
            end = index;
            break;
        }
    }
    format!(
        "{} -oBatchMode=yes -oNumberOfPasswordPrompts=0{}",
        &configured[..end],
        &configured[end..]
    )
}

fn query(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = git(root).args(args).output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(clean(&output.stderr));
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim_end_matches('\n').to_owned())
        .map_err(|e| e.to_string())
}

// Drop escape sequences and control characters, including OSC sequences.
fn clean(bytes: &[u8]) -> String {
    let input = String::from_utf8_lossy(bytes);
    let mut chars = input.chars().peekable();
    let mut text = String::new();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' || (c == '\u{1b}' && chars.next() == Some('\\')) {
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if !c.is_control() || c == '\n' || c == '\t' {
            text.push(c);
        }
    }
    text.trim().to_owned()
}

#[derive(PartialEq, Eq)]
struct Identity {
    root: (u64, u64),
    git_dir: PathBuf,
    git_inode: (u64, u64),
}

fn identity(root: &Path) -> Result<Identity, String> {
    let resolved = root.canonicalize().map_err(|e| e.to_string())?;
    if resolved != root {
        return Err("checkout path changed".into());
    }
    let actual = PathBuf::from(query(root, &["rev-parse", "--show-toplevel"])?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if actual != root {
        return Err("path is no longer the selected checkout root".into());
    }
    let git_dir = PathBuf::from(query(root, &["rev-parse", "--absolute-git-dir"])?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root_meta = fs::metadata(root).map_err(|e| e.to_string())?;
    let git_meta = fs::metadata(&git_dir).map_err(|e| e.to_string())?;
    Ok(Identity {
        root: (root_meta.dev(), root_meta.ino()),
        git_inode: (git_meta.dev(), git_meta.ino()),
        git_dir,
    })
}

fn eligible(root: &Path, expected: &Identity) -> Result<String, (&'static str, String)> {
    if identity(root).as_ref() != Ok(expected) {
        return Err((
            "checkout_changed",
            "checkout identity changed after discovery".into(),
        ));
    }
    for operation in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "rebase-merge",
        "rebase-apply",
        "sequencer",
    ] {
        if expected
            .git_dir
            .join(operation)
            .try_exists()
            .map_err(|e| ("inspection_failed", e.to_string()))?
        {
            return Err((
                "operation_in_progress",
                "finish the current Git operation before updating".into(),
            ));
        }
    }
    let reference = query(root, &["symbolic-ref", "--quiet", "HEAD"])
        .map_err(|_| ("detached_head", "checkout has detached HEAD".into()))?;
    // --short can return heads/main when a tag also claims main. Configuration
    // keys always use the branch name relative to refs/heads/.
    let branch = reference.strip_prefix("refs/heads/").ok_or_else(|| {
        (
            "inspection_failed",
            "HEAD does not identify a local branch".into(),
        )
    })?;
    // Read configuration rather than requiring the cached remote ref to exist.
    for suffix in ["remote", "merge"] {
        let key = format!("branch.{branch}.{suffix}");
        let value = query(root, &["config", "--get", &key]).unwrap_or_default();
        if value.is_empty() {
            return Err((
                "missing_upstream",
                "branch has no configured upstream".into(),
            ));
        }
    }
    let status = query(
        root,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ],
    )
    .map_err(|e| ("inspection_failed", e))?;
    if !status.is_empty() {
        return Err((
            "dirty_checkout",
            "checkout has staged, unstaged, or untracked changes".into(),
        ));
    }
    query(root, &["rev-parse", "--verify", "HEAD"]).map_err(|e| ("inspection_failed", e))
}

fn diagnostic(
    report: &mut CommandReport,
    code: &str,
    path: &Path,
    message: impl AsRef<str>,
    error: bool,
) {
    report.diagnostics.push(ReportDiagnostic {
        code: code.into(),
        severity: if error { "error" } else { "warning" }.into(),
        message: clean(format!("{}: {}", path.display(), message.as_ref()).as_bytes()),
        data: Some(BTreeMap::from([(
            "path".into(),
            clean(path.to_string_lossy().as_bytes()),
        )])),
    });
    if error {
        report.exit_status = 1;
    }
}

fn fatal(message: impl ToString) -> WorkflowError {
    WorkflowError::Fatal {
        message: message.to_string(),
    }
}

pub(crate) fn run(
    paths: &AppPaths,
    check: bool,
    timeout: Duration,
) -> Result<CommandReport, WorkflowError> {
    let config =
        match load_library(&paths.library_config()).map_err(|e| WorkflowError::InvalidInput {
            message: e.to_string(),
        })? {
            LoadResult::Missing => LibraryConfig::empty(),
            LoadResult::Valid(loaded) => loaded.value().clone(),
            LoadResult::Unsupported { version, .. } => {
                return Err(WorkflowError::InvalidInput {
                    message: format!("unsupported Library configuration version {version}"),
                });
            }
            LoadResult::Invalid { issues } => {
                return Err(WorkflowError::InvalidInput {
                    message: format!(
                        "invalid Library configuration: {}",
                        issues
                            .iter()
                            .map(|issue| format!("{}: {}", issue.path, issue.message))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ),
                });
            }
        };
    let version: Output = git(paths.home()).arg("--version").output().map_err(fatal)?;
    if !version.status.success() {
        return Err(fatal("cannot execute Git"));
    }
    let interrupt = InterruptGuard::install().map_err(fatal)?;
    let snapshot = scan_library(
        &config,
        &paths.library_config(),
        paths.home(),
        paths.environment(),
    );
    let mut report = CommandReport {
        format_version: 1,
        status: ReportStatus::InSync,
        exit_status: 0,
        mode: if check {
            "library_update_check"
        } else {
            "library_update"
        }
        .into(),
        target: paths.library_config().display().to_string(),
        changes: vec![],
        diagnostics: vec![],
    };
    for d in snapshot.diagnostics() {
        let error = !matches!(
            d.code,
            "source_key_collision" | "overlapping_locations_allowed"
        );
        diagnostic(
            &mut report,
            if d.code == "discovery_failed" {
                "discovery_incomplete"
            } else {
                d.code
            },
            d.path.as_deref().unwrap_or(&paths.library_config()),
            &d.message,
            error,
        );
    }
    let roots: BTreeSet<_> = snapshot
        .sources()
        .filter(|s| s.kind() == SourceKind::Git)
        .filter_map(|s| s.root().map(Path::to_owned))
        .collect();
    let mut plan = Vec::new();
    for root in roots {
        let expected = match query(&root, &["rev-parse", "--show-superproject-working-tree"]) {
            Ok(superproject) if !superproject.is_empty() => {
                diagnostic(
                    &mut report,
                    "submodule_skipped",
                    &root,
                    "submodule pull skipped; manage its checkout through Git",
                    false,
                );
                continue;
            }
            Ok(_) => identity(&root),
            Err(error) => Err(error),
        };
        plan.push((expected, root));
    }
    for (expected, root) in plan {
        let mut row = ReportChange {
            path: clean(root.to_string_lossy().as_bytes()),
            action: "pull".into(),
            safety: "safe".into(),
            outcome: ReportOutcome::Blocked,
        };
        if interrupt.cancelled() {
            diagnostic(
                &mut report,
                "cancelled",
                &root,
                "pull not attempted; batch cancelled",
                true,
            );
            row.safety = "blocked".into();
        } else {
            let eligible = expected
                .map_err(|e| ("inspection_failed", e))
                .and_then(|id| eligible(&root, &id));
            match eligible {
                Err((code, message)) => {
                    row.safety = "blocked".into();
                    diagnostic(&mut report, code, &root, message, true);
                }
                Ok(_) if check => {
                    row.outcome = ReportOutcome::WouldApply;
                    diagnostic(
                        &mut report,
                        "remote_state_not_checked",
                        &root,
                        "Would attempt pull; remote state not checked.",
                        false,
                    );
                    report.exit_status = 1;
                }
                Ok(before) => {
                    let mut command = git(&root);
                    // Preserve custom SSH commands, adding OpenSSH batch behavior.
                    let ssh = std::env::var("GIT_SSH_COMMAND")
                        .ok()
                        .or_else(|| query(&root, &["config", "--get", "core.sshCommand"]).ok())
                        .unwrap_or_else(|| "ssh".into());
                    command.env("GIT_SSH_COMMAND", noninteractive_ssh(&ssh));
                    command.args([
                        "-c",
                        "core.askPass=/usr/bin/false",
                        "-c",
                        "credential.interactive=false",
                        "-c",
                        "pull.rebase=false",
                        "-c",
                        "merge.autoStash=false",
                        "-c",
                        "rebase.autoStash=false",
                        "-c",
                        "pull.autoStash=false",
                        "pull",
                        "--ff-only",
                        "--no-rebase",
                        "--no-autostash",
                        "--no-recurse-submodules",
                    ]);
                    row.outcome = ReportOutcome::Failed;
                    match update_process::capture(&mut command, timeout, &interrupt) {
                        Ok(output) => match output.completion {
                            Completion::Exited(status) if status.success() => {
                                match query(&root, &["rev-parse", "--verify", "HEAD"]) {
                                    Ok(after) => {
                                        row.outcome = if before == after {
                                            ReportOutcome::Unchanged
                                        } else {
                                            ReportOutcome::Applied
                                        }
                                    }
                                    Err(error) => diagnostic(
                                        &mut report,
                                        "inspection_failed",
                                        &root,
                                        error,
                                        true,
                                    ),
                                }
                            }
                            Completion::Exited(_) => {
                                let message = if output.stderr.is_empty() {
                                    clean(&output.stdout)
                                } else {
                                    clean(&output.stderr)
                                };
                                diagnostic(&mut report, "pull_failed", &root, message, true);
                            }
                            Completion::Timeout => {
                                diagnostic(
                                    &mut report,
                                    "pull_timeout",
                                    &root,
                                    format!(
                                        "pull exceeded {} seconds; Git state may have changed",
                                        timeout.as_secs()
                                    ),
                                    true,
                                );
                                report
                                    .diagnostics
                                    .last_mut()
                                    .unwrap()
                                    .data
                                    .as_mut()
                                    .unwrap()
                                    .insert(
                                        "timeout_seconds".into(),
                                        timeout.as_secs().to_string(),
                                    );
                            }
                            Completion::Cancelled => diagnostic(
                                &mut report,
                                "cancelled",
                                &root,
                                "pull interrupted; Git state may have changed",
                                true,
                            ),
                        },
                        Err(error) => {
                            diagnostic(&mut report, "pull_failed", &root, error.to_string(), true)
                        }
                    }
                }
            }
        }
        report.changes.push(row);
    }
    if interrupt.cancelled() {
        report.exit_status = 130;
        if !report.diagnostics.iter().any(|d| d.code == "cancelled") {
            diagnostic(
                &mut report,
                "cancelled",
                &paths.library_config(),
                "batch cancelled",
                false,
            );
        }
    }
    report.status = if report.exit_status == 0 {
        ReportStatus::InSync
    } else {
        ReportStatus::NotConverged
    };
    Ok(report)
}

pub(crate) fn render_text(report: &CommandReport, terminal: bool) -> String {
    let mut text = String::new();
    if report.changes.is_empty() {
        text.push_str("No repositories to update.\n");
    }
    let mut unchanged = 0;
    for row in &report.changes {
        let label = match row.outcome {
            ReportOutcome::Unchanged if terminal => "up-to-date",
            ReportOutcome::Unchanged => {
                unchanged += 1;
                continue;
            }
            ReportOutcome::Applied => "Updated",
            ReportOutcome::WouldApply => "Would attempt pull; remote state not checked.",
            ReportOutcome::Blocked => "Blocked",
            _ => "Failed",
        };
        text.push_str(&format!("{label}: {}\n", row.path));
    }
    if unchanged > 0 {
        text.push_str(&format!("{unchanged} repositories unchanged.\n"));
    }
    for d in &report.diagnostics {
        if d.code != "remote_state_not_checked" {
            text.push_str(&format!("{}: {}\n", d.code, d.message));
        }
    }
    text
}
