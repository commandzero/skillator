# target-registry Specification

## Purpose

Defines machine-local registration of configured Target worktrees so Skillator can inspect known desired state and later present a Target switcher.

## Requirements

### Requirement: Configured Target worktrees have a strict machine-local registry
Skillator SHALL store registered Target worktrees in `~/.skillator/targets.yaml` as deterministic version 1 configuration containing canonical absolute paths. Each Git worktree SHALL have its own entry. Unknown fields, duplicate paths, unsupported versions, malformed paths, and invalid YAML SHALL yield no partially trusted registry and MUST prevent registry writes.

#### Scenario: Linked worktrees are distinct
- **WHEN** a primary worktree and one linked worktree both have configured Targets
- **THEN** the registry contains their two canonical worktree-root paths

#### Scenario: Registered Target becomes unavailable
- **WHEN** a registered path no longer resolves to a Git worktree
- **THEN** Skillator preserves the entry and reports it as unavailable
### Requirement: Successful Target configuration writes register the worktree
A successful `init`, Target CLI Enablement mutation, Target TUI save, or linked-worktree synchronization SHALL add the canonical worktree root to the registry. Worktree synchronization SHALL register its destination after valid Repository Configuration is successfully published or confirmed. Re-registering an existing canonical path SHALL be idempotent. Check mode and failed or rolled-back Target operations MUST NOT change the registry.

#### Scenario: Initialize and register a Target
- **WHEN** `init` successfully publishes or confirms valid Repository Configuration
- **THEN** Skillator records the canonical Target worktree root exactly once

#### Scenario: Preview does not register
- **WHEN** an unregistered Target mutation runs with `--check`
- **THEN** Skillator reports the planned registration and leaves `targets.yaml` unchanged

#### Scenario: Synchronize and register a linked worktree
- **WHEN** worktree synchronization successfully publishes or confirms valid Repository Configuration in an unregistered linked worktree
- **THEN** Skillator records the linked worktree root exactly once
### Requirement: Registered Targets can be inspected and removed explicitly
`skillator targets list` SHALL report every registered canonical path in deterministic order with a stable status of `available`, `unavailable`, `unconfigured`, `invalid`, or `uninspectable` and relevant diagnostics. `skillator targets remove <directory>` SHALL unregister one matching canonical path without deleting its repository, worktree, configuration, control files, or Materializations. Removing an entry that is already absent SHALL succeed with `unchanged`. An unavailable entry SHALL be addressable by the exact absolute path shown by `targets list`.

#### Scenario: List mixed Target states
- **WHEN** the registry contains available, missing, and invalid Target entries
- **THEN** `targets list` reports each path and its status without changing the registry

#### Scenario: Unregister an unavailable Target
- **WHEN** the user removes the exact absolute path of an unavailable registered Target
- **THEN** Skillator removes only that registry entry and does not inspect or delete the former worktree contents
### Requirement: Stale Target registrations can be pruned explicitly
`skillator targets prune` SHALL prepare one stale-checked registry update that removes entries whose path is definitively absent, no longer names a Git worktree, or no longer contains Repository Configuration. It SHALL preserve entries whose paths or configurations are invalid or uninspectable and report diagnostics for them. Pruning SHALL modify only `targets.yaml` and MUST NOT delete repositories, worktrees, configuration, control files, or Materializations.

#### Scenario: Prune a deleted worktree
- **WHEN** a registered canonical path is definitively absent
- **THEN** Skillator removes the registry entry and reports the stale reason

#### Scenario: Prune an unconfigured repository
- **WHEN** a registered Git worktree no longer contains `.agents/skillator.yaml`
- **THEN** Skillator removes the registry entry without changing the worktree

#### Scenario: Preserve an uninspectable Target
- **WHEN** Skillator cannot inspect a registered path or configuration because of a permission or I/O error
- **THEN** it preserves the entry and reports the diagnostic

#### Scenario: No stale Targets
- **WHEN** every entry is configured or must be preserved
- **THEN** Skillator reports `unchanged`
### Requirement: Registered Targets support Library dependency inspection
Library Location removal SHALL inspect valid Repository Configuration from every available registered Target and User Scope against the post-removal Library Snapshot. Missing, unavailable, or invalid registered Targets SHALL produce diagnostics without inventing desired state. The removal SHALL preserve all Enablements.

#### Scenario: Registered Target loses resolution
- **WHEN** removing a Location makes a registered Target Enablement unresolved
- **THEN** the mutation report identifies the Target path, Skill Directory key, Source Key, and Skill path

#### Scenario: Unavailable registered Target
- **WHEN** a registered Target path is unavailable during Library removal
- **THEN** Skillator preserves the registration and reports that its dependencies could not be inspected
