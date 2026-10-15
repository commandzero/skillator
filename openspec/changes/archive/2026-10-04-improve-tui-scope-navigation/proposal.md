# Proposal

## Why

The TUI hides Library behind a workspace shortcut and mixes User and Repo directories in one tab strip, making scope and write destinations difficult to discover. PR38's replacement leader-authoritative SSH library delivery makes a clear scope/host/directory hierarchy necessary.

## What Changes

- Replace the workspace header with first-line `Library`, `User`, and `Repo` tabs; right-align `Skillator` on that same line.
- Put active scope labels and paths in the bottom status line, not the header. Use bone Library borders, blue User borders, and existing purple Repo borders.
- Add a second-level strip: local leader and configured follower hosts under Library; agent directories under User and Repo. Keep `.agents` and `.claude` independently configurable within each directory scope.
- Make `Ctrl+T` scope-aware. User/Repo open a filter-as-you-type directory picker with arrow selection and custom paths. Library opens follower setup with an SSH-configuration credential hint.
- Probe followers with the equivalent of `ssh -T {destination} hostname`; keep stdout separate from warnings/errors, retain the entered SSH destination, and store the validated reported hostname separately.
- Reuse PR38's `~/.skillator/config.yaml` host registry and transport safeguards. Existing host entries without reported hostname metadata remain valid; follower `leader` configurations retain their role.
- Preserve staged-save, scope isolation, inheritance, first-run, and reconciliation safeguards. Selecting a follower never initiates synchronization.
- Change scope navigation to `Ctrl+Left/Right`; retain `Tab/Shift+Tab` for sub-tabs and `Ctrl+L` as a direct Library/last-directory-scope shortcut.

## Capabilities

### New Capabilities

- `library-hosts`: Persistent follower configuration, safe SSH hostname discovery, and read-only inspection of follower replicas using the PR38 host registry.

### Modified Capabilities

- `tui-workflows`: Two-level scope navigation, scope-aware directory creation, footer identity, colors, and isolated staged saves.

## Impact

`src/tui.rs`, application session orchestration in `src/app.rs`, directory validation in `src/config.rs`, and PR38's `src/remote/config.rs` and SSH transport boundary. Behavior coverage belongs in existing TUI/configuration and remote tests; README/help/changelog require updates during implementation.

PR32 was closed without merging and superseded by PR38. This implementation stacks on PR38's leader-authoritative delivery: content moves only leader → follower into the owned `~/.skillator/library/replica`; User/Repo desired state remains host-local. No old bidirectional protocol, synchronization history, or Git-bootstrap machinery is restored.

Planning choice: follower tabs inspect the owned replica read-only over ordinary SSH/POSIX tools, without requiring Skillator or Git on followers. Local Library uses its configuration anchor; follower status uses its replica anchor. Remote editing and new TUI rsync controls are outside this change. Adding a follower only verifies SSH access and stages configuration; it does not install dependencies, establish host trust, or synchronize content. A pull-initiating follower configured with `leader` cannot add downstream followers without an explicit role change outside this workflow.
