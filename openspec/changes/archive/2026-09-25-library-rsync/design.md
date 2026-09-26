## Context

See [proposal.md](proposal.md) for motivation and scope. The library discovers sources live beneath configured Locations. Git origin determines source identity, and user Enablements refer to that identity plus a skill-relative path.

Current configuration has separate library, user-scope, and target-registry files. There is no main host configuration, transport protocol, or shared synchronization baseline. Existing materialization code provides content fingerprints, containment checks, staging, locks, and recovery, but its baseline is not a multi-host edit history.

## Goals / Non-goals

1. Keep observation and planning separate from mutation so preview, stale checks, and deterministic multi-host decisions share one path.
2. Reuse containment and materialization behavior while adding a private remote protocol and transport adapters inside the existing crate.
3. Treat Git checkout identity, skill working-tree content, user desired state, and materialization outcomes as distinct state.
4. Avoid a distributed transaction, a general Git manager, and automatic text merging.

## Decisions

### Main configuration and compatibility

Add optional `~/.skillator/config.yaml` without migrating existing library or user files. Use a strict versioned document with an alias-to-destination map:

```yaml
version: 1
hosts:
  build:
    destination: build
  development:
    destination: developer@development
```

Aliases select participants and label reports. Reserve `local`. Leave port, jump-host, identity-file, and authentication settings to SSH configuration. Do not replicate this file, credentials, or the target registry.

Use a private, versioned Skillator protocol over SSH stdin/stdout, with diagnostics on stderr. Negotiate protocol version 3 and required capabilities before writes. Compatible application releases need not have identical version strings. A hidden internal CLI entry point implements observation, staging, publication, verification, and recovery; it is not a public user workflow.

Pass arguments structurally wherever possible. Validate SSH destinations and use a fixed, safely quoted remote entry point; transport paths and protocol payloads must not become shell code. Bound message size and subprocess timeouts. Use the same SSH identity for protocol and rsync operations.

### Participant identity and history

Persist a stable random participant identity on first successful application, separate from hostname. A cloned state directory or changed identity must be diagnosed rather than silently reusing another participant's history. Preview cannot create this identity.

Store versioned synchronization state beneath `~/.skillator/rsync/`, outside the strict live-inventory library configuration. Record source identity, home-relative path, commit reference where relevant, each participant's acknowledged base, content hash, file type and executable mode, deletion evidence, desired-state entries, and incomplete publication records. Content needed for rollback stays in recoverable staging, not the library registry.

Use per-participant acknowledgements rather than one global success marker. Unselected or failed hosts retain their last acknowledged base. Baseline loss means first contact, which cannot authorize historical deletion. This costs more bookkeeping than two sequential rsync calls but prevents host order and partial failure from choosing the winner.

### Paths, inventory, and transfer units

Resolve existing expressions on the originating machine, prove home containment, then transfer the home-relative suffix. Preserve matching receiving registrations and append missing registrations after successful preparation. Existing exclusion or overlap disagreements block the affected Location. Registration removal is deferred; it must not imply deleting a source tree.

Scope file comparison by source and skill-relative path. Include complete discovered skill trees plus historical synchronized paths so removing a manifest does not hide deletion. Keep executable bits; do not synchronize ownership, timestamps as identity, or arbitrary repository metadata. Never recurse into `.git` through rsync.

Materialize acquired library links as physical content on receivers and write incoming changes through the validated originating skill target without replacing its library link. Require the physical target to remain within home. Apply the existing self-contained rule to internal skill links. Deduplicate physically identical transfer targets; inconsistent logical mappings block publication.

### Exact-commit Git bootstrap

The initiating host supplies origin identity and checked-out commit for each Git source. Clone an absent destination into staging, obtain the exact commit, verify it, and publish a complete checkout at the mapped source root. Preserve the branch name as provenance; a detached checkout at the exact commit is the initial representation. Branch movement and update policy belong to future update work.

Never copy `.git` directories, hooks, config, indexes, alternates, or worktree control files. A source represented locally by a worktree or submodule becomes an independent checkout remotely. If an exact submodule source commit or an unpublished commit cannot be obtained from origin, report it and leave that source unbootstrapped. Do not forward credentials automatically or use an initiating repository as an implicit Git server.

At an existing matching checkout, observe working-tree skill content without modifying the index or unrelated files. Dirtiness alone is acceptable because no branch update occurs. A different commit, different source identity, non-repository destination, or unmerged index blocks that source across participants. Remote-only Git sources have no initiating reference; report them for explicit local setup rather than inventing a revision policy.

The initial idea of copying skills plus origin metadata does not produce a usable checkout. Local disposable-repository tests showed that a first pull rejects copied untracked skill files even when identical to upstream. Creating a full checkout at the exact commit avoids that collision and keeps update behavior separate. Automatic fast-forwarding was considered and excluded from this design.

### Comparison and policy precedence

1. Validate participant capabilities, paths, source identity, and commit alignment before considering conflict policy. Policy flags cannot override these checks.
2. Use committed content at the shared Git commit as the first-contact base for tracked skill files. Use absent state for new untracked paths only where observed absence is trustworthy. Different existing untracked contents without shared history conflict.
3. Use acknowledged synchronization baselines thereafter. A single changed value propagates; identical concurrent values coalesce. Distinct edits, type collisions, or deletion against an edit conflict. Rename is delete plus add in this version.
4. Resolve conflicts first. `local` chooses the initiating value. `remote` chooses one distinct changed remote value only; multiple differing remote candidates remain unresolved. `ask` presents provenance and a choice or skip. No implicit text merge occurs.
5. Apply the missing policy to non-conflicting one-sided absence. `copy` restores, `ignore` records an intentional difference, and `remove` needs acknowledged prior presence on the affected peers. Applying an ignored or skipped outcome must not fabricate a common content baseline.

Choosing an existing value does not authorize deleting unrelated siblings or changing a Git index. A deletion chosen explicitly during conflict resolution is distinct from automatically interpreting missing files as deletions.

### User state and materialization

Merge user configuration semantically by Skill Directory key and Enablement identity. Preserve linked or copied mode and home-relative directory paths. First contact unions independent selections. Later explicit removal of an Enablement propagates independently of the file missing policy. A missing entire configuration is unknown state, not evidence that the user deselected every skill.

Couple directory definition changes to their dependent Enablements. Resolve incompatible edits through the same conflict policy. Do not enable a newly received selection until its library source is usable locally. Reconcile through existing protected mutation workflows so unmanaged materializations cannot be overwritten by a file conflict flag.

Keep desired-state publication and materialization results distinct in reports and history. Prefer the prepared rollback-capable mutation path rather than the older save-before-reconcile workflow. An edited copied user materialization remains guarded drift; automatically promoting that edit back into the library is outside this version.

### Execution and recovery

1. Load and validate local configuration and host selection. Read-only preflight all selected endpoints and dependencies. Any endpoint failure aborts before persistent writes anywhere.
2. Gather available inventory, source references, user state, and history from every selected participant. In check mode, report absent-checkout bootstrap and any unverified follow-on work without creating anything.
3. In apply mode, acquire deterministic participant and local-path locks, then revalidate source references after session start and before any exact-commit clone. Validate each returned staging path beneath its participant's observed home before rsync writes. Prepare clones for missing destinations only after these checks. Origin failures block that source and independent sources can continue.
4. Build one multi-host plan. Resolve interactive conflicts before publication. Route rsync through initiating-host staging; remote hosts do not connect to each other. Transfer only planned entries with explicit file lists, never broad mirror deletion.
5. Verify staged hashes and types. Recheck source and destination preconditions immediately before publication, then preserve originals and publish on each filesystem. Reconcile dependent user state.
6. Verify results, acknowledge each successful entry, and release locks. On failure, attempt local rollback and preserve recoverable state. Report partial completion without claiming all-host atomicity.

Once a run has started publishing, a lost host does not trigger an unsafe attempt to erase successful independent work on every other host. Retained acknowledgements and recovery records make retry explicit. A concurrent change blocks the affected entry; it never receives an unconditional last-writer-wins overwrite.

### Reports and validation

Reuse the existing report envelope and exit codes. Add configured alias attribution for this mode only. Structured reports never prompt. Preflight and read-only preview require established SSH host trust and suppress trust-file updates and persistent connection state. Missing trust is a preflight failure with guidance to establish it separately. Missing receiving library and user configuration is valid empty initial state; malformed existing configuration is an error.

Behavior tests use disposable local origins and multiple isolated homes behind a controllable transport adapter. Verify a real SSH/rsync path separately on macOS and Linux. Keep a small set of real Git tests because a mock cannot establish checkout behavior, index preservation, or unavailable-commit handling.

## Risks / Trade-offs

1. Full clones can be large. Bootstrap once per absent source, report the work explicitly, and avoid promising sparse or partial clone support in version 1.
2. A remote origin may require separate authentication. Fail with the source and configured host alias; do not install credentials or choose another commit.
3. Existing repositories may be at different commits. Block those sources; require the user to align them with separate Git operations.
4. Multi-host publication can be interrupted. Keep verified per-participant history, recoverable originals, and precise partial reports.
5. First-contact deletions cannot be inferred safely for untracked files. Report unmatched entries under `remove`; do not guess.
6. Host homes can contain linked parents and paths that change during a run. Reuse physical containment checks and validate immediately before writes.

## Migration plan

1. Introduce optional main configuration and versioned protocol without altering existing configuration formats or command behavior.
2. Users install a compatible Skillator plus Git and rsync on each host and define SSH destinations locally.
3. First check previews locations, prerequisites, exact-commit bootstrap, conflicts, and unknown history. First apply creates state only after global preflight.
4. Disabling host configuration stops future remote synchronization. Existing materialized skills, clones, and registrations remain usable; no uninstall or automatic cleanup runs.
5. Older Skillator releases that cannot negotiate this protocol are rejected before writes. State format changes require explicit readers or an error preserving existing state.

## Assumptions and deferred work

1. Existing mismatched checkouts block instead of moving even when clean. This follows the final separation between synchronization and repository updates.
2. New checkouts use detached HEAD at the exact initiating commit. Branch setup for later updates is deferred.
3. Main configuration filename and host mapping shape are new implementation choices; existing library configuration stays unchanged.
4. Remote-only Git source bootstrap, repository registration removal, history garbage collection, automatic text merging, and promoting copied user-materialization edits are deferred. Each has an explicit preserved or blocked outcome above.
5. No blocking design questions remain. These planning artifacts do not authorize implementation.
