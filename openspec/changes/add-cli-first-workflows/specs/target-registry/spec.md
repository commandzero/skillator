## Purpose

Defines machine-local registration of configured Target worktrees so Skillator can inspect known desired state and later present a Target switcher.

## ADDED Requirements

### Requirement: Configured Target worktrees have a strict machine-local registry
Skillator SHALL store registered Target worktrees in `~/.skillator/targets.yaml` as deterministic version 1 configuration containing canonical absolute paths. Each Git worktree SHALL have its own entry. Unknown fields, duplicate paths, unsupported versions, malformed paths, and invalid YAML SHALL yield no partially trusted registry and MUST prevent registry writes.

#### Scenario: Linked worktrees are distinct
- **WHEN** a primary worktree and one linked worktree both have configured Targets
- **THEN** the registry contains their two canonical worktree-root paths

#### Scenario: Registered Target becomes unavailable
- **WHEN** a registered path no longer resolves to a Git worktree
- **THEN** Skillator preserves the entry and reports it as unavailable

### Requirement: Successful Target configuration writes register the worktree
A successful `init`, Target CLI Enablement mutation, or Target TUI save SHALL add the canonical worktree root to the registry. Re-registering an existing canonical path SHALL be idempotent. Check mode and failed or rolled-back Target operations MUST NOT change the registry.

#### Scenario: Initialize and register a Target
- **WHEN** `init` successfully publishes or confirms valid Repository Configuration
- **THEN** Skillator records the canonical Target worktree root exactly once

#### Scenario: Preview does not register
- **WHEN** an unregistered Target mutation runs with `--check`
- **THEN** Skillator reports the planned registration and leaves `targets.yaml` unchanged

### Requirement: Registered Targets support Library dependency inspection
Library Location removal SHALL inspect valid Repository Configuration from every available registered Target and User Scope against the post-removal Library Snapshot. Missing, unavailable, or invalid registered Targets SHALL produce diagnostics without inventing desired state. The removal SHALL preserve all Enablements.

#### Scenario: Registered Target loses resolution
- **WHEN** removing a Location makes a registered Target Enablement unresolved
- **THEN** the mutation report identifies the Target path, Skill Directory key, Source Key, and Skill path

#### Scenario: Unavailable registered Target
- **WHEN** a registered Target path is unavailable during Library removal
- **THEN** Skillator preserves the registration and reports that its dependencies could not be inspected
