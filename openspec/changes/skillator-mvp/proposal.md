## Why

Agent Skills are commonly installed globally or copied into each repository, creating either unnecessary context in unrelated sessions or version drift between projects. Skillator gives users one local Library of Skills and a safe, portable way to activate only the Skills a Git repository needs.

## What Changes

- Add a user-scoped Library that discovers local Skill Sources and explicitly registers the Sources and Skills available for activation.
- Add a strict, tracked Repository Configuration that declares Skill Directories and portable per-directory Enablements without storing machine-local Source paths.
- Add Linked and Copied Materializations, observed-state comparison, and guarded reconciliation with partial-apply and recovery guarantees.
- Add a manually invoked Target TUI for enabling and disabling Skills and a separate Library TUI for curating the Library; all edits remain staged until save.
- Add a non-interactive `skillator sync` and check mode with concise text, JSON, or YAML reports and stable exit statuses.
- Support Git Target Repositories on macOS, Linux, and WSL. Preserve unsupported configuration versions without rewriting them.
- Exclude online acquisition, marketplaces, Source updating or publishing, automatic worktree hooks, reusable templates, promotion of Copied edits, schema migration workflows, native Windows, and non-Git Targets from the MVP.

## Capabilities

### New Capabilities

- `library-management`: Configure Library Locations, discover and register local Sources and Skills, preserve portable identity, and surface availability and validation diagnostics.
- `target-configuration`: Select a Git Target Repository and manage strict version 1 Repository Configuration containing Skill Directories and portable Enablements.
- `materialization-reconciliation`: Observe and reconcile Linked or Copied Materializations with bounded authorization, conflict isolation, Git exclusions, rollback, and recovery.
- `cli-contract`: Invoke the Target TUI, Library TUI, synchronization, and check workflows with deterministic reports and stable process outcomes.
- `tui-workflows`: Interactively curate the Library and stage per-Skill-Directory Target changes using the approved table layouts, navigation, confirmation, and save behavior.

### Modified Capabilities

_None._

## Impact

- Introduces the first functional Rust implementation in the existing `skillator` crate.
- Adds strict YAML configuration at `~/.skillator/library.yaml` and `<target>/.agents/skillator.yaml`.
- Creates and reconciles managed filesystem entries and per-Skill-Directory `.gitignore` control files inside selected Git worktrees.
- Requires terminal UI, CLI parsing, YAML serialization, filesystem traversal, Git inspection, locking, and structured output dependencies.
- Establishes new behavioral contracts only; there is no prior Skillator release or configuration schema to migrate.
