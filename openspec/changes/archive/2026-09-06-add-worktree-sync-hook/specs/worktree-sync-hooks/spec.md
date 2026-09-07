## Purpose

Provides an explicit, safe opt-in Git hook that synchronizes a newly created linked worktree with the primary Target configuration.

## ADDED Requirements

### Requirement: Hook lifecycle is explicit and repository-scoped

Skillator SHALL provide non-interactive `skillator hook install [repository]`, `skillator hook status [repository]`, and `skillator hook uninstall [repository]` commands. The repository argument SHALL default to `.` and SHALL resolve through Git. Installation and removal SHALL affect only the repository's resolved hooks path and SHALL not modify Repository Configuration, User Scope, Library Configuration, Target registrations, the index, or tracked repository files.

#### Scenario: Install a hook in a Git repository

- **WHEN** the user runs `skillator hook install` in a Git worktree with no conflicting `post-checkout` hook
- **THEN** Skillator installs its managed hook at Git's resolved hooks path and reports the installed state

#### Scenario: Install outside Git

- **WHEN** the user runs `skillator hook install` for a directory that is not inside a Git worktree
- **THEN** Skillator reports invalid input and leaves every file unchanged

#### Scenario: Preview installation

- **WHEN** the user runs `skillator hook install --check`
- **THEN** Skillator performs the same Git discovery and conflict checks as apply mode, reports the planned hook change, and writes nothing

#### Scenario: Repeated installation

- **WHEN** the managed hook is already installed and unchanged
- **THEN** `skillator hook install` reports an unchanged result and leaves the hook bytes and permissions unchanged

### Requirement: Existing hooks are preserved and conflicts are explicit

Skillator SHALL never silently overwrite an unrelated `post-checkout` hook. An existing regular hook SHALL be reported as a guarded conflict unless the user explicitly authorizes installation with `--force`. A forced installation SHALL preserve the existing hook and invoke it as part of the resulting chain. An unreadable, non-regular, or otherwise uninspectable hook SHALL be reported as blocked and SHALL not be replaced.

#### Scenario: Existing hook blocks ordinary installation

- **WHEN** an unrelated `post-checkout` hook exists and the user runs `skillator hook install` without `--force`
- **THEN** Skillator reports the conflict, makes no hook change, and returns the guarded non-converged status

#### Scenario: Forced installation chains an existing hook

- **WHEN** an unrelated readable `post-checkout` hook exists and the user runs `skillator hook install --force`
- **THEN** Skillator preserves the existing hook, installs its managed integration, and runs both hooks for later events

#### Scenario: Uninstall preserves a changed hook

- **WHEN** the managed hook or its preserved predecessor has changed since installation
- **THEN** `skillator hook uninstall` reports the ownership conflict and leaves the current hook files unchanged

### Requirement: The managed hook only synchronizes new linked worktrees

The managed hook SHALL invoke `skillator sync worktree .` only after Git populates a newly created linked worktree. It SHALL skip ordinary branch switches and file checkouts, the primary worktree, and a repository's main worktree after `git clone`. Git's `--no-checkout` behavior remains an explicit-sync case because it does not invoke `post-checkout`.

#### Scenario: New linked worktree is synchronized

- **WHEN** the user runs `git worktree add` without `--no-checkout` and the managed hook is installed
- **THEN** the hook runs worktree synchronization from the new linked worktree, using the primary configuration and current Library

#### Scenario: Ordinary checkout does not synchronize

- **WHEN** the user switches branches or checks out a file in an existing worktree
- **THEN** the hook does not run worktree synchronization

#### Scenario: Clone does not synchronize as a linked worktree

- **WHEN** the hook runs after `git clone`
- **THEN** the hook recognizes the main worktree and does not invoke linked-worktree synchronization

#### Scenario: No-checkout requires an explicit sync

- **WHEN** the user runs `git worktree add --no-checkout`
- **THEN** the hook does not run and the user can synchronize later with `skillator sync worktree <directory>`

### Requirement: Hook execution is safe for Git and does not block worktree creation

Before invoking Skillator, the managed hook SHALL clear Git's hook-local repository environment so that Skillator rediscovers the current and primary worktrees normally. A missing Skillator executable, unavailable primary configuration, unresolved Library Skill, guarded change, or other synchronization failure SHALL be reported as a diagnostic and SHALL not make the hook return a failure solely because synchronization did not converge. The hook SHALL honor `SKILLATOR_NO_AUTO_SYNC=1` as a per-operation opt-out.

#### Scenario: Synchronization fails after worktree creation

- **WHEN** the new worktree cannot be synchronized because its primary configuration or Library is unavailable
- **THEN** the hook reports the failure, leaves the created worktree available, and returns success from the Skillator hook path

#### Scenario: One operation opts out

- **WHEN** `SKILLATOR_NO_AUTO_SYNC=1` is present during `git worktree add`
- **THEN** the hook skips synchronization without changing any Skillator state

#### Scenario: Existing hook failure remains visible

- **WHEN** a preserved pre-existing hook in the chain returns a failure
- **THEN** Skillator does not hide that hook's result or claim that Git's original hook succeeded

### Requirement: Hook status exposes deterministic ownership state

`skillator hook status` SHALL report whether the managed hook is absent, installed and unchanged, installed with local modifications, or blocked by an unrelated hook. It SHALL include the resolved hook path and SHALL support the same deterministic text, JSON, and YAML output rules as other read-only CLI reports.

#### Scenario: Status for an installed hook

- **WHEN** the user runs `skillator hook status --format json`
- **THEN** Skillator emits one deterministic machine-readable report identifying the resolved hook path and managed state without writing files

#### Scenario: Status for a conflict

- **WHEN** an unrelated `post-checkout` hook exists
- **THEN** status reports the conflict and does not classify the hook as Skillator-managed

### Requirement: Explicit synchronization remains supported

The existing `skillator sync worktree [directory]` command SHALL remain the authoritative synchronization operation when the hook is absent, disabled, bypassed by `--no-checkout`, or unable to converge. Hook installation SHALL not alter its command semantics or its existing safety and reporting behavior.

#### Scenario: Manual retry after an automatic failure

- **WHEN** automatic synchronization reports a failure
- **THEN** the user can run `skillator sync worktree <directory>` to retry using the normal check, force, lock, and report rules
