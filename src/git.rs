//! Structured, read-only Git facts.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("cannot execute Git: {0}")]
    Io(#[from] std::io::Error),
    #[error("not inside a Git worktree: {message}")]
    NotWorktree { message: String },
    #[error("Git returned non-UTF-8 output")]
    NonUtf8,
    #[error("path is a bare Git repository")]
    Bare,
    #[error("Git command `{command}` failed with status {status}: {message}")]
    Command {
        command: &'static str,
        status: i32,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathFacts {
    pub tracked: bool,
    pub staged: bool,
    pub unmerged: bool,
    pub ignored: bool,
    pub ignore_rule: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GitRepository {
    root: PathBuf,
    git_dir: PathBuf,
    bare: bool,
    superproject: Option<PathBuf>,
}

impl GitRepository {
    pub fn discover(path: &Path) -> Result<Self, GitError> {
        let bare_output = git_at(path, ["rev-parse", "--is-bare-repository"])?;
        if bare_output.status.success() && stdout(&bare_output)?.trim() == "true" {
            return Err(GitError::Bare);
        }
        let root_output = git_at(path, ["rev-parse", "--show-toplevel"])?;
        if !root_output.status.success() {
            return Err(GitError::NotWorktree {
                message: String::from_utf8_lossy(&root_output.stderr)
                    .trim()
                    .to_owned(),
            });
        }
        let root = PathBuf::from(stdout(&root_output)?.trim()).canonicalize()?;
        let git_dir_output = git_at(&root, ["rev-parse", "--absolute-git-dir"])?;
        if !git_dir_output.status.success() {
            return Err(GitError::NotWorktree {
                message: String::from_utf8_lossy(&git_dir_output.stderr)
                    .trim()
                    .to_owned(),
            });
        }
        let git_dir = PathBuf::from(stdout(&git_dir_output)?.trim());
        let super_output = git_at(&root, ["rev-parse", "--show-superproject-working-tree"])?;
        let superproject = super_output
            .status
            .success()
            .then(|| stdout(&super_output).ok().map(str::trim).map(str::to_owned))
            .flatten()
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        Ok(Self {
            root,
            git_dir,
            bare: false,
            superproject,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub fn is_bare(&self) -> bool {
        self.bare
    }

    pub fn superproject(&self) -> Option<&Path> {
        self.superproject.as_deref()
    }

    pub fn origin(&self) -> Result<Option<String>, GitError> {
        let output = self.command(["remote", "get-url", "origin"])?;
        if output.status.success() {
            Ok(Some(stdout(&output)?.trim().to_owned()))
        } else {
            Ok(None)
        }
    }

    pub fn facts_for(&self, relative: impl AsRef<Path>) -> Result<PathFacts, GitError> {
        let relative = relative.as_ref();
        let tracked_output = self.command_os([
            OsStr::new("ls-files"),
            OsStr::new("--error-unmatch"),
            OsStr::new("--"),
            relative.as_os_str(),
        ])?;
        let tracked = status_bool(&tracked_output, "git ls-files", 0, 1)?;
        let staged_output = self.command_os([
            OsStr::new("diff"),
            OsStr::new("--cached"),
            OsStr::new("--quiet"),
            OsStr::new("--"),
            relative.as_os_str(),
        ])?;
        let staged = status_bool(&staged_output, "git diff --cached", 1, 0)?;
        let unmerged_output = self.command_os([
            OsStr::new("ls-files"),
            OsStr::new("-u"),
            OsStr::new("--"),
            relative.as_os_str(),
        ])?;
        require_success(&unmerged_output, "git ls-files -u")?;
        let unmerged = !unmerged_output.stdout.is_empty();
        let ignored_output = self.command_os([
            OsStr::new("check-ignore"),
            OsStr::new("-v"),
            OsStr::new("--no-index"),
            OsStr::new("--"),
            relative.as_os_str(),
        ])?;
        let has_ignore_rule = status_bool(&ignored_output, "git check-ignore", 0, 1)?;
        let ignore_rule = has_ignore_rule
            .then(|| String::from_utf8(ignored_output.stdout).ok())
            .flatten()
            .map(|value| value.trim().to_owned());
        let ignored = ignore_rule.as_deref().is_some_and(|line| {
            let rule = line
                .split_once('\t')
                .map(|(rule, _)| rule)
                .unwrap_or(line)
                .rsplit_once(':')
                .map(|(_, pattern)| pattern)
                .unwrap_or(line);
            !rule.starts_with('!')
        });
        Ok(PathFacts {
            tracked,
            staged,
            unmerged,
            ignored,
            ignore_rule,
        })
    }

    fn command<const N: usize>(&self, args: [&str; N]) -> Result<Output, GitError> {
        git_at(&self.root, args)
    }

    fn command_os<const N: usize>(&self, args: [&OsStr; N]) -> Result<Output, GitError> {
        let mut command = git_command(&self.root);
        command.args(args);
        Ok(command.output()?)
    }
}

fn git_at<I, S>(path: &Path, args: I) -> Result<Output, GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Ok(git_command(path).args(args).output()?)
}

fn git_command(path: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(path).env("GIT_CONFIG_NOSYSTEM", "1");
    command
}

fn stdout(output: &Output) -> Result<&str, GitError> {
    std::str::from_utf8(&output.stdout).map_err(|_| GitError::NonUtf8)
}

fn status_bool(
    output: &Output,
    command: &'static str,
    true_status: i32,
    false_status: i32,
) -> Result<bool, GitError> {
    match output.status.code() {
        Some(status) if status == true_status => Ok(true),
        Some(status) if status == false_status => Ok(false),
        status => Err(command_error(output, command, status.unwrap_or(-1))),
    }
}

fn require_success(output: &Output, command: &'static str) -> Result<(), GitError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(command_error(
            output,
            command,
            output.status.code().unwrap_or(-1),
        ))
    }
}

fn command_error(output: &Output, command: &'static str, status: i32) -> GitError {
    GitError::Command {
        command,
        status,
        message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    }
}
