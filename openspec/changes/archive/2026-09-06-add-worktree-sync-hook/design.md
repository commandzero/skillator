## Context

The existing worktree workflow already has a safe `skillator sync worktree [directory]` operation that copies the primary Target configuration and reconciles the linked worktree. Git invokes `post-checkout` after `git worktree add` performs its checkout, but the event is also used by `git clone`, ordinary checkouts, and switches. Git exports repository-local environment variables to hooks, and linked worktrees use private Git metadata alongside a shared common directory. See the proposal and delta specs for the user-facing contract.

## Goals / Non-Goals

**Goals:**

- Make automatic synchronization an explicit, repository-local opt-in.
- Detect only newly populated linked worktrees and leave clone, switch, file-checkout, and no-checkout behavior alone.
- Preserve existing hooks and make installation, removal, and retry safe under concurrent or partially completed operations.
- Reuse the existing worktree synchronization workflow, locks, safety classifications, and reports.
- Keep hook failures from making an already-created worktree appear unusable.

**Non-Goals:**

- Wrapping or replacing `git worktree add`, `git clone`, or other Git commands.
- Enabling hooks globally or changing a repository's tracked files, `.gitignore`, or Skillator configuration.
- Synchronizing every existing worktree after a primary-worktree commit, merge, or configuration edit.
- Adding a background watcher or a new worktree removal hook.

## Decisions

### Use a generated `post-checkout` hook

`skillator hook install` will resolve the effective hooks directory with Git, then install a generated executable `post-checkout` script. The script checks the hook arguments for Git's null previous ref and verifies that the current Git directory is a linked-worktree private directory rather than the common directory used by the main worktree. This filters out normal checkouts and the similar `post-checkout` event emitted by `git clone`.

An event-specific Git wrapper or a new `skillator worktree add` command was rejected. Git already owns worktree creation, and the existing explicit Git-plus-Skillator sequence must remain usable in agents and CI.

### Keep installation local and opt-in

Installation operates on the repository's resolved hooks path, including a configured `core.hooksPath`, and does not write repository content or persistent Skillator configuration. The command is explicit rather than being hidden inside `init` or `sync`, because hooks execute code during unrelated Git commands and are not a safe default for every checkout.

A global installation mode is out of scope. Linked worktrees normally resolve hooks through the common repository Git directory, so installing from the primary worktree covers its linked worktrees without changing each worktree's files.

### Preserve existing `post-checkout` hooks with guarded chaining

The generated hook carries a Skillator marker and a deterministic version. An absent hook or an unchanged Skillator hook can be installed or refreshed idempotently. An unrelated readable regular hook is a guarded conflict. `--force` may move it to a Skillator-owned predecessor file, preserve its bytes and permissions, and install a wrapper that invokes the predecessor and then Skillator. Unreadable, non-regular, ambiguous, or already-occupied predecessor paths are blocked rather than guessed through.

Uninstall compares the managed wrapper and predecessor fingerprints before changing anything. If the wrapper is unchanged, it removes it and restores the preserved predecessor when one exists. If either file was edited, uninstall reports the ownership conflict and leaves both files in place. This avoids deleting user changes made after installation.

### Sanitize the hook environment before calling Skillator

The shell hook will perform its initial Git checks while Git's hook environment is present, then clear the variables returned by `git rev-parse --local-env-vars` before starting Skillator. Skillator can then rediscover the destination and primary worktrees through its normal `git -C` calls instead of accidentally reusing the hook's private `GIT_DIR`.

The hook invokes `skillator sync worktree .` by executable name at event time, so normal package upgrades take effect without rewriting every hook. If the executable is not on the hook's `PATH`, the hook reports a retryable diagnostic and exits through the non-blocking path.

### Make synchronization advisory to Git

The hook runs the existing worktree sync after Git has created the worktree. It forwards any preserved predecessor's exit status, but it does not turn a Skillator non-converged or fatal result into a hook failure. The report is sent to the hook's diagnostic stream with color disabled, and the message includes the explicit retry command. `SKILLATOR_NO_AUTO_SYNC=1` skips the Skillator part of the hook for one Git operation.

This avoids claiming that `git worktree add` failed when the directory already exists and only Skillator's optional setup could not complete. The explicit `skillator sync worktree <directory>` command remains authoritative for agents, CI, `--no-checkout`, and manual retries.

### Use a dedicated hook report type

Hook install, status, and uninstall will use a compact report with the repository, resolved hook path, ownership state, changes, diagnostics, and stable format version. It will follow the existing text, JSON, YAML, ANSI, ordering, and exit-status conventions without forcing hook-specific fields into reconciliation reports.

## Risks / Trade-offs

- [A hook can surprise users by doing filesystem work during `git worktree add`] → Installation is explicit, repository-scoped, and opt-out is available per operation. Documentation keeps the explicit two-command workflow visible.
- [The hook may run with a restricted `PATH` in an IDE or GUI] → Resolve the executable at event time, report a concise retry command, and never block worktree creation when it is unavailable.
- [Existing hook chaining can lose user behavior if handled incorrectly] → Require `--force` for readable conflicts, preserve exact bytes and permissions, fingerprint managed files, and refuse ambiguous or modified uninstall cases.
- [A clone and a new linked worktree share the null previous ref] → Check linked-worktree Git metadata before invoking Skillator and cover both events with integration tests.
- [Primary configuration or Library state can change while the hook runs] → Reuse existing Target locks and stale checks; treat a non-converged sync as an advisory diagnostic and permit an explicit retry.
- [Hook implementations are Unix shell programs] → Keep the script POSIX-compatible and retain Skillator's documented macOS, Linux, and WSL support boundary.

## Migration Plan

1. Add hook discovery, ownership, report, and atomic file-operation helpers without changing synchronization behavior.
2. Add the `hook` CLI group and generated hook template, then cover install, conflict, chaining, status, uninstall, opt-out, clone, checkout, worktree-add, and failure paths.
3. Document `skillator hook install` and the explicit fallback in CLI help, README, and the project-owned agent skill.
4. Existing installations require no migration. Users who want automatic behavior run the install command in each repository; uninstall removes only unchanged Skillator-owned files.

Rollback consists of running `skillator hook uninstall` or removing an unchanged managed hook. It does not alter Target, User Scope, Library, or tracked repository state.
