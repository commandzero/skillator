---
name: skillator
description: Manage Skillator Library Locations and Target or User Scope Skill Enablements when a task requires inspecting, linking, copying, removing, or reconciling local agent skills.
---

# Skillator

Use the installed executable's `--help` output as the command reference.

## Workflow

1. Inspect the relevant command help and run `skillator library list [filter] --format json`. Continue once one canonical Source Key and Skill path identify the requested Skill. If the results are ambiguous, present the candidates and wait for a choice.
2. Inspect current state with the relevant list or check command. Run `skillator init --check` only when Repository Configuration is absent. Continue once the intended scope, Target Repository, and Skill Directory are explicit.
3. Preview the exact mutation with `--check --format json`. Continue only when the report contains no Blocked Change or Recovery Required state.
4. If the preview requires `--force`, report the affected path and reason and wait for explicit authorization. Authorization to manage a Skill does not imply authorization to replace diverged or unmanaged content.
5. Apply the same command without `--check`. Add `--force` only after the preceding authorization.
6. Run the corresponding check again. Finish when it reports converged state, or report the remaining diagnostics and preserve recoverable content.

## Decisions

- Prefer `link` when edits in the Library should appear immediately in the active Skill.
- Use `copy` when the active Skill must remain an independent snapshot. Treat later divergence as ambiguous ownership, not automatic permission to replace it.
- A canonical selector is the Source Key and Source-relative Skill path shown by Library JSON output. Display names and directory basenames are not identities.
- Removing a Library Location unregisters the path. It does not delete its directory or remove saved Enablements.
- User Scope changes affect machine-local agent sessions. Target changes affect one selected Git worktree.
