---
name: skillator
description: Manage local agent skills with Skillator. Use when registering skill libraries, choosing project or user skills, syncing worktrees, or cleaning library and worktree registrations.
---

# Skillator

Read `skillator --help` and the relevant subcommand's help for the installed version's syntax and options.

## Workflow

1. Inspect the requested scope using `target list`, `user list`, `targets list`, or `library locations`. Resolve the project path and skill folder from the request and current directory. Use `user` only for account-wide changes. For inspection requests, report the findings and finish here.
2. For a skill change, find its source key and source-relative path with `skillator library list [filter] --format json`. Use those fields as its selector. If multiple results still fit the request, ask the user to choose. For a project missing configuration, preview and apply `skillator init` before changing skills.
3. Preview each change with `--check --format json`. Account for every affected path. If a change is blocked or needs recovery, resolve the reported cause before applying it. When `--force` is required, proceed only if the user has authorized replacing or removing the affected content; otherwise ask with the path and reason.
4. Apply the previewed command without `--check`, keeping the same scope and selector. Add `--force` only when step 3 permits it.
5. Verify with the corresponding list or check command. Finish when saved choices and installed skills match the request. Report any remaining mismatch and its affected path.

## Linking and copying

Prefer `link` so library edits appear immediately. Use `copy` when the user wants an independent snapshot. Preserve edits to a copied skill unless replacing them is authorized.

## Library and registry changes

For a remote collection, clone it with Git, then register its local folder with `library add`. Registration makes skills available for selection; linking or copying activates them.

Removing a library folder unregisters it and leaves its files and saved skill choices in place. Report any choices that will lose their source. When pruning library or worktree registrations, review every path the preview would forget. Verify the resulting registrations with `library locations` or `targets list`.

## Worktree sync

Use Git to create a linked worktree when requested. Preview and apply `skillator sync worktree` for that destination, then verify its skills and its entry in `targets list`.

Worktree sync uses the primary worktree's choices and this machine's library. Use `sync target` when the intent is to apply a checkout's own saved choices. Sync reads existing settings; use the setup and selection commands when those settings need to change.
