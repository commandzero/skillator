//! Opt-in Git hook integration for linked worktree synchronization.

use crate::app::{ReportChange, ReportDiagnostic, ReportOutcome, ReportStatus, SyncMode};
use crate::fs_safety::{rename_exchange, rename_noreplace};
use crate::git::GitRepository;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, Metadata, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const HOOK_NAME: &str = "post-checkout";
const PREDECESSOR_NAME: &str = "post-checkout.skillator-original";
const HOOK_MARKER: &str = "# skillator-managed-post-checkout: v1";
const ORIGINAL_HASH_PREFIX: &str = "# skillator-original-sha256: ";
const ORIGINAL_MODE_PREFIX: &str = "# skillator-original-mode: ";
const ZERO_REF: &str = "0000000000000000000000000000000000000000";
const GENERATED_MODE: u32 = 0o755;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("{message}")]
    InvalidInput { message: String },
    #[error("{message}")]
    Fatal { message: String },
}

impl HookError {
    pub fn exit_status(&self) -> u8 {
        match self {
            Self::InvalidInput { .. } => 3,
            Self::Fatal { .. } => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookState {
    Absent,
    Installed,
    Modified,
    Conflict,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookReport {
    pub format_version: u8,
    pub status: ReportStatus,
    pub exit_status: u8,
    pub mode: String,
    pub repository: String,
    pub hook_path: String,
    pub state: HookState,
    pub changes: Vec<ReportChange>,
    pub diagnostics: Vec<ReportDiagnostic>,
}

#[derive(Debug, Clone)]
struct HookContext {
    repository: GitRepository,
    hook_path: PathBuf,
    predecessor_path: PathBuf,
}

#[derive(Debug, Clone)]
struct OriginalHook {
    bytes: Vec<u8>,
    mode: u32,
}

#[derive(Debug, Clone)]
struct HookInspection {
    state: HookState,
    hook_bytes: Option<Vec<u8>>,
    hook_mode: Option<u32>,
    original: Option<OriginalHook>,
    reason: Option<String>,
}

pub struct HookWorkflow;

impl HookWorkflow {
    pub fn status(repository: impl AsRef<Path>) -> Result<HookReport, HookError> {
        let context = HookContext::discover(repository.as_ref())?;
        let inspection = inspect(&context)?;
        Ok(report(
            &context,
            "hook_status",
            inspection.state,
            matches!(
                inspection.state,
                HookState::Modified | HookState::Conflict | HookState::Blocked
            ),
            Vec::new(),
            inspection
                .reason
                .map(|reason| vec![diagnostic("hook_state", reason)])
                .unwrap_or_default(),
        ))
    }

    pub fn install(repository: impl AsRef<Path>, mode: SyncMode) -> Result<HookReport, HookError> {
        let context = HookContext::discover(repository.as_ref())?;
        let inspection = inspect(&context)?;
        let force = matches!(mode, SyncMode::Apply { force: true });
        let checking = matches!(mode, SyncMode::Check);
        let action_mode = if checking {
            "hook_install_check"
        } else {
            "hook_install"
        };

        match inspection.state {
            HookState::Installed => Ok(report(
                &context,
                action_mode,
                HookState::Installed,
                false,
                Vec::new(),
                Vec::new(),
            )),
            HookState::Absent => {
                let change = change(
                    &context.hook_path,
                    "install_hook",
                    "safe",
                    if checking {
                        ReportOutcome::WouldApply
                    } else {
                        ReportOutcome::Applied
                    },
                );
                if checking {
                    return Ok(report(
                        &context,
                        action_mode,
                        HookState::Absent,
                        true,
                        vec![change],
                        Vec::new(),
                    ));
                }
                write_new_hook(&context, None)?;
                Ok(report(
                    &context,
                    action_mode,
                    HookState::Installed,
                    false,
                    vec![change],
                    Vec::new(),
                ))
            }
            HookState::Conflict if !force => {
                let message = inspection
                    .reason
                    .unwrap_or_else(|| "an unrelated post-checkout hook already exists".to_owned());
                let change = change(
                    &context.hook_path,
                    "install_hook",
                    "guarded",
                    ReportOutcome::WouldRequireForce,
                );
                Ok(report(
                    &context,
                    action_mode,
                    HookState::Conflict,
                    true,
                    vec![change],
                    vec![diagnostic("hook_conflict", message)],
                ))
            }
            HookState::Conflict => {
                let Some(bytes) = inspection.hook_bytes else {
                    return Err(HookError::Fatal {
                        message: "hook conflict did not include readable bytes".to_owned(),
                    });
                };
                let mode = inspection.hook_mode.unwrap_or(GENERATED_MODE);
                let original = OriginalHook { bytes, mode };
                let outcome = if checking {
                    ReportOutcome::WouldApply
                } else {
                    ReportOutcome::Applied
                };
                let changes = vec![
                    change(
                        &context.predecessor_path,
                        "preserve_hook",
                        "guarded",
                        outcome,
                    ),
                    change(&context.hook_path, "install_hook", "guarded", outcome),
                ];
                if checking {
                    return Ok(report(
                        &context,
                        action_mode,
                        HookState::Conflict,
                        true,
                        changes,
                        vec![diagnostic(
                            "hook_force_required",
                            "an existing hook will be preserved and chained with --force"
                                .to_owned(),
                        )],
                    ));
                }
                match chain_existing_hook(&context, &original) {
                    Ok(()) => Ok(report(
                        &context,
                        action_mode,
                        HookState::Installed,
                        false,
                        changes,
                        Vec::new(),
                    )),
                    Err(HookError::InvalidInput { message }) => Ok(report(
                        &context,
                        action_mode,
                        HookState::Blocked,
                        true,
                        vec![
                            change(
                                &context.predecessor_path,
                                "preserve_hook",
                                "blocked",
                                ReportOutcome::Blocked,
                            ),
                            change(
                                &context.hook_path,
                                "install_hook",
                                "blocked",
                                ReportOutcome::Blocked,
                            ),
                        ],
                        vec![diagnostic("hook_install_blocked", message)],
                    )),
                    Err(error) => Err(error),
                }
            }
            HookState::Modified | HookState::Blocked => {
                let message = inspection.reason.unwrap_or_else(|| {
                    "the existing Skillator hook cannot be changed safely".to_owned()
                });
                let change = change(
                    &context.hook_path,
                    "install_hook",
                    "blocked",
                    ReportOutcome::Blocked,
                );
                Ok(report(
                    &context,
                    action_mode,
                    inspection.state,
                    true,
                    vec![change],
                    vec![diagnostic("hook_blocked", message)],
                ))
            }
        }
    }

    pub fn uninstall(
        repository: impl AsRef<Path>,
        mode: SyncMode,
    ) -> Result<HookReport, HookError> {
        let context = HookContext::discover(repository.as_ref())?;
        let inspection = inspect(&context)?;
        let checking = matches!(mode, SyncMode::Check);
        let action_mode = if checking {
            "hook_uninstall_check"
        } else {
            "hook_uninstall"
        };
        match inspection.state {
            HookState::Absent => Ok(report(
                &context,
                action_mode,
                HookState::Absent,
                false,
                Vec::new(),
                inspection
                    .reason
                    .map(|reason| vec![diagnostic("hook_state", reason)])
                    .unwrap_or_default(),
            )),
            HookState::Installed => {
                let outcome = if checking {
                    ReportOutcome::WouldApply
                } else {
                    ReportOutcome::Applied
                };
                let mut changes = vec![change(&context.hook_path, "remove_hook", "safe", outcome)];
                if inspection.original.is_some() {
                    changes.push(change(
                        &context.predecessor_path,
                        "restore_hook",
                        "safe",
                        outcome,
                    ));
                }
                if checking {
                    return Ok(report(
                        &context,
                        action_mode,
                        HookState::Installed,
                        true,
                        changes,
                        Vec::new(),
                    ));
                }
                remove_managed_hook(&context, &inspection)?;
                Ok(report(
                    &context,
                    action_mode,
                    HookState::Absent,
                    false,
                    changes,
                    Vec::new(),
                ))
            }
            HookState::Modified | HookState::Conflict | HookState::Blocked => {
                let message = inspection.reason.unwrap_or_else(|| {
                    "the existing hook is not an unchanged Skillator-managed hook".to_owned()
                });
                Ok(report(
                    &context,
                    action_mode,
                    inspection.state,
                    true,
                    vec![change(
                        &context.hook_path,
                        "remove_hook",
                        "blocked",
                        ReportOutcome::Blocked,
                    )],
                    vec![diagnostic("hook_ownership_conflict", message)],
                ))
            }
        }
    }
}

impl HookContext {
    fn discover(path: &Path) -> Result<Self, HookError> {
        let repository =
            GitRepository::discover(path).map_err(|error| HookError::InvalidInput {
                message: error.to_string(),
            })?;
        let hooks_dir = repository
            .hooks_path()
            .map_err(|error| HookError::InvalidInput {
                message: error.to_string(),
            })?;
        let hook_path = hooks_dir.join(HOOK_NAME);
        let predecessor_path = hook_path
            .parent()
            .ok_or_else(|| HookError::Fatal {
                message: format!(
                    "Git returned a hook path without a parent: {}",
                    hook_path.display()
                ),
            })?
            .join(PREDECESSOR_NAME);
        Ok(Self {
            repository,
            hook_path,
            predecessor_path,
        })
    }
}

fn inspect(context: &HookContext) -> Result<HookInspection, HookError> {
    let hook = read_entry(&context.hook_path)?;
    let predecessor = read_entry(&context.predecessor_path)?;
    let Some(hook) = hook else {
        return Ok(match predecessor {
            None => HookInspection {
                state: HookState::Absent,
                hook_bytes: None,
                hook_mode: None,
                original: None,
                reason: None,
            },
            Some(Entry::Regular { .. }) => HookInspection {
                state: HookState::Blocked,
                hook_bytes: None,
                hook_mode: None,
                original: None,
                reason: Some(
                    "a Skillator predecessor exists without its managed post-checkout hook"
                        .to_owned(),
                ),
            },
            Some(Entry::Blocked(reason)) => HookInspection {
                state: HookState::Blocked,
                hook_bytes: None,
                hook_mode: None,
                original: None,
                reason: Some(reason),
            },
        });
    };

    let (hook_bytes, hook_mode) = match hook {
        Entry::Regular { bytes, mode } => (bytes, mode),
        Entry::Blocked(reason) => {
            return Ok(HookInspection {
                state: HookState::Blocked,
                hook_bytes: None,
                hook_mode: None,
                original: None,
                reason: Some(reason),
            });
        }
    };

    let Some(metadata) = parse_metadata(&hook_bytes) else {
        return Ok(HookInspection {
            state: HookState::Conflict,
            hook_bytes: Some(hook_bytes),
            hook_mode: Some(hook_mode),
            original: None,
            reason: Some("an unrelated post-checkout hook already exists".to_owned()),
        });
    };

    let original = match metadata.hash.as_deref() {
        Some("none") | None => {
            if let Some(entry) = predecessor {
                return Ok(HookInspection {
                    state: match entry {
                        Entry::Regular { .. } => HookState::Modified,
                        Entry::Blocked(_) => HookState::Blocked,
                    },
                    hook_bytes: Some(hook_bytes),
                    hook_mode: Some(hook_mode),
                    original: None,
                    reason: Some(
                        "a Skillator predecessor exists without a chained hook reference"
                            .to_owned(),
                    ),
                });
            }
            None
        }
        Some(expected_hash) => match predecessor {
            Some(Entry::Regular { bytes, mode })
                if hash_hex(&bytes) == expected_hash && metadata.mode == Some(mode) =>
            {
                Some(OriginalHook { bytes, mode })
            }
            Some(Entry::Regular { .. }) => {
                return Ok(HookInspection {
                    state: HookState::Modified,
                    hook_bytes: Some(hook_bytes),
                    hook_mode: Some(hook_mode),
                    original: None,
                    reason: Some(
                        "the preserved predecessor hook has changed since installation".to_owned(),
                    ),
                });
            }
            Some(Entry::Blocked(reason)) => {
                return Ok(HookInspection {
                    state: HookState::Blocked,
                    hook_bytes: Some(hook_bytes),
                    hook_mode: Some(hook_mode),
                    original: None,
                    reason: Some(reason),
                });
            }
            None => {
                return Ok(HookInspection {
                    state: HookState::Modified,
                    hook_bytes: Some(hook_bytes),
                    hook_mode: Some(hook_mode),
                    original: None,
                    reason: Some("the preserved predecessor hook is missing".to_owned()),
                });
            }
        },
    };
    let expected = render_hook_script(original.as_ref());
    if hook_bytes == expected && hook_mode == GENERATED_MODE {
        Ok(HookInspection {
            state: HookState::Installed,
            hook_bytes: Some(hook_bytes),
            hook_mode: Some(hook_mode),
            original,
            reason: None,
        })
    } else {
        Ok(HookInspection {
            state: HookState::Modified,
            hook_bytes: Some(hook_bytes),
            hook_mode: Some(hook_mode),
            original: None,
            reason: Some("the Skillator-managed post-checkout hook has changed".to_owned()),
        })
    }
}

#[derive(Debug, Clone)]
struct HookMetadata {
    hash: Option<String>,
    mode: Option<u32>,
}

fn parse_metadata(bytes: &[u8]) -> Option<HookMetadata> {
    let text = std::str::from_utf8(bytes).ok()?;
    if !text.lines().any(|line| line == HOOK_MARKER) {
        return None;
    }
    let hash = text
        .lines()
        .find_map(|line| line.strip_prefix(ORIGINAL_HASH_PREFIX))
        .map(str::to_owned);
    let mode = text
        .lines()
        .find_map(|line| line.strip_prefix(ORIGINAL_MODE_PREFIX))
        .and_then(|value| (value != "none").then(|| u32::from_str_radix(value, 8).ok()))
        .flatten();
    Some(HookMetadata { hash, mode })
}

#[derive(Debug)]
enum Entry {
    Regular { bytes: Vec<u8>, mode: u32 },
    Blocked(String),
}

fn read_entry(path: &Path) -> Result<Option<Entry>, HookError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Ok(Some(Entry::Blocked(format!(
                "cannot inspect {}: {error}",
                path.display()
            ))));
        }
    };
    if !metadata.file_type().is_file() {
        return Ok(Some(Entry::Blocked(format!(
            "{} is not a regular file",
            path.display()
        ))));
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Ok(Some(Entry::Blocked(format!(
                "cannot read {}: {error}",
                path.display()
            ))));
        }
    };
    Ok(Some(Entry::Regular {
        bytes,
        mode: permission_mode(&metadata),
    }))
}

fn permission_mode(metadata: &Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        GENERATED_MODE
    }
}

fn set_permission_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

fn write_new_hook(context: &HookContext, original: Option<&OriginalHook>) -> Result<(), HookError> {
    if let Some(parent) = context.hook_path.parent() {
        fs::create_dir_all(parent).map_err(fatal_io)?;
    }
    write_noreplace(
        &context.hook_path,
        &render_hook_script(original),
        GENERATED_MODE,
    )
}

fn chain_existing_hook(context: &HookContext, original: &OriginalHook) -> Result<(), HookError> {
    if let Some(parent) = context.hook_path.parent() {
        fs::create_dir_all(parent).map_err(fatal_io)?;
    }
    if read_entry(&context.predecessor_path)?.is_some() {
        return Err(HookError::InvalidInput {
            message: format!(
                "cannot preserve the existing hook because {} already exists",
                context.predecessor_path.display()
            ),
        });
    }
    let current = read_entry(&context.hook_path)?;
    let Some(Entry::Regular { bytes, mode }) = current else {
        return Err(HookError::InvalidInput {
            message: "the existing hook changed before installation".to_owned(),
        });
    };
    if bytes != original.bytes || mode != original.mode {
        return Err(HookError::InvalidInput {
            message: "the existing hook changed before installation".to_owned(),
        });
    }
    rename_noreplace(&context.hook_path, &context.predecessor_path).map_err(fatal_io)?;
    if let Err(error) = write_noreplace(
        &context.hook_path,
        &render_hook_script(Some(original)),
        GENERATED_MODE,
    ) {
        let restore = rename_noreplace(&context.predecessor_path, &context.hook_path);
        return match restore {
            Ok(()) => Err(error),
            Err(restore_error) => Err(HookError::Fatal {
                message: format!(
                    "cannot install the managed hook ({error}); restoring the existing hook also failed: {restore_error}"
                ),
            }),
        };
    }
    Ok(())
}

fn remove_managed_hook(
    context: &HookContext,
    inspection: &HookInspection,
) -> Result<(), HookError> {
    let current = read_entry(&context.hook_path)?;
    let Some(Entry::Regular { bytes, mode }) = current else {
        return Err(HookError::InvalidInput {
            message: "the managed hook changed before uninstall".to_owned(),
        });
    };
    if inspection.hook_bytes.as_deref() != Some(bytes.as_slice())
        || inspection.hook_mode != Some(mode)
    {
        return Err(HookError::InvalidInput {
            message: "the managed hook changed before uninstall".to_owned(),
        });
    }
    if let Some(original) = inspection.original.as_ref() {
        let predecessor = read_entry(&context.predecessor_path)?;
        let Some(Entry::Regular {
            bytes: predecessor_bytes,
            mode: predecessor_mode,
        }) = predecessor
        else {
            return Err(HookError::InvalidInput {
                message: "the preserved predecessor changed before uninstall".to_owned(),
            });
        };
        if predecessor_bytes != original.bytes || predecessor_mode != original.mode {
            return Err(HookError::InvalidInput {
                message: "the preserved predecessor changed before uninstall".to_owned(),
            });
        }

        // Exchange first, then validate both paths again. If either path was
        // changed after the initial inspection, exchanging back preserves the
        // user's bytes instead of deleting them.
        exchange_hooks(context)?;
        let exchanged_hook = read_entry(&context.hook_path)?;
        let exchanged_predecessor = read_entry(&context.predecessor_path)?;
        let hook_matches = matches!(
            exchanged_hook,
            Some(Entry::Regular { bytes, mode }) if bytes == original.bytes && mode == original.mode
        );
        let predecessor_matches = matches!(
            exchanged_predecessor,
            Some(Entry::Regular { bytes, mode })
                if inspection.hook_bytes.as_deref() == Some(bytes.as_slice())
                    && inspection.hook_mode == Some(mode)
        );
        if !hook_matches || !predecessor_matches {
            exchange_hooks(context)?;
            return Err(HookError::InvalidInput {
                message: "the managed hook or preserved predecessor changed during uninstall"
                    .to_owned(),
            });
        }

        // Move the wrapper out of the user-visible path before removing it.
        // This makes a concurrent replacement of the predecessor path visible
        // without ever unlinking that replacement.
        let temporary = move_to_temporary(&context.predecessor_path, "uninstall")?;
        let temporary_matches = matches!(
            read_entry(&temporary)?,
            Some(Entry::Regular { bytes, mode })
                if inspection.hook_bytes.as_deref() == Some(bytes.as_slice())
                    && inspection.hook_mode == Some(mode)
        );
        if !temporary_matches {
            restore_temporary(&temporary, &context.predecessor_path)?;
            exchange_hooks(context)?;
            return Err(HookError::InvalidInput {
                message: "the managed hook or preserved predecessor changed during uninstall"
                    .to_owned(),
            });
        }
        if !matches!(
            read_entry(&context.hook_path)?,
            Some(Entry::Regular { bytes, mode }) if bytes == original.bytes && mode == original.mode
        ) {
            restore_temporary(&temporary, &context.predecessor_path)?;
            exchange_hooks(context)?;
            return Err(HookError::InvalidInput {
                message: "the managed hook changed during uninstall".to_owned(),
            });
        }
        fs::remove_file(&temporary).map_err(fatal_io)?;
    } else {
        let temporary = move_to_temporary(&context.hook_path, "uninstall")?;
        let matches = matches!(
            read_entry(&temporary)?,
            Some(Entry::Regular { bytes, mode })
                if inspection.hook_bytes.as_deref() == Some(bytes.as_slice())
                    && inspection.hook_mode == Some(mode)
        );
        if !matches {
            restore_temporary(&temporary, &context.hook_path)?;
            return Err(HookError::InvalidInput {
                message: "the managed hook changed during uninstall".to_owned(),
            });
        }
        if read_entry(&context.hook_path)?.is_some() {
            fs::remove_file(&temporary).map_err(fatal_io)?;
            return Err(HookError::InvalidInput {
                message: "the managed hook changed during uninstall".to_owned(),
            });
        }
        fs::remove_file(&temporary).map_err(fatal_io)?;
    }
    Ok(())
}

fn move_to_temporary(path: &Path, label: &str) -> Result<PathBuf, HookError> {
    let parent = path.parent().ok_or_else(|| HookError::Fatal {
        message: format!("path has no parent: {}", path.display()),
    })?;
    loop {
        let temporary = temporary_path(parent, label);
        match rename_noreplace(path, &temporary) {
            Ok(()) => return Ok(temporary),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(HookError::InvalidInput {
                    message: format!("{} changed before uninstall", path.display()),
                });
            }
            Err(error) => return Err(fatal_io(error)),
        }
    }
}

fn exchange_hooks(context: &HookContext) -> Result<(), HookError> {
    rename_exchange(&context.hook_path, &context.predecessor_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            HookError::InvalidInput {
                message: "the managed hook or preserved predecessor changed during uninstall"
                    .to_owned(),
            }
        } else {
            fatal_io(error)
        }
    })
}

fn restore_temporary(temporary: &Path, destination: &Path) -> Result<(), HookError> {
    rename_noreplace(temporary, destination).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            HookError::Fatal {
                message: format!(
                    "cannot restore {} to {}; the destination changed during uninstall",
                    temporary.display(),
                    destination.display()
                ),
            }
        } else {
            fatal_io(error)
        }
    })
}

fn temporary_path(parent: &Path, label: &str) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".{HOOK_NAME}.skillator-{}-{sequence}-{label}.tmp",
        std::process::id()
    ))
}

fn write_noreplace(path: &Path, bytes: &[u8], mode: u32) -> Result<(), HookError> {
    let parent = path.parent().ok_or_else(|| HookError::Fatal {
        message: format!("path has no parent: {}", path.display()),
    })?;
    let (temporary, mut file) = loop {
        let temporary = temporary_path(parent, "write");
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(fatal_io(error)),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        set_permission_mode(&temporary, mode)?;
        rename_noreplace(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            HookError::InvalidInput {
                message: format!(
                    "{} changed while the hook was being installed",
                    path.display()
                ),
            }
        } else {
            fatal_io(error)
        }
    })
}

fn render_hook_script(original: Option<&OriginalHook>) -> Vec<u8> {
    let hash = original.map_or_else(|| "none".to_owned(), |value| hash_hex(&value.bytes));
    let mode = original.map_or_else(|| "none".to_owned(), |value| format!("{:o}", value.mode));
    format!(
        "#!/bin/sh\n{HOOK_MARKER}\n{ORIGINAL_HASH_PREFIX}{hash}\n{ORIGINAL_MODE_PREFIX}{mode}\n\noriginal_status=0\nhooks_dir=$(git rev-parse --git-path hooks 2>/dev/null) || exit 0\noriginal=\"$hooks_dir/{PREDECESSOR_NAME}\"\nif [ -x \"$original\" ]; then\n  \"$original\" \"$@\"\n  original_status=$?\nfi\n\nif [ \"${{SKILLATOR_NO_AUTO_SYNC-}}\" = \"1\" ]; then\n  exit \"$original_status\"\nfi\nif [ \"${{1-}}\" != \"{ZERO_REF}\" ]; then\n  exit \"$original_status\"\nfi\n\ngit_dir=$(git rev-parse --git-dir 2>/dev/null) || exit \"$original_status\"\ncommon_dir=$(git rev-parse --git-common-dir 2>/dev/null) || exit \"$original_status\"\nif [ \"$git_dir\" = \"$common_dir\" ]; then\n  exit \"$original_status\"\nfi\nfor name in $(git rev-parse --local-env-vars 2>/dev/null); do\n  unset \"$name\"\ndone\n\nif command -v skillator >/dev/null 2>&1; then\n  skillator sync worktree . --color never >&2\n  sync_status=$?\n  if [ \"$sync_status\" -ne 0 ]; then\n    printf '%s\\n' 'skillator: automatic worktree sync did not converge; run `skillator sync worktree .`' >&2\n  fi\nelse\n  printf '%s\\n' 'skillator: executable not found; run `skillator sync worktree .` manually' >&2\nfi\nexit \"$original_status\"\n"
    )
    .into_bytes()
}

fn hash_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn report(
    context: &HookContext,
    mode: &str,
    state: HookState,
    not_converged: bool,
    changes: Vec<ReportChange>,
    diagnostics: Vec<ReportDiagnostic>,
) -> HookReport {
    HookReport {
        format_version: 1,
        status: if not_converged {
            ReportStatus::NotConverged
        } else {
            ReportStatus::InSync
        },
        exit_status: if not_converged { 1 } else { 0 },
        mode: mode.to_owned(),
        repository: context.repository.root().to_string_lossy().into_owned(),
        hook_path: context.hook_path.to_string_lossy().into_owned(),
        state,
        changes,
        diagnostics,
    }
}

fn change(path: &Path, action: &str, safety: &str, outcome: ReportOutcome) -> ReportChange {
    ReportChange {
        path: path.to_string_lossy().into_owned(),
        action: action.to_owned(),
        safety: safety.to_owned(),
        outcome,
    }
}

fn diagnostic(code: &str, message: String) -> ReportDiagnostic {
    ReportDiagnostic {
        code: code.to_owned(),
        severity: "warning".to_owned(),
        message,
        data: None,
    }
}

fn fatal_io(error: std::io::Error) -> HookError {
    HookError::Fatal {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_hook_contains_worktree_guards_and_environment_cleanup() {
        let script = String::from_utf8(render_hook_script(None)).unwrap();
        assert!(script.contains(HOOK_MARKER));
        assert!(script.contains(ZERO_REF));
        assert!(script.contains("git rev-parse --git-common-dir"));
        assert!(script.contains("git rev-parse --local-env-vars"));
        assert!(script.contains("SKILLATOR_NO_AUTO_SYNC"));
        assert!(script.contains("skillator sync worktree ."));
    }

    #[test]
    fn chained_hook_records_original_fingerprint_and_mode() {
        let original = OriginalHook {
            bytes: b"#!/bin/sh\necho old\n".to_vec(),
            mode: 0o700,
        };
        let script = String::from_utf8(render_hook_script(Some(&original))).unwrap();
        let metadata = parse_metadata(script.as_bytes()).unwrap();
        assert_eq!(
            metadata.hash.as_deref(),
            Some(hash_hex(&original.bytes).as_str())
        );
        assert_eq!(metadata.mode, Some(0o700));
    }

    fn test_repository() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let output = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(directory.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(output.status.success(), "git init failed: {output:?}");
        directory
    }

    #[test]
    fn workflow_install_status_and_uninstall_are_idempotent() {
        let directory = test_repository();
        let check = HookWorkflow::install(directory.path(), SyncMode::Check).unwrap();
        assert_eq!(check.state, HookState::Absent);
        assert_eq!(check.exit_status, 1);

        let installed =
            HookWorkflow::install(directory.path(), SyncMode::Apply { force: false }).unwrap();
        assert_eq!(installed.state, HookState::Installed);
        assert_eq!(installed.exit_status, 0);

        let status = HookWorkflow::status(directory.path()).unwrap();
        assert_eq!(status.state, HookState::Installed);
        assert_eq!(status.exit_status, 0);

        let repeated =
            HookWorkflow::install(directory.path(), SyncMode::Apply { force: false }).unwrap();
        assert!(repeated.changes.is_empty());

        let removed =
            HookWorkflow::uninstall(directory.path(), SyncMode::Apply { force: false }).unwrap();
        assert_eq!(removed.state, HookState::Absent);
        assert_eq!(
            HookWorkflow::status(directory.path()).unwrap().state,
            HookState::Absent
        );
    }

    #[cfg(unix)]
    #[test]
    fn force_install_preserves_and_uninstall_restores_existing_hook() {
        let directory = test_repository();
        let repository = GitRepository::discover(directory.path()).unwrap();
        let hook = repository.hooks_path().unwrap().join(HOOK_NAME);
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        let marker = directory.path().join("original-ran");
        let original = format!("#!/bin/sh\ntouch {}\n", marker.display());
        std::fs::write(&hook, &original).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).unwrap();

        let blocked =
            HookWorkflow::install(directory.path(), SyncMode::Apply { force: false }).unwrap();
        assert_eq!(blocked.state, HookState::Conflict);
        assert_eq!(std::fs::read_to_string(&hook).unwrap(), original);

        let installed =
            HookWorkflow::install(directory.path(), SyncMode::Apply { force: true }).unwrap();
        assert_eq!(installed.state, HookState::Installed);
        assert_eq!(
            std::fs::metadata(&hook).unwrap().permissions().mode() & 0o7777,
            GENERATED_MODE
        );
        let predecessor = hook.parent().unwrap().join(PREDECESSOR_NAME);
        assert_eq!(std::fs::read_to_string(&predecessor).unwrap(), original);

        let hook_result = std::process::Command::new(&hook)
            .args([ZERO_REF, "new", "1"])
            .current_dir(directory.path())
            .env("SKILLATOR_NO_AUTO_SYNC", "1")
            .output()
            .unwrap();
        assert!(hook_result.status.success());
        assert!(marker.exists());

        HookWorkflow::uninstall(directory.path(), SyncMode::Apply { force: false }).unwrap();
        assert_eq!(std::fs::read_to_string(&hook).unwrap(), original);
        assert!(!predecessor.exists());
    }

    #[test]
    fn uninstall_refuses_a_modified_managed_hook() {
        let directory = test_repository();
        HookWorkflow::install(directory.path(), SyncMode::Apply { force: false }).unwrap();
        let repository = GitRepository::discover(directory.path()).unwrap();
        let hook = repository.hooks_path().unwrap().join(HOOK_NAME);
        let mut changed = std::fs::read(&hook).unwrap();
        changed.extend_from_slice(b"# local edit\n");
        std::fs::write(&hook, &changed).unwrap();

        let report =
            HookWorkflow::uninstall(directory.path(), SyncMode::Apply { force: false }).unwrap();
        assert_eq!(report.state, HookState::Modified);
        assert_eq!(std::fs::read(&hook).unwrap(), changed);
    }

    #[test]
    fn stale_uninstall_does_not_remove_a_hook_changed_after_planning() {
        let directory = test_repository();
        HookWorkflow::install(directory.path(), SyncMode::Apply { force: false }).unwrap();
        let context = HookContext::discover(directory.path()).unwrap();
        let inspection = inspect(&context).unwrap();
        std::fs::write(&context.hook_path, b"#!/bin/sh\nexit 0\n").unwrap();

        let result = remove_managed_hook(&context, &inspection);
        assert!(result.is_err());
        assert!(context.hook_path.exists());
        assert_eq!(
            std::fs::read(&context.hook_path).unwrap(),
            b"#!/bin/sh\nexit 0\n"
        );
    }

    #[test]
    fn force_install_refuses_an_occupied_predecessor_path() {
        let directory = test_repository();
        let repository = GitRepository::discover(directory.path()).unwrap();
        let hook = repository.hooks_path().unwrap().join(HOOK_NAME);
        let predecessor = hook.parent().unwrap().join(PREDECESSOR_NAME);
        std::fs::write(&hook, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(&predecessor, b"reserved\n").unwrap();

        let report =
            HookWorkflow::install(directory.path(), SyncMode::Apply { force: true }).unwrap();
        assert_eq!(report.state, HookState::Blocked);
        assert_eq!(report.exit_status, 1);
        assert_eq!(std::fs::read(&hook).unwrap(), b"#!/bin/sh\nexit 0\n");
        assert_eq!(std::fs::read(&predecessor).unwrap(), b"reserved\n");
    }
}
