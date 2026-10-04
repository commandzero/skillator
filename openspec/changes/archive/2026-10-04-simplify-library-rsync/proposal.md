# Proposal

## Why

The abandoned implementation in PR #32 reconciles independently managed libraries across computers, but the intended workflow has one authoritative leader and receiving followers. Introduce ordinary rsync delivery on current `main`: a leader pushes its skills to followers, while a follower pulls the leader's current skills.

## What Changes

- Add `skillator library rsync` as a leader-authoritative command. Running it on the leader pushes to configured followers; running it on a follower pulls from its configured leader. Follower edits never flow back.
- Keep existing SSH authentication, leader-side follower selection, write-free preview, and ordinary text/JSON/YAML reports. Configuration uses either the existing `hosts` map for a leader or one `leader.destination` for a follower, never both; no participant identity or enrollment is needed.
- Export all skills discovered on the leader, including supporting files and acquisition-link content, into a dedicated follower replica. Preserve source-relative grouping so equal skill names do not collide. Never mirror a home or delete unrelated follower content.
- Use standard rsync to update the replica and remove stale content within that replica. Reject incomplete local discovery before mirroring so an unavailable source cannot look like an intentional deletion.
- Abandon the unpublished conflict/missing policies, remote observations and planning, synchronization baselines, participant IDs, cohort inference, enrollment, remote library-registration merging, custom remote Skillator protocol, and rsync server wrappers.
- Do not introduce exact-commit Git bootstrap or recorded Git branch provenance. Receivers get skill files, not Git checkouts. Git acquisition and `library update` remain separate main-host operations.
- Keep user selections and materializations outside this command. A pushed-to follower needs only SSH/shell/rsync; a follower initiating a pull runs Skillator locally and uses the leader's Skillator to prepare a fresh export. No receiving Git installation or library management is required.
- Supersede the overlapping review catalogue with one small leader discovery/export step, one SSH/rsync adapter supporting push and pull, and one aggregate report. Pull preparation is a small export-only helper, not a remote reconciliation protocol.

## Capabilities

### New Capabilities

- `library-rsync`: Add authoritative one-way delivery to dedicated receiver replicas using standard rsync; the abandoned implementation is not part of the `main` baseline.

### Modified Capabilities

- `cli-contract`: Add the rsync command, leader pushes, follower pulls, and reports without introducing conflict/missing policies, Git bootstrap, or custom remote protocol commands.
- `library-management`: Define file delivery without remote registration merging or modifications to the main library or receiving library configuration.

## Impact

Add the small `src/remote/` implementation and its integration with `src/cli.rs`, sharing existing configuration parsing and materialization link checks. Preserve unrelated local acquisition, library-update, Target, and worktree behavior from `main`. Add `tests/library-rsync.rs`, user documentation, changelog, and affected main specs. No background watcher, enrollment workflow, distributed transaction, compatibility shim, or synchronization database is introduced; the existing tempfile development dependency becomes a runtime dependency.

PR #32 is closed and superseded by this change. The old branch and its historical artifacts remain outside the replacement PR. No unpublished Rust API removal, library-writer locking, descriptor-binding helpers, or old review receipt is carried onto `main`.
