//! Command-line parsing, dispatch, and report rendering.

use crate::app::{
    AppPaths, CommandReport, ReportStatus, SyncMode, SyncWorkflow, WorktreeSyncWorkflow,
};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};
use crossterm::style::Stylize;
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "skillator",
    version,
    about = "Manage agent Skill links for a Git repository",
    after_help = "Examples:\n  skillator\n  skillator library\n  skillator sync --check --format=json"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(value_name = "DIRECTORY")]
    directory: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Curate Sources and Skills in the user Library.
    Library,
    /// Reconcile a Target Repository without opening the TUI.
    Sync(SyncArgs),
    /// Project the primary worktree's local Target state into a linked worktree.
    Worktree {
        #[command(subcommand)]
        command: WorktreeCommand,
    },
}

#[derive(Debug, Subcommand)]
enum WorktreeCommand {
    /// Synchronize the primary worktree's local Target configuration.
    Sync(SyncArgs),
}

#[derive(Debug, clap::Args)]
struct SyncArgs {
    /// Report required changes without writing.
    #[arg(long, conflicts_with = "force")]
    check: bool,
    /// Authorize every viable Guarded Change.
    #[arg(long)]
    force: bool,
    /// Select the report encoding.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    /// Select text color behavior.
    #[arg(long, value_enum)]
    color: Option<ColorPolicy>,
    #[arg(value_name = "DIRECTORY")]
    directory: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Yaml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorPolicy {
    Auto,
    Always,
    Never,
}

pub fn run() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let code = error.exit_code() as u8;
            let _ = error.print();
            return ExitCode::from(code);
        }
    };
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return diagnostic(
            5,
            "HOME is not set; cannot locate ~/.skillator/library.yaml",
        );
    };
    let paths = AppPaths::new(home);
    match cli.command {
        Some(Commands::Sync(arguments)) => run_sync(&paths, arguments),
        Some(Commands::Worktree {
            command: WorktreeCommand::Sync(arguments),
        }) => run_worktree_sync(&paths, arguments),
        Some(Commands::Library) => {
            if !interactive_terminal() {
                return diagnostic(3, "skillator library requires an interactive terminal");
            }
            match crate::tui::run_library(&paths) {
                Ok(status) => ExitCode::from(status),
                Err(error) => diagnostic(error.exit_status(), &error.to_string()),
            }
        }
        None => {
            if !interactive_terminal() {
                return diagnostic(
                    3,
                    "skillator requires an interactive terminal for the Target TUI",
                );
            }
            let directory = cli.directory.unwrap_or_else(|| PathBuf::from("."));
            match crate::tui::run_target(&paths, &directory) {
                Ok(status) => ExitCode::from(status),
                Err(error) => diagnostic(error.exit_status(), &error.to_string()),
            }
        }
    }
}

fn run_sync(paths: &AppPaths, arguments: SyncArgs) -> ExitCode {
    if arguments.format != OutputFormat::Text && arguments.color.is_some() {
        let mut command = Cli::command();
        let error = command.error(
            ErrorKind::ArgumentConflict,
            "--color cannot be used with --format=json or --format=yaml",
        );
        let _ = error.print();
        return ExitCode::from(2);
    }
    let directory = arguments
        .directory
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    let mode = if arguments.check {
        SyncMode::Check
    } else {
        SyncMode::Apply {
            force: arguments.force,
        }
    };
    let report = match SyncWorkflow::run(paths, &directory, mode) {
        Ok(report) => report,
        Err(error) => return diagnostic(error.exit_status(), &error.to_string()),
    };
    render_report(arguments, report)
}

fn run_worktree_sync(paths: &AppPaths, arguments: SyncArgs) -> ExitCode {
    if arguments.format != OutputFormat::Text && arguments.color.is_some() {
        let mut command = Cli::command();
        let error = command.error(
            ErrorKind::ArgumentConflict,
            "--color cannot be used with --format=json or --format=yaml",
        );
        let _ = error.print();
        return ExitCode::from(2);
    }
    let directory = arguments
        .directory
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    let mode = if arguments.check {
        SyncMode::Check
    } else {
        SyncMode::Apply {
            force: arguments.force,
        }
    };
    let report = match WorktreeSyncWorkflow::run(paths, &directory, mode) {
        Ok(report) => report,
        Err(error) => return diagnostic(error.exit_status(), &error.to_string()),
    };
    render_report(arguments, report)
}

fn render_report(arguments: SyncArgs, report: CommandReport) -> ExitCode {
    let rendered = match arguments.format {
        OutputFormat::Text => Ok(render_text(
            &report,
            arguments.color.unwrap_or(ColorPolicy::Auto),
        )),
        OutputFormat::Json => serde_json::to_string_pretty(&report)
            .map(|mut value| {
                value.push('\n');
                value
            })
            .map_err(|error| error.to_string()),
        OutputFormat::Yaml => render_yaml(&report),
    };
    let output = match rendered {
        Ok(output) => output,
        Err(error) => return diagnostic(5, &format!("cannot render report: {error}")),
    };
    if let Err(error) = std::io::stdout().lock().write_all(output.as_bytes()) {
        return diagnostic(5, &format!("cannot write report: {error}"));
    }
    ExitCode::from(report.exit_status)
}

fn interactive_terminal() -> bool {
    is_terminal::is_terminal(std::io::stdin()) && is_terminal::is_terminal(std::io::stdout())
}

fn diagnostic(code: u8, message: &str) -> ExitCode {
    let _ = writeln!(std::io::stderr().lock(), "skillator: {message}");
    ExitCode::from(code)
}

pub fn render_text(report: &CommandReport, color: ColorPolicy) -> String {
    let color = match color {
        ColorPolicy::Always => true,
        ColorPolicy::Never => false,
        ColorPolicy::Auto => {
            is_terminal::is_terminal(std::io::stdout())
                && std::env::var_os("TERM").is_none_or(|term| term != "dumb")
                && std::env::var_os("NO_COLOR").is_none()
        }
    };
    if report.status == ReportStatus::InSync
        && report.changes.is_empty()
        && report.diagnostics.is_empty()
    {
        return "In sync.\n".to_owned();
    }
    let mut output = String::new();
    if report.status == ReportStatus::InSync {
        output.push_str("In sync.\n");
    }
    for change in &report.changes {
        let marker = match change.outcome {
            crate::app::ReportOutcome::WouldApply => "would_apply",
            crate::app::ReportOutcome::WouldRequireForce => "would_require_force",
            crate::app::ReportOutcome::Applied => "applied",
            crate::app::ReportOutcome::NotAuthorized => "not_authorized",
            crate::app::ReportOutcome::Blocked => "blocked",
            crate::app::ReportOutcome::Failed => "failed",
            crate::app::ReportOutcome::RolledBack => "rolled_back",
            crate::app::ReportOutcome::RecoveryRequired => "recovery_required",
        };
        if color {
            output.push_str(&format!(
                "{} {} ({}, {})\n",
                marker.cyan(),
                change.path,
                change.action,
                change.safety
            ));
        } else {
            output.push_str(&format!(
                "{marker} {} ({}, {})\n",
                change.path, change.action, change.safety
            ));
        }
    }
    for diagnostic in &report.diagnostics {
        if color {
            output.push_str(&format!(
                "{}: {}\n",
                diagnostic.severity.as_str().yellow(),
                diagnostic.message
            ));
        } else {
            output.push_str(&format!(
                "{}: {}\n",
                diagnostic.severity, diagnostic.message
            ));
        }
    }
    output
}

pub fn render_yaml(report: &CommandReport) -> Result<String, String> {
    let value = serde_json::to_value(report).map_err(|error| error.to_string())?;
    let mut output = String::from("---\n");
    emit_yaml(&value, 0, &mut output)?;
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

fn emit_yaml(value: &Value, indent: usize, output: &mut String) -> Result<(), String> {
    match value {
        Value::Object(values) => {
            if values.is_empty() {
                output.push_str(&" ".repeat(indent));
                output.push_str("{}\n");
            } else {
                for (key, value) in values {
                    output.push_str(&" ".repeat(indent));
                    output
                        .push_str(&serde_json::to_string(key).map_err(|error| error.to_string())?);
                    output.push(':');
                    if scalar(value) || empty_collection(value) {
                        output.push(' ');
                        emit_scalar(value, output)?;
                        output.push('\n');
                    } else {
                        output.push('\n');
                        emit_yaml(value, indent + 2, output)?;
                    }
                }
            }
        }
        Value::Array(values) => {
            if values.is_empty() {
                output.push_str(&" ".repeat(indent));
                output.push_str("[]\n");
            } else {
                for value in values {
                    output.push_str(&" ".repeat(indent));
                    output.push('-');
                    if scalar(value) || empty_collection(value) {
                        output.push(' ');
                        emit_scalar(value, output)?;
                        output.push('\n');
                    } else {
                        output.push('\n');
                        emit_yaml(value, indent + 2, output)?;
                    }
                }
            }
        }
        _ => {
            output.push_str(&" ".repeat(indent));
            emit_scalar(value, output)?;
            output.push('\n');
        }
    }
    Ok(())
}

fn scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

fn empty_collection(value: &Value) -> bool {
    matches!(value, Value::Array(values) if values.is_empty())
        || matches!(value, Value::Object(values) if values.is_empty())
}

fn emit_scalar(value: &Value, output: &mut String) -> Result<(), String> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => output.push_str(&value.to_string()),
        Value::String(value) => {
            output.push_str(&serde_json::to_string(value).map_err(|error| error.to_string())?)
        }
        Value::Array(values) if values.is_empty() => output.push_str("[]"),
        Value::Object(values) if values.is_empty() => output.push_str("{}"),
        _ => return Err("non-scalar value reached scalar emitter".to_owned()),
    }
    Ok(())
}
