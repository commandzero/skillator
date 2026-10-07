---
type: Playbook
title: Deliver leader skills to follower libraries
description: Configure leader pushes and fresh follower pulls with SSH and ordinary rsync.
status: draft
generated: { by: openai-codex/gpt-6.1-sol, at: 2026-10-07T03:48:45Z }
---

# Deliver leader skills to follower libraries

One leader owns the library content. `skillator library rsync` pushes from that leader or pulls from it when invoked on a follower. Content always moves **leader → follower**. Follower edits are overwritten; no merge, conflict prompt, or follower upload occurs. User selections and materializations remain host-local.

## Configure the leader

Create `~/.skillator/config.yaml` on the authoritative host:

```yaml
version: 1
hosts:
  build:
    destination: build
  development:
    destination: developer@development
```

The existing `hosts` form means leader. Aliases label reports and select followers; `local` remains reserved. The leader must have Skillator, rsync, and its usual library-discovery dependencies, including Git for Git Sources. Configure its Library through the normal workspace before syncing.

Host entries also accept optional `hostname` metadata, for example:

```yaml
version: 1
hosts:
  build:
    destination: build
    hostname: worker-07.example.net
```

`destination` is the SSH connection target; `hostname` is the separately validated name reported by that machine and is only display metadata. Existing destination-only entries remain valid. Older builds reject this new field: remove `hostname` before downgrading, without changing destinations.
```sh
# Preview all followers without creating their replicas.
skillator library rsync --check

# Push current leader skills to all configured followers.
skillator library rsync

# Push only the selected followers.
skillator library rsync --hosts build,development
```

A pushed-to follower needs SSH, a POSIX shell, rsync, and POSIX `find`, `cmp`, and `mkdir` on its noninteractive PATH. Creating a fresh replica also requires `rm` and `rmdir` for marker-failure cleanup. Each dependency is checked before the operation that needs it makes replica writes. The follower does not need Skillator or Git and does not need a Library configuration.

Before replica creation, both local and remote rsync executables must pass a write-free probe of the required transfer options, including `--delete-delay`. An executable that only answers `--version` is insufficient; update older rsync installations before retrying.

Receiver `find` must also pass a write-free probe of the same regular-file/symlink and `-links +1` predicates used for inode safety. Finding an executable on PATH is insufficient. The probe runs before any replica directory or ownership marker is created, including when inspection reports an absent replica.
## Configure a follower that initiates pulls

Create `~/.skillator/config.yaml` on that follower:

```yaml
version: 1
leader:
  destination: developer@main
```

Use exactly one form: nonempty `hosts` on a leader, or one `leader.destination` on a follower. Both forms, neither form, unknown fields, duplicate keys, malformed destinations, and unsupported versions are rejected. Configuration is local; there are no host IDs or enrollment steps. `--hosts` is invalid on a follower.

```sh
# Preview a fresh pull from the configured leader.
skillator library rsync --check

# Pull the leader's current skills into this follower's replica.
skillator library rsync

# Use the normal machine report format.
skillator library rsync --format json
```

The initiating follower needs Skillator, SSH, and rsync. The leader's noninteractive PATH must contain Skillator, rsync, `cmp`, `rm`, `find`, and `chmod`, plus its library-discovery dependencies. These export validation and cleanup utilities are checked before preparing a temporary export. The pull asks the leader to prepare a fresh private temporary export using its installed Skillator; a previous leader push is not required. Before transfer, Skillator verifies the export's physical temporary-directory parent, physical root, and ownership marker; cleanup repeats those checks before removing it. The hidden export-only helper is not a general remote command protocol or cached publication.

All roles need a rsync that supports `--delete-delay`. Upgrade an older system copy when that option is unavailable; Skillator does not install or replace it.

Destinations are ordinary SSH aliases or hostnames, optionally prefixed with `user@`. IPv6 literals must be bracketed, for example `destination: 'user@[2001:db8::1]'`; unbracketed colons, port suffixes, and remote paths are rejected. An invocation selecting an IPv6 destination requires the initiating rsync to advertise IPv6 capability, as GNU rsync does. System openrsync accepts `--ipv6` but misparses bracketed hosts, so it is refused before connections, export preparation, or replica writes for such an invocation. Receiving rsync servers do not need to parse the destination.

Use SSH configuration for ports, jump hosts, keys, and authentication. Establish host trust separately. Connections use batch authentication, require trusted host keys, and suppress trust-file and persistent connection updates.

## Register and browse followers in the TUI

Select Library and press `Ctrl+T`. Enter a unique follower name matching an SSH alias in `~/.ssh/config`; `local` is reserved. Skillator runs an argument-separated `ssh -T destination hostname` with batch authentication and existing trusted keys. It never edits SSH configuration or establishes trust. A valid single hostname is required on stdout; stderr warnings are displayed separately. Authentication, trust, connection, timeout, or malformed-output failures leave the registry unchanged. Fix the SSH setup externally and retry.

A successful probe stages a host tab and preserves the input alias as its destination even if the reported hostname differs. Press `s` on Local and confirm to publish `~/.skillator/config.yaml`; quit or discard without saving to leave it unchanged. After a successful save adds a follower, Skillator asks whether to initialize it with a sync; when several new hosts were saved, it asks in registration order. Declining keeps each registration without creating its replica. A host-only `Ctrl+S` save-and-exit waits for these decisions and any accepted delivery before exiting. Invalid registries and externally changed files are not overwritten. A pull-initiating follower's `leader` role is preserved and cannot register downstream followers. Library inventory and host-registry saves report their separate outcomes.

Use `Tab` / `Shift+Tab` to browse Local and follower Library tabs. On a saved remote host, `s` and `Ctrl+S` both request synchronization of **only the selected follower** from this leader; neither saves pending Local/registry edits nor exits. Confirm the warning before transfer: rsync may overwrite content and delete stale files only inside that follower's **owned** `~/.skillator/library/replica`. Cancel the prompt to leave the replica unchanged. On Local, `s` retains its confirmed save behavior and `Ctrl+S` retains safe save-and-exit. Pending changes still require the normal save/discard/return decision before host navigation; an unsaved registration or dirty Library inventory is not transfer input. An externally changed saved alias/destination or invalid/unavailable leader source blocks delivery, rather than silently targeting another host or using cached inventory.

Delivery runs in the background so navigation remains responsive; repeated sync keys do not start duplicate transfers. Switching away cancels the running local SSH/rsync process groups and ignores stale results. Interrupted rsync may leave partial changes **inside the owned replica**; there is no atomic transfer, rollback, or rsync/network timeout guarantee. The TUI presents transfer/cleanup failures, including a retained private export path when cleanup fails, and refreshes inspection after successful delivery. Follower tabs otherwise inspect only the owned replica through ordinary SSH/POSIX tools, without remote Skillator or Git. They display delivered Source/Skill metadata and document diagnostics, not a copy of local inventory. Mutation actions remain disabled. Missing, offline, unmarked, multiply linked, or symlinked replicas remain unavailable with diagnostics; other scopes stay usable.

TUI hostname probes have a 15-second overall deadline and 4 KiB stdout/stderr limits. Replica inspection has a 30-second deadline, a 4 MiB inventory-output limit, a 4 KiB stderr limit, and a 256 KiB limit per skill document. Escape cancels a pending probe; switching away cancels inspection and ignores stale replies. These bounds apply to TUI SSH operations, not rsync delivery.

Follower inspection also requires `readlink -n` on the receiver's noninteractive PATH (supported by BSD/macOS and GNU/Linux implementations). A `SKILL.md` relative symlink is readable when its entire chain stays inside that skill's directory; directory links resolve from their physical parent, and at most 40 link hops are followed. Absolute, escaping, broken, cyclic, multiply linked and non-regular documents are rejected. Inspection preserves the registered `SKILL.md` identity and never rewrites links or replica content.

## Inventory and replica layout

The leader exports every discovered valid skill, including hidden and unselected skills, with its complete supporting files. Location exclusions apply to directories, regular files, and symlinks, including supporting content inside a skill. An exclusion matching a discovered skill's root `SKILL.md` rejects the export rather than delivering an invalid skill; nested metadata and supporting-file exclusions remain effective. Unavailable Locations, invalid skills, ambiguous Source Keys, overlapping skill exports, unreadable content, and unsupported entries block the export before replica mutation. Library expressions use UTF-8 environment variables; unrepresentable variables are treated as unavailable rather than panicking. An explicitly configured, available empty library can remove stale replica skills; missing or inaccessible input cannot authorize deletion.

Naming-style deviations and differences between a safe frontmatter name and its source directory are advisory warnings, not invalid skills: they do not block export or delivery. Local, Target and follower skill details show these warnings separately from blocking errors. Delivery preserves the original `SKILL.md`; it does not rewrite metadata or normalize directory names.

Every follower receives the same layout beneath `~/.skillator/library/replica`:

```text
<SourceKey>/_skills/<SkillPath>/SKILL.md
```

For example, `local/library` plus `demo` becomes `local/library/_skills/demo/SKILL.md`. A skill at a Source root uses `_skills/SKILL.md`. Equal skill names from different Sources remain separate. Overlapping root/nested skills are rejected rather than merged ambiguously.

Acquisition-link roots are materialized as ordinary usable directories. Self-contained relative internal links remain links and retain their source timestamps, so repeated fresh exports converge with rsync implementations that preserve link times; escaping or dangling links block delivery. Regular files preserve their executable modes, and skill-root and supporting-directory modes are retained rather than widened by the export's umask. Export preparation uses read-only hard links where possible and copies data with the original modes and times whenever linking is unavailable, including readable files that reject same-filesystem links. Copies do not inherit immutable flags that would block staging or cleanup. The leader's files and acquisition links are not chmodded or rewritten.

Only skill content moves. Git administrative files, unrelated repository files outside skill directories, Library and User Scope configuration, credentials, registries, and materializations are not exported. No Git checkout, revision alignment, origin fetch, or remote registration occurs. Follower Library and User Scope configuration are neither read nor changed by a pull, even when malformed.

Registering a Git repository does not select its entire working tree for transfer. Both push and pull run rsync against an export containing only discovered skill directories, not against the registered repository root. For example, a repository's `tools/check/SKILL.md`, `tools/check/template.txt`, and `tools/check/scripts/run.sh` are delivered; its root `README.md`, `src/`, `docs/`, `.github/`, and `.git/` are not.

The directory containing `SKILL.md` defines the skill-content boundary; all its supporting files participate, not just Markdown files or Git-tracked files. If `SKILL.md` is at the repository root, that root is a skill and its non-Git contents are included. Place skills in dedicated subdirectories when the repository also contains unrelated project files.

## Ownership and deletion boundary

Rsync overwrites follower content and removes stale files **only inside `~/.skillator/library/replica`**. Skillator creates that root with a regular `.skillator-rsync-owned` marker containing its fixed ownership signature. Existing unmarked roots, invalid markers, and symlinked root/ancestors are refused; they are not adopted. Both previews and delivery reject multiply linked regular files and symlinks, including the marker, because rsync metadata updates could otherwise affect an inode outside the replica. Do not place unrelated content in an owned replica.

If ownership-marker initialization fails for a newly created replica, Skillator removes only its own partial marker and empty root so a healthy retry can create the replica normally. Foreign markers and unrelated contents are preserved. Cleanup failures report the original error and retained root on stderr.

Both the remote receiver's home and an initiating follower's local home must resolve to an existing absolute directory without control characters. The physical home path is normalized before checking or creating replica entries, so trailing slashes and redundant path components are accepted consistently. The local follower HOME and the generated leader export path must be representable as UTF-8 in both their configured and physical forms; unsupported paths fail before local replica creation or export publication. Symlinks below that home remain forbidden.

Remote replica inspection is read-only. Skillator validates its complete UTF-8 response and canonical physical root before a separate creation call. Creation rechecks the physical home and replica ancestors and refuses to write if the root changed since inspection. Unsolicited inspection output and non-UTF-8 physical receiver homes therefore fail before receiver directory creation.

Other library locations, checkouts, home siblings, configurations, and user materializations are untouched. The marker is protected from rsync deletion. Content comparison uses checksums, so equal-size/equal-timestamp edits are still replaced. There is no background watcher.

## Preview, reports, and retry

`--check` prepares a temporary leader export but performs no persistent library, replica, marker, or configuration writes. Missing replicas are reported as pending without creating them. Fresh export directory timestamps and marker timestamps do not produce false pending changes; internal link timestamps are preserved from the source. Validated exports are removed after previews and pulls, including transfer failure.

Read-only skill directories retain their source modes during transfer. Before deleting a temporary export, cleanup grants owner access only to its physical directories; it does not follow links or chmod files that may share leader inodes. Ordinary preparation errors use the same cleanup, so a partially built read-only export is removed rather than silently retained. The leader helper also cleans its export if publishing the path fails, including a broken stdout or SSH connection. If cleanup fails, stderr reports both the original failure and the retained export path.

If the leader helper published an export path but a subsequent physical validation call fails, stderr names the candidate export that may remain for manual inspection. Skillator refuses to delete that path until its location and ownership are validated.

Text reports distinguish push and pull. JSON and YAML retain the normal report envelope with `mode: library_rsync`; changes use follower aliases or `leader`, `push`/`pull` actions, and the home-relative replica path. Cleanup diagnostics identify `leader` even when the transfer needed no changes. Network addresses and temporary export paths are not inserted into machine reports. Child diagnostics go to stderr.

Exit codes are `0` for successful delivery or an in-sync check, `1` for pending check work or a reported transfer/cleanup failure, `2` for invalid arguments, `3` for invalid or unavailable required local input, and `5` for fatal command/output failures. Independent follower pushes continue when another follower fails.

An invalid local follower HOME, unsafe replica path, ownership marker, or inode boundary is rejected before contacting the leader with exit status `3` and no stdout report, including in `--check` mode. After that local validation, remote preparation, replica initialization, transfer, and cleanup failures remain reported failures with exit status `1`.
Interrupted rsync can leave a partially updated owned replica. Fix the connection or input and rerun: the next complete transfer converges to current leader content. This is ordinary rsync, not an atomic multi-host transaction. There is no synchronization history, acknowledgement, rollback journal, or all-host rollback guarantee. If temporary export cleanup fails, stderr reports its retained path for manual removal; the command reports failure. No bounded-memory or network-operation timeout guarantee is provided.

## Migrate earlier unreleased builds

Existing host maps now push as leaders. Choose one authoritative host, retain its `hosts` configuration, and give any pull-initiating follower the single `leader.destination` configuration above. Delivery uses the new shared replica root; previous registered Locations and Git checkouts are preserved, not migrated or deleted. Configure local skill use separately if those replicas should supply a follower's inventory.

The old conflict/missing policies, hidden peer/server commands, Git bootstrap, identity, history, and recovery machinery are removed, with no legacy mode or compatibility shim. Old `~/.skillator/rsync/` data and recoverable files are not read or removed. Preserve them and use the old build for any recovery you still need before retiring that build. Removing a follower alias merely stops future pushes; it does not delete any remote content.
