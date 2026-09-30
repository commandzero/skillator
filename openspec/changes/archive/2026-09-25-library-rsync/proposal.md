## Why

An agent running over SSH cannot use the initiating machine's skill library or user selections. [Issue #28](https://github.com/commandzero/skillator/issues/28) calls for explicit two-way synchronization so skills remain usable and edits survive across hosts.

## What changes

1. Add `skillator library rsync`, using configured SSH hosts, installed remote Skillator, Git, and rsync. Include all hosts by default and support `--hosts host1,host2`.
2. Mirror all discovered skills and user selections at corresponding home-relative paths, including external library locations. Recreate user materializations on each host.
3. Bootstrap missing Git sources from their recorded origins at the initiating host's exact commit. Block existing checkouts at a different commit. Transfer untracked skill content and uncommitted skill edits with rsync.
4. Gather all selected hosts before planning changes. Resolve conflicts with `--conflict local|remote|ask`, defaulting to `ask`. Do not choose between conflicting remote versions by host order.
5. Handle missing skill content with `--missing copy|remove|ignore`, defaulting to `copy`. Propagate removals only with history proving prior presence. Synchronize explicit user deselections independently.
6. Preflight every selected host before writes. A missing or incompatible Skillator, failed connection, or missing dependency aborts the run. Support read-only `--check` and structured reports.

## Capabilities

### New capabilities

1. `library-rsync`: Host configuration, path mapping, Git bootstrap, multi-host synchronization history, conflict and removal policies, and recovery.

### Modified capabilities

1. `library-management`: Explicit remote synchronization can create inventory and register corresponding library locations.
2. `user-scope-onboarding`: Explicit synchronization exchanges user desired state and reconciles host-local materializations.
3. `cli-contract`: Add the remote command, allow its narrowly scoped Git clone behavior, and define host-aware machine reports.

## Impact

The change affects CLI parsing, configuration and application workflows, library discovery, user-scope reconciliation, and report rendering. Add private SSH, Git bootstrap, rsync transport, and synchronization-state modules while retaining the single-crate structure. Remote operations require compatible Skillator, Git, and rsync installations; SSH uses existing user authentication.

## Non-goals

1. Repository updates, automatic pull, merge, rebase, reset, commits, pushes, or a `library update` command.
2. Automatic dependency installation, project selection synchronization, target registry replication, or background scheduling.
3. Paths outside the participating user's home, arbitrary directory backup, or host-to-host SSH connections.
