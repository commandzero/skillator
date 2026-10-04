# Design

## Context

See `proposal.md` for motivation and `specs/` for the replacement contract. The current `src/remote/` implementation combines remote snapshots, Git comparisons, bidirectional planning, history, staged publication, recovery, and a custom remote process protocol. Existing local library discovery already provides validated Source Keys, skill-relative paths, exclusions, and acquisition-link handling.

The user clarified that one main host owns the library. This supersedes the earlier request to preserve the reconciliation design's remote Git bootstrap and provenance: those features are not required when receiving computers only get skill files. Local acquisition links still work and stay unchanged.

The replacement PR starts from `origin/main` at `233e0db`, where `library rsync` does not exist. The catalogue below describes why the unpublished PR #32 machinery is abandoned; its commits and archives are not prerequisites or part of this PR. The new capability is an addition to `main`, while CLI and Library requirements are modified to describe its integration. Existing local APIs, locks, acquisition, update, and filesystem helpers retain their `main` behavior.

## Goals / Non-Goals

**Goals:** one leader export, ordinary rsync in either initiation direction, bounded follower-replica deletion, and an honest transfer report.

**Non-Goals:** independent follower editing, Git environment replication, remote configuration management, participant enrollment, baselines, distributed locks, custom receiver software, automatic scheduling, or user-materialization management. A leader invocation pushes to configured followers; a follower invocation pulls its configured leader. Neither introduces a watcher or uploads follower skills.

## Decisions

### 1. Export only the discovered skill trees

Reuse normal discovery on the leader; fail before replica writes when leader discovery is incomplete or an export path is ambiguous. Do not discover the follower's local library or compare its Git state. A follower's unrelated or malformed Library/User Scope Configuration is not pull input.

Build a temporary local export directory with layout `<SourceKey>/_skills/<SkillPath>`, treating skill path `.` as the `_skills` directory itself. Existing Source Key segments contain only lowercase ASCII letters, digits, and single hyphens; `_skills` cannot collide with a Source Key segment. This retains readable grouping, avoids invented IDs and long flattened names, and separates otherwise equal skill paths. Reject nested/overlapping skill exports instead of selecting an arbitrary winner.

Materialize acquisition-link roots as directories and preserve supported self-contained internal links. Reuse existing local copy-safety rules. Hard-link regular files into the read-only export when possible, with a real copy only across filesystems; never alter linked file permissions or content. Do not copy `.git`, unrelated repository files, configuration, or materializations. Include the constant ownership marker. A leader push owns local export cleanup; a follower pull owns cleanup of the prepared leader export after transfer or inspection.

This small projection is necessary because registered skill roots can live in multiple repositories and acquired links. Mirroring whole repositories would transfer unrelated content; one rsync process per file would restore the old transfer orchestration. The export contains no hashes, histories, participant records, or transaction journal.

#### Fresh export for a follower-initiated pull

Configuration determines direction without a redundant role flag. The existing `hosts` map means leader; `leader: { destination: "<SSH destination>" }` means follower. Reject both forms together. `--hosts` selects followers only on a leader; a follower contacts only its configured leader and does not need to be enrolled or named in the leader's host list.

For a pull, SSH invokes one hidden export-only operation, `skillator library rsync --prepare-export`, on the leader. It performs the same leader discovery/export used by pushes and prints only the absolute path of its private temporary export. It does not push to the leader's other followers, read a follower inventory, or modify leader skills/configuration. The operation uses a system-temporary directory with a fixed `skillator-library-export-` name prefix, restrictive permissions, and the constant export ownership marker; it reports discovery failure before exposing any usable export.

The follower uses ordinary rsync with that remote export as the source and its local owned replica as the destination. The export is prepared on demand even for preview, so a pull reflects current leader files without requiring a previous push. After transfer or inspection, clean the export over SSH, checking the returned absolute path, fixed temporary basename prefix, non-symlink directory, and regular constant marker before removing only that generated directory. Invalid output, failed preparation, transfer failure, and failed cleanup must be reported; no directory named by unchecked output may be removed. Failed or interrupted cleanup may leave a named temporary export for manual removal, never a claimed successful write-free preview.

This is a small local export helper, not a bidirectional synchronization protocol, version handshake, persistent cache, history database, or remote rsync server wrapper. A pull therefore requires Skillator on the leader and invoking follower; a pushed-to follower still needs only SSH/shell/rsync.

### 2. Use a fixed, owned receiver destination

The follower path is `$HOME/.skillator/library/replica`, remote for a leader push and local for a follower pull. Check it using ordinary SSH shell operations when remote and existing local filesystem checks when local. Verify required rsync availability and reject symlinked below-home ancestors/root before writes. An existing replica must contain a regular, non-symlink `.skillator-rsync-owned` file with the constant text `skillator library rsync replica v1\n`; an unmarked destination is refused rather than adopted or emptied. The marker proves directory ownership only, not host identity or synchronization history.

Application creates the absent follower destination and marker remotely for push or locally for pull. Preview never does. Do not inspect or change follower Library/User Scope Configuration. Preserve old corresponding locations and all sibling library content.

Use existing SSH configuration/authentication, non-interactive authentication, and established host trust. Quote shell arguments centrally; reject option-like destinations. Apart from the leader's small export-only operation for pull, no remote Skillator session, JSON protocol, rsync server wrapper, or credentials transfer is needed.

### 3. Let rsync do the file synchronization

Use ordinary recursive rsync with file permissions/times, links, checksums, safe-link handling, delayed deletion, and itemized changes: `-rltp --checksum --safe-links --delete-delay --omit-dir-times --exclude=/.skillator-rsync-owned --itemize-changes --out-format=%i`. The source is always the leader export with a trailing slash, local for push and remote for pull; the destination is always the checked follower replica, remote for push and local for pull. Do not use ownership/device preservation, `--inplace`, or receiver directory-link following.

Checksums avoid missing content changes that happen to retain the same size and timestamp, including edits on a receiver. Rsync determines current differences directly; no saved content baseline is necessary. `--delete-delay` removes stale files only inside the owned replica. Standard rsync replaces differing destination files; it is not a conflict resolver or distributed transaction.

Ignore export-directory times, and exclude/protect the replica ownership marker from rsync because application creates it separately. Fresh temporary directories and freshly created markers must not turn an unchanged export into perpetual pending work. Preserve real skill-file times and modes; do not change hard-linked source metadata to normalize generated export data.

Use `--dry-run` for an existing follower replica. For an absent replica, report initial creation/delivery without persistent replica preparation. Capture child output so JSON/YAML remain one report document. Itemized output determines required work; summarize each follower replica instead of reconstructing a per-file distributed plan. Use the existing change action to distinguish `push` from `pull`, configured follower labels for pushes, and `leader` for pulls, without adding fields to the outer report envelope or exposing network addresses. Retain actionable child-error context on stderr.

### 4. Keep orchestration small

On the leader, perform discovery/export once, then push to selected followers in deterministic order with independent checks/outcomes; continue after a follower failure. On a follower, prepare one fresh leader export and pull it into the local replica; do not fan out or upload content. Keep configuration parsing and shared report conventions. Use ordinary functions and named outcomes, not lifecycle enums, endpoint traits, or transaction abstractions.

Replace the coordinator and transport implementation with the leader-push/follower-pull path. Delete `session.rs`, `state.rs`, `snapshot.rs`, `planner.rs`, `stage.rs`, obsolete remote test support, and old CLI server entrypoints. Retain only the small new export-only operation and config/process/SSH helpers directly needed by ordinary rsync. Move small orchestration into the existing remote entrypoint; preserve shared filesystem helpers only where actual local consumers need them.

### 5. Resolve the entire simplification catalogue by deletion first

The review compared origin/main `233e0dbb1795da9f7c0471994b1d10babd36eaee` with feature head `61ddf8adc1c4edfd2d71ac75ad069fae31313aab`. Its candidates overlap; they are not a mandate to introduce every suggested helper.

| Standards review entries | Cutover disposition |
| --- | --- |
| X3, X4, X5, X9, X10, X11 | Delete remote fingerprints/history save-read cycles, Git command/checkpoint work, payload limits/manifests, protocol versions, and fault wrappers. |
| C1, C3, C4, C5, C6, C8, C9, C10, C12, C13, C15 | Delete the distributed coordinator, multi-source propagation loops, planned-entry tuples, participant/source ownership state, acknowledgements, base-value enums, and private-state fixtures. Use one leader export, ordinary push/pull, and a named transfer result. |
| S1–S9 | Delete remote lifecycle, staged rename/recovery, reservation, repeated validation/snapshot passes, and their scaffolding rather than refactoring that state machine. |
| N1, N2, N3, N5 | Delete remote inspection and provenance models, configuration-byte wrappers, and Git tree tuples. |
| T1, T2, T3, T5 | Delete remote observer, history, home-path protocol, and old transaction fixture duplication. Reuse existing local export safety and simple disposable receiver fixtures. |
| R1–R5, R7 | Delete test fault dispatch, custom manifests, push/pull wrapper paths, protocol parsing, transfer serving, and server arguments. |
| X1, X2, X6, X7, X8 | Reuse local discovery/path/exclusion conventions; remove remote-only duplicates. Preserve the empty skill-root sentinel and intentional reserved-name distinctions in surviving local callers. |
| C2, C7, C11, C14, R6 | Sort once; use configured-host error context, the shared report envelope, the existing comma-separated host selector, and one safe SSH construction path. |
| F1 | Keep shared descriptor-opening behavior intact for surviving callers; remove only remote-only dead helpers rather than building a new generic abstraction. |
| N4, T4, F2 | Keep the explicitly reviewed non-equivalent local safety operations where still used; do not force superficial unification. |

| Spec review entries | Cutover disposition |
| --- | --- |
| A1 | Use readable push/pull/preview/failure text, not debug-formatted coordinator state. |
| A2, B4, C1 | Remove first-contact policy and cohort inference entirely; the main host is always authoritative. |
| B1 | Preserve local acquisition links and deliver their content; remove remote alias reconstruction and alias-history metadata. |
| B2, B3, B5, C3 | Remove Git provenance/bootstrap, unused remote invalid-skill fields, version probing, and Git checkpoints. |
| B6 | Reuse local discovery diagnostics without a second remote code-remapping layer. |
| C2, D4, D5, D7, D8 | Remove repeated remote snapshots, physical-path maps, context histories, duplicate observers and sorts. Keep one export of the leader's library per invocation and stable follower ordering. |
| D1, D2, D3, D6, D10 | Remove remote stages, payload paths, all-host reserve/begin and session locks, and returned-stage checks. Retain the much smaller dedicated-destination ownership and path checks needed for ordinary rsync. |
| D9 | Determine preview changes from ordinary rsync dry-run, not report strings or speculative location registrations. |
| D11 | Remove old transaction fault scaffolding with its deleted implementation; keep behavior-focused leader-push/follower-pull regressions. |

Unnumbered fixture/seam suggestions are superseded by the small adapter and disposable receiver fixtures. No unrelated Git, acquisition, Target, worktree, or library-update redesign is included.

## Risks / Trade-offs

- Follower edits are replaceable → Help and documentation say the leader is authoritative whether it pushes or the follower pulls.
- An incomplete leader export could erase good copies → Block replica mutation when discovery on the leader is unavailable, unreadable, invalid, or ambiguous.
- A wrong destination could erase unrelated files → Use only the fixed dedicated replica, require its ownership marker, reject redirected roots, and never adopt an existing unmarked directory.
- Concurrent main-host edits may be observed during ordinary rsync → Do not promise snapshot isolation; run again after editing stops. No history or distributed locking is added.
- A receiving account can concurrently mutate its directories → Destination checks protect ordinary mistakes, not an adversarial remote account racing path validation. Do not claim the old custom publication guarantees.
- Temporary export costs local metadata work and cross-filesystem copies → Reuse one leader export for each invocation, hard-link same-filesystem regular files without modifying them, and clean generated exports after push or pull.
- Existing remote selections may refer to old locations → Those locations and user materializations remain untouched; this command only delivers library files and does not promise automatic agent enablement.
- A follower pull can lose its SSH connection before leader-export cleanup → Report failed cleanup and the generated temporary path for manual removal; do not add a session ledger, background janitor, or silently claim successful preview.

## Migration Plan

1. Apply the new command, remove old flags and hidden protocol entrypoints, and replace the affected main requirements. Update the `library-rsync` Purpose to describe authoritative one-way skill delivery.
2. Existing `hosts` configuration remains leader configuration; a follower uses only `version: 1` and `leader: { destination: "<SSH destination>" }`. First push or pull creates its dedicated replica. Old locations, checkouts, history, and recoverable user files are neither read as baselines nor automatically deleted.
3. Update `docs/library-rsync.md`, CLI help, and changelog with both configurations, invocation direction, the shared destination, source authority, deletion boundary, removed Git bootstrap/policies, and unchanged user selections. Do not provide a legacy reconciliation mode or migration shim.
4. Replace obsolete two-way tests with leader-push/follower-pull regressions. Run the actual CLI over SSH for pushes to a receiver lacking Skillator/Git and pulls from a leader with Skillator; then run repository preflight once. Do not archive until implementation and semantic review are complete.

## Design Smoke Evidence

A throwaway fixture ran the real installed rsync with the proposed options. It proved write-free initial dry-run, complete skill delivery with executable mode and internal link, propagation despite equal size/timestamp, replacement of receiver edits without changing the source, stale-skill deletion confined to the replica, and a clean converged dry-run. The fixture was removed. This establishes local rsync semantics only; real SSH and the product CLI remain implementation acceptance checks.

An additional throwaway fixture exercised real rsync's remote-destination push and remote-source pull through a local shell adapter: two followers received a push; a subsequent pull obtained current leader bytes, replaced follower edits without upstream changes, preserved the other follower and sibling content, removed stale replica data, and converged. Another real-rsync fixture observed false pending changes from fresh export-directory/marker timestamps with the original options, then proved that omitting directory times and excluding the owned marker gives a true no-op while preserving the marker during deletion. All fixtures were removed; neither proof exercised real SSH authentication/networking or the Skillator CLI/export helper.

## Implementation Smoke Evidence

The actual debug CLI ran over authenticated TCP SSH against three isolated local OpenSSH servers on macOS. Both pushed-to receivers had rsync and shell utilities on PATH but neither Skillator nor Git. Leader pushes and follower pulls proved fresh leader updates without preceding pushes, replacement of follower edits without uploads, hidden/unselected skill delivery, executable modes and internal links, unavailable leader input preserving replica content, bounded deletion with unrelated siblings/configurations intact, an untouched second follower during pulls, valid machine reports, export cleanup, and final no-op checks.

A separate HFS+ disk image supplied leader skills on a different device from the export directory. An actual SSH pull proved cross-filesystem copy fallback with exact bytes, executable mode, modification time, internal links, and unchanged source content; absent-replica preview remained write-free. The image was detached afterward.

Semantic review identified three defects, each reproduced before its fix and confirmed afterward over actual SSH: non-normalized leader TMPDIR paths retained rejected exports; implicit local export cleanup concealed failure; and an empty first delivery under umask 077 omitted its applied creation outcome. Canonical export paths, explicit reported local cleanup, and replica-creation tracking corrected them. Fourteen deterministic integration regressions passed, including these cases, failed remote cleanup, invalid export paths, and retry after partial rsync failure. The semantic recheck found no remaining concrete gaps; the standards review found no blocking violation.

All three SSH services, isolated homes, generated keys/trust/configuration files, temporary exports, disk image, and throwaway scaffolding were removed.

Repository preflight passed: formatting, strict Clippy, locked all-target tests, doctests, shell/workflow checks, repository-tool tests, OKF validation, all eleven main specifications, and release safeguards.
