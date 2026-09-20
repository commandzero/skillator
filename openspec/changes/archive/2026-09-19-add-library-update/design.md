## Context

Library discovery already finds nested Git Sources and keeps colliding Source rows.
Some directory inspection failures are silently skipped and must become diagnostics for a trustworthy update report.

## Goals / Non-goals

1. Reuse discovery boundaries and command reports.
2. Keep updates separate from Target reconciliation and persistent inventory.
3. Do not roll back completed pulls or automate branch repair.

## Decisions

### Discover once and deduplicate checkout paths

Build a fixed, canonical-path-sorted plan from Git Sources, including repositories without valid Skills and colliding Source Keys.
Deduplicate checkout roots, not remote URLs or shared Git directories. Separate worktrees require separate checks.

Identify and skip submodules, including directly registered Locations, with an advisory and no pull row.
Do not follow acquired Skill links, traverse `.git`, select an enclosing repository outside a Location, or update bare repositories.

### Pull only eligible checkouts

Revalidate identity, attached branch, upstream, clean porcelain status including untracked files, and absence of in-progress operations immediately before pulling.
Use read-only queries with optional Git locks disabled. Ignored files alone do not block a pull.

Invoke Git directly with explicit arguments, disabling rebase, autostash, and submodule recursion while requiring fast-forward integration.
Use the full symbolic HEAD ref and strip `refs/heads/` to look up the configured upstream, even when a tag shares the branch name.
Compare HEAD before and after success to classify applied versus unchanged.

Run sequentially, retain successes, and continue after independent failures.
Respect Git locks without removing them or forcing updates.

### Bound subprocess lifetime

Use a monotonic deadline with `--timeout <seconds>`, default 30 positive whole seconds per pull.
Run each pull in a process group and drain captured output concurrently.

On timeout, terminate the group, allow at most 1 second for cleanup, force termination if needed, and reap the child without hanging on inherited pipes.
Report `pull_timeout` and continue. Do not retry or roll back partially changed Git state.

Ctrl+C uses the same cleanup, stops scheduling, emits a partial report, and returns 130.
Retain completed outcomes, mark the interrupted pull failed, and mark remaining planned pulls blocked with cancellation diagnostics.

### Report local eligibility accurately

Preview never fetches, contacts remotes, refreshes the index, or writes metadata.
Text says "Would attempt pull; remote state not checked." Machine output retains `would_apply` with `remote_state_not_checked`.

Use the existing command-report envelope and stable diagnostic codes.
Detect terminal stdout separately from color policy. List each unchanged repository as `up-to-date` in terminal text; keep the unchanged count for redirected text.
Capture Git output, strip ANSI, disable terminal credentials, askpass, editors, and SSH interaction, and never mix raw progress into machine output.
Place enforced SSH batch options before configured executable arguments, preserving quoted executable paths and other arguments. OpenSSH keeps the first value for each option.

## Risks / Trade-offs

1. External Git activity can race inspection. Revalidate and rely on Git's locking and refusal behavior; do not promise a transaction against arbitrary writers.
2. Failed or timed-out pulls can change refs or working content. Report failure without claiming no writes.
3. Hooks and custom transport helpers are user programs, not sandboxed by this command.
4. Slow healthy pulls may exceed 30 seconds. Users can raise the timeout.
5. Parent pulls can change recorded submodule commits. Leave submodule checkouts untouched and report the skip.
6. Existing links expose Source changes immediately; copies need explicit synchronization.

## Migration plan

1. Add implementation, isolated local-remote integration tests, CLI help, and workflow documentation together.
2. Keep existing configuration formats unchanged.
3. Removing the command needs no migration and cannot undo prior pulls.
