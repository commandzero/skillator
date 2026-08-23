## Why

Target enablements depend on a developer's local Library and are often different for each clone of the same repository. Committing `.agents/skillator.yaml` shares machine-specific choices and turns local Skillator materializations into repository churn, while existing repository-owned skills need to remain normal tracked files.

## What Changes

- **BREAKING** Treat `.agents/skillator.yaml` as clone-local Target configuration. It remains beside the repository but is ignored by Git rather than tracked.
- Add stable root `.gitignore` rules for clone-local Skillator state: `.agents/skillator.yaml` and `.agents/.gitignore`.
- Make `.agents/.gitignore` a local generated control file that ignores only Skillator-managed materializations and recovery artifacts. It must leave unlisted repository-owned skills trackable.
- Add `skillator worktree sync`. From a registered Git worktree, it copies the primary worktree's local Target configuration, regenerates local ignore rules, and reconciles configured skills into the current worktree.
- Refuse worktree sync when the current directory is not a registered linked worktree, the primary worktree lacks valid local Target configuration, or safe materialization preconditions are not met.

## Capabilities

### New Capabilities

- `worktree-sync`: Synchronize clone-local Target configuration and configured Skill materializations from a primary Git worktree into a registered linked worktree.

### Modified Capabilities

- `target-configuration`: Make Target configuration clone-local and define its repository-root ignore contract.
- `materialization-reconciliation`: Selectively ignore only Skillator-managed entries while preserving repository-owned skills.
- `cli-contract`: Add the bounded non-interactive `worktree sync` command and its outcomes.

## Impact

- Affected code: configuration paths and save flows, Git ignore inspection and publication, reconciliation planning, command parsing, and report rendering.
- Existing repositories must stop tracking `.agents/skillator.yaml`; Skillator will need a safe, explicit transition that preserves the local file.
- The root `.gitignore` gains stable shared rules, while each clone retains its own `.agents/skillator.yaml`, `.agents/.gitignore`, and materialized skill entries.
