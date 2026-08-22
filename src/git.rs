//! Structured, read-only Git facts.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreePair {
    primary: PathBuf,
    current: PathBuf,
}

impl WorktreePair {
    pub fn primary_root(&self) -> &Path {
        &self.primary
    }

    pub fn current_root(&self) -> &Path {
        &self.current
    }
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

    /// Return the primary and current roots when this repository is a
    /// registered linked worktree. Git's worktree metadata is the source of
    /// truth; directory names and sibling layout are deliberately ignored.
    pub fn linked_worktree_pair(&self) -> Result<WorktreePair, GitError> {
        let output = self.command(["worktree", "list", "--porcelain"])?;
        require_success(&output, "git worktree list")?;
        let mut roots = Vec::new();
        let mut pending = None;
        for line in stdout(&output)?.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                pending = Some(PathBuf::from(path));
            } else if line.is_empty()
                && let Some(path) = pending.take()
            {
                roots.push(path.canonicalize()?);
            }
        }
        if let Some(path) = pending.take() {
            roots.push(path.canonicalize()?);
        }

        let Some(primary) = roots.first().cloned() else {
            return Err(GitError::NotWorktree {
                message: "Git has no registered worktrees".to_owned(),
            });
        };
        if self.root == primary {
            return Err(GitError::NotWorktree {
                message: "current directory is the primary worktree, not a linked worktree"
                    .to_owned(),
            });
        }
        if !roots.iter().any(|root| root == &self.root) {
            return Err(GitError::NotWorktree {
                message: "current directory is not a registered Git worktree".to_owned(),
            });
        }
        Ok(WorktreePair {
            primary,
            current: self.root.clone(),
        })
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
        let relative = relative.as_ref().to_owned();
        self.facts_for_many(std::slice::from_ref(&relative))?
            .remove(&relative)
            .ok_or_else(|| GitError::Command {
                command: "git facts",
                status: -1,
                message: format!("Git returned no facts for {}", relative.display()),
            })
    }

    /// Collect tracked, staged, unmerged, and ignored facts for many paths with
    /// one Git process per fact kind rather than one process per path.
    pub fn facts_for_many(
        &self,
        relatives: &[PathBuf],
    ) -> Result<BTreeMap<PathBuf, PathFacts>, GitError> {
        if relatives.is_empty() {
            return Ok(BTreeMap::new());
        }

        let path_args = |command: &str| {
            let mut args = vec![OsString::from(command), OsString::from("--")];
            args.extend(relatives.iter().map(|path| path.as_os_str().to_owned()));
            args
        };

        let tracked_output = self.command_os(path_args("ls-files"))?;
        require_success(&tracked_output, "git ls-files")?;
        let tracked_paths = output_paths(&tracked_output)?;

        let staged_output = self.command_os(
            [
                OsStr::new("diff"),
                OsStr::new("--cached"),
                OsStr::new("--name-only"),
            ]
            .into_iter()
            .chain([OsStr::new("--")])
            .chain(relatives.iter().map(|path| path.as_os_str()))
            .collect::<Vec<_>>(),
        )?;
        require_success(&staged_output, "git diff --cached")?;
        let staged_paths = output_paths(&staged_output)?;

        let unmerged_output = self.command_os(
            [OsStr::new("ls-files"), OsStr::new("-u")]
                .into_iter()
                .chain([OsStr::new("--")])
                .chain(relatives.iter().map(|path| path.as_os_str()))
                .collect::<Vec<_>>(),
        )?;
        require_success(&unmerged_output, "git ls-files -u")?;
        let unmerged_paths = output_paths_with_tab_suffix(&unmerged_output)?;

        let ignored_output = self.command_os(
            [
                OsStr::new("check-ignore"),
                OsStr::new("-v"),
                OsStr::new("--no-index"),
            ]
            .into_iter()
            .chain([OsStr::new("--")])
            .chain(relatives.iter().map(|path| path.as_os_str()))
            .collect::<Vec<_>>(),
        )?;
        let ignored_paths = match ignored_output.status.code() {
            Some(0) => output_ignore_paths(&ignored_output)?,
            Some(1) => Vec::new(),
            status => {
                return Err(command_error(
                    &ignored_output,
                    "git check-ignore",
                    status.unwrap_or(-1),
                ));
            }
        };

        Ok(relatives
            .iter()
            .map(|relative| {
                let tracked = tracked_paths
                    .iter()
                    .any(|path| path_matches(relative, path));
                let staged = staged_paths.iter().any(|path| path_matches(relative, path));
                let unmerged = unmerged_paths
                    .iter()
                    .any(|path| path_matches(relative, path));
                let ignore_rule = ignored_paths
                    .iter()
                    .find(|(path, _)| path_matches(relative, path))
                    .map(|(_, rule)| rule.clone());
                let ignored = ignore_rule.as_deref().is_some_and(ignore_rule_is_active);
                (
                    relative.clone(),
                    PathFacts {
                        tracked,
                        staged,
                        unmerged,
                        ignored,
                        ignore_rule,
                    },
                )
            })
            .collect())
    }

    fn command<const N: usize>(&self, args: [&str; N]) -> Result<Output, GitError> {
        git_at(&self.root, args)
    }

    fn command_os<I, S>(&self, args: I) -> Result<Output, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = git_command(&self.root);
        command.args(args);
        Ok(command.output()?)
    }
}

fn output_paths(output: &Output) -> Result<Vec<PathBuf>, GitError> {
    let text = stdout(output)?;
    Ok(text.lines().map(PathBuf::from).collect())
}

fn output_paths_with_tab_suffix(output: &Output) -> Result<Vec<PathBuf>, GitError> {
    let text = stdout(output)?;
    Ok(text
        .lines()
        .filter_map(|line| line.split_once('\t').map(|(_, path)| PathBuf::from(path)))
        .collect())
}

fn output_ignore_paths(output: &Output) -> Result<Vec<(PathBuf, String)>, GitError> {
    let text = stdout(output)?;
    Ok(text
        .lines()
        .filter_map(|line| {
            line.rsplit_once('\t')
                .map(|(_, path)| (PathBuf::from(path), line.to_owned()))
        })
        .collect())
}

fn path_matches(requested: &Path, reported: &Path) -> bool {
    requested == reported || reported.starts_with(requested)
}

fn ignore_rule_is_active(line: &str) -> bool {
    let rule = line
        .split_once('\t')
        .map(|(rule, _)| rule)
        .unwrap_or(line)
        .rsplit_once(':')
        .map(|(_, pattern)| pattern)
        .unwrap_or(line);
    !rule.starts_with('!')
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
