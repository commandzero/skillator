# Proposal

## Why

The TUI hides Library behind a workspace shortcut and mixes User and Repo directories in one tab strip, making scope and write destinations difficult to discover. PR32's SSH-host library synchronization makes a clear scope/host/directory hierarchy necessary.

## What Changes

- Replace the workspace header with first-line `Library`, `User`, and `Repo` tabs; right-align `Skillator` on that same line.
- Put active scope labels and paths in the bottom status line, not the header. Use bone Library borders, blue User borders, and existing purple Repo borders.
- Add a second-level strip: local leader and configured follower hosts under Library; agent directories under User and Repo. Keep `.agents` and `.claude` independently configurable within each directory scope.
- Make `Ctrl+T` scope-aware. User/Repo open a filter-as-you-type directory picker with arrow selection and custom paths. Library opens follower setup with an SSH-configuration credential hint.
- Probe followers with the equivalent of `ssh -T {destination} hostname`; keep stdout separate from warnings/errors, retain the entered SSH destination, and store the validated reported hostname separately.
- Reuse PR32's `~/.skillator/config.yaml` host registry and transport safeguards. Existing host entries without reported hostname metadata remain valid.
- Preserve staged-save, scope isolation, inheritance, first-run, and reconciliation safeguards. Selecting a follower never initiates synchronization.
- Change scope navigation to `Ctrl+Left/Right`; retain `Tab/Shift+Tab` for sub-tabs and `Ctrl+L` as a direct Library/last-directory-scope shortcut.

## Capabilities

### New Capabilities

- `library-hosts`: Persistent follower configuration, safe SSH hostname discovery, and follower Library inspection using the PR32 host registry.

### Modified Capabilities

- `tui-workflows`: Two-level scope navigation, scope-aware directory creation, footer identity, colors, and isolated staged saves.

## Impact

`src/tui.rs`, application session orchestration in `src/app.rs`, directory validation in `src/config.rs`, and PR32's `src/remote/config.rs` and transport/session boundary. Behavior coverage belongs in existing TUI/configuration tests and the remote tests introduced by PR32; README/help/changelog require updates during implementation.

This worktree does not contain PR32's remote modules or `library-rsync` main spec. Integrate against PR32's final contracts before implementation; do not create a second host registry or weaken its strict parsing. PR32 synchronizes library content bidirectionally and keeps User/Repo desired state host-local: “leader/follower” names UI roles, not a new one-way synchronization policy.

Planning choice: follower tabs inspect remote Library inventory read-only; remote editing and new TUI rsync controls are outside this change. Adding a follower only verifies SSH access and stages configuration; it does not install dependencies, establish host trust, or synchronize content.
