# worktree-sync Specification

## Purpose

Defines how a registered linked Git worktree receives clone-local Target configuration and materialized Skills from its primary worktree.

## Requirements

### Requirement: Worktree sync projects primary local Target state

`skillator worktree sync` SHALL run only from a registered linked Git worktree. It SHALL discover the primary and current roots through Git's registered worktree metadata, read and validate the primary `.agents/skillator.yaml`, and atomically publish the same configuration only into the current worktree. It SHALL then reconcile the current worktree using that copied desired state and the current user's Library. It MUST NOT change the primary worktree, the Library configuration, or either worktree's Git index.

#### Scenario: Linked worktree receives configuration and Skills

- **WHEN** a linked worktree runs worktree sync and the primary has valid local Target configuration
- **THEN** the linked worktree receives an equivalent ignored configuration and materializes each resolvable configured Skill in its own Skill Directories

#### Scenario: Current directory is not a linked worktree

- **WHEN** worktree sync runs in the primary worktree, an ordinary checkout, or outside Git
- **THEN** it reports invalid input and makes no writes

#### Scenario: Primary configuration is unavailable

- **WHEN** the primary worktree lacks readable valid `.agents/skillator.yaml`
- **THEN** worktree sync reports the source configuration problem and leaves the linked worktree unchanged

### Requirement: Worktree sync preserves uncommitted local intent

Worktree sync SHALL not silently overwrite a differing local Target configuration. When the linked worktree already has configuration whose bytes differ from the primary worktree's configuration, the command SHALL report a guarded replacement. An absent destination configuration SHALL be Safe. An identical configuration SHALL be a no-op. A differing untracked configuration SHALL be Guarded and require `--force`. Tracked, staged, unmerged, unreadable, non-file, or changed-after-planning destination configuration SHALL be Blocked. `--check` SHALL report the replacement without writing, and `--force` SHALL authorize it together with other viable guarded reconciliation work.

#### Scenario: Differing destination configuration

- **WHEN** the linked worktree has local configuration whose bytes differ from the primary
- **THEN** ordinary worktree sync leaves it unchanged and reports a guarded replacement, while `--force` may replace it

### Requirement: Worktree sync reports partial convergence

After configuration publication, worktree sync SHALL continue independent safe materializations and report unresolved Skills, Guarded Changes, Blocked Changes, and failures using the same exit and report contract as ordinary sync. It SHALL return success only when the current worktree reaches In Sync state.

#### Scenario: One Source is unavailable

- **WHEN** copied configuration names one Skill absent from the current Library and another resolvable Skill
- **THEN** worktree sync materializes the resolvable Skill, preserves desired state, reports the unavailable Skill, and returns the non-converged result
