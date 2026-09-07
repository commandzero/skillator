## Why

Routine Library and Enablement changes currently require the TUI, which makes repository setup and agent-driven workflows awkward. Skillator needs a small non-interactive command set that edits the same desired state and uses the same safety checks as the TUI.

## What Changes

- Extend `skillator library` with commands to add, remove, and prune Library Locations, inspect configured Locations, and list discovered Skills with an optional Source Key filter.
- Add `skillator init [repository]` to create the initial clone-local Repository Configuration and required control files without enabling Skills.
- Add a machine-local registry of configured Target worktrees with commands to list, remove, and prune its entries.
- Add Target commands to list saved Enablements or link, copy, and remove one canonically selected Skill in a chosen Skill Directory.
- Add matching User Scope commands that list saved Enablements, initialize User Scope on first mutation, and link, copy, or remove one Skill.
- Give every non-interactive mutation a check mode, deterministic machine-readable output, idempotent outcomes, stale-write protection, and the existing guarded-change authorization model.
- Add a project-owned agent skill that resolves canonical Skill identities, previews mutations, applies them, and verifies convergence through the new CLI commands.
- Preserve `skillator` as the Target TUI and bare `skillator library` as the Library TUI.
- Group explicit Target and linked-worktree reconciliation under `skillator sync`, with bare `sync` selecting the workflow from the current Git context.
- Register a linked worktree as a Target after successful worktree synchronization.
- Compose with `git clone` and `git worktree add` for repository acquisition and worktree creation instead of wrapping Git operations.

## Capabilities

### New Capabilities

- `agent-cli-workflow`: Define the project-owned agent skill for safe CLI-driven Library, Target, and User Scope operations.
- `target-registry`: Record configured Target worktrees in machine-local state and expose commands to inspect and clean the registry.

### Modified Capabilities

- `cli-contract`: Expand the public command tree, shared output rules, selectors, check behavior, and stable outcomes for CLI-first state changes.
- `library-management`: Allow non-interactive Location edits and filtered listing of live Skill inventory.
- `target-configuration`: Initialize Repository Configuration and edit individual Target Enablements without the TUI.
- `user-scope-onboarding`: Initialize and edit individual User Scope Enablements without the TUI.
- `materialization-reconciliation`: Apply configuration and Materialization changes through one prepared, stale-checked operation with existing Safe, Guarded, and Blocked classifications.
- `tui-workflows`: Show repository-owned Skills as read-only `[r] repo` rows and save their tracking exceptions without creating Library Enablements.

## Impact

The change affects clap parsing and dispatch, application workflows, report schemas and rendering, Library and Target configuration saves, reconciliation planning, tests, help text, and README command documentation. It adds `~/.skillator/targets.yaml` and a repository-owned Skill allow-listed by the exception section in `.agents/.gitignore`. Git remains an external command composed by users and agents; no new runtime dependency is expected.
