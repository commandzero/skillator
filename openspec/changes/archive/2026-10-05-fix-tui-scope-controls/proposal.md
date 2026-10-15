## Why

Root startup must reflect the current directory, and macOS reserves Ctrl+Left/Right for desktop navigation. A Library toggle also prevents consistent scope movement from a Library-only launch; its bone border needs a neutral gray replacement.

## What Changes

- Root startup selects Repo inside a Git worktree and User in the physical home directory, including when home itself is a Git worktree. Context selection applies before Library configuration exists; Library onboarding remains available when opening Library explicitly.
- **BREAKING**: Ctrl+H/Ctrl+L cycle scopes left/right. Remove Ctrl-arrow scope bindings and the Ctrl+L Library toggle, retaining scoped sub-tab/browse state and dirty guards.
- Library borders and active scope accents use indexed gray 245. User blue, Repo purple and bone titles/hotkeys remain unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `tui-workflows`: context startup, portable directional shortcuts and gray Library styling.

## Impact

TUI startup, key mapping and obsolete toggle state; navigation tests; README, Help, contributor checks, changelog and synchronized TUI requirements. No host registry, SSH, reconciliation or skill-content changes.
