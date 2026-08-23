## 1. Command and report contracts

- [ ] 1.1 Extend clap parsing with nested `library`, `target`, and `user` commands while preserving both no-argument TUI routes and existing sync commands.
- [ ] 1.2 Add canonical `<source-key>:<skill-path>` parsing and validation with focused unit tests for slash-containing fields, missing separators, and invalid domain values.
- [ ] 1.3 Add compact versioned report types and text, JSON, and YAML rendering for inventory, initialization, and Enablement mutations.
- [ ] 1.4 Test shared `--check`, `--force`, format, color, stdout, stderr, and exit-status behavior across the new command families.

## 2. Library commands

- [ ] 2.1 Add a stale-checked Library workflow that prepares and applies one Location addition without acquiring or enabling Skills.
- [ ] 2.2 Add a stale-checked Library workflow that removes one exact configured Location, preserves its directory and dependent Enablements, and reports newly unresolved identities.
- [ ] 2.3 Implement deterministic `library locations` output for configured expressions, resolved paths, and availability diagnostics.
- [ ] 2.4 Implement `library list [filter]` over live inventory with case-insensitive Source Key prefix matching and canonical Skill fields.
- [ ] 2.5 Add workflow and CLI integration tests for duplicate additions, overlaps, missing removals, owner and full-key filters, empty results, invalid Skills, and machine-format equivalence.

## 3. Target initialization

- [ ] 3.1 Add a prepared Target initialization workflow for version 1 configuration with the `agents` preset and no Enablements.
- [ ] 3.2 Reuse repository control-file and Git tracking checks so initialization is idempotent and preserves tracked legacy configuration.
- [ ] 3.3 Add failure-injection and CLI tests proving check mode is write-free and partial initialization rolls back recoverable changes.

## 4. Target Enablement mutations

- [ ] 4.1 Add Skill Directory selection by explicit key, saved `agents` key, or sole-directory fallback, with an ambiguity diagnostic for every other case.
- [ ] 4.2 Add prepared Target link and copy workflows that resolve one Registered valid Skill, update one Enablement, and plan its Materialization from the same snapshot.
- [ ] 4.3 Add prepared Target removal that protects unmanaged entries and requires force for Guarded Materialization removal.
- [ ] 4.4 Commit configuration, control-file, and authorized Materialization work under existing fingerprint, staging, rollback, lock, and recovery rules.
- [ ] 4.5 Test idempotent link and copy, mode changes, multiple Skill Directories, collisions, unresolved selectors, stale sessions, guarded removals, blocked publication, and recovery failures.

## 5. User Scope mutations

- [ ] 5.1 Add prepared User Scope link and copy workflows that create the default `agents` configuration on first mutation and perform no Git or control-file work.
- [ ] 5.2 Add prepared User Scope removal with the same managed-entry and Guarded Change rules as Repository removal.
- [ ] 5.3 Test first-mutation initialization, idempotency, home-relative containment, unmanaged collisions, guarded removal, check mode, and repository isolation.

## 6. Agent workflow and documentation

- [ ] 6.1 Create the project-owned Skill with the skill-creator and writing-for-agents guidance, covering canonical resolution, inspection, preview, apply, verification, and human authorization boundaries.
- [ ] 6.2 Test the Skill against representative Library registration, Target link or copy, User Scope removal, ambiguous selection, and force-required prompts; prune instructions that duplicate executable help.
- [ ] 6.3 Update README command documentation and examples for `library list [filter]`, `library locations`, Target initialization, and Target and User Scope mutations.
- [ ] 6.4 Run formatting, the full test suite, clippy with warnings denied, strict OpenSpec validation, and command-help snapshots.
