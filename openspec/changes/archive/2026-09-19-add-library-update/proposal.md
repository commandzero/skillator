## Why

Updating Library Skills currently requires pulling each Git repository separately.
[Issue #29](https://github.com/commandzero/skillator/issues/29) requests one command across registered Library Locations.

## What changes

1. Add `skillator library update` to pull each distinct eligible checkout once.
2. Permit only fast-forward pulls of clean attached branches with a configured upstream. Continue after independent failures.
3. Skip submodule pulls with an advisory, including directly registered submodules.
4. Add local-only `--check` and text, JSON, and YAML reports. Preview says "Would attempt pull; remote state not checked."
5. Default each pull to 30 seconds, configurable through `--timeout <seconds>`. Stop timed-out subprocesses and continue; Ctrl+C stops the batch.
6. Preserve configuration, Enablements, and copies. Existing links expose updated Source content.

## Capabilities

### New capabilities

None.

### Modified capabilities

1. `library-management`: Discover distinct checkouts and update independent Sources.
2. `cli-contract`: Define command options, preview, timeout, cancellation, and reports.

## Impact

Extend Library discovery, Git process handling, application workflows, CLI reports, tests, and documentation.
No configuration migration is needed. Clone, worktree management, branch switching, stashing, merge, rebase, and Materialization synchronization remain separate.
