## Purpose

Defines how a linked Git worktree receives clone-local Skillator Target configuration and materialized skills from the primary worktree without sharing those files through Git.

## ADDED Requirements

### Requirement: Worktree sync projects primary local Target state
`skillator worktree sync` SHALL run only from a registered linked Git worktree. It SHALL discover the primary worktree through Git's registered worktree metadata, read its local `.agents/skillator.yaml`, validate it, and atomically publish the same configuration in the current worktree. It SHALL then reconcile the current worktree using that copied desired state and the current user's Library. It MUST NOT change the primary worktree, the Library configuration, or either worktree's Git index.

#### Scenario: Linked worktree receives configuration and skills
- **WHEN** a linked worktree runs worktree sync and the primary worktree has valid local Target configuration
- **THEN** the linked worktree receives an equivalent ignored configuration and materializes each resolvable configured skill in its own Skill Directories

#### Scenario: Current directory is not a linked worktree
- **WHEN** worktree sync runs in a primary worktree, an ordinary repository checkout, or outside a Git worktree
- **THEN** it reports invalid worktree input and makes no writes

#### Scenario: Primary configuration is unavailable
- **WHEN** the primary worktree lacks readable valid `.agents/skillator.yaml`
- **THEN** worktree sync reports the source configuration problem and leaves the linked worktree unchanged

### Requirement: Worktree sync preserves uncommitted local intent
Worktree sync SHALL not silently overwrite a differing local Target configuration. When the linked worktree already has configuration whose bytes differ from the primary worktree's configuration, the command SHALL report a guarded replacement. `--check` SHALL report that replacement without writing, and `--force` SHALL authorize it together with other viable guarded reconciliation work.

#### Scenario: Different linked-worktree configuration
- **WHEN** the linked worktree has local Target configuration that differs from the primary configuration
- **THEN** ordinary worktree sync leaves it unchanged and reports a guarded replacement, while `--force` may replace it

### Requirement: Worktree sync reports partial convergence honestly
After configuration publication, worktree sync SHALL continue independent safe materializations and report unresolved Skills, Guarded Changes, Blocked Changes, and failures using the same exit and report contract as ordinary sync. It SHALL return success only when the current worktree reaches In Sync state.

#### Scenario: One Source is unavailable
- **WHEN** copied worktree configuration names one Skill absent from the current user's Library and another resolvable Skill
- **THEN** worktree sync materializes the resolvable Skill, preserves the copied desired state, reports the unavailable Skill, and returns the non-converged result
