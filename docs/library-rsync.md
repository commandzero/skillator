---
type: Playbook
title: Synchronize libraries across SSH hosts
description: Configure hosts, synchronize library content at exact Git revisions, and recover partial runs.
status: draft
generated: { by: openai-codex/gpt-6-astra, at: 2026-09-26T19:55:21Z }
---

# Synchronize libraries across SSH hosts

`skillator library rsync` synchronizes library skill files between this machine and configured SSH hosts. Changes can originate on any selected host. All selected hosts participate in one comparison, so an edit on one remote can reach another in the same run. User selections and materializations remain host-local.

## Configure hosts

Install Skillator with synchronization protocol 5, Git, and rsync on every host. The remote noninteractive shell must find all three on PATH. Skillator reports missing or incompatible dependencies and does not install them.

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

Library acquisition links retain their originating link and transfer the skill content to receivers. Internal skill links must be relative and remain inside their skill directory.

Rsync transfers skill content only. It does not copy Git administrative files, project or user selections, the standard `~/.agents/skills` materialization directory, the host configuration, credentials, or the target registry. A Git bootstrap creates a complete checkout, which can include unrelated repository files.

Payloads are batched per peer in groups of at most 64 entries. Remote-to-remote content passes through the initiating machine. Each batch uses an isolated staging view, and every entry retains its own content verification, race checks, publication, recovery, and acknowledgement. A failed batch does not publish its destination files or prevent independent peers from completing.

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

A choice does not authorize deleting unrelated siblings or changing user materializations. There is no `--force` option for this command.

## User selections stay local

The command neither reads nor changes `~/.agents/skillator.yaml`. Different, absent, or malformed user configurations do not block library synchronization. Skill Directory definitions, Enablements, and linked or copied modes are not exchanged or included in synchronization history.

Enable or disable skills independently on each host with the normal user-scope commands. Library synchronization does not reconcile user materializations or promote copied user edits into the library. A copied materialization stays unchanged until a local user-scope action updates it.

## Preview and reports

`--check` creates no clones, staging, persistent locks, configuration, identities, or history. It reports missing Git checkouts as exact-commit bootstrap plus unverified follow-on work. Run it again after bootstrap to inspect those destinations. It cannot prove future origin availability or prevent changes made after observation.

Every selected host must pass read-only dependency and configuration preflight before persistent writes start anywhere. A failed host aborts that run. Failures after preflight can leave successful independent work in place.

JSON and YAML use the normal report envelope with `mode: library_rsync`. Changes identify the configured `host` alias and home-relative path. Reports cover library content, registrations, Git bootstrap, and synchronization state, not user-scope mutations. Exit codes are 0 for convergence, 1 for remaining work, 2 for invalid arguments, 3 for unavailable or invalid required input, 4 for a busy target, and 5 for fatal command/output failures.

## Retry and recovery

Synchronization identity and acknowledged history live under `~/.skillator/rsync/`. Do not copy this state directory between machines. A duplicated identity or replaced host is an error. Restore the original state if available. If a host was deliberately replaced, back up the initiating state and remove only that host's saved entry from the `aliases` map to permit conservative first contact. Lost shared history never authorizes automatic deletion.

Protocol 5 uses history format 2. Earlier unreleased protocol and history versions are rejected rather than migrated. Before upgrading from an earlier build, finish pending recovery with that build. Back up and move aside `~/.skillator/rsync/state.json` on every participating host to restart with conservative first contact. Do not change the version field or discard retained recovery journals and backups.

Each publication verifies staged content and rechecks the observed source and destination. A stale entry is preserved and reported for retry. Concurrent initiators, Library configuration saves, and `library update` share the user-home write lock.

File reads reject a final symlink and validate the opened descriptor as a regular file before consuming content. Its device and inode must match the initial metadata inspection, preventing a replacement between that inspection and opening from redirecting the read. Nonblocking opens prevent a concurrently substituted FIFO from hanging synchronization.

Symlink targets are read without following them, and link identity is checked again after the read. Containment or I/O errors while filtering historical paths abort planning with the affected path rather than silently omitting it.

After interruption, rerun the command. It inspects retained publication journals, restores recoverable originals where safe, and refuses to overwrite later edits. Preserve any reported journal and backup when manual recovery is required. Once the old and current values are reconciled, retry. Successful entries retain their own acknowledgements; a failed host does not cause all other hosts to roll back.

Partial multi-host completion can leave different acknowledged baselines. An all-host retry then reports a history conflict rather than guessing. Retry the failed peer with `--hosts <alias>` against its unchanged baseline, then retry the complete cohort. If history remains inconsistent, preserve the state and reconcile it before proceeding.

The names `.skillator-rsync-<32 hex digits>`, `.skillator-clone-<32 hex digits>`, and `.skillator-alias-<32 hex digits>` are reserved temporary entries and excluded from discovery during synchronization. Do not use them for skill content.

Removing a host from configuration stops future contact. Removing a library registration is not replicated and does not delete its source tree.
