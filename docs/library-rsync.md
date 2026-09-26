---
type: Playbook
title: Synchronize libraries across SSH hosts
description: Configure hosts, synchronize exact Git revisions and user selections, and recover partial runs.
status: draft
generated: { by: codex/gpt-6, at: 2026-09-19T00:00:00Z }
---

# Synchronize libraries across SSH hosts

`skillator library rsync` synchronizes library skill files and account-wide selections between this machine and configured SSH hosts. Changes can originate on any selected host. All selected hosts participate in one comparison, so an edit on one remote can reach another in the same run.

## Configure hosts

Install Skillator with synchronization protocol 2, Git, and rsync on every host. The remote noninteractive shell must find all three on PATH. Skillator reports missing or incompatible dependencies and does not install them.

Create `~/.skillator/config.yaml` on the initiating machine:

```yaml
version: 1
hosts:
  build:
    destination: build
  development:
    destination: developer@development
```

Use SSH configuration for ports, jump hosts, keys, and authentication. Establish SSH host trust separately before running Skillator. Synchronization uses batch authentication, requires an already trusted host key, and suppresses trust-file and persistent connection updates. The protocol session and rsync use the same SSH destination and settings.

Aliases label reports and select hosts. `local` is reserved for the initiating machine. Duplicate aliases, unknown fields, malformed destinations, and an empty host selection are errors. This configuration stays local.

```sh
# Preview every configured host.
skillator library rsync --check

# Synchronize every configured host.
skillator library rsync

# Select a subset explicitly.
skillator library rsync --hosts build,development

# Produce a report without interactive conflict questions.
skillator library rsync --format json
```

## What moves

Skillator includes all discovered skills, including hidden and unselected skills, and their supporting files. Location exclusions still apply. Invalid skills retain their diagnostics and do not become enabled. File identity includes content, type, and executable mode.

The central library and external locations keep their paths relative to each user's home. For example, `~/Development/acme/skills` maps to `/home/developer/Development/acme/skills` on a host whose home is `/home/developer`. Locations must be directories below the home. Registering the home itself as `~` is unsupported by `library rsync`; choose its skill-containing subdirectories instead. Locations outside the home, including links that escape it, fail validation. Existing receiving registrations and equivalent path expressions remain intact; missing registrations use portable `~/...` paths. Conflicting exclusions or overlap settings block the affected location.

Library acquisition links retain their originating link and transfer the skill content to receivers. Internal skill links must be relative and remain inside their skill directory. User materialization links are rebuilt against each receiving library.

Rsync transfers skill content only. It does not copy Git administrative files, project selections, user materialization directories, the host configuration, credentials, or the target registry. A Git bootstrap creates a complete checkout, which can include unrelated repository files.

## Git revisions stay fixed

For a missing Git source, Skillator clones its recorded origin and checks out the initiating machine's exact commit. The new checkout uses detached HEAD. Skillator retains the initiating branch name, when present, with the source origin and commit in synchronization history. This provenance does not require branch-name alignment or move either checkout. An upstream branch advancing does not change that commit.

Each receiving host needs its own access to the origin. An unavailable origin or an unpublished commit that cannot be fetched blocks that source. Skillator never substitutes a branch head or forwards Git credentials automatically.

An existing checkout must match the initiating source identity, origin, and commit. A mismatch, unmerged index, or occupied non-repository destination blocks that source on all participants. Other sources can continue. Align mismatched checkouts separately with Git, preserving any local work, then retry.

Dirty tracked skill files and untracked skills synchronize as working-tree content. Existing indexes and unrelated dirty files remain untouched. A local worktree or submodule source becomes an independent remote checkout at its exact commit. A Git source found only remotely needs a corresponding initiating checkout before it can synchronize.

Repository updates are separate. This command does not pull, merge, rebase, reset an existing checkout, create commits, move existing branches, or push. A future library update workflow is outside this command.

## Choose conflict and missing-file policies

```sh
skillator library rsync --conflict ask --missing copy
skillator library rsync --conflict local
skillator library rsync --missing remove
```

| Option | Behavior |
| --- | --- |
| `--conflict ask` | Default. Offer candidate versions in an interactive text session, including absence. Enter skips. |
| `--conflict local` | Choose the initiating value for a conflict, including an explicit deletion. |
| `--conflict remote` | Choose a single distinct changed remote value. Different competing remote edits remain unresolved. |
| `--missing copy` | Default. Restore a non-conflicting missing entry from an existing participant. |
| `--missing remove` | Propagate non-conflicting absence only with acknowledged prior presence on the affected participants. First-contact unmatched entries remain intact. |
| `--missing ignore` | Preserve a non-conflicting one-sided absence without inventing a shared baseline. |

Conflicts take precedence over missing-file policy. Deleting a file while another participant edits it is a conflict. Timestamps do not choose winners, and Skillator does not merge text automatically. Noninteractive commands, machine formats, and check mode never prompt. Unresolved conflicts return status 1 while independent work can still complete.

A choice does not authorize deleting unrelated siblings or overwriting user materialization drift. There is no `--force` option for this command.

## User selections

Skill Directory definitions and Enablements merge by their keys, including linked or copied mode. First contact combines independent selections. After shared history exists, explicit deselection propagates even with `--missing copy`; the library skill remains available. An absent entire user configuration does not mean every skill was deselected.

Directory removal stays coupled to its selections. Concurrent incompatible changes use the conflict policy. Unavailable or blocked sources cannot gain new dependent selections. Existing unresolved selections remain.

Receiving hosts use normal protected user reconciliation. Unmanaged entries and edited copied materializations remain guarded. Reports distinguish a saved user configuration from materialization outcomes; failed materialization prevents an in-sync result. Copied user edits are not promoted into the library.

## Preview and reports

`--check` creates no clones, staging, persistent locks, configuration, identities, or history. It reports missing Git checkouts as exact-commit bootstrap plus unverified follow-on work. Run it again after bootstrap to inspect those destinations. It cannot prove future origin availability or prevent changes made after observation.

Every selected host must pass read-only dependency and configuration preflight before persistent writes start anywhere. A failed host aborts that run. Failures after preflight can leave successful independent work in place.

JSON and YAML use the normal report envelope with `mode: library_rsync`. Changes identify the configured `host` alias and home-relative path. Reports distinguish applied, proposed, blocked, unauthorized, and failed user mutations. Exit codes are 0 for convergence, 1 for remaining work, 2 for invalid arguments, 3 for unavailable or invalid required input, 4 for a busy target, and 5 for fatal command/output failures.

## Retry and recovery

Synchronization identity and acknowledged history live under `~/.skillator/rsync/`. Do not copy this state directory between machines. A duplicated identity or replaced host is an error. Restore the original state if available. If a host was deliberately replaced, back up the initiating state and remove only that host's saved entry from the `aliases` map to permit conservative first contact. Lost shared history never authorizes automatic deletion.

Each publication verifies staged content and rechecks the observed source and destination. A stale entry is preserved and reported for retry. Concurrent initiators cannot hold the same participant lock.

After interruption, rerun the command. It inspects retained publication journals, restores recoverable originals where safe, and refuses to overwrite later edits. Preserve any reported journal and backup when manual recovery is required. Once the old and current values are reconciled, retry. Successful entries retain their own acknowledgements; a failed host does not cause all other hosts to roll back.

The names `.skillator-rsync-<32 hex digits>` and `.skillator-clone-<32 hex digits>` are reserved temporary entries and excluded from discovery during synchronization. Do not use them for skill content.

Removing a host from configuration stops future contact. Removing a library registration is not replicated and does not delete its source tree.
