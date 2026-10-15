## Context

The existing scope-cycle reducer and dirty guards already support directional movement. The old Ctrl+L toggle retains unnecessary return-scope state, and root first-run onboarding overrides context selection. See proposal.md for motivation.

## Goals / Non-Goals

Goals: one directional navigation contract; physical-home precedence; context startup including absent Library config; gray Library scope accents.

Non-goals: SSH, registry persistence, reconciliation, content changes, or replacing per-scope directory/browse restoration.

## Decisions

- Select the initial scope only at root startup: physical cwd equal to physical home selects User; other Git cwd selects Repo; non-Git cwd selects User. Manual navigation never reapplies this default. Remove automatic root redirection to Library; explicitly opening Library retains first-run onboarding.
- Map Ctrl+H and Ctrl+L to existing PreviousScope and NextScope. Verify native Ctrl+H event decoding and preserve Backspace inside editable overlays. Remove the toggle action and return-scope state, not compatibility aliases.
- Use indexed gray 245 for Library borders/active accents. Keep bone titles and persistent hotkeys; User/Repo/modal colors are unchanged.

## Risks / Trade-offs

Home can be a Git worktree and can have a symlink spelling; compare physical paths. Ctrl+H may be encoded as a terminal Backspace event; native PTY proof must cover it and editor deletion separately. First-run root startup is a deliberate onboarding behavior change, documented with explicit Library setup navigation. Archived predecessor artifacts/receipts remain immutable.
