---
name: skillator
description: Manage Skillator Library Locations, registered Targets, and Target or User Scope Skill Enablements when a task requires acquiring, inspecting, linking, copying, removing, pruning, or synchronizing local agent skills.
---

# Skillator

Use the installed executable's `--help` output as the command reference.

## Workflow

1. Inspect the relevant command help. Use Git to clone a remote Skills repository, then preview and apply its Library registration. Skillator does not wrap `git clone`.
2. Resolve the requested Skill with `skillator library list [filter] --format json`. Continue once one canonical Source Key and Skill path identify it. If the results are ambiguous, present the candidates and wait for a choice.
3. Inspect saved and observed state with `target list`, `user list`, `targets list`, or `library locations`. Run `skillator init --check` only when Repository Configuration is absent. Continue once the intended scope, Target Repository, and Skill Directory are explicit.
4. Preview the exact mutation with `--check --format json`. Continue only when the report contains no Blocked Change or Recovery Required state.
5. If the preview requires `--force`, report the affected path and reason and wait for explicit authorization. Authorization to manage a Skill does not imply authorization to replace diverged or unmanaged content.
6. Apply the same command without `--check`. Add `--force` only after the preceding authorization.
7. Run the corresponding list or check command. Finish when saved and observed state match the request, or report the remaining diagnostics and preserve recoverable content.

## Decisions

- Prefer `link` when edits in the Library should appear immediately in the active Skill.
- Use `copy` when the active Skill must remain an independent snapshot. Treat later divergence as ambiguous ownership, not automatic permission to replace it.
- A canonical selector is the Source Key and Source-relative Skill path shown by Library JSON output. Display names and directory basenames are not identities.
- Removing a Library Location unregisters the path. It does not delete its directory or remove saved Enablements.
- Preview Library or Target registry pruning and report every path Skillator would forget. Prune changes configuration only; it never deletes repository or Library content.
- Use `git worktree add` to create a linked worktree, then preview and apply `skillator sync worktree` against it. Verify that `targets list` contains the destination.
- User Scope changes affect machine-local agent sessions. Target changes affect one selected Git worktree.
