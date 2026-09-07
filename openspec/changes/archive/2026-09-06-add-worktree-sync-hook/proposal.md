## Why

Creating a linked Git worktree currently takes two commands: `git worktree add` and `skillator sync worktree`. That gap is easy to miss in interactive work and in tools that create worktrees on a user's behalf. Git already exposes a post-checkout event for worktree creation, so Skillator can offer an opt-in hook that keeps the explicit two-command workflow available while removing this setup trap for users who want it.

## What Changes

- Add an opt-in `skillator hook` command group to install, inspect, and remove Skillator's Git hook integration for a repository.
- Install a `post-checkout` hook at Git's resolved hooks path without silently overwriting an existing hook.
- Run automatic synchronization only for a newly populated linked worktree, not for ordinary branch switches, clones, or `--no-checkout` operations.
- Invoke the existing `skillator sync worktree .` workflow from the new worktree after sanitizing Git's hook environment.
- Make automatic synchronization non-blocking by default: report failures and leave the worktree available for an explicit retry.
- Provide an environment opt-out for individual Git operations and document that explicit synchronization remains the agent and CI fallback.
- Add deterministic command reports, help text, documentation, and tests for installation, detection, chaining, opt-out, and failure behavior.

## Capabilities

### New Capabilities

- `worktree-sync-hooks`: Manage an opt-in Git `post-checkout` integration that synchronizes newly created linked worktrees.

### Modified Capabilities

- `cli-contract`: Add the non-interactive `hook install`, `hook status`, and `hook uninstall` commands and their shared preview, output, and exit-status rules.

## Impact

The change affects CLI parsing and dispatch, Git hook-path and worktree discovery, hook installation and chaining, command reports, integration tests, README/help text, and the project-owned agent skill. It adds no runtime dependency and does not wrap `git worktree add`. Existing Target and worktree synchronization workflows remain the source of truth. Hook files and Git configuration remain machine-local; repository content and Skillator configuration are unchanged by installation.
