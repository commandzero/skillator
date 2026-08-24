## 1. Command and report contracts

- [x] 1.1 Extend clap parsing with nested `library`, `target`, and `user` commands while preserving both no-argument TUI routes and existing sync commands.
- [x] 1.2 Add canonical `<source-key>:<skill-path>` parsing and validation with focused unit tests for slash-containing fields, missing separators, and invalid domain values.
- [x] 1.3 Add compact versioned report types and text, JSON, and YAML rendering for inventory, initialization, and Enablement mutations.
- [x] 1.4 Test shared `--check`, `--force`, format, color, stdout, stderr, and exit-status behavior across the new command families.

## 2. Library commands

- [x] 2.1 Add a stale-checked Library workflow that prepares and applies one Location addition without acquiring or enabling Skills.
- [x] 2.2 Add a stale-checked Library workflow that removes one exact configured Location, preserves its directory and dependent Enablements, and reports newly unresolved identities.
- [x] 2.3 Implement deterministic `library locations` output for configured expressions, resolved paths, and availability diagnostics.
- [x] 2.4 Implement `library list [filter]` over live inventory with case-insensitive Source Key prefix matching and canonical Skill fields.
- [x] 2.5 Add workflow and CLI integration tests for duplicate additions, overlaps, missing removals, owner and full-key filters, empty results, invalid Skills, and machine-format equivalence.

## 3. Target registry

- [x] 3.1 Add a strict deterministic codec for `~/.skillator/targets.yaml` with canonical absolute worktree paths and stale-checked saves.
- [x] 3.2 Register a Target after successful initialization, Target CLI mutation, or Target TUI save while keeping check and failed operations write-free.
- [x] 3.3 Inspect User Scope and available registered Target configurations when reporting Enablements affected by Library Location removal.
- [x] 3.4 Test duplicate registration, linked worktrees, unavailable and invalid Targets, stale registry writes, and affected-Enablement reporting.

## 4. Target initialization

- [x] 4.1 Add a prepared Target initialization workflow for version 1 configuration with the `agents` preset and no Enablements.
- [x] 4.2 Reuse repository control-file and Git tracking checks so initialization is idempotent and preserves tracked legacy configuration.
- [x] 4.3 Add failure-injection and CLI tests proving check mode is write-free and partial initialization rolls back recoverable changes.

## 5. Target Enablement mutations

- [x] 5.1 Add Skill Directory selection by explicit key, saved `agents` key, or sole-directory fallback, with an ambiguity diagnostic for every other case.
- [x] 5.2 Add prepared Target link and copy workflows that resolve one Registered valid Skill, update one Enablement, and plan its Materialization from the same snapshot.
- [x] 5.3 Add prepared Target removal that protects unmanaged entries and requires force for Guarded Materialization removal.
- [x] 5.4 Commit configuration, control-file, and authorized Materialization work under existing fingerprint, staging, rollback, lock, recovery, and Target registration rules.
- [x] 5.5 Test idempotent link and copy, mode changes, multiple Skill Directories, collisions, unresolved selectors, stale sessions, guarded removals, blocked publication, and recovery failures.

## 6. User Scope mutations

- [x] 6.1 Add prepared User Scope link and copy workflows that create the default `agents` configuration on first mutation and perform no Git or control-file work.
- [x] 6.2 Add prepared User Scope removal with the same managed-entry and Guarded Change rules as Repository removal.
- [x] 6.3 Test first-mutation initialization, idempotency, home-relative containment, unmanaged collisions, guarded removal, check mode, and repository isolation.

## 7. Agent workflow and documentation

- [x] 7.1 Allow-list `.agents/skills/skillator/` in the `.agents/.gitignore` exception section while keeping other `.agents/skills` entries ignored.
- [x] 7.2 Create the project-owned Skill with the skill-creator and writing-for-agents guidance, covering canonical resolution, inspection, preview, apply, verification, and human authorization boundaries.
- [x] 7.3 Test the Skill against representative Library registration, Target link or copy, User Scope removal, ambiguous selection, and force-required prompts; prune instructions that duplicate executable help.
- [x] 7.4 Update README command documentation and examples for the Target registry, `library list [filter]`, `library locations`, Target initialization, and Target and User Scope mutations.
- [x] 7.5 Run formatting, the full test suite, clippy with warnings denied, strict OpenSpec validation, and command-help snapshots.

## 8. Parent-owned repository controls

- [x] 8.1 Make `.agents/.gitignore` the sole owner of clone-local configuration and Skill Directory ignore rules without editing the repository root `.gitignore`.
- [x] 8.2 Preserve repository tracking exceptions below the documented exception-list marker when regenerating the control file.
- [x] 8.3 Remove root-ignore dependencies from Target saves and worktree synchronization, and add regression coverage for unchanged root rules and preserved exceptions.
- [x] 8.4 Re-run formatting, tests, clippy, strict OpenSpec validation, and diff checks for the revised contract.

## 9. Repository-owned Target rows

- [x] 9.1 Discover physical unmanaged Skills in Repository Skill Directories and render saved exceptions as `[r] repo` rows.
- [x] 9.2 Let `m` stage repo mode for an unexcepted repository candidate while preventing Space or later mode changes from unchecking `[r]` rows.
- [x] 9.3 Save repo rows as exact parent `.gitignore` exceptions without creating Repository Enablements or Library reconciliation work.
- [x] 9.4 Document the Target TUI behavior and verify reducer, rendering, workflow, full-suite, clippy, and strict OpenSpec coverage.

## 10. Compact footer and scrollable Help

- [x] 10.1 Replace mode enumerations with `m mode`, add `/ filter`, and remove page navigation from both main-window legends.
- [x] 10.2 Make Help scroll by row and page, and make `q` close non-editable overlays like `Esc` without intercepting text entry.
- [x] 10.3 Keep the complete Target and Library mode cycles plus the `q` alias documented in Help.
- [x] 10.4 Add reducer and rendering coverage, then rerun formatting, tests, clippy, strict OpenSpec validation, and diff checks.

## 11. Repository-first Target ordering

- [x] 11.1 Place the Repository group before every Library Source group on Repository tabs.
- [x] 11.2 Add ordering coverage and rerun formatting, tests, clippy, strict OpenSpec validation, and diff checks.

## 12. Scope-aware Target header

- [x] 12.1 Show the active home-relative Skill Directory in the Target header for User tabs while preserving the repository root for Repository tabs.
- [x] 12.2 Add rendering coverage and rerun formatting, tests, clippy, strict OpenSpec validation, and diff checks.

## 13. Context-aware sync command tree

- [x] 13.1 Move Target initialization to top-level `init [directory]` while preserving Target link, copy, and remove commands.
- [x] 13.2 Nest explicit Target and worktree reconciliation under `sync`, remove top-level `worktree`, and dispatch bare `sync` from the current Git context.
- [x] 13.3 Update help, README, agent guidance, diagnostics, and CLI coverage for the new command tree.
- [x] 13.4 Rerun skill validation, formatting, tests, clippy, strict OpenSpec validation, and diff checks.
