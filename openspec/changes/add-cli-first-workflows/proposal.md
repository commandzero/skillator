## Why

Routine Library and Enablement changes currently require the TUI, which makes repository setup and agent-driven workflows awkward. Skillator needs a small non-interactive command set that edits the same desired state and uses the same safety checks as the TUI.

## What Changes

- Extend `skillator library` with commands to add and remove Library Locations, inspect configured Locations, and list discovered Skills with an optional Source Key filter.
- Add `skillator target init [repository]` to create the initial clone-local Repository Configuration and required control files without enabling Skills.
- Add Target commands to link, copy, and remove one canonically selected Skill in a chosen Skill Directory.
- Add matching User Scope commands that initialize User Scope on first mutation and link, copy, or remove one Skill.
- Give every non-interactive mutation a check mode, deterministic machine-readable output, idempotent outcomes, stale-write protection, and the existing guarded-change authorization model.
- Add a project-owned agent skill that resolves canonical Skill identities, previews mutations, applies them, and verifies convergence through the new CLI commands.
- Preserve `skillator` as the Target TUI and bare `skillator library` as the Library TUI.

## Capabilities

### New Capabilities

- `agent-cli-workflow`: Define the project-owned agent skill for safe CLI-driven Library, Target, and User Scope operations.

### Modified Capabilities

- `cli-contract`: Expand the public command tree, shared output rules, selectors, check behavior, and stable outcomes for CLI-first state changes.
- `library-management`: Allow non-interactive Location edits and filtered listing of live Skill inventory.
- `target-configuration`: Initialize Repository Configuration and edit individual Target Enablements without the TUI.
- `user-scope-onboarding`: Initialize and edit individual User Scope Enablements without the TUI.
- `materialization-reconciliation`: Apply configuration and Materialization changes through one prepared, stale-checked operation with existing Safe, Guarded, and Blocked classifications.

## Impact

The change affects clap parsing and dispatch, application workflows, report schemas and rendering, Library and Target configuration saves, reconciliation planning, tests, help text, and README command documentation. It adds a repository-owned Skill under `.agents/skills`. No new runtime dependency or configuration format version is expected.
