//! Command-line parsing, dispatch, and report rendering.

use crate::app::{
    AppPaths, CommandReport, LibraryInventoryReport, LibraryLocationsReport, LibraryWorkflow,
    ReportStatus, SkillSelector, SyncMode, SyncWorkflow, TargetWorkflow, UserScopeWorkflow,
    WorktreeSyncWorkflow,
};
use crate::domain::MaterializationKind;
use crate::git::GitRepository;
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
    about = "Manage agent skills in your library, user account, and Git repositories",
    after_help = "Examples:\n  skillator\n  skillator library\n  skillator init\n  skillator sync\n  skillator sync target --check --format=json"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(value_name = "DIRECTORY")]
    directory: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Add skill sources and choose which skills appear in Target.
    Library {
        #[command(subcommand)]
        command: Option<LibraryCommand>,
    },
    /// Create .agents/skillator.yaml with no skills enabled.
    Init(InitArgs),
    /// Link, copy, or remove skills in a Git repository.
    Target {
        #[command(subcommand)]
        command: TargetCommand,
    },
    /// Link, copy, or remove skills for the current user.
    User {
        #[command(subcommand)]
        command: UserCommand,
    },
    /// Update installed skills to match saved configuration.
    ///
    /// With no subcommand, Skillator uses worktree sync in a linked worktree
    /// and target sync everywhere else.
    Sync(SyncCommandArgs),
}

#[derive(Debug, Subcommand)]
enum LibraryCommand {
    /// Add a directory of skills to the Library.
    Add {
        location: String,
        #[arg(long)]
        allow_overlap: bool,
        #[command(flatten)]
        output: MutationOutputArgs,
    },
    /// Remove a directory from the Library without deleting it.
    Remove {
        location: String,
        #[command(flatten)]
        output: MutationOutputArgs,
    },
    /// List directories registered with the Library.
    Locations(OutputArgs),
    /// List available skills, optionally filtered by source.
    List {
        filter: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
}

#[derive(Debug, Subcommand)]
enum TargetCommand {
    /// Link one Library skill into a Git repository.
    Link(TargetMutationArgs),
    /// Copy one Library skill into a Git repository.
    Copy(TargetMutationArgs),
    /// Remove one managed skill from a Git repository.
    Remove(TargetMutationArgs),
}

#[derive(Debug, clap::Args)]
struct InitArgs {
    #[arg(value_name = "DIRECTORY")]
    directory: Option<PathBuf>,
    #[command(flatten)]
    output: MutationOutputArgs,
}

#[derive(Debug, Subcommand)]
enum UserCommand {
    /// Link one Library skill for the current user.
    Link(UserMutationArgs),
    /// Copy one Library skill for the current user.
    Copy(UserMutationArgs),
    /// Remove one managed skill for the current user.
    Remove(UserMutationArgs),
}

#[derive(Debug, clap::Args)]
struct TargetMutationArgs {
    #[arg(value_name = "SOURCE:SKILL")]
    selector: String,
    #[arg(long)]
    directory: Option<String>,
    #[arg(value_name = "REPOSITORY")]
    repository: Option<PathBuf>,
    #[command(flatten)]
    output: GuardedMutationOutputArgs,
}

#[derive(Debug, clap::Args)]
struct UserMutationArgs {
    #[arg(value_name = "SOURCE:SKILL")]
    selector: String,
    #[command(flatten)]
    output: GuardedMutationOutputArgs,
}

#[derive(Debug, clap::Args)]
struct OutputArgs {
    /// Choose the output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    /// Control color in text output.
    #[arg(long, value_enum)]
    color: Option<ColorPolicy>,
}

#[derive(Debug, clap::Args)]
struct MutationOutputArgs {
    /// Show what would change without writing files.
    #[arg(long)]
    check: bool,
    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Debug, clap::Args)]
struct GuardedMutationOutputArgs {
    /// Show what would change without writing files.
    #[arg(long, conflicts_with = "force")]
    check: bool,
    /// Allow replacement of files Skillator would otherwise preserve.
    #[arg(long)]
    force: bool,
    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Debug, clap::Args)]
#[command(args_conflicts_with_subcommands = true)]
struct SyncCommandArgs {
    #[command(subcommand)]
    command: Option<SyncCommand>,
    #[command(flatten)]
    output: SyncOutputArgs,
}

#[derive(Debug, Subcommand)]
enum SyncCommand {
    /// Update skills from this checkout's Target configuration.
    Target(SyncArgs),
    /// Copy Target configuration from the primary worktree, then update skills.
    Worktree(SyncArgs),
}

#[derive(Debug, clap::Args)]
struct SyncArgs {
    #[arg(value_name = "DIRECTORY")]
    directory: Option<PathBuf>,
    #[command(flatten)]
    output: SyncOutputArgs,
}

#[derive(Debug, clap::Args)]
struct SyncOutputArgs {
    /// Show what would change without writing files.
    #[arg(long, conflicts_with = "force")]
    check: bool,
    /// Allow replacement of files Skillator would otherwise preserve.
    #[arg(long)]
    force: bool,
    /// Choose the output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    /// Control color in text output.
    #[arg(long, value_enum)]
    color: Option<ColorPolicy>,
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
    if cli.command.is_some()
        && let Some(directory) = cli.directory.as_ref()
    {
        let mut command = Cli::command();
        let error = command.error(
            ErrorKind::UnknownArgument,
            format!(
                "unexpected directory before command: {}",
                directory.display()
            ),
        );
        let code = error.exit_code() as u8;
        let _ = error.print();
        return ExitCode::from(code);
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return diagnostic(
            5,
            "HOME is not set; cannot locate ~/.skillator/library.yaml",
        );
    };
    let paths = AppPaths::new(home);
    match cli.command {
        Some(Commands::Init(arguments)) => run_init(&paths, arguments),
        Some(Commands::Sync(arguments)) => run_sync_command(&paths, arguments),
        Some(Commands::Library { command: None }) => {
            if !interactive_terminal() {
                return diagnostic(3, "skillator library requires an interactive terminal");
            }
            match crate::tui::run_library(&paths) {
                Ok(status) => ExitCode::from(status),
                Err(error) => diagnostic(error.exit_status(), &error.to_string()),
            }
        }
        Some(Commands::Library {
            command: Some(command),
        }) => run_library_command(&paths, command),
        Some(Commands::Target { command }) => run_target_command(&paths, command),
        Some(Commands::User { command }) => run_user_command(&paths, command),
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

fn mutation_mode(check: bool, force: bool) -> SyncMode {
    if check {
        SyncMode::Check
    } else {
        SyncMode::Apply { force }
    }
}

fn validate_output(output: &OutputArgs) -> Result<(), ExitCode> {
    if output.format != OutputFormat::Text && output.color.is_some() {
        let mut command = Cli::command();
        let error = command.error(
            ErrorKind::ArgumentConflict,
            "--color cannot be used with --format=json or --format=yaml",
        );
        let _ = error.print();
        return Err(ExitCode::from(2));
    }
    Ok(())
}

fn run_library_command(paths: &AppPaths, command: LibraryCommand) -> ExitCode {
    match command {
        LibraryCommand::Add {
            location,
            allow_overlap,
            output,
        } => {
            if let Err(code) = validate_output(&output.output) {
                return code;
            }
            finish_report(
                LibraryWorkflow::add_location(
                    paths,
                    location,
                    allow_overlap,
                    mutation_mode(output.check, false),
                ),
                &output.output,
            )
        }
        LibraryCommand::Remove { location, output } => {
            if let Err(code) = validate_output(&output.output) {
                return code;
            }
            finish_report(
                LibraryWorkflow::remove_location(
                    paths,
                    &location,
                    mutation_mode(output.check, false),
                ),
                &output.output,
            )
        }
        LibraryCommand::Locations(output) => {
            if let Err(code) = validate_output(&output) {
                return code;
            }
            match LibraryWorkflow::locations(paths) {
                Ok(report) => render_locations(report, &output),
                Err(error) => diagnostic(error.exit_status(), &error.to_string()),
            }
        }
        LibraryCommand::List { filter, output } => {
            if let Err(code) = validate_output(&output) {
                return code;
            }
            match LibraryWorkflow::inventory(paths, filter.as_deref()) {
                Ok(report) => render_inventory(report, &output),
                Err(error) => diagnostic(error.exit_status(), &error.to_string()),
            }
        }
    }
}

fn run_target_command(paths: &AppPaths, command: TargetCommand) -> ExitCode {
    match command {
        TargetCommand::Link(arguments) => {
            run_target_mutation(paths, arguments, Some(MaterializationKind::Linked))
        }
        TargetCommand::Copy(arguments) => {
            run_target_mutation(paths, arguments, Some(MaterializationKind::Copied))
        }
        TargetCommand::Remove(arguments) => run_target_mutation(paths, arguments, None),
    }
}

fn run_init(paths: &AppPaths, arguments: InitArgs) -> ExitCode {
    if let Err(code) = validate_output(&arguments.output.output) {
        return code;
    }
    finish_report(
        TargetWorkflow::init(
            paths,
            arguments.directory.unwrap_or_else(|| PathBuf::from(".")),
            mutation_mode(arguments.output.check, false),
        ),
        &arguments.output.output,
    )
}

fn run_target_mutation(
    paths: &AppPaths,
    arguments: TargetMutationArgs,
    materialization: Option<MaterializationKind>,
) -> ExitCode {
    if let Err(code) = validate_output(&arguments.output.output) {
        return code;
    }
    let selector = match SkillSelector::parse(&arguments.selector) {
        Ok(selector) => selector,
        Err(error) => return diagnostic(error.exit_status(), &error.to_string()),
    };
    finish_report(
        TargetWorkflow::mutate_enablement(
            paths,
            arguments.repository.unwrap_or_else(|| PathBuf::from(".")),
            &selector,
            arguments.directory.as_deref(),
            materialization,
            mutation_mode(arguments.output.check, arguments.output.force),
        ),
        &arguments.output.output,
    )
}

fn run_user_command(paths: &AppPaths, command: UserCommand) -> ExitCode {
    let (arguments, materialization) = match command {
        UserCommand::Link(arguments) => (arguments, Some(MaterializationKind::Linked)),
        UserCommand::Copy(arguments) => (arguments, Some(MaterializationKind::Copied)),
        UserCommand::Remove(arguments) => (arguments, None),
    };
    if let Err(code) = validate_output(&arguments.output.output) {
        return code;
    }
    let selector = match SkillSelector::parse(&arguments.selector) {
        Ok(selector) => selector,
        Err(error) => return diagnostic(error.exit_status(), &error.to_string()),
    };
    finish_report(
        UserScopeWorkflow::mutate_enablement(
            paths,
            &selector,
            materialization,
            mutation_mode(arguments.output.check, arguments.output.force),
        ),
        &arguments.output.output,
    )
}

fn finish_report(
    report: Result<CommandReport, crate::app::WorkflowError>,
    output: &OutputArgs,
) -> ExitCode {
    match report {
        Ok(report) => render_command_report(report, output),
        Err(error) => diagnostic(error.exit_status(), &error.to_string()),
    }
}

fn run_sync_command(paths: &AppPaths, arguments: SyncCommandArgs) -> ExitCode {
    match arguments.command {
        Some(SyncCommand::Target(arguments)) => run_target_sync(paths, arguments),
        Some(SyncCommand::Worktree(arguments)) => run_worktree_sync(paths, arguments),
        None => {
            let arguments = SyncArgs {
                directory: None,
                output: arguments.output,
            };
            if GitRepository::discover(std::path::Path::new("."))
                .and_then(|repository| repository.linked_worktree_pair())
                .is_ok()
            {
                run_worktree_sync(paths, arguments)
            } else {
                run_target_sync(paths, arguments)
            }
        }
    }
}

fn run_target_sync(paths: &AppPaths, arguments: SyncArgs) -> ExitCode {
    if arguments.output.format != OutputFormat::Text && arguments.output.color.is_some() {
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
    let mode = if arguments.output.check {
        SyncMode::Check
    } else {
        SyncMode::Apply {
            force: arguments.output.force,
        }
    };
    let report = match SyncWorkflow::run(paths, &directory, mode) {
        Ok(report) => report,
        Err(error) => return diagnostic(error.exit_status(), &error.to_string()),
    };
    render_report(arguments, report)
}

fn run_worktree_sync(paths: &AppPaths, arguments: SyncArgs) -> ExitCode {
    if arguments.output.format != OutputFormat::Text && arguments.output.color.is_some() {
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
    let mode = if arguments.output.check {
        SyncMode::Check
    } else {
        SyncMode::Apply {
            force: arguments.output.force,
        }
    };
    let report = match WorktreeSyncWorkflow::run(paths, &directory, mode) {
        Ok(report) => report,
        Err(error) => return diagnostic(error.exit_status(), &error.to_string()),
    };
    render_report(arguments, report)
}

fn render_report(arguments: SyncArgs, report: CommandReport) -> ExitCode {
    render_command_report(
        report,
        &OutputArgs {
            format: arguments.output.format,
            color: arguments.output.color,
        },
    )
}

fn render_command_report(report: CommandReport, arguments: &OutputArgs) -> ExitCode {
    let rendered = match arguments.format {
        OutputFormat::Text => Ok(render_text(
            &report,
            arguments.color.unwrap_or(ColorPolicy::Auto),
        )),
        OutputFormat::Json => render_json(&report),
        OutputFormat::Yaml => render_serialized_yaml(&report),
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

fn render_inventory(report: LibraryInventoryReport, arguments: &OutputArgs) -> ExitCode {
    let rendered = match arguments.format {
        OutputFormat::Text => {
            let mut text = String::new();
            for source in &report.sources {
                text.push_str(&source.key);
                text.push('\n');
                for skill in &source.skills {
                    text.push_str("  ");
                    text.push_str(&skill.path);
                    if let Some(name) = &skill.name {
                        text.push_str("  ");
                        text.push_str(name);
                    }
                    if !skill.valid {
                        text.push_str("  invalid");
                    }
                    text.push('\n');
                }
            }
            Ok(text)
        }
        OutputFormat::Json => render_json(&report),
        OutputFormat::Yaml => render_serialized_yaml(&report),
    };
    write_rendered(rendered, 0)
}

fn render_locations(report: LibraryLocationsReport, arguments: &OutputArgs) -> ExitCode {
    let rendered = match arguments.format {
        OutputFormat::Text => Ok(report
            .locations
            .iter()
            .map(|location| {
                format!(
                    "{}\t{}\t{}\n",
                    location.expression,
                    location.resolved.as_deref().unwrap_or("unresolved"),
                    if location.available {
                        "available"
                    } else {
                        "unavailable"
                    }
                )
            })
            .collect()),
        OutputFormat::Json => render_json(&report),
        OutputFormat::Yaml => render_serialized_yaml(&report),
    };
    write_rendered(rendered, 0)
}

fn render_json(value: &impl serde::Serialize) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map(|mut value| {
            value.push('\n');
            value
        })
        .map_err(|error| error.to_string())
}

fn render_serialized_yaml(value: &impl serde::Serialize) -> Result<String, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    render_yaml_value(&value)
}

fn write_rendered(rendered: Result<String, String>, status: u8) -> ExitCode {
    let output = match rendered {
        Ok(output) => output,
        Err(error) => return diagnostic(5, &format!("cannot render report: {error}")),
    };
    if let Err(error) = std::io::stdout().lock().write_all(output.as_bytes()) {
        return diagnostic(5, &format!("cannot write report: {error}"));
    }
    ExitCode::from(status)
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
    render_yaml_value(&value)
}

fn render_yaml_value(value: &Value) -> Result<String, String> {
    let mut output = String::from("---\n");
    emit_yaml(value, 0, &mut output)?;
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
